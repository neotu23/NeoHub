//! Legacy schema migrations, applied verbatim.
//!
//! The SQL files under `migrations/legacy/` are byte-identical copies of
//! `src/main/db/migrations/0001_init.sql` … `0007_app_autostart.sql`. They are
//! embedded (`include_str!`) and executed in order, recorded in
//! `_schema_migrations` under the file name **without** the `.sql` suffix — exactly
//! the keys the Electron build wrote. Switching to `PRAGMA user_version` would re-run
//! the old `ALTER TABLE` statements against an already-migrated database and fail.
//!
//! Rust-only schema changes (if any) go to `migrations/native/` with their own version
//! keys; they must never collide with the 0001–0007 names.

/// One legacy migration, executed inside its own transaction.
pub struct Migration {
    /// Version key stored in `_schema_migrations` (file name sans `.sql`).
    pub version: &'static str,
    /// Verbatim SQL from the Electron build.
    pub sql: &'static str,
}

/// All legacy migrations, in application order.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: "0001_init",
        sql: include_str!("../migrations/legacy/0001_init.sql"),
    },
    Migration {
        version: "0002_tasks",
        sql: include_str!("../migrations/legacy/0002_tasks.sql"),
    },
    Migration {
        version: "0003_run_history_tasks",
        sql: include_str!("../migrations/legacy/0003_run_history_tasks.sql"),
    },
    Migration {
        version: "0004_env_task_scope",
        sql: include_str!("../migrations/legacy/0004_env_task_scope.sql"),
    },
    Migration {
        version: "0005_app_folders",
        sql: include_str!("../migrations/legacy/0005_app_folders.sql"),
    },
    Migration {
        version: "0006_env_scope_unique",
        sql: include_str!("../migrations/legacy/0006_env_scope_unique.sql"),
    },
    Migration {
        version: "0007_app_autostart",
        sql: include_str!("../migrations/legacy/0007_app_autostart.sql"),
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_ordered_and_suffix_free() {
        let mut previous = "";
        for migration in MIGRATIONS {
            assert!(
                !migration.version.ends_with(".sql"),
                "version keys must match the Electron rows: {}",
                migration.version
            );
            assert!(
                migration.version > previous,
                "migrations must be sorted: {} after {}",
                migration.version,
                previous
            );
            previous = migration.version;
        }
        assert_eq!(MIGRATIONS.len(), 7);
        assert_eq!(MIGRATIONS[0].version, "0001_init");
        assert_eq!(MIGRATIONS[6].version, "0007_app_autostart");
    }

    #[test]
    fn sql_is_embedded_verbatim() {
        // Spot-check that the embedded text is the real file, not a stub.
        assert!(
            MIGRATIONS[0]
                .sql
                .contains("CREATE TABLE IF NOT EXISTS apps")
        );
        assert!(
            MIGRATIONS[1]
                .sql
                .contains("CREATE TABLE IF NOT EXISTS tasks")
        );
        assert!(
            MIGRATIONS[5]
                .sql
                .contains("CREATE UNIQUE INDEX uq_env_task")
        );
        assert!(
            MIGRATIONS[6]
                .sql
                .contains("ALTER TABLE apps ADD COLUMN auto_start")
        );
    }
}
