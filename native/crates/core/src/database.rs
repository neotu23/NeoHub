//! Database maintenance: where the file is, export a copy, start over.
//!
//! The rules come straight from the Electron build's `db:*` handlers, because both mistakes they
//! avoid are silent data loss:
//!
//! * **Export is `VACUUM INTO`, not a file copy.** In WAL mode the newest writes live in the
//!   `-wal` sidecar; copying just `devharbor.db` produces a file that opens fine and is missing
//!   the last minutes of work.
//! * **Reset closes the connection first.** Checkpointing folds the WAL into the main file, and
//!   then all three files (`db`, `-wal`, `-shm`) are moved aside together. Leaving a `-wal`
//!   behind lets the next launch recover stale rows into the "fresh" database.

use crate::CoreError;
use crate::service::CoreService;
use std::path::{Path, PathBuf};

/// Result of a reset, so the caller can tell the user where the old data went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResetOutcome {
    /// Backup files that were written (one per file that existed).
    pub moved: Vec<PathBuf>,
    /// The (now empty) database path.
    pub database: PathBuf,
}

impl CoreService {
    /// Path of the live database file, or `None` for an in-memory service.
    pub fn database_path(&self) -> Option<PathBuf> {
        self.with_storage(|storage| Ok(Some(storage.data_dir().db_file())))
            .ok()
            .flatten()
    }

    /// Write a consistent copy of the database to `dest`.
    ///
    /// `VACUUM INTO` refuses to overwrite, so an existing file is an error rather than a
    /// surprise: the caller picked the path.
    pub fn export_database(&self, dest: &Path) -> Result<PathBuf, CoreError> {
        if dest.exists() {
            return Err(CoreError::Domain(
                neohub_domain::DomainError::InvalidInput {
                    field: "path",
                    message: format!("目标已存在：{}", dest.display()),
                },
            ));
        }
        let dest = dest.to_path_buf();
        self.with_storage(|storage| {
            // A single statement, executed by SQLite: no torn copy, WAL included.
            storage
                .conn()
                .execute("VACUUM INTO ?1", [dest.to_string_lossy().to_string()])
                .map_err(neohub_storage::StorageError::from)?;
            Ok(())
        })?;
        Ok(dest)
    }

    /// Close the database, move it (and its sidecars) aside, then open a fresh one.
    ///
    /// Refuses while any task is running: killing the database out from under a live process
    /// would leave the UI describing a task whose history can no longer be written.
    pub fn reset_database(&self) -> Result<ResetOutcome, CoreError> {
        let running = self
            .running_tasks()
            .into_iter()
            .filter(|task| task.state.is_live())
            .count();
        if running > 0 {
            return Err(CoreError::Domain(
                neohub_domain::DomainError::InvalidInput {
                    field: "database",
                    message: format!("还有 {running} 个任务在运行，先停止再重置"),
                },
            ));
        }

        let path = self.database_path().ok_or_else(|| {
            CoreError::Domain(neohub_domain::DomainError::InvalidInput {
                field: "database",
                message: "内存数据库无法重置".into(),
            })
        })?;

        let stamp = neohub_storage::db::now_ms();
        let mut moved = Vec::new();

        // Drop the connection first: this checkpoints the WAL, and nothing can reopen while the
        // slot is empty (every other entry point sees the "closed" error).
        {
            let mut guard = self.storage_slot();
            if let Some(storage) = guard.as_ref() {
                let _ = storage.checkpoint();
            }
            *guard = None;
        }

        for suffix in ["", "-wal", "-shm"] {
            let source = PathBuf::from(format!("{}{suffix}", path.display()));
            if !source.exists() {
                continue;
            }
            let backup = PathBuf::from(format!("{}{suffix}.reset-{stamp}.bak", path.display()));
            std::fs::rename(&source, &backup).map_err(|error| {
                CoreError::Domain(neohub_domain::DomainError::InvalidInput {
                    field: "database",
                    message: format!("无法移动 {}：{error}", source.display()),
                })
            })?;
            moved.push(backup);
        }

        // Open a fresh database in the same place (migrations run again).
        let reopened = neohub_storage::Storage::open(neohub_storage::DataDir::new(
            path.parent().unwrap_or(Path::new(".")),
        ))
        .map_err(CoreError::Storage)?;
        *self.storage_slot() = Some(reopened);

        Ok(ResetOutcome {
            moved,
            database: path,
        })
    }
}
