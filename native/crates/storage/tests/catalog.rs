//! SQL-layer behaviour: uniqueness, ordering, folder operations and defensive JSON reads.

use neohub_domain::{AppId, CommandKind, NodeVersionPref, PackageManager, ReadinessSignal, TaskId};
use neohub_storage::catalog::{AppPatch, NewApp, NewTask};
use neohub_storage::{Storage, StorageError};

fn app(id: &str, path: &str) -> NewApp {
    NewApp {
        id: AppId::from_existing(id),
        name: id.to_string(),
        path: path.to_string(),
        color: "#7aa2f7".into(),
        icon: None,
        node_version_pref: NodeVersionPref::Auto,
        package_manager: Some(PackageManager::Npm),
        default_script: Some("dev".into()),
        custom_command: None,
        working_dir: path.to_string(),
        auto_restart_on_change: false,
        auto_start: false,
        watch_globs: vec![],
        port_hint: None,
        tags: vec![],
        folder: None,
        created_at: 1,
        updated_at: 1,
    }
}

fn task(id: &str, app_id: &str, name: &str, position: i64) -> NewTask {
    NewTask {
        id: TaskId::from_existing(id),
        app_id: AppId::from_existing(app_id),
        name: name.to_string(),
        position,
        command_kind: CommandKind::Script,
        script: Some(name.to_string()),
        custom_command: None,
        working_dir_override: None,
        package_manager_override: None,
        node_version_pref_override: None,
        depends_on: vec![],
        readiness: ReadinessSignal::None,
        one_shot: false,
        enabled: true,
        env_overrides: Default::default(),
        created_at: 1,
        updated_at: 1,
    }
}

#[test]
fn a_path_can_only_be_registered_once() {
    let storage = Storage::open_in_memory().unwrap();
    storage
        .catalog()
        .insert_app(&app("a1", "/tmp/dup"))
        .unwrap();
    let err = storage
        .catalog()
        .insert_app(&app("a2", "/tmp/dup"))
        .expect_err("duplicate path must be refused");
    match err {
        StorageError::DuplicatePath { path, existing } => {
            assert_eq!(path, "/tmp/dup");
            assert_eq!(existing, "a1");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn tasks_come_back_in_position_order_and_reorder_rewrites_it() {
    let storage = Storage::open_in_memory().unwrap();
    storage
        .catalog()
        .insert_app(&app("a1", "/tmp/order"))
        .unwrap();
    let cat = storage.catalog();
    cat.insert_task(&task("t3", "a1", "third", 2)).unwrap();
    cat.insert_task(&task("t1", "a1", "first", 0)).unwrap();
    cat.insert_task(&task("t2", "a1", "second", 1)).unwrap();

    let app_id = AppId::from_existing("a1");
    let names: Vec<String> = cat
        .list_tasks(&app_id)
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(names, vec!["first", "second", "third"]);

    cat.reorder_tasks(
        &app_id,
        &[
            TaskId::from_existing("t3"),
            TaskId::from_existing("t1"),
            TaskId::from_existing("t2"),
        ],
        99,
    )
    .unwrap();
    let names: Vec<String> = cat
        .list_tasks(&app_id)
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(names, vec!["third", "first", "second"]);
}

#[test]
fn a_corrupt_json_cell_degrades_to_its_default_instead_of_failing_the_query() {
    let storage = Storage::open_in_memory().unwrap();
    storage
        .catalog()
        .insert_app(&app("a1", "/tmp/corrupt"))
        .unwrap();
    // Simulate a hand-edited / truncated cell.
    storage
        .conn()
        .execute(
            "UPDATE apps SET tags = '{not json', watch_globs = 'also broken' WHERE id = 'a1'",
            [],
        )
        .unwrap();

    let apps = storage.catalog().list_apps().expect("list must not fail");
    assert_eq!(apps.len(), 1);
    assert!(apps[0].tags.is_empty());
    assert!(apps[0].watch_globs.is_empty());
}

#[test]
fn folder_rename_and_clear_only_touch_grouping() {
    let storage = Storage::open_in_memory().unwrap();
    storage.catalog().insert_app(&app("a1", "/tmp/f1")).unwrap();
    storage.catalog().insert_app(&app("a2", "/tmp/f2")).unwrap();
    let cat = storage.catalog();

    let mut patch = AppPatch {
        name: "a1".into(),
        color: "#fff".into(),
        icon: None,
        node_version_pref: NodeVersionPref::Auto,
        package_manager: None,
        default_script: None,
        custom_command: None,
        working_dir: "/tmp/f1".into(),
        auto_restart_on_change: false,
        auto_start: false,
        watch_globs: vec![],
        port_hint: None,
        tags: vec![],
        folder: Some("Client Work".into()),
        last_started_at: None,
        last_exit_code: None,
        updated_at: 2,
    };
    cat.update_app(&AppId::from_existing("a1"), &patch).unwrap();
    patch.name = "a2".into();
    patch.working_dir = "/tmp/f2".into();
    patch.folder = Some("Client work".into()); // same folder, different case
    cat.update_app(&AppId::from_existing("a2"), &patch).unwrap();

    // DISTINCT is case-sensitive (same as the legacy query), so both spellings exist as
    // separate folder names until a rename folds them together.
    assert_eq!(
        cat.list_folders().unwrap(),
        vec!["Client Work".to_string(), "Client work".to_string()]
    );

    // Case-insensitive rename folds both rows into the new name.
    assert_eq!(
        cat.rename_folder("client work", Some("Archive"), 3)
            .unwrap(),
        2
    );
    assert_eq!(cat.list_folders().unwrap(), vec!["Archive".to_string()]);

    // Clearing moves apps to "(Ungrouped)" (NULL) without touching their path.
    assert_eq!(cat.clear_folder("archive", 4).unwrap(), 2);
    assert!(cat.list_folders().unwrap().is_empty());
    let a1 = cat.get_app(&AppId::from_existing("a1")).unwrap().unwrap();
    assert_eq!(a1.folder, None);
    assert_eq!(a1.path, "/tmp/f1", "the project path is untouched");
}

#[test]
fn dependents_of_finds_the_tasks_that_would_break() {
    let storage = Storage::open_in_memory().unwrap();
    storage
        .catalog()
        .insert_app(&app("a1", "/tmp/deps"))
        .unwrap();
    let cat = storage.catalog();
    cat.insert_task(&task("t1", "a1", "db", 0)).unwrap();
    let mut dependent = task("t2", "a1", "api", 1);
    dependent.depends_on = vec![TaskId::from_existing("t1")];
    cat.insert_task(&dependent).unwrap();

    let dependents = cat
        .dependents_of(&AppId::from_existing("a1"), &TaskId::from_existing("t1"))
        .unwrap();
    assert_eq!(dependents.len(), 1);
    assert_eq!(dependents[0].name, "api");
}
