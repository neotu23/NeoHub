//! Catalog persistence: apps, tasks, folders.
//!
//! This module is the SQL layer only — it validates what the schema can express
//! (NOT NULL, unique path, FK) and maps rows to domain types. Business rules that need
//! a wider view (realpath normalisation, dependency cycles, one-shot/task-name rules,
//! atomic app+task+env creation) live in `neohub-core::catalog`, so there is exactly one
//! place that decides them.
//!
//! Corrupt JSON columns degrade per field instead of failing the whole query — one
//! hand-edited cell must not blank out every app in the list, matching the legacy
//! `safeJson` behaviour.

use crate::{Result, StorageError};
use neohub_domain::{
    App, AppId, CommandKind, NodeVersionPref, PackageManager, ReadinessSignal, Task, TaskId,
};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::de::DeserializeOwned;

/// Fields needed to insert a new app row. Time stamps are filled by the caller so the
/// whole create-app transaction shares one clock reading.
#[derive(Clone, Debug)]
pub struct NewApp {
    pub id: AppId,
    pub name: String,
    pub path: String,
    pub color: String,
    pub icon: Option<String>,
    pub node_version_pref: NodeVersionPref,
    pub package_manager: Option<PackageManager>,
    pub default_script: Option<String>,
    pub custom_command: Option<String>,
    pub working_dir: String,
    pub auto_restart_on_change: bool,
    pub auto_start: bool,
    pub watch_globs: Vec<String>,
    pub port_hint: Option<u16>,
    pub tags: Vec<String>,
    pub folder: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Full-row update for an app. `path` / `id` / `created_at` are intentionally absent:
/// they are immutable provenance and must not be patchable from IPC or MCP.
#[derive(Clone, Debug)]
pub struct AppPatch {
    pub name: String,
    pub color: String,
    pub icon: Option<String>,
    pub node_version_pref: NodeVersionPref,
    pub package_manager: Option<PackageManager>,
    pub default_script: Option<String>,
    pub custom_command: Option<String>,
    pub working_dir: String,
    pub auto_restart_on_change: bool,
    pub auto_start: bool,
    pub watch_globs: Vec<String>,
    pub port_hint: Option<u16>,
    pub tags: Vec<String>,
    pub folder: Option<String>,
    pub last_started_at: Option<i64>,
    pub last_exit_code: Option<i32>,
    pub updated_at: i64,
}

/// Fields needed to insert a task row.
#[derive(Clone, Debug)]
pub struct NewTask {
    pub id: TaskId,
    pub app_id: AppId,
    pub name: String,
    pub position: i64,
    pub command_kind: CommandKind,
    pub script: Option<String>,
    pub custom_command: Option<String>,
    pub working_dir_override: Option<String>,
    pub package_manager_override: Option<PackageManager>,
    pub node_version_pref_override: Option<NodeVersionPref>,
    pub depends_on: Vec<TaskId>,
    pub readiness: ReadinessSignal,
    pub one_shot: bool,
    pub enabled: bool,
    pub env_overrides: std::collections::BTreeMap<String, String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Full-row update for a task. `app_id` and `env_overrides` are not patchable:
/// the app is the task's owner, and `env_overrides` is a frozen legacy column.
#[derive(Clone, Debug)]
pub struct TaskPatch {
    pub name: String,
    pub position: i64,
    pub command_kind: CommandKind,
    pub script: Option<String>,
    pub custom_command: Option<String>,
    pub working_dir_override: Option<String>,
    pub package_manager_override: Option<PackageManager>,
    pub node_version_pref_override: Option<NodeVersionPref>,
    pub depends_on: Vec<TaskId>,
    pub readiness: ReadinessSignal,
    pub one_shot: bool,
    pub enabled: bool,
    pub updated_at: i64,
}

/// Row-level access to apps/tasks/folders.
pub struct Catalog<'c> {
    conn: &'c Connection,
}

impl<'c> Catalog<'c> {
    pub fn new(conn: &'c Connection) -> Self {
        Catalog { conn }
    }

    pub fn connection(&self) -> &'c Connection {
        self.conn
    }

    // ---- apps ----------------------------------------------------------------

    /// All apps, most recently updated first (the legacy sidebar/dashboard order).
    pub fn list_apps(&self) -> Result<Vec<App>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM apps ORDER BY updated_at DESC")?;
        let rows = stmt.query_map([], row_to_app)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get_app(&self, id: &AppId) -> Result<Option<App>> {
        let mut stmt = self.conn.prepare("SELECT * FROM apps WHERE id = ?1")?;
        Ok(stmt
            .query_row(params![id.as_str()], row_to_app)
            .optional()?)
    }

    pub fn get_app_by_path(&self, path: &str) -> Result<Option<App>> {
        let mut stmt = self.conn.prepare("SELECT * FROM apps WHERE path = ?1")?;
        Ok(stmt.query_row(params![path], row_to_app).optional()?)
    }

    /// Insert an app row. Returns `StorageError::DuplicatePath` when the path is taken.
    pub fn insert_app(&self, app: &NewApp) -> Result<()> {
        if let Some(existing) = self.get_app_by_path(&app.path)? {
            return Err(StorageError::DuplicatePath {
                path: app.path.clone(),
                existing: existing.id.to_string(),
            });
        }
        self.conn.execute(
            "INSERT INTO apps
              (id, name, path, color, icon, node_version_pref, package_manager, default_script,
               custom_command, working_dir, auto_restart_on_change, auto_start, watch_globs,
               port_hint, tags, folder, last_started_at, last_exit_code, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                     NULL, NULL, ?17, ?18)",
            params![
                app.id.as_str(),
                app.name,
                app.path,
                app.color,
                app.icon,
                json(&app.node_version_pref)?,
                app.package_manager.map(|p| p.as_str()),
                app.default_script,
                app.custom_command,
                app.working_dir,
                app.auto_restart_on_change as i64,
                app.auto_start as i64,
                json(&app.watch_globs)?,
                app.port_hint.map(|p| p as i64),
                json(&app.tags)?,
                app.folder,
                app.created_at,
                app.updated_at,
            ],
        )?;
        Ok(())
    }

    /// Overwrite the mutable columns of an app. Returns the row as stored.
    pub fn update_app(&self, id: &AppId, patch: &AppPatch) -> Result<App> {
        let changed = self.conn.execute(
            "UPDATE apps SET
               name = ?1, color = ?2, icon = ?3, node_version_pref = ?4, package_manager = ?5,
               default_script = ?6, custom_command = ?7, working_dir = ?8,
               auto_restart_on_change = ?9, auto_start = ?10, watch_globs = ?11, port_hint = ?12,
               tags = ?13, folder = ?14, last_started_at = ?15, last_exit_code = ?16,
               updated_at = ?17
             WHERE id = ?18",
            params![
                patch.name,
                patch.color,
                patch.icon,
                json(&patch.node_version_pref)?,
                patch.package_manager.map(|p| p.as_str()),
                patch.default_script,
                patch.custom_command,
                patch.working_dir,
                patch.auto_restart_on_change as i64,
                patch.auto_start as i64,
                json(&patch.watch_globs)?,
                patch.port_hint.map(|p| p as i64),
                json(&patch.tags)?,
                patch.folder,
                patch.last_started_at,
                patch.last_exit_code,
                patch.updated_at,
                id.as_str(),
            ],
        )?;
        if changed == 0 {
            return Err(StorageError::NotFound {
                entity: "app",
                id: id.to_string(),
            });
        }
        self.get_app(id)?.ok_or_else(|| StorageError::NotFound {
            entity: "app",
            id: id.to_string(),
        })
    }

    /// Delete an app. Tasks/env/history go with it via `ON DELETE CASCADE`.
    pub fn remove_app(&self, id: &AppId) -> Result<()> {
        self.conn
            .execute("DELETE FROM apps WHERE id = ?1", params![id.as_str()])?;
        Ok(())
    }

    // ---- folders -------------------------------------------------------------

    /// Distinct non-empty folders, case-insensitive alphabetical.
    pub fn list_folders(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT folder FROM apps
             WHERE folder IS NOT NULL AND folder != ''
             ORDER BY folder COLLATE NOCASE ASC",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Rename every app in `from` to `to` (case-insensitive match; `None` clears).
    pub fn rename_folder(&self, from: &str, to: Option<&str>, now: i64) -> Result<usize> {
        let changed = self.conn.execute(
            "UPDATE apps SET folder = ?1, updated_at = ?2
             WHERE folder IS NOT NULL AND folder = ?3 COLLATE NOCASE",
            params![to, now, from],
        )?;
        Ok(changed)
    }

    /// Move every app in `name` back to "(Ungrouped)". The project directory on disk is
    /// untouched — clearing a folder only removes the grouping.
    pub fn clear_folder(&self, name: &str, now: i64) -> Result<usize> {
        let changed = self.conn.execute(
            "UPDATE apps SET folder = NULL, updated_at = ?1
             WHERE folder IS NOT NULL AND folder = ?2 COLLATE NOCASE",
            params![now, name],
        )?;
        Ok(changed)
    }

    // ---- tasks ---------------------------------------------------------------

    /// Tasks of one app in display order.
    pub fn list_tasks(&self, app_id: &AppId) -> Result<Vec<Task>> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM tasks WHERE app_id = ?1 ORDER BY position ASC, created_at ASC",
        )?;
        let rows = stmt.query_map(params![app_id.as_str()], row_to_task)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Every task of every app in one ordered scan (replaces an N+1 of `list_tasks`).
    pub fn list_all_tasks(&self) -> Result<Vec<Task>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM tasks ORDER BY app_id ASC, position ASC, created_at ASC")?;
        let rows = stmt.query_map([], row_to_task)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get_task(&self, id: &TaskId) -> Result<Option<Task>> {
        let mut stmt = self.conn.prepare("SELECT * FROM tasks WHERE id = ?1")?;
        Ok(stmt
            .query_row(params![id.as_str()], row_to_task)
            .optional()?)
    }

    pub fn insert_task(&self, task: &NewTask) -> Result<()> {
        self.conn.execute(
            "INSERT INTO tasks
              (id, app_id, name, position, command_kind, script, custom_command,
               working_dir_override, package_manager_override, node_version_pref_override,
               depends_on, readiness, one_shot, enabled, env_overrides, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                task.id.as_str(),
                task.app_id.as_str(),
                task.name,
                task.position,
                task.command_kind.as_str(),
                task.script,
                task.custom_command,
                task.working_dir_override,
                task.package_manager_override.map(|p| p.as_str()),
                match &task.node_version_pref_override {
                    Some(pref) => Some(json(pref)?),
                    None => None,
                },
                json(&task.depends_on)?,
                json(&task.readiness)?,
                task.one_shot as i64,
                task.enabled as i64,
                json(&task.env_overrides)?,
                task.created_at,
                task.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn update_task(&self, id: &TaskId, patch: &TaskPatch) -> Result<Task> {
        let changed = self.conn.execute(
            "UPDATE tasks SET
               name = ?1, position = ?2, command_kind = ?3, script = ?4, custom_command = ?5,
               working_dir_override = ?6, package_manager_override = ?7,
               node_version_pref_override = ?8, depends_on = ?9, readiness = ?10,
               one_shot = ?11, enabled = ?12, updated_at = ?13
             WHERE id = ?14",
            params![
                patch.name,
                patch.position,
                patch.command_kind.as_str(),
                patch.script,
                patch.custom_command,
                patch.working_dir_override,
                patch.package_manager_override.map(|p| p.as_str()),
                match &patch.node_version_pref_override {
                    Some(pref) => Some(json(pref)?),
                    None => None,
                },
                json(&patch.depends_on)?,
                json(&patch.readiness)?,
                patch.one_shot as i64,
                patch.enabled as i64,
                patch.updated_at,
                id.as_str(),
            ],
        )?;
        if changed == 0 {
            return Err(StorageError::NotFound {
                entity: "task",
                id: id.to_string(),
            });
        }
        self.get_task(id)?.ok_or_else(|| StorageError::NotFound {
            entity: "task",
            id: id.to_string(),
        })
    }

    /// Delete one task. `env_vars.task_id` rows cascade. Dependents are NOT silently
    /// rewritten — the caller (core) refuses the delete while other tasks depend on it.
    pub fn remove_task(&self, id: &TaskId) -> Result<()> {
        self.conn
            .execute("DELETE FROM tasks WHERE id = ?1", params![id.as_str()])?;
        Ok(())
    }

    /// Set `position` to the index of each id. Must run inside a transaction.
    pub fn reorder_tasks(&self, app_id: &AppId, task_ids: &[TaskId], now: i64) -> Result<()> {
        for (index, id) in task_ids.iter().enumerate() {
            self.conn.execute(
                "UPDATE tasks SET position = ?1, updated_at = ?2 WHERE id = ?3 AND app_id = ?4",
                params![index as i64, now, id.as_str(), app_id.as_str()],
            )?;
        }
        Ok(())
    }

    /// Tasks in `app_id` that depend on `task_id` (used to refuse unsafe deletes).
    pub fn dependents_of(&self, app_id: &AppId, task_id: &TaskId) -> Result<Vec<Task>> {
        Ok(self
            .list_tasks(app_id)?
            .into_iter()
            .filter(|t| t.depends_on.iter().any(|d| d == task_id))
            .collect())
    }
}

/// Give every app that predates the multi-task model exactly one task, derived from its
/// `default_script` / `custom_command`.
///
/// Idempotent: apps that already have a task are skipped. Apps with neither a script nor
/// a custom command are skipped too — they need an explicit task, exactly as before.
pub fn backfill_tasks_for_existing_apps(conn: &Connection) -> Result<usize> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.default_script, a.custom_command
         FROM apps a
         WHERE NOT EXISTS (SELECT 1 FROM tasks t WHERE t.app_id = a.id)",
    )?;
    let candidates = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);

    if candidates.is_empty() {
        return Ok(0);
    }

    let now = crate::db::now_ms();
    let tx = conn.unchecked_transaction()?;
    let mut created = 0usize;
    for (app_id, default_script, custom_command) in candidates {
        // Same decision tree as the legacy backfill: custom command wins, then the
        // default script; no script at all means "leave it to the user".
        let (name, kind, script, command) = if let Some(command) = custom_command {
            ("main".to_string(), "custom", None, Some(command))
        } else if let Some(script) = default_script {
            (script.clone(), "script", Some(script), None)
        } else {
            continue;
        };
        tx.execute(
            "INSERT INTO tasks
               (id, app_id, name, position, command_kind, script, custom_command,
                working_dir_override, package_manager_override, node_version_pref_override,
                depends_on, readiness, one_shot, enabled, env_overrides, created_at, updated_at)
             VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6, NULL, NULL, NULL, '[]', '{\"kind\":\"none\"}',
                     0, 1, '{}', ?7, ?7)",
            params![
                TaskId::new().as_str(),
                app_id,
                name,
                kind,
                script,
                command,
                now
            ],
        )?;
        created += 1;
    }
    tx.commit()?;
    Ok(created)
}

// ---- row mapping -----------------------------------------------------------

fn json<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|e| StorageError::CorruptJson {
        column: "serialize",
        message: e.to_string(),
    })
}

/// Parse a JSON column defensively. `None` (SQL NULL) and unparsable text both fall back
/// to the caller's default instead of failing the query.
fn parse_json<T: DeserializeOwned>(raw: Option<&str>, fallback: T) -> T {
    match raw {
        None => fallback,
        Some(text) => serde_json::from_str(text).unwrap_or(fallback),
    }
}

fn parse_pref(raw: Option<String>) -> NodeVersionPref {
    parse_json(raw.as_deref(), NodeVersionPref::Auto)
}

fn parse_pm(raw: Option<String>) -> Option<PackageManager> {
    raw.as_deref().and_then(PackageManager::parse)
}

fn row_to_app(row: &Row<'_>) -> rusqlite::Result<App> {
    Ok(App {
        id: AppId::from_existing(row.get::<_, String>("id")?),
        name: row.get("name")?,
        path: row.get("path")?,
        color: row.get("color")?,
        icon: row.get("icon")?,
        node_version_pref: parse_pref(row.get("node_version_pref")?),
        package_manager: parse_pm(row.get("package_manager")?),
        default_script: row.get("default_script")?,
        custom_command: row.get("custom_command")?,
        working_dir: row.get("working_dir")?,
        auto_restart_on_change: row.get::<_, i64>("auto_restart_on_change")? != 0,
        auto_start: row.get::<_, i64>("auto_start")? != 0,
        watch_globs: parse_json(
            row.get::<_, Option<String>>("watch_globs")?.as_deref(),
            Vec::new(),
        ),
        port_hint: row.get::<_, Option<i64>>("port_hint")?.map(|p| p as u16),
        tags: parse_json(row.get::<_, Option<String>>("tags")?.as_deref(), Vec::new()),
        folder: row.get("folder")?,
        last_started_at: row.get("last_started_at")?,
        last_exit_code: row
            .get::<_, Option<i64>>("last_exit_code")?
            .map(|v| v as i32),
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn row_to_task(row: &Row<'_>) -> rusqlite::Result<Task> {
    let command_kind = row.get::<_, String>("command_kind")?;
    let depends_on: Vec<String> = parse_json(
        row.get::<_, Option<String>>("depends_on")?.as_deref(),
        Vec::new(),
    );
    Ok(Task {
        id: TaskId::from_existing(row.get::<_, String>("id")?),
        app_id: AppId::from_existing(row.get::<_, String>("app_id")?),
        name: row.get("name")?,
        position: row.get("position")?,
        command_kind: CommandKind::parse(&command_kind).unwrap_or(CommandKind::Script),
        script: row.get("script")?,
        custom_command: row.get("custom_command")?,
        working_dir_override: row.get("working_dir_override")?,
        package_manager_override: parse_pm(row.get("package_manager_override")?),
        node_version_pref_override: row
            .get::<_, Option<String>>("node_version_pref_override")?
            .as_deref()
            .and_then(|raw| serde_json::from_str(raw).ok()),
        depends_on: depends_on.into_iter().map(TaskId::from_existing).collect(),
        readiness: parse_json(
            row.get::<_, Option<String>>("readiness")?.as_deref(),
            ReadinessSignal::None,
        ),
        one_shot: row.get::<_, i64>("one_shot")? != 0,
        enabled: row.get::<_, i64>("enabled")? != 0,
        env_overrides: parse_json(
            row.get::<_, Option<String>>("env_overrides")?.as_deref(),
            Default::default(),
        ),
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}
