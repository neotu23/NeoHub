//! Database handle, data-directory resolution and migration runner.
//!
//! The database file keeps the legacy name (`devharbor.db`) inside a **separate**
//! data directory (`NeoHub-Rust-Dev` by default, or `--data-dir <path>`), so the Rust
//! build and the Electron build can never read/write the same file by accident.
//!
//! Threading: `Storage` owns a single `rusqlite::Connection` and is not `Sync`. The core
//! actor (phase 5) owns it on its own thread and hands work to it; nothing else opens a
//! second connection. `busy_timeout` is set explicitly so a concurrent process (e.g. the
//! legacy app during a migration) surfaces as a timeout rather than an immediate
//! `database is locked`.

use crate::{Result, StorageError};
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};

/// Where NeoHub stores its database and session files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataDir {
    path: PathBuf,
}

impl DataDir {
    /// `~/Library/Application Support/NeoHub-Rust-Dev` — the development default.
    ///
    /// Deliberately different from the Electron build's `devharbor`/`NeoHub`/`DevHarbor`
    /// directories: running both apps must never share a database.
    pub fn development_default() -> Result<Self> {
        let home = std::env::var_os("HOME").ok_or_else(|| StorageError::Io {
            path: "$HOME".to_string(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "HOME is not set"),
        })?;
        Ok(DataDir {
            path: PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("NeoHub-Rust-Dev"),
        })
    }

    /// Wrap an explicit directory (tests use `tempfile`; the app passes `--data-dir`).
    pub fn new(path: impl Into<PathBuf>) -> Self {
        DataDir { path: path.into() }
    }

    /// Honour `--data-dir <path>` / `--data-dir=<path>`, else fall back to the default.
    pub fn from_args<I, S>(args: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let arg = arg.as_ref();
            if arg == "--data-dir" {
                if let Some(value) = args.next() {
                    return Ok(DataDir::new(value.as_ref()));
                }
            } else if let Some(value) = arg.strip_prefix("--data-dir=")
                && !value.is_empty()
            {
                return Ok(DataDir::new(value));
            }
        }
        DataDir::development_default()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The database file — same file name as the legacy build so migration tooling can
    /// reuse it, but inside this data directory.
    pub fn db_file(&self) -> PathBuf {
        self.path.join("devharbor.db")
    }

    fn ensure_exists(&self) -> Result<()> {
        std::fs::create_dir_all(&self.path).map_err(|source| StorageError::Io {
            path: self.path.display().to_string(),
            source,
        })
    }
}

/// An open database plus the directory it lives in.
pub struct Storage {
    conn: Connection,
    data_dir: DataDir,
}

impl Storage {
    /// Open (creating if needed) the database in `data_dir`, then migrate and backfill.
    pub fn open(data_dir: DataDir) -> Result<Self> {
        data_dir.ensure_exists()?;
        let file = data_dir.db_file();
        let conn = Connection::open_with_flags(
            &file,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_URI,
        )
        .map_err(StorageError::Sqlite)?;
        Self::finish_open(conn, data_dir)
    }

    /// In-memory database for tests; migrations and backfill run exactly like production.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::finish_open(conn, DataDir::new(":memory:"))
    }

    fn finish_open(conn: Connection, data_dir: DataDir) -> Result<Self> {
        configure(&conn)?;
        apply_migrations(&conn)?;
        // Backfill runs after the schema is in place and before the handle is handed out,
        // mirroring the Electron boot order (migrations → backfill → serve).
        crate::catalog::backfill_tasks_for_existing_apps(&conn)?;
        Ok(Storage { conn, data_dir })
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn data_dir(&self) -> &DataDir {
        &self.data_dir
    }

    pub fn catalog(&self) -> crate::catalog::Catalog<'_> {
        crate::catalog::Catalog::new(&self.conn)
    }

    pub fn env_store(&self) -> crate::env::EnvStore<'_> {
        crate::env::EnvStore::new(&self.conn)
    }

    pub fn history(&self) -> crate::history::RunHistoryStore<'_> {
        crate::history::RunHistoryStore::new(&self.conn)
    }

    pub fn settings(&self) -> crate::settings::SettingsStore<'_> {
        crate::settings::SettingsStore::new(&self.conn)
    }

    /// Flush the WAL into the main file so an export/backup taken right after is complete.
    pub fn checkpoint(&self) -> Result<()> {
        self.conn
            .pragma_update(None, "wal_checkpoint", "TRUNCATE")?;
        Ok(())
    }
}

/// Connection pragmas the legacy build relied on, plus an explicit busy timeout.
fn configure(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    // 5 s: long enough for a second process to finish a short write, short enough that a
    // stuck lock surfaces as an error instead of a frozen UI.
    conn.busy_timeout(std::time::Duration::from_millis(5_000))?;
    Ok(())
}

/// Create `_schema_migrations` if absent and apply every not-yet-applied migration.
///
/// Each migration runs in its own transaction together with its version row, exactly
/// like the Electron runner: a failing migration must not leave a half-applied schema
/// with no record of what happened.
pub fn apply_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _schema_migrations (
            version TEXT PRIMARY KEY,
            applied_at INTEGER NOT NULL
        )",
    )?;

    let applied: std::collections::HashSet<String> = {
        let mut stmt = conn.prepare("SELECT version FROM _schema_migrations")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };

    for migration in crate::migrations::MIGRATIONS {
        if applied.contains(migration.version) {
            continue;
        }
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(migration.sql)?;
        let now = now_ms();
        tx.execute(
            "INSERT INTO _schema_migrations (version, applied_at) VALUES (?1, ?2)",
            rusqlite::params![migration.version, now],
        )?;
        tx.commit()?;
    }
    Ok(())
}

/// Current unix time in milliseconds — the unit every timestamp column uses.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
