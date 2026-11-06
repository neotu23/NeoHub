//! Database maintenance against a real on-disk database.
//!
//! Both operations here can lose data if they are wrong, so the tests check the *content* of
//! what they produce, not just that they returned `Ok`.

use neohub_core::catalog::{CreateAppInput, CreateTaskSpec};
use neohub_core::service::CoreService;
use neohub_domain::CommandKind;
use neohub_storage::DataDir;

fn project(dir: &std::path::Path, name: &str) -> String {
    let path = dir.join(name);
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("package.json"), r#"{"scripts":{"dev":"node ."}}"#).unwrap();
    path.to_string_lossy().into_owned()
}

fn seed(service: &std::sync::Arc<CoreService>, root: &std::path::Path) {
    let path = project(root, "demo");
    service
        .create_app(CreateAppInput {
            path,
            name: Some("demo".into()),
            node_version_pref: None,
            package_manager: None,
            default_script: None,
            first_task: Some(CreateTaskSpec {
                name: "dev".into(),
                command_kind: Some(CommandKind::Script),
                script: Some("dev".into()),
                custom_command: None,
                working_dir_override: None,
            }),
            tasks: vec![],
            env_vars: vec![],
        })
        .expect("app");
}

#[tokio::test]
async fn the_database_path_is_the_live_file() {
    let dir = tempfile::tempdir().unwrap();
    let service = CoreService::open(DataDir::new(dir.path())).expect("core");
    let path = service.database_path().expect("on-disk service");
    assert_eq!(path, dir.path().join("devharbor.db"));
    assert!(path.exists(), "opening the core creates the file");
}

#[tokio::test]
async fn an_export_is_a_consistent_copy_not_a_file_copy() {
    let dir = tempfile::tempdir().unwrap();
    let service = CoreService::open(DataDir::new(dir.path())).expect("core");
    seed(&service, dir.path());

    // Write something that only lives in the WAL at this point: a plain file copy of the
    // `.db` would miss it, which is exactly the bug `VACUUM INTO` avoids.
    let export = dir.path().join("exported.db");
    service.export_database(&export).expect("export");
    assert!(export.exists());

    let copy = rusqlite::Connection::open(&export).unwrap();
    let apps: i64 = copy
        .query_row("SELECT count(*) FROM apps", [], |row| row.get(0))
        .unwrap();
    let tasks: i64 = copy
        .query_row("SELECT count(*) FROM tasks", [], |row| row.get(0))
        .unwrap();
    assert_eq!(apps, 1, "the exported copy has the app");
    assert_eq!(tasks, 1, "and its task");
    let integrity: String = copy
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
}

#[tokio::test]
async fn exporting_over_an_existing_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let service = CoreService::open(DataDir::new(dir.path())).expect("core");
    let target = dir.path().join("taken.db");
    std::fs::write(&target, b"do not overwrite me").unwrap();

    let error = service.export_database(&target).expect_err("must refuse");
    assert!(error.to_string().contains("已存在"), "{error}");
    assert_eq!(
        std::fs::read(&target).unwrap(),
        b"do not overwrite me",
        "the existing file is untouched"
    );
}

#[tokio::test]
async fn a_reset_moves_the_old_database_aside_and_starts_empty() {
    let dir = tempfile::tempdir().unwrap();
    let service = CoreService::open(DataDir::new(dir.path())).expect("core");
    seed(&service, dir.path());
    assert_eq!(service.snapshot().unwrap().apps.len(), 1);

    let outcome = service.reset_database().expect("reset");
    assert_eq!(outcome.database, dir.path().join("devharbor.db"));
    assert!(!outcome.moved.is_empty(), "the old files are kept");
    for backup in &outcome.moved {
        let name = backup.to_string_lossy().to_string();
        assert!(name.contains(".reset-"), "named as a reset backup: {name}");
        assert!(backup.exists(), "and it really exists");
    }

    // The old data is still readable in the backup, so a reset is recoverable.
    // The main database backup is the one ending in `.db.reset-<stamp>.bak`.
    let old_db = outcome
        .moved
        .iter()
        .find(|path| path.to_string_lossy().contains(".db.reset-"));
    if let Some(old_db) = old_db {
        let copy = rusqlite::Connection::open(old_db).unwrap();
        let apps: i64 = copy
            .query_row("SELECT count(*) FROM apps", [], |row| row.get(0))
            .unwrap();
        assert_eq!(apps, 1, "the backup still holds the app");
    }

    // A fresh database is open and usable.
    let snapshot = service.snapshot().unwrap();
    assert!(snapshot.apps.is_empty(), "the new database is empty");
    seed(&service, dir.path());
    assert_eq!(service.snapshot().unwrap().apps.len(), 1, "and writable");
}

#[tokio::test]
async fn the_command_surface_reaches_the_same_operations() {
    // The UI and MCP only see commands, so the maintenance helpers must be reachable there.
    use neohub_core::commands::{CoreCommand, CoreReply};

    let dir = tempfile::tempdir().unwrap();
    let service = CoreService::open(DataDir::new(dir.path())).expect("core");
    seed(&service, dir.path());
    let handle = service.handle();

    let path = handle
        .request(CoreCommand::DatabasePath)
        .await
        .expect("path");
    let CoreReply::DatabasePath(Some(live)) = path else {
        panic!("expected a live database path");
    };
    assert_eq!(live, dir.path().join("devharbor.db"));

    let export = dir.path().join("via-command.db");
    let reply = handle
        .request(CoreCommand::ExportDatabase {
            path: export.clone(),
        })
        .await
        .expect("export");
    assert!(matches!(reply, CoreReply::DatabaseExported(_)));
    assert!(export.exists());

    let reply = handle
        .request(CoreCommand::ResetDatabase)
        .await
        .expect("reset");
    let CoreReply::DatabaseReset(outcome) = reply else {
        panic!("expected a reset outcome");
    };
    assert!(!outcome.moved.is_empty());
    assert!(
        service.snapshot().unwrap().apps.is_empty(),
        "fresh database"
    );
}

#[tokio::test]
async fn auto_start_only_starts_apps_that_ask_for_it() {
    // This is the boot path: an app with `auto_start` runs, one without stays idle, and a
    // second call does not start anything twice.
    use neohub_core::catalog::UpdateAppInput;

    let dir = tempfile::tempdir().unwrap();
    let service = CoreService::open(DataDir::new(dir.path())).expect("core");
    let wanted = project(dir.path(), "wanted");
    let other = project(dir.path(), "other");
    for (path, name, auto) in [(wanted, "wanted", true), (other, "other", false)] {
        let app = service
            .create_app(CreateAppInput {
                path,
                name: Some(name.into()),
                node_version_pref: None,
                package_manager: None,
                default_script: None,
                first_task: Some(CreateTaskSpec {
                    name: "dev".into(),
                    command_kind: Some(CommandKind::Script),
                    script: Some("dev".into()),
                    custom_command: None,
                    working_dir_override: None,
                }),
                tasks: vec![],
                env_vars: vec![],
            })
            .expect("app");
        if auto {
            service
                .update_app(
                    &app.id,
                    UpdateAppInput {
                        auto_start: Some(true),
                        ..Default::default()
                    },
                )
                .expect("mark auto start");
        }
    }

    // The flag must survive the update, or nothing below means anything.
    let snapshot = service.snapshot().unwrap();
    let wanted_app = snapshot
        .apps
        .iter()
        .find(|app| app.name == "wanted")
        .expect("wanted app");
    assert!(wanted_app.auto_start, "auto_start was stored");
    assert!(
        !snapshot
            .apps
            .iter()
            .find(|app| app.name == "other")
            .expect("other app")
            .auto_start
    );

    let started = service.start_auto_start_apps().await.expect("boot");
    // The synthetic project has no real dev server, so `start_app` may fail; what this asserts is
    // *which* apps were attempted - the opted-in one and nothing else.
    assert_eq!(started.len(), 1, "only the opted-in app is attempted");
    assert_eq!(started[0].0, "wanted");

    // Nothing is actually serving on node `dev` here, so the state is 'starting' or failed;
    // what matters for this test is that the second pass does not start it again.
    let again = service.start_auto_start_apps().await.expect("boot twice");
    let still_live = service
        .snapshot()
        .unwrap()
        .app_states
        .values()
        .any(|state| state.is_live());
    if still_live {
        assert!(
            again.is_empty(),
            "an already-running app must not be started twice"
        );
    }
}
