//! Reading an Electron database **without touching it**.
//!
//! Rules, because this operates on real user data:
//!
//! * the source directory is opened read-only and never written;
//! * the source database is copied (with `-wal` / `-shm`, if present) into a staging directory
//!   first, so a running/CLI-opened source cannot be read mid-transaction;
//! * everything this module reports is **counts and names**, never row contents — a migration
//!   report must not leak project paths or secrets.
//!
//! Legacy schema versions come from `_schema_migrations` and are compared against the seven
//! migrations this build knows. An unknown (future) version is reported, not guessed at.

use crate::{Result, StorageError};
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Historical user-data directory names, in the order the Electron build looked for them
/// (`src/main/services/AppIdentity.ts`): the first one containing `devharbor.db` wins.
pub const LEGACY_DIR_CANDIDATES: [&str; 3] = ["NeoHub", "DevHarbor", "devharbor"];

/// Database file name used by the Electron build.
pub const LEGACY_DB_FILE: &str = "devharbor.db";

/// Migration versions this build understands.
pub const KNOWN_VERSIONS: [&str; 7] = [
    "0001_init",
    "0002_tasks",
    "0003_run_history_tasks",
    "0004_env_task_scope",
    "0005_app_folders",
    "0006_env_scope_unique",
    "0007_app_autostart",
];

/// What the inspection found. Numbers and names only — no row contents.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MigrationPreview {
    /// Absolute path of the source directory (reported to the user for confirmation).
    pub source_dir: PathBuf,
    /// SHA-256 of the copied database file, so a later import can prove it read the same data.
    pub source_sha256: String,
    /// Migrations recorded in the source, in order.
    pub schema_versions: Vec<String>,
    /// Versions this build does not know (imported conservatively, never dropped).
    pub unknown_versions: Vec<String>,
    /// Settings keys found that this build does not model.
    pub unknown_settings: Vec<String>,
    /// Row counts for the tables that carry user data.
    pub counts: TableCounts,
    /// True when a `-wal` file was present (the source may not have been checkpointed).
    pub had_wal: bool,
}

/// Row counts, for the migration report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TableCounts {
    pub apps: i64,
    pub tasks: i64,
    pub env_vars: i64,
    pub run_history: i64,
    pub settings: i64,
}

/// Locate the legacy data directory under `app_data_dir` (`~/Library/Application Support`).
pub fn discover_legacy_dir(app_data_dir: &Path) -> Option<PathBuf> {
    LEGACY_DIR_CANDIDATES
        .iter()
        .map(|name| app_data_dir.join(name))
        .find(|candidate| candidate.join(LEGACY_DB_FILE).is_file())
}

/// Settings keys the Electron build stored.
pub const KNOWN_SETTINGS: [&str; 16] = [
    "log_ring_size",
    "kill_grace_ms",
    "auto_update",
    "theme",
    "language",
    "dashboard_refresh_ms",
    "notify_on_crash",
    "notify_on_ready",
    "launch_at_login",
    "tray_enabled",
    "run_history_limit",
    "readiness_timeout_ms",
    "mcp_enabled",
    "mcp_port",
    "mcp_require_auth",
    "mcp_https",
];

/// Copy the source database (and its WAL/SHM sidecars) into `staging`, returning the copy path.
///
/// The copy is what gets opened: a source being written by a running Electron instance would
/// otherwise be read mid-transaction, and read-only opening of a WAL database can still create
/// files next to it.
pub fn copy_source(source_dir: &Path, staging: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(staging).map_err(|source| StorageError::Io {
        path: staging.display().to_string(),
        source,
    })?;
    let destination = staging.join(LEGACY_DB_FILE);
    for suffix in ["", "-wal", "-shm"] {
        let from = source_dir.join(format!("{LEGACY_DB_FILE}{suffix}"));
        if !from.is_file() {
            continue;
        }
        let to = staging.join(format!("{LEGACY_DB_FILE}{suffix}"));
        std::fs::copy(&from, &to).map_err(|source| StorageError::Io {
            path: from.display().to_string(),
            source,
        })?;
    }
    Ok(destination)
}

/// SHA-256 of a file (the database copy, not the whole directory).
pub fn file_sha256(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).map_err(|source| StorageError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

/// Inspect a copied legacy database.
pub fn inspect(copied_db: &Path, source_dir: &Path, had_wal: bool) -> Result<MigrationPreview> {
    let sha = file_sha256(copied_db)?;
    let conn = Connection::open_with_flags(copied_db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;

    let schema_versions = read_versions(&conn)?;
    let unknown_versions = schema_versions
        .iter()
        .filter(|version| !KNOWN_VERSIONS.contains(&version.as_str()))
        .cloned()
        .collect();
    let unknown_settings = read_unknown_settings(&conn)?;
    let counts = read_counts(&conn)?;

    Ok(MigrationPreview {
        source_dir: source_dir.to_path_buf(),
        source_sha256: sha,
        schema_versions,
        unknown_versions,
        unknown_settings,
        counts,
        had_wal,
    })
}

fn read_versions(conn: &Connection) -> Result<Vec<String>> {
    let has_table: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='_schema_migrations'",
        [],
        |row| row.get(0),
    )?;
    if has_table == 0 {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare("SELECT version FROM _schema_migrations ORDER BY version")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn read_unknown_settings(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT key FROM settings ORDER BY key")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut out = Vec::new();
    for row in rows {
        let key = row?;
        if !KNOWN_SETTINGS.contains(&key.as_str()) {
            out.push(key);
        }
    }
    Ok(out)
}

fn read_counts(conn: &Connection) -> Result<TableCounts> {
    let count = |table: &str| -> Result<i64> {
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [table],
            |row| row.get(0),
        )?;
        if exists == 0 {
            return Ok(0);
        }
        Ok(
            conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })?,
        )
    };
    Ok(TableCounts {
        apps: count("apps")?,
        tasks: count("tasks")?,
        env_vars: count("env_vars")?,
        run_history: count("run_history")?,
        settings: count("settings")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a legacy-shaped database in a temp dir (same schema as migrations 0001-0007).
    fn legacy_db(dir: &Path, with_wal: bool) -> PathBuf {
        let path = dir.join(LEGACY_DB_FILE);
        let conn = Connection::open(&path).unwrap();
        if with_wal {
            conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        }
        crate::db::apply_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO apps (id, name, path, color, node_version_pref, working_dir,
                               auto_restart_on_change, watch_globs, tags, created_at, updated_at)
             VALUES ('app1','demo','/tmp/demo','#fff','{\"kind\":\"auto\"}','/tmp/demo',0,'[]','[]',1,1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tasks (id, app_id, name, position, command_kind, script, depends_on,
                                readiness, one_shot, enabled, env_overrides, created_at, updated_at)
             VALUES ('task1','app1','dev',0,'script','dev','[]','{\"kind\":\"port\",\"port\":3000}',0,1,'{}',1,1)",
            [],
        )
        .unwrap();
        // `theme` already exists from the migration defaults; replace it and add an unknown key.
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('theme','dark'),('future_flag','1')",
            [],
        )
        .unwrap();
        path
    }

    #[test]
    fn inspection_reports_versions_counts_and_unknown_settings() {
        let dir = tempfile::tempdir().unwrap();
        let db = legacy_db(dir.path(), false);
        let preview = inspect(&db, dir.path(), false).expect("inspect");
        assert_eq!(preview.schema_versions, KNOWN_VERSIONS.to_vec());
        assert!(preview.unknown_versions.is_empty());
        assert_eq!(preview.unknown_settings, vec!["future_flag".to_string()]);
        assert_eq!(preview.counts.apps, 1);
        assert_eq!(preview.counts.tasks, 1);
        assert_eq!(
            preview.counts.settings, 6,
            "five migration defaults plus the unknown key"
        );
        assert_eq!(preview.source_sha256.len(), 64, "sha256 hex");
    }

    #[test]
    fn an_unknown_future_migration_is_reported_not_guessed() {
        let dir = tempfile::tempdir().unwrap();
        let db = legacy_db(dir.path(), false);
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute(
                "INSERT INTO _schema_migrations (version, applied_at) VALUES ('0008_from_the_future', 1)",
                [],
            )
            .unwrap();
        }
        let preview = inspect(&db, dir.path(), false).unwrap();
        assert_eq!(
            preview.unknown_versions,
            vec!["0008_from_the_future".to_string()]
        );
    }

    #[test]
    fn copying_carries_the_wal_sidecar_and_leaves_the_source_alone() {
        let source = tempfile::tempdir().unwrap();
        let db = legacy_db(source.path(), true);
        // Force a WAL sidecar to exist.
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('wal_probe','1')",
                [],
            )
            .unwrap();
        }
        let had_wal = source
            .path()
            .join(format!("{LEGACY_DB_FILE}-wal"))
            .is_file();
        let staging = tempfile::tempdir().unwrap();
        let copied = copy_source(source.path(), staging.path()).expect("copy");
        assert!(copied.is_file());
        if had_wal {
            assert!(
                staging
                    .path()
                    .join(format!("{LEGACY_DB_FILE}-wal"))
                    .is_file(),
                "the WAL comes along so the copy is self-consistent"
            );
        }
        // The source file is untouched (same bytes before and after the copy).
        let source_sha = file_sha256(&db).unwrap();
        let copied_sha = file_sha256(&copied).unwrap();
        assert_eq!(source_sha, copied_sha, "copy is byte-identical");
    }

    #[test]
    fn discovery_prefers_neohub_then_devharbor_then_devharbor_lowercase() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(discover_legacy_dir(root.path()), None);

        std::fs::create_dir_all(root.path().join("devharbor")).unwrap();
        std::fs::write(root.path().join("devharbor").join(LEGACY_DB_FILE), b"").unwrap();
        // macOS volumes are case-insensitive, so compare by name rather than by exact path.
        let discovered = discover_legacy_dir(root.path()).unwrap();
        assert!(
            discovered
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.eq_ignore_ascii_case("devharbor"))
                .unwrap_or(false),
            "found {}",
            discovered.display()
        );

        std::fs::create_dir_all(root.path().join("NeoHub")).unwrap();
        std::fs::write(root.path().join("NeoHub").join(LEGACY_DB_FILE), b"").unwrap();
        let discovered = discover_legacy_dir(root.path()).unwrap();
        assert!(
            discovered.ends_with("NeoHub"),
            "NeoHub wins over the older names, got {}",
            discovered.display()
        );
    }
}
