//! Catalog business rules on top of real SQLite: path normalisation, atomic creation,
//! task validation and folder operations.

use neohub_core::catalog::{AddTaskInput, CreateAppInput, CreateTaskSpec, UpdateTaskInput};
use neohub_core::{AppCatalog, CoreError};
use neohub_domain::{CommandKind, DomainError, NodeVersionPref, ReadinessSignal, TaskId};
use neohub_storage::{DataDir, Storage};

struct Fixture {
    _dir: tempfile::TempDir,
    storage: Storage,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let storage = Storage::open(DataDir::new(dir.path())).expect("storage");
        Fixture { _dir: dir, storage }
    }

    fn catalog(&self) -> AppCatalog<'_> {
        AppCatalog::new(&self.storage)
    }
}

fn script_task(name: &str, script: &str) -> CreateTaskSpec {
    CreateTaskSpec {
        name: name.into(),
        command_kind: Some(CommandKind::Script),
        script: Some(script.into()),
        custom_command: None,
        working_dir_override: None,
    }
}

#[test]
fn create_app_writes_the_app_its_tasks_and_its_env_in_one_go() {
    let fx = Fixture::new();
    let project = fx._dir.path().join("demo-web");
    std::fs::create_dir(&project).unwrap();

    let catalog = fx.catalog();
    let app = catalog
        .create_app(CreateAppInput {
            path: project.to_string_lossy().into_owned(),
            name: Some("demo-web".into()),
            node_version_pref: Some(NodeVersionPref::System),
            package_manager: None,
            default_script: Some("dev".into()),
            first_task: Some(script_task("web", "dev")),
            tasks: vec![script_task("worker", "start")],
            env_vars: vec![
                ("DEMO_FLAG".into(), "synthetic".into(), false),
                ("DEMO_SECRET".into(), "hunter2".into(), true),
            ],
        })
        .expect("create");

    assert_eq!(app.name, "demo-web");
    assert_eq!(app.node_version_pref, NodeVersionPref::System);
    // The stored path is the canonical realpath, not the caller's string.
    assert_eq!(
        app.path,
        std::fs::canonicalize(&project)
            .unwrap()
            .to_string_lossy()
            .into_owned()
    );

    let tasks = catalog.list_tasks(&app.id).unwrap();
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].name, "web");
    assert_eq!(tasks[1].name, "worker");
    assert_eq!(tasks[0].position, 0);
    assert_eq!(tasks[1].position, 1);

    let env = fx.storage.env_store().list_app(&app.id).unwrap();
    assert_eq!(env.len(), 2);
    assert!(env.iter().any(|v| v.key == "DEMO_SECRET" && v.is_secret));
}

#[test]
fn create_app_rolls_back_when_a_task_is_invalid() {
    let fx = Fixture::new();
    let project = fx._dir.path().join("rollback");
    std::fs::create_dir(&project).unwrap();

    let catalog = fx.catalog();
    let err = catalog
        .create_app(CreateAppInput {
            path: project.to_string_lossy().into_owned(),
            name: None,
            node_version_pref: None,
            package_manager: None,
            default_script: None,
            first_task: Some(script_task("dup", "dev")),
            tasks: vec![script_task("dup", "start")],
            env_vars: vec![],
        })
        .expect_err("duplicate task names must fail the whole create");
    assert!(
        matches!(
            err,
            CoreError::Domain(DomainError::DuplicateTaskName { .. })
        ),
        "unexpected error: {err}"
    );
    assert!(
        catalog.list_apps().unwrap().is_empty(),
        "no half-created app may remain"
    );
}

#[test]
fn create_app_refuses_a_missing_directory_and_a_duplicate_path() {
    let fx = Fixture::new();
    let catalog = fx.catalog();

    let missing = catalog
        .create_app(CreateAppInput {
            path: "/tmp/neohub-does-not-exist-xyz".into(),
            name: None,
            node_version_pref: None,
            package_manager: None,
            default_script: None,
            first_task: None,
            tasks: vec![],
            env_vars: vec![],
        })
        .expect_err("missing path");
    assert!(matches!(err_kind(missing), "PathMissing"));

    let project = fx._dir.path().join("twice");
    std::fs::create_dir(&project).unwrap();
    let input = || CreateAppInput {
        path: project.to_string_lossy().into_owned(),
        name: None,
        node_version_pref: None,
        package_manager: None,
        default_script: None,
        first_task: None,
        tasks: vec![],
        env_vars: vec![],
    };
    let first = catalog.create_app(input()).unwrap();
    let dup = catalog.create_app(input()).expect_err("duplicate");
    match dup {
        CoreError::Domain(DomainError::DuplicatePath { existing, .. }) => {
            assert_eq!(existing, first.id);
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn a_symlinked_path_deduplicates_against_its_real_target() {
    let fx = Fixture::new();
    let real = fx._dir.path().join("real-project");
    std::fs::create_dir(&real).unwrap();
    let link = fx._dir.path().join("link-project");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let catalog = fx.catalog();
    catalog
        .create_app(CreateAppInput {
            path: real.to_string_lossy().into_owned(),
            name: None,
            node_version_pref: None,
            package_manager: None,
            default_script: None,
            first_task: None,
            tasks: vec![],
            env_vars: vec![],
        })
        .unwrap();

    let err = catalog
        .create_app(CreateAppInput {
            path: link.to_string_lossy().into_owned(),
            name: None,
            node_version_pref: None,
            package_manager: None,
            default_script: None,
            first_task: None,
            tasks: vec![],
            env_vars: vec![],
        })
        .expect_err("a symlink to a registered project is the same project");
    assert!(matches!(
        err,
        CoreError::Domain(DomainError::DuplicatePath { .. })
    ));
}

#[test]
fn task_dependencies_reject_self_unknown_and_cycles() {
    let fx = Fixture::new();
    let project = fx._dir.path().join("deps");
    std::fs::create_dir(&project).unwrap();
    let catalog = fx.catalog();
    let app = catalog
        .create_app(CreateAppInput {
            path: project.to_string_lossy().into_owned(),
            name: None,
            node_version_pref: None,
            package_manager: None,
            default_script: None,
            first_task: Some(script_task("db", "db")),
            tasks: vec![],
            env_vars: vec![],
        })
        .unwrap();
    let db_task = catalog.list_tasks(&app.id).unwrap()[0].clone();

    // Unknown dependency.
    let err = catalog
        .add_task(
            &app.id,
            AddTaskInput {
                name: "api".into(),
                script: Some("api".into()),
                depends_on: vec![TaskId::from_existing("does-not-exist")],
                ..Default::default()
            },
        )
        .expect_err("unknown dependency");
    assert!(matches!(
        err,
        CoreError::Domain(DomainError::UnknownDependency { .. })
    ));

    // Valid dependency is accepted.
    let api = catalog
        .add_task(
            &app.id,
            AddTaskInput {
                name: "api".into(),
                script: Some("api".into()),
                depends_on: vec![db_task.id.clone()],
                ..Default::default()
            },
        )
        .expect("api task");

    // Cycle: make db depend on api.
    let err = catalog
        .update_task(
            &db_task.id,
            UpdateTaskInput {
                depends_on: Some(vec![api.id.clone()]),
                ..Default::default()
            },
        )
        .expect_err("cycle");
    assert!(matches!(
        err,
        CoreError::Domain(DomainError::DependencyCycle { .. })
    ));

    // A task depending on itself.
    let err = catalog
        .add_task(
            &app.id,
            AddTaskInput {
                name: "self".into(),
                script: Some("self".into()),
                depends_on: vec![api.id.clone()],
                ..Default::default()
            },
        )
        .expect("self task created without deps");
    let self_task = err;
    let err = catalog
        .update_task(
            &self_task.id,
            UpdateTaskInput {
                depends_on: Some(vec![self_task.id.clone()]),
                ..Default::default()
            },
        )
        .expect_err("self dependency");
    assert!(matches!(
        err,
        CoreError::Domain(DomainError::SelfDependency { .. })
    ));
}

#[test]
fn one_shot_tasks_must_use_exit_readiness() {
    let fx = Fixture::new();
    let project = fx._dir.path().join("oneshot");
    std::fs::create_dir(&project).unwrap();
    let catalog = fx.catalog();
    let app = catalog
        .create_app(CreateAppInput {
            path: project.to_string_lossy().into_owned(),
            name: None,
            node_version_pref: None,
            package_manager: None,
            default_script: None,
            first_task: None,
            tasks: vec![],
            env_vars: vec![],
        })
        .unwrap();

    let err = catalog
        .add_task(
            &app.id,
            AddTaskInput {
                name: "migrate".into(),
                custom_command: Some("npm run migrate".into()),
                command_kind: Some(CommandKind::Custom),
                one_shot: true,
                readiness: Some(ReadinessSignal::None),
                ..Default::default()
            },
        )
        .expect_err("one-shot without exit readiness");
    assert!(matches!(
        err,
        CoreError::Domain(DomainError::OneShotRequiresExitReadiness)
    ));

    let ok = catalog
        .add_task(
            &app.id,
            AddTaskInput {
                name: "migrate".into(),
                custom_command: Some("npm run migrate".into()),
                command_kind: Some(CommandKind::Custom),
                one_shot: true,
                readiness: Some(ReadinessSignal::Exit { code: Some(0) }),
                ..Default::default()
            },
        )
        .expect("one-shot with exit readiness");
    assert!(ok.one_shot);
}

#[test]
fn a_task_with_dependents_cannot_be_removed() {
    let fx = Fixture::new();
    let project = fx._dir.path().join("guard");
    std::fs::create_dir(&project).unwrap();
    let catalog = fx.catalog();
    let app = catalog
        .create_app(CreateAppInput {
            path: project.to_string_lossy().into_owned(),
            name: None,
            node_version_pref: None,
            package_manager: None,
            default_script: None,
            first_task: Some(script_task("db", "db")),
            tasks: vec![],
            env_vars: vec![],
        })
        .unwrap();
    let db = catalog.list_tasks(&app.id).unwrap()[0].clone();
    catalog
        .add_task(
            &app.id,
            AddTaskInput {
                name: "api".into(),
                script: Some("api".into()),
                depends_on: vec![db.id.clone()],
                ..Default::default()
            },
        )
        .unwrap();

    let err = catalog.remove_task(&db.id).expect_err("still required");
    match err {
        CoreError::Domain(DomainError::TaskHasDependents { name, dependents }) => {
            assert_eq!(name, "db");
            assert_eq!(dependents, vec!["api".to_string()]);
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn reorder_requires_every_task_exactly_once() {
    let fx = Fixture::new();
    let project = fx._dir.path().join("reorder");
    std::fs::create_dir(&project).unwrap();
    let catalog = fx.catalog();
    let app = catalog
        .create_app(CreateAppInput {
            path: project.to_string_lossy().into_owned(),
            name: None,
            node_version_pref: None,
            package_manager: None,
            default_script: None,
            first_task: Some(script_task("one", "one")),
            tasks: vec![script_task("two", "two")],
            env_vars: vec![],
        })
        .unwrap();
    let tasks = catalog.list_tasks(&app.id).unwrap();

    let err = catalog
        .reorder_tasks(&app.id, &[tasks[0].id.clone()])
        .expect_err("partial list");
    assert!(matches!(
        err,
        CoreError::Domain(DomainError::InvalidInput {
            field: "taskIds",
            ..
        })
    ));

    catalog
        .reorder_tasks(&app.id, &[tasks[1].id.clone(), tasks[0].id.clone()])
        .unwrap();
    let names: Vec<String> = catalog
        .list_tasks(&app.id)
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(names, vec!["two", "one"]);
}

#[test]
fn clearing_a_folder_never_deletes_the_project_directory() {
    let fx = Fixture::new();
    let project = fx._dir.path().join("keep-me");
    std::fs::create_dir(&project).unwrap();
    let catalog = fx.catalog();
    let app = catalog
        .create_app(CreateAppInput {
            path: project.to_string_lossy().into_owned(),
            name: None,
            node_version_pref: None,
            package_manager: None,
            default_script: None,
            first_task: None,
            tasks: vec![],
            env_vars: vec![],
        })
        .unwrap();
    catalog
        .update_app(
            &app.id,
            neohub_core::catalog::UpdateAppInput {
                folder: Some(Some("Synthetic".into())),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        catalog.list_folders().unwrap(),
        vec!["Synthetic".to_string()]
    );

    assert_eq!(catalog.clear_folder("synthetic").unwrap(), 1);
    assert!(catalog.list_folders().unwrap().is_empty());
    assert!(project.exists(), "the project directory is untouched");
    assert_eq!(catalog.get_app(&app.id).unwrap().path, app.path);
}

fn err_kind(err: CoreError) -> &'static str {
    match err {
        CoreError::Domain(DomainError::PathMissing { .. }) => "PathMissing",
        CoreError::Domain(DomainError::DuplicatePath { .. }) => "DuplicatePath",
        CoreError::Domain(DomainError::InvalidInput { .. }) => "InvalidInput",
        CoreError::Domain(_) => "other-domain",
        CoreError::Storage(_) => "storage",
    }
}
