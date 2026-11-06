//! `run_history` persistence.
//!
//! One row per task that started. `task_id` is nullable by schema (`0003`) so legacy
//! rows stay valid; pruning keeps at most `run_history_limit` rows per app and runs on
//! boot, matching the Electron behaviour.

use crate::Result;
use neohub_domain::{AppId, PackageManager, RunHistoryRow, TaskId};
use rusqlite::{Connection, OptionalExtension, Row, params};

/// Alias kept for the public re-export in `lib.rs`.
pub type HistoryRow = RunHistoryRow;

/// Row-level access to `run_history`.
pub struct RunHistoryStore<'c> {
    conn: &'c Connection,
}

impl<'c> RunHistoryStore<'c> {
    pub fn new(conn: &'c Connection) -> Self {
        RunHistoryStore { conn }
    }

    /// Insert the opening row for a run; returns its id.
    pub fn start_run(&self, row: &RunHistoryRow) -> Result<()> {
        self.conn.execute(
            "INSERT INTO run_history
               (id, app_id, task_id, task_name, started_at, ended_at, script, custom_command,
                node_version, package_manager, exit_code, exit_signal, was_killed_by_user, log_file)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, NULL)",
            params![
                row.id,
                row.app_id.as_str(),
                row.task_id.as_ref().map(|t| t.as_str()),
                row.task_name,
                row.started_at,
                row.ended_at,
                row.script,
                row.custom_command,
                row.node_version,
                row.package_manager.map(|p| p.as_str()),
                row.exit_code,
                row.exit_signal,
                row.was_killed_by_user as i64,
            ],
        )?;
        Ok(())
    }

    /// Close a run with its outcome. A second call is a no-op so a double exit event
    /// cannot rewrite history.
    pub fn finish_run(
        &self,
        id: &str,
        ended_at: i64,
        exit_code: Option<i32>,
        exit_signal: Option<&str>,
        killed_by_user: bool,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE run_history SET ended_at = ?2, exit_code = ?3, exit_signal = ?4,
                    was_killed_by_user = ?5
             WHERE id = ?1 AND ended_at IS NULL",
            params![id, ended_at, exit_code, exit_signal, killed_by_user as i64],
        )?;
        Ok(())
    }

    /// Most recent runs for one app (newest first).
    pub fn list_for_app(&self, app_id: &AppId, limit: Option<u32>) -> Result<Vec<RunHistoryRow>> {
        let limit = i64::from(limit.unwrap_or(50));
        let mut stmt = self.conn.prepare(
            "SELECT * FROM run_history WHERE app_id = ?1
             ORDER BY started_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![app_id.as_str(), limit], row_to_history)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// The newest row for an app, used on boot to restore its sticky `exited`/`crashed`
    /// state instead of resetting everything to `idle`.
    pub fn latest_for_app(&self, app_id: &AppId) -> Result<Option<RunHistoryRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM run_history WHERE app_id = ?1
             ORDER BY started_at DESC LIMIT 1",
        )?;
        Ok(stmt
            .query_row(params![app_id.as_str()], row_to_history)
            .optional()?)
    }

    /// Keep at most `keep` rows per app (older rows are deleted).
    pub fn prune(&self, app_id: &AppId, keep: u32) -> Result<usize> {
        let removed = self.conn.execute(
            "DELETE FROM run_history
             WHERE app_id = ?1 AND id NOT IN (
               SELECT id FROM run_history WHERE app_id = ?1
               ORDER BY started_at DESC LIMIT ?2
             )",
            params![app_id.as_str(), i64::from(keep)],
        )?;
        Ok(removed)
    }

    /// Mark every still-open run as ended (used on shutdown: nothing may stay "running"
    /// across a restart unless the process is genuinely still alive).
    pub fn close_all_open(&self, ended_at: i64) -> Result<usize> {
        let changed = self.conn.execute(
            "UPDATE run_history SET ended_at = ?1 WHERE ended_at IS NULL",
            params![ended_at],
        )?;
        Ok(changed)
    }

    pub fn count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM run_history", [], |row| row.get(0))?)
    }
}

fn row_to_history(row: &Row<'_>) -> rusqlite::Result<RunHistoryRow> {
    let id: String = row.get("id")?;
    if id.is_empty() {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(RunHistoryRow {
        id,
        app_id: AppId::from_existing(row.get::<_, String>("app_id")?),
        task_id: row
            .get::<_, Option<String>>("task_id")?
            .map(TaskId::from_existing),
        task_name: row.get("task_name")?,
        script: row.get("script")?,
        custom_command: row.get("custom_command")?,
        node_version: row.get("node_version")?,
        package_manager: row
            .get::<_, Option<String>>("package_manager")?
            .as_deref()
            .and_then(PackageManager::parse),
        started_at: row.get("started_at")?,
        ended_at: row.get("ended_at")?,
        exit_code: row.get::<_, Option<i64>>("exit_code")?.map(|v| v as i32),
        exit_signal: row.get("exit_signal")?,
        was_killed_by_user: row.get::<_, i64>("was_killed_by_user")? != 0,
    })
}
