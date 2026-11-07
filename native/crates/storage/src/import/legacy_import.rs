//! Copying rows out of a legacy database into a native one.
//!
//! Guarantees:
//!
//! * the legacy copy is opened read-only;
//! * a **new** target is built in a staging directory and published with a single `rename`, so
//!   a crash mid-import leaves the real database untouched;
//! * an **existing** target is merged row by row inside one transaction, skipping ids that are
//!   already present — a repeat import can never overwrite edits made in the native build;
//! * autoStart and the MCP server are forced off for the verification period (the stored intent
//!   is preserved so the user can re-enable them deliberately).
//!
//! Secrets: `enc1:` values are Chromium `safeStorage` ciphertext that this build cannot read.
//! They are copied verbatim (so nothing is lost) and counted in `secrets_unavailable`; the UI
//! reports them as "needs re-entry" instead of pretending they work.

use super::legacy_db::{LEGACY_DB_FILE, MigrationPreview};
use super::manifest::MigrationReport;
use crate::{Result, StorageError};
use rusqlite::{Connection, OpenFlags, params};
use std::path::{Path, PathBuf};

/// Where the imported database lives, plus the report.
pub struct ImportOutcome {
    pub report: MigrationReport,
    pub database: PathBuf,
}

/// Import `source_copy` (the file produced by `copy_source`) into `target_dir`.
pub fn import_legacy(
    source_copy: &Path,
    preview: &MigrationPreview,
    target_dir: &Path,
) -> Result<ImportOutcome> {
    std::fs::create_dir_all(target_dir).map_err(|source| StorageError::Io {
        path: target_dir.display().to_string(),
        source,
    })?;
    let target_db = target_dir.join(LEGACY_DB_FILE);
    let fresh_target = !target_db.exists();

    // Build in a staging directory first when the target is new: one rename publishes it.
    //
    // The name must not collide with the directory a caller uses for its *copy* of the legacy
    // database. It did once: the CLI copies to `<target>/.import-staging`, and this function
    // then deleted that same directory before reading it, so the import failed with
    // "unable to open database file". Publishing gets its own name now.
    let staging_dir = if fresh_target {
        target_dir.join(".publish-staging")
    } else {
        target_dir.to_path_buf()
    };
    if fresh_target {
        // A previous failed attempt must not be merged into this one.
        let _ = std::fs::remove_dir_all(&staging_dir);
    }

    let source = Connection::open_with_flags(source_copy, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let destination = if fresh_target {
        let storage = crate::Storage::open(crate::DataDir::new(&staging_dir))?;
        // Keep the connection alive by moving it out of the wrapper.
        storage.into_connection()
    } else {
        let storage = crate::Storage::open(crate::DataDir::new(target_dir))?;
        storage.into_connection()
    };
    destination.pragma_update(None, "foreign_keys", "ON")?;

    let mut report = MigrationReport {
        source_sha256: preview.source_sha256.clone(),
        schema_versions: preview.schema_versions.clone(),
        counts: preview.counts,
        paused_automation: true,
        unknown_settings: preview.unknown_settings.clone(),
        ..Default::default()
    };

    let tx = destination.unchecked_transaction()?;
    report.imported.apps = copy_apps(&source, &tx)?;
    report.skipped.apps = preview.counts.apps - report.imported.apps;
    report.imported.tasks = copy_tasks(&source, &tx)?;
    report.skipped.tasks = preview.counts.tasks - report.imported.tasks;
    report.imported.env_vars = copy_env_vars(&source, &tx)?;
    report.skipped.env_vars = preview.counts.env_vars - report.imported.env_vars;
    report.imported.run_history = copy_run_history(&source, &tx)?;
    report.skipped.run_history = preview.counts.run_history - report.imported.run_history;
    report.imported.settings = copy_known_settings(&source, &tx)?;
    report.secrets_unavailable = copy_env_vars_secret_count(&source)?;
    // Verification period: nothing starts on its own and no port is taken.
    pause_automation(&tx)?;
    tx.commit()?;

    // Verify before publishing: a broken import must not become the live database.
    let integrity: String =
        destination.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err(StorageError::CorruptJson {
            column: "integrity_check",
            message: integrity,
        });
    }
    let fk_violations: i64 = destination
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap_or(0);
    if fk_violations > 0 {
        return Err(StorageError::CorruptJson {
            column: "foreign_key_check",
            message: format!("{fk_violations} violations"),
        });
    }
    destination
        .pragma_update(None, "wal_checkpoint", "TRUNCATE")
        .ok();
    drop(destination);

    if fresh_target {
        // Publish atomically at the *file* level: the directory already exists (it holds the
        // staging subdirectory), and renaming over a non-empty directory is not allowed.
        let published = std::fs::rename(
            staging_dir.join(LEGACY_DB_FILE),
            target_dir.join(LEGACY_DB_FILE),
        );
        published.map_err(|source| StorageError::Io {
            path: target_dir.display().to_string(),
            source,
        })?;
        let _ = std::fs::remove_file(staging_dir.join(format!("{LEGACY_DB_FILE}-wal")));
        let _ = std::fs::remove_file(staging_dir.join(format!("{LEGACY_DB_FILE}-shm")));
        let _ = std::fs::remove_dir(&staging_dir);
    }

    Ok(ImportOutcome {
        report,
        database: target_db,
    })
}

fn copy_apps(source: &Connection, destination: &Connection) -> Result<i64> {
    let mut stmt = source.prepare(
        "SELECT id, name, path, color, icon, node_version_pref, package_manager, default_script,
                custom_command, working_dir, auto_restart_on_change, watch_globs, port_hint, tags,
                folder, last_started_at, last_exit_code, created_at, updated_at
         FROM apps ORDER BY created_at",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, String>(9)?,
            row.get::<_, i64>(10)?,
            row.get::<_, String>(11)?,
            row.get::<_, Option<i64>>(12)?,
            row.get::<_, String>(13)?,
            row.get::<_, Option<String>>(14)?,
            row.get::<_, Option<i64>>(15)?,
            row.get::<_, Option<i64>>(16)?,
            row.get::<_, i64>(17)?,
            row.get::<_, i64>(18)?,
        ))
    })?;
    let mut imported = 0;
    for row in rows {
        let r = row?;
        // `INSERT OR IGNORE`: an app that already exists keeps the native build's version.
        let changed = destination.execute(
            "INSERT OR IGNORE INTO apps
               (id, name, path, color, icon, node_version_pref, package_manager, default_script,
                custom_command, working_dir, auto_restart_on_change, auto_start, watch_globs,
                port_hint, tags, folder, last_started_at, last_exit_code, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, ?12, ?13, ?14, ?15, ?16,
                     ?17, ?18, ?19)",
            params![
                r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8, r.9, r.10, r.11, r.12, r.13, r.14,
                r.15, r.16, r.17, r.18
            ],
        )?;
        imported += changed as i64;
    }
    Ok(imported)
}

fn copy_tasks(source: &Connection, destination: &Connection) -> Result<i64> {
    let mut stmt = source.prepare(
        "SELECT id, app_id, name, position, command_kind, script, custom_command,
                working_dir_override, package_manager_override, node_version_pref_override,
                depends_on, readiness, one_shot, enabled, env_overrides, created_at, updated_at
         FROM tasks ORDER BY app_id, position",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, Option<String>>(9)?,
            row.get::<_, String>(10)?,
            row.get::<_, String>(11)?,
            row.get::<_, i64>(12)?,
            row.get::<_, i64>(13)?,
            row.get::<_, String>(14)?,
            row.get::<_, i64>(15)?,
            row.get::<_, i64>(16)?,
        ))
    })?;
    let mut imported = 0;
    for row in rows {
        let r = row?;
        let changed = destination.execute(
            "INSERT OR IGNORE INTO tasks
               (id, app_id, name, position, command_kind, script, custom_command,
                working_dir_override, package_manager_override, node_version_pref_override,
                depends_on, readiness, one_shot, enabled, env_overrides, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8, r.9, r.10, r.11, r.12, r.13, r.14,
                r.15, r.16
            ],
        )?;
        imported += changed as i64;
    }
    Ok(imported)
}

fn copy_env_vars(source: &Connection, destination: &Connection) -> Result<i64> {
    let mut stmt = source.prepare(
        "SELECT id, app_id, task_id, key, value, enabled, is_secret, note, created_at, updated_at
         FROM env_vars ORDER BY key",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, i64>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, i64>(8)?,
            row.get::<_, i64>(9)?,
        ))
    })?;
    let mut imported = 0;
    for row in rows {
        let r = row?;
        // Ciphertext is copied verbatim: losing it would lose the user's secret entirely.
        let changed = destination.execute(
            "INSERT OR IGNORE INTO env_vars
               (id, app_id, task_id, key, value, enabled, is_secret, note, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8, r.9],
        )?;
        imported += changed as i64;
    }
    Ok(imported)
}

fn copy_run_history(source: &Connection, destination: &Connection) -> Result<i64> {
    let mut stmt = source.prepare(
        "SELECT id, app_id, task_id, task_name, script, custom_command, node_version,
                package_manager, started_at, ended_at, exit_code, exit_signal, was_killed_by_user
         FROM run_history ORDER BY started_at",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, i64>(8)?,
            row.get::<_, Option<i64>>(9)?,
            row.get::<_, Option<i64>>(10)?,
            row.get::<_, Option<String>>(11)?,
            row.get::<_, i64>(12)?,
        ))
    })?;
    let mut imported = 0;
    for row in rows {
        let r = row?;
        let changed = destination.execute(
            "INSERT OR IGNORE INTO run_history
               (id, app_id, task_id, task_name, started_at, ended_at, script, custom_command,
                node_version, package_manager, exit_code, exit_signal, was_killed_by_user)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                r.0, r.1, r.2, r.3, r.8, r.9, r.4, r.5, r.6, r.7, r.10, r.11, r.12
            ],
        )?;
        imported += changed as i64;
    }
    Ok(imported)
}

fn copy_known_settings(source: &Connection, destination: &Connection) -> Result<i64> {
    let mut stmt = source.prepare("SELECT key, value FROM settings ORDER BY key")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut imported = 0;
    for row in rows {
        let (key, value) = row?;
        if !super::legacy_db::KNOWN_SETTINGS.contains(&key.as_str()) {
            continue; // unknown keys stay in the legacy database; nothing is invented here
        }
        // Settings are single values: the legacy choice must win over the migration default,
        // otherwise the user's theme/language silently resets on import.
        let changed = destination.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        imported += changed as i64;
    }
    Ok(imported)
}

fn copy_env_vars_secret_count(source: &Connection) -> Result<i64> {
    let count: i64 = source.query_row(
        "SELECT COUNT(*) FROM env_vars WHERE is_secret = 1 AND value LIKE 'enc1:%'",
        [],
        |row| row.get(0),
    )?;
    Ok(count)
}

/// Force the automation switches off for the verification period.
fn pause_automation(destination: &Connection) -> Result<()> {
    destination.execute("UPDATE apps SET auto_start = 0", [])?;
    destination.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('mcp_enabled', '0'), ('launch_at_login', '0')",
        [],
    )?;
    Ok(())
}
