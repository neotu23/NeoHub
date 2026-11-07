//! Migration atomicy and idempotence, on synthetic legacy databases only.
//!
//! The properties under test are the ones that make importing a user's real database safe:
//! a new target is published with one rename, a failed import leaves nothing behind, and a
//! repeat import never overwrites rows edited in the native build afterwards.

use neohub_storage::import::{copy_source, import_legacy, inspect};
use neohub_storage::{DataDir, Storage};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// Build a legacy database the way the Electron build would have left it.
fn legacy(dir: &Path, apps: usize, corrupt_json: bool) -> PathBuf {
    let path = dir.join("devharbor.db");
    let conn = Connection::open(&path).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    neohub_storage::db::apply_migrations(&conn).unwrap();
    for index in 0..apps {
        conn.execute(
            "INSERT INTO apps (id, name, path, color, node_version_pref, package_manager,
                               default_script, working_dir, auto_restart_on_change, auto_start,
                               watch_globs, tags, folder, created_at, updated_at)
             VALUES (?1, ?2, ?3, '#fff', '{\"kind\":\"auto\"}', 'pnpm', 'dev', ?3, 0, 1,
                     ?4, ?5, 'Imported', 10, 10)",
            rusqlite::params![
                format!("app{index}"),
                format!("demo-{index}"),
                format!("/tmp/demo-{index}"),
                if corrupt_json {
                    "{broken".to_string()
                } else {
                    "[]".to_string()
                },
                "[\"web\"]",
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tasks (id, app_id, name, position, command_kind, script, depends_on,
                                readiness, one_shot, enabled, env_overrides, created_at, updated_at)
             VALUES (?1, ?2, 'dev', 0, 'script', 'dev', '[]', '{\"kind\":\"port\",\"port\":4319}',
                     0, 1, '{}', 10, 10)",
            rusqlite::params![format!("task{index}"), format!("app{index}")],
        )
        .unwrap();
    }
    conn.execute(
        "INSERT INTO env_vars (id, app_id, task_id, key, value, enabled, is_secret, note, created_at, updated_at)
         VALUES ('env1','app0',NULL,'DB_PASSWORD','enc1:v10:cipher','1',1,'legacy',10,10)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value)
         VALUES ('theme','dark'), ('mcp_enabled','1'), ('future_key','x')",
        [],
    )
    .unwrap();
    path
}

fn import_from(source_dir: &Path, target_dir: &Path) -> neohub_storage::import::MigrationReport {
    let staging = tempfile::tempdir().unwrap();
    let copied = copy_source(source_dir, staging.path()).expect("copy");
    let preview = inspect(&copied, source_dir, false).expect("inspect");
    import_legacy(&copied, &preview, target_dir)
        .expect("import")
        .report
}

#[test]
fn a_first_import_publishes_the_whole_database_and_pauses_automation() {
    let source = tempfile::tempdir().unwrap();
    legacy(source.path(), 2, false);
    let target = tempfile::tempdir().unwrap();
    let target_dir = target.path().join("NeoHub-Rust");

    let report = import_from(source.path(), &target_dir);
    assert_eq!(report.imported.apps, 2);
    assert_eq!(report.imported.tasks, 2);
    assert_eq!(report.imported.env_vars, 1);
    assert!(report.paused_automation);
    assert_eq!(
        report.secrets_unavailable, 1,
        "the legacy enc1: secret is counted as needing re-entry"
    );
    assert_eq!(report.unknown_settings, vec!["future_key".to_string()]);
    assert!(
        target_dir.join("devharbor.db").is_file(),
        "database published"
    );
    assert!(
        !target_dir.join(".publish-staging").exists(),
        "staging directory is consumed by the rename"
    );

    // The imported rows keep their ids, folder, tags and secret ciphertext.
    let storage = Storage::open(DataDir::new(&target_dir)).expect("open");
    let apps = storage.catalog().list_apps().expect("apps");
    assert_eq!(apps.len(), 2);
    assert!(
        apps.iter()
            .all(|app| app.folder.as_deref() == Some("Imported"))
    );
    assert!(
        apps.iter().all(|app| !app.auto_start),
        "autoStart is paused"
    );
    assert!(apps.iter().all(|app| app.tags == vec!["web".to_string()]));
    let env = storage.env_store().list_app(&apps[0].id).expect("env");
    assert_eq!(env.len(), 1);
    assert_eq!(env[0].value, "enc1:v10:cipher", "ciphertext is preserved");
    assert!(env[0].is_secret);
    let settings = storage.settings().load().expect("settings");
    assert!(!settings.mcp_enabled, "MCP is paused for verification");
    assert_eq!(
        settings.theme,
        neohub_domain::ThemeSetting::Dark,
        "known settings are imported"
    );
}

#[test]
fn importing_the_same_source_twice_imports_nothing_the_second_time() {
    let source = tempfile::tempdir().unwrap();
    legacy(source.path(), 1, false);
    let target = tempfile::tempdir().unwrap();
    let target_dir = target.path().join("NeoHub-Rust");

    let first = import_from(source.path(), &target_dir);
    assert_eq!(first.imported.apps, 1);
    let second = import_from(source.path(), &target_dir);
    assert_eq!(second.imported.apps, 0, "repeat import adds nothing");
    assert_eq!(second.skipped.apps, 1);
    assert!(second.is_repeat());

    let storage = Storage::open(DataDir::new(&target_dir)).expect("open");
    assert_eq!(storage.catalog().list_apps().unwrap().len(), 1);
}

#[test]
fn a_native_edit_after_the_import_is_not_overwritten_by_a_repeat() {
    let source = tempfile::tempdir().unwrap();
    legacy(source.path(), 1, false);
    let target = tempfile::tempdir().unwrap();
    let target_dir = target.path().join("NeoHub-Rust");
    import_from(source.path(), &target_dir);

    // The user renames the app in the native build.
    {
        let storage = Storage::open(DataDir::new(&target_dir)).expect("open");
        let app = storage.catalog().list_apps().unwrap().remove(0);
        let mut patch = neohub_storage::AppPatch {
            name: "renamed-natively".into(),
            color: app.color.clone(),
            icon: None,
            node_version_pref: app.node_version_pref.clone(),
            package_manager: app.package_manager,
            default_script: app.default_script.clone(),
            custom_command: None,
            working_dir: app.working_dir.clone(),
            auto_restart_on_change: false,
            auto_start: false,
            watch_globs: vec![],
            port_hint: None,
            tags: vec![],
            folder: app.folder.clone(),
            last_started_at: None,
            last_exit_code: None,
            updated_at: 99,
        };
        patch.updated_at = 99;
        storage
            .catalog()
            .update_app(&app.id, &patch)
            .expect("rename");
    }

    import_from(source.path(), &target_dir);
    let storage = Storage::open(DataDir::new(&target_dir)).expect("open");
    let app = storage.catalog().list_apps().unwrap().remove(0);
    assert_eq!(
        app.name, "renamed-natively",
        "a repeat import must not revert native edits"
    );
}

#[test]
fn corrupt_json_columns_do_not_break_the_import() {
    let source = tempfile::tempdir().unwrap();
    legacy(source.path(), 1, true);
    let target = tempfile::tempdir().unwrap();
    let target_dir = target.path().join("NeoHub-Rust");
    let report = import_from(source.path(), &target_dir);
    assert_eq!(report.imported.apps, 1);

    let storage = Storage::open(DataDir::new(&target_dir)).expect("open");
    let app = storage.catalog().list_apps().unwrap().remove(0);
    assert!(
        app.watch_globs.is_empty(),
        "a corrupt watch_globs column degrades to empty, not a crash"
    );
    assert_eq!(
        app.tags,
        vec!["web".to_string()],
        "the intact tags column still imports"
    );
    assert_eq!(app.folder.as_deref(), Some("Imported"));
}

#[test]
fn a_failed_publish_leaves_no_database_behind() {
    let source = tempfile::tempdir().unwrap();
    legacy(source.path(), 1, false);
    let staging = tempfile::tempdir().unwrap();
    let copied = copy_source(source.path(), staging.path()).expect("copy");
    let preview = inspect(&copied, source.path(), false).expect("inspect");

    // The target "directory" is a file, so creating/publishing it must fail cleanly.
    let blocked = tempfile::tempdir().unwrap();
    let target_dir = blocked.path().join("not-a-directory");
    std::fs::write(&target_dir, b"i am a file").unwrap();
    let result = import_legacy(&copied, &preview, &target_dir);
    assert!(result.is_err(), "publishing into a file path must fail");
    assert!(
        target_dir.is_file(),
        "the existing file is not replaced or deleted"
    );
}

#[test]
fn the_source_database_is_never_modified() {
    let source = tempfile::tempdir().unwrap();
    let db = legacy(source.path(), 1, false);
    let before = neohub_storage::import::file_sha256(&db).expect("hash");
    let target = tempfile::tempdir().unwrap();
    import_from(source.path(), &target.path().join("NeoHub-Rust"));
    let after = neohub_storage::import::file_sha256(&db).expect("hash");
    assert_eq!(
        before, after,
        "the legacy database is byte-identical afterwards"
    );
}

#[test]
fn importing_works_when_the_callers_copy_sits_in_the_target_directory() {
    // The CLI copies the legacy database to `<target>/.import-staging` and then imports from
    // there. The publishing step used to reuse that same directory name and delete the very
    // copy it was about to read, which surfaced as "unable to open database file".
    let dir = tempfile::tempdir().unwrap();
    // The fixture writes devharbor.db into `dir` and returns that file path; the copy step takes
    // the directory that holds it.
    let db_file = legacy(dir.path(), 2, false);
    let source_dir = db_file.parent().expect("source dir").to_path_buf();
    let target = dir.path().join("target");

    // Same shape as the CLI: copy first, inspect the copy, then import from it.
    let copy_dir = target.join(".import-staging");
    let copied = copy_source(&source_dir, &copy_dir).expect("copy");
    let preview = inspect(&copied, &source_dir, false).expect("inspect");
    let outcome = import_legacy(&copied, &preview, &target).expect("import from the caller's copy");

    assert!(outcome.database.exists());
    assert_eq!(outcome.report.imported.apps, 2, "both apps landed");
    assert!(
        !target.join(".publish-staging").exists(),
        "the publish staging is cleaned up"
    );
}
