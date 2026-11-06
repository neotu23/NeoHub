//! `env_vars` persistence: three scopes in one table.
//!
//! Scope is encoded by which ids are set — `(NULL, NULL)` global, `(app, NULL)` app,
//! `(_, task)` task — and uniqueness is enforced by the three partial indexes that
//! migration `0006_env_scope_unique` installed:
//! `uq_env_global(key)`, `uq_env_app(app_id, key)`, `uq_env_task(task_id, key)`.
//!
//! Encryption of `is_secret` values is *not* handled here: phase 4 adds `SecretStore`
//! so ciphertext never reaches core as plaintext by accident.

use crate::Result;
use neohub_domain::{AppId, EnvVar, EnvVarId, TaskId};
use rusqlite::{Connection, Row, params};

/// Row-level access to `env_vars`.
pub struct EnvStore<'c> {
    conn: &'c Connection,
}

impl<'c> EnvStore<'c> {
    pub fn new(conn: &'c Connection) -> Self {
        EnvStore { conn }
    }

    /// Global scope variables (`app_id IS NULL AND task_id IS NULL`).
    pub fn list_global(&self) -> Result<Vec<EnvVar>> {
        self.query(
            "SELECT * FROM env_vars WHERE app_id IS NULL AND task_id IS NULL ORDER BY key ASC",
            [],
        )
    }

    /// App scope (rows whose `task_id` is NULL).
    pub fn list_app(&self, app_id: &AppId) -> Result<Vec<EnvVar>> {
        self.query(
            "SELECT * FROM env_vars
             WHERE app_id = ?1 AND task_id IS NULL
             ORDER BY key ASC",
            params![app_id.as_str()],
        )
    }

    /// Task scope.
    pub fn list_task(&self, task_id: &TaskId) -> Result<Vec<EnvVar>> {
        self.query(
            "SELECT * FROM env_vars WHERE task_id = ?1 ORDER BY key ASC",
            params![task_id.as_str()],
        )
    }

    /// Replace the global scope atomically (delete-then-insert).
    pub fn set_global(&mut self, vars: &[EnvVar], now: i64) -> Result<()> {
        self.replace_scope(None, None, vars, now)
    }

    /// Replace one app's app-scope rows atomically.
    pub fn set_app(&mut self, app_id: &AppId, vars: &[EnvVar], now: i64) -> Result<()> {
        self.replace_scope(Some(app_id), None, vars, now)
    }

    /// Replace one task's task-scope rows atomically.
    pub fn set_task(
        &mut self,
        app: Option<&AppId>,
        task_id: &TaskId,
        vars: &[EnvVar],
        now: i64,
    ) -> Result<()> {
        self.replace_scope(app, Some(task_id), vars, now)
    }

    fn replace_scope(
        &mut self,
        app_id: Option<&AppId>,
        task_id: Option<&TaskId>,
        vars: &[EnvVar],
        now: i64,
    ) -> Result<()> {
        // Callers such as the atomic create-app flow already hold a transaction; in that
        // case write straight into it so the whole create stays one unit of work.
        if !self.conn.is_autocommit() {
            return write_scope(self.conn, app_id, task_id, vars, now);
        }
        let tx = self.conn.unchecked_transaction()?;
        write_scope(&tx, app_id, task_id, vars, now)?;
        tx.commit()?;
        Ok(())
    }

    fn query(&self, sql: &str, params: impl rusqlite::Params) -> Result<Vec<EnvVar>> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params, row_to_env)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Delete rows for one task (used when a task is removed outside the FK cascade path).
    pub fn remove_task_scope(&self, task_id: &TaskId) -> Result<()> {
        self.conn.execute(
            "DELETE FROM env_vars WHERE task_id = ?1",
            params![task_id.as_str()],
        )?;
        Ok(())
    }

    /// Count rows for one app incl. its task scopes (used by migration reporting).
    pub fn count_for_app(&self, app_id: &AppId) -> Result<i64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM env_vars WHERE app_id = ?1",
            params![app_id.as_str()],
            |row| row.get(0),
        )?;
        Ok(count)
    }
}

/// Scope-specific delete + insert, shared by the self-contained and in-transaction paths.
fn write_scope(
    conn: &Connection,
    app_id: Option<&AppId>,
    task_id: Option<&TaskId>,
    vars: &[EnvVar],
    now: i64,
) -> Result<()> {
    // Scope-specific delete keeps the other scopes intact.
    match (app_id, task_id) {
        (None, None) => {
            conn.execute(
                "DELETE FROM env_vars WHERE app_id IS NULL AND task_id IS NULL",
                [],
            )?;
        }
        (Some(app), None) => {
            conn.execute(
                "DELETE FROM env_vars WHERE app_id = ?1 AND task_id IS NULL",
                params![app.as_str()],
            )?;
        }
        (app, Some(task)) => {
            conn.execute(
                "DELETE FROM env_vars WHERE task_id = ?1 OR (app_id = ?2 AND task_id = ?1)",
                params![task.as_str(), app.map(|a| a.as_str())],
            )?;
        }
    }
    for var in vars {
        if var.key.trim().is_empty() {
            continue;
        }
        conn.execute(
            "INSERT INTO env_vars
               (id, app_id, task_id, key, value, enabled, is_secret, note, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                var.id.as_str(),
                app_id.map(|a| a.as_str()),
                task_id.map(|t| t.as_str()),
                var.key.trim(),
                var.value,
                var.enabled as i64,
                var.is_secret as i64,
                var.note,
                now,
                now,
            ],
        )?;
    }
    Ok(())
}

fn row_to_env(row: &Row<'_>) -> rusqlite::Result<EnvVar> {
    let raw_id: String = row.get("id")?;
    Ok(EnvVar {
        id: if raw_id.is_empty() {
            EnvVarId::new()
        } else {
            EnvVarId::from_existing(raw_id)
        },
        app_id: row
            .get::<_, Option<String>>("app_id")?
            .map(AppId::from_existing),
        task_id: row
            .get::<_, Option<String>>("task_id")?
            .map(TaskId::from_existing),
        key: row.get("key")?,
        value: row.get("value")?,
        enabled: row.get::<_, i64>("enabled")? != 0,
        is_secret: row.get::<_, i64>("is_secret")? != 0,
        note: row.get("note")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

/// Convenience constructor used by `CreateAppInput` callers.
pub fn new_var(
    app_id: Option<AppId>,
    task_id: Option<TaskId>,
    key: &str,
    value: &str,
    is_secret: bool,
) -> EnvVar {
    let now = crate::db::now_ms();
    EnvVar {
        id: EnvVarId::new(),
        app_id,
        task_id,
        key: key.to_string(),
        value: value.to_string(),
        enabled: true,
        is_secret,
        note: None,
        created_at: now,
        updated_at: now,
    }
}
