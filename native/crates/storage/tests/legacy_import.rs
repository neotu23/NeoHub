//! Legacy compatibility: the Rust schema must be a drop-in continuation of the
//! Electron database — same migration keys, same backfill outcome, same cascade and
//! scope rules. Everything here runs against the embedded `0001`–`0007` SQL.

use neohub_domain::{AppId, EnvVar, NodeVersionPref, PackageManager, TaskId};
use neohub_storage::catalog::{NewApp, backfill_tasks_for_existing_apps};
use neohub_storage::env::new_var;
use neohub_storage::{DataDir, Storage};

fn new_app(id: &str, path: &str, script: Option<&str>, command: Option<&str>) -> NewApp {
    NewApp {
        id: AppId::from_existing(id),
        name: id.to_string(),
        path: path.to_string(),
        color: "#7aa2f7".into(),
        icon: None,
        node_version_pref: NodeVersionPref::Auto,
        package_manager: Some(PackageManager::Pnpm),
        default_script: script.map(str::to_owned),
        custom_command: command.map(str::to_owned),
        working_dir: path.to_string(),
        auto_restart_on_change: false,
        auto_start: false,
        watch_globs: vec![],
        port_hint: None,
        tags: vec![],
        folder: None,
        created_at: 1_700_000_000_000,
        updated_at: 1_700_000_000_000,
    }
}

#[test]
fn applies_all_seven_legacy_migrations_with_the_original_version_keys() {
    let storage = Storage::open_in_memory().expect("open");
    let mut stmt = storage
        .conn()
        .prepare("SELECT version FROM _schema_migrations ORDER BY version ASC")
        .unwrap();
    let versions: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        versions,
        vec![
            "0001_init",
            "0002_tasks",
            "0003_run_history_tasks",
            "0004_env_task_scope",
            "0005_app_folders",
            "0006_env_scope_unique",
            "0007_app_autostart",
        ],
        "version keys must match the Electron rows (no .sql suffix)"
    );
}

#[test]
fn reopening_an_already_migrated_database_is_idempotent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = DataDir::new(dir.path());
    {
        let storage = Storage::open(data_dir.clone()).expect("first open");
        storage
            .catalog()
            .insert_app(&new_app("app_1", "/tmp/project-one", Some("dev"), None))
            .expect("insert");
    }
    // Second open: migrations must be skipped, not re-run (old ALTER TABLEs would fail).
    let storage = Storage::open(data_dir).expect("second open");
    let apps = storage.catalog().list_apps().expect("list");
    assert_eq!(apps.len(), 1);
    let count: i64 = storage
        .conn()
        .query_row("SELECT COUNT(*) FROM _schema_migrations", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 7);
}

#[test]
fn backfill_gives_scriptless_apps_one_task_and_prefers_custom_commands() {
    let storage = Storage::open_in_memory().expect("open");
    let catalog = storage.catalog();
    catalog
        .insert_app(&new_app("app_script", "/tmp/script-app", Some("dev"), None))
        .unwrap();
    catalog
        .insert_app(&new_app(
            "app_custom",
            "/tmp/custom-app",
            Some("dev"),
            Some("node ./start.js"),
        ))
        .unwrap();
    catalog
        .insert_app(&new_app("app_empty", "/tmp/empty-app", None, None))
        .unwrap();

    let created = backfill_tasks_for_existing_apps(storage.conn()).expect("backfill");
    assert_eq!(created, 2, "only the two apps with a command get a task");

    let script_app_tasks = catalog
        .list_tasks(&AppId::from_existing("app_script"))
        .unwrap();
    assert_eq!(script_app_tasks.len(), 1);
    assert_eq!(script_app_tasks[0].name, "dev");
    assert_eq!(script_app_tasks[0].script.as_deref(), Some("dev"));
    assert_eq!(script_app_tasks[0].custom_command, None);
    assert_eq!(script_app_tasks[0].position, 0);

    let custom_app_tasks = catalog
        .list_tasks(&AppId::from_existing("app_custom"))
        .unwrap();
    assert_eq!(
        custom_app_tasks[0].custom_command.as_deref(),
        Some("node ./start.js"),
        "a custom command wins over the default script"
    );
    assert_eq!(custom_app_tasks[0].name, "main");
    assert_eq!(
        custom_app_tasks[0].readiness.kind(),
        "none",
        "backfilled tasks use readiness 'none'"
    );

    assert!(
        catalog
            .list_tasks(&AppId::from_existing("app_empty"))
            .unwrap()
            .is_empty(),
        "an app with neither command is left for the user to configure"
    );

    // Idempotent: a second run creates nothing.
    assert_eq!(backfill_tasks_for_existing_apps(storage.conn()).unwrap(), 0);
}

#[test]
fn the_three_env_scopes_may_reuse_the_same_key() {
    let storage = Storage::open_in_memory().expect("open");
    let app_id = AppId::from_existing("app_env");
    storage
        .catalog()
        .insert_app(&new_app("app_env", "/tmp/env-app", Some("dev"), None))
        .unwrap();
    let task_id = TaskId::from_existing("task_env");
    storage
        .catalog()
        .insert_task(&neohub_storage::NewTask {
            id: task_id.clone(),
            app_id: app_id.clone(),
            name: "dev".into(),
            position: 0,
            command_kind: neohub_domain::CommandKind::Script,
            script: Some("dev".into()),
            custom_command: None,
            working_dir_override: None,
            package_manager_override: None,
            node_version_pref_override: None,
            depends_on: vec![],
            readiness: neohub_domain::ReadinessSignal::None,
            one_shot: false,
            enabled: true,
            env_overrides: Default::default(),
            created_at: 1,
            updated_at: 1,
        })
        .unwrap();

    let mut env = storage.env_store();
    let now = 1_700_000_000_000;
    // Same key at all three scopes: migration 0006 replaced the unusable table-level
    // UNIQUE(app_id, key) with three partial indexes, so this must save cleanly.
    env.set_global(&[new_var(None, None, "API_URL", "global", false)], now)
        .expect("global scope");
    env.set_app(
        &app_id,
        &[new_var(Some(app_id.clone()), None, "API_URL", "app", false)],
        now,
    )
    .expect("app scope");
    env.set_task(
        Some(&app_id),
        &task_id,
        &[new_var(
            Some(app_id.clone()),
            Some(task_id.clone()),
            "API_URL",
            "task",
            false,
        )],
        now,
    )
    .expect("task scope");

    assert_eq!(env.list_global().unwrap()[0].value, "global");
    assert_eq!(env.list_app(&app_id).unwrap()[0].value, "app");
    assert_eq!(env.list_task(&task_id).unwrap()[0].value, "task");
    assert_eq!(
        env.list_app(&app_id).unwrap().len(),
        1,
        "app scope is isolated"
    );
}

#[test]
fn deleting_an_app_cascades_to_its_tasks_env_and_history() {
    let storage = Storage::open_in_memory().expect("open");
    let app_id = AppId::from_existing("app_cascade");
    storage
        .catalog()
        .insert_app(&new_app("app_cascade", "/tmp/cascade", Some("dev"), None))
        .unwrap();
    let task_id = TaskId::from_existing("task_cascade");
    storage
        .catalog()
        .insert_task(&neohub_storage::NewTask {
            id: task_id.clone(),
            app_id: app_id.clone(),
            name: "dev".into(),
            position: 0,
            command_kind: neohub_domain::CommandKind::Script,
            script: Some("dev".into()),
            custom_command: None,
            working_dir_override: None,
            package_manager_override: None,
            node_version_pref_override: None,
            depends_on: vec![],
            readiness: neohub_domain::ReadinessSignal::None,
            one_shot: false,
            enabled: true,
            env_overrides: Default::default(),
            created_at: 1,
            updated_at: 1,
        })
        .unwrap();
    storage
        .env_store()
        .set_app(
            &app_id,
            &[EnvVar {
                is_secret: false,
                ..new_var(Some(app_id.clone()), None, "KEY", "value", false)
            }],
            1,
        )
        .unwrap();
    storage
        .history()
        .start_run(&neohub_domain::RunHistoryRow {
            id: "run_cascade".into(),
            app_id: app_id.clone(),
            task_id: Some(task_id.clone()),
            task_name: Some("dev".into()),
            script: Some("dev".into()),
            custom_command: None,
            node_version: Some("v22.22.3".into()),
            package_manager: Some(PackageManager::Pnpm),
            started_at: 1,
            ended_at: None,
            exit_code: None,
            exit_signal: None,
            was_killed_by_user: false,
        })
        .unwrap();

    storage.catalog().remove_app(&app_id).unwrap();

    let counts: (i64, i64, i64) = storage
        .conn()
        .query_row(
            "SELECT (SELECT COUNT(*) FROM tasks), (SELECT COUNT(*) FROM env_vars),
                    (SELECT COUNT(*) FROM run_history)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(counts, (0, 0, 0), "FK cascade must clean every child row");
}

#[test]
fn legacy_run_history_rows_keep_a_nullable_task_id() {
    let storage = Storage::open_in_memory().expect("open");
    let app_id = AppId::from_existing("app_hist");
    storage
        .catalog()
        .insert_app(&new_app("app_hist", "/tmp/hist", Some("dev"), None))
        .unwrap();
    storage
        .history()
        .start_run(&neohub_domain::RunHistoryRow {
            id: "run_legacy".into(),
            app_id: app_id.clone(),
            task_id: None,
            task_name: None,
            script: None,
            custom_command: Some("legacy command".into()),
            node_version: None,
            package_manager: None,
            started_at: 42,
            ended_at: Some(43),
            exit_code: Some(0),
            exit_signal: None,
            was_killed_by_user: false,
        })
        .unwrap();

    let rows = storage.history().list_for_app(&app_id, Some(10)).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].task_id, None);
    assert_eq!(rows[0].exit_code, Some(0));
    assert!(rows[0].task_name.is_none());
}

#[test]
fn missing_columns_from_older_databases_are_added_by_the_migrations() {
    // Simulates an app registered before 0005/0007: the folder and auto_start columns
    // arrive from the migrations, so reading them must work with their defaults.
    let storage = Storage::open_in_memory().expect("open");
    storage
        .catalog()
        .insert_app(&new_app("app_old", "/tmp/old-app", Some("dev"), None))
        .unwrap();
    let app = storage
        .catalog()
        .get_app(&AppId::from_existing("app_old"))
        .unwrap()
        .expect("app");
    assert_eq!(app.folder, None, "0005 default: ungrouped");
    assert!(!app.auto_start, "0007 default: no auto start");
    assert_eq!(app.node_version_pref, NodeVersionPref::Auto);
}
