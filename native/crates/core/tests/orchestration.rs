//! App-level orchestration against real fixture processes: layered start, readiness,
//! rollback of only-this-start tasks, reverse stop, disabled tasks and restart.

use neohub_core::catalog::{AddTaskInput, CreateAppInput as Input, CreateTaskSpec};
use neohub_core::commands::CoreCommand;
use neohub_core::service::CoreService;
use neohub_domain::{AppId, CommandKind, ProcessState, ReadinessSignal, TaskId};
use neohub_storage::DataDir;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../test-fixtures/process")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

struct Harness {
    _dir: tempfile::TempDir,
    service: std::sync::Arc<CoreService>,
}

impl Harness {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let service = CoreService::open(DataDir::new(dir.path())).expect("service");
        Harness { _dir: dir, service }
    }

    /// Create an app with one custom-command task per entry, then add extra tasks.
    fn make_app(&self, name: &str, first_task: CreateTaskSpec) -> AppId {
        let project = self._dir.path().join(name);
        std::fs::create_dir_all(&project).unwrap();
        let path = project.to_string_lossy().into_owned();
        self.service
            .storage_call(|storage| {
                let catalog = neohub_core::AppCatalog::new(storage);
                catalog
                    .create_app(Input {
                        path,
                        name: Some(name.to_string()),
                        node_version_pref: None,
                        package_manager: None,
                        default_script: None,
                        first_task: Some(first_task),
                        tasks: vec![],
                        env_vars: vec![],
                    })
                    .expect("create app")
                    .id
            })
            .expect("storage open")
    }

    fn add_task(
        &self,
        app: &AppId,
        name: &str,
        command: &str,
        readiness: ReadinessSignal,
        depends_on: Vec<TaskId>,
        enabled: bool,
    ) -> TaskId {
        self.service
            .storage_call(|storage| {
                let catalog = neohub_core::AppCatalog::new(storage);
                catalog
                    .add_task(
                        app,
                        AddTaskInput {
                            name: name.to_string(),
                            command_kind: Some(CommandKind::Custom),
                            custom_command: Some(command.to_string()),
                            depends_on,
                            readiness: Some(readiness),
                            enabled: Some(enabled),
                            ..Default::default()
                        },
                    )
                    .expect("add task")
                    .id
            })
            .expect("storage open")
    }
}

fn custom(command: String) -> CreateTaskSpec {
    CreateTaskSpec {
        name: "db".into(),
        command_kind: Some(CommandKind::Custom),
        script: None,
        custom_command: Some(command),
        working_dir_override: None,
    }
}

async fn wait_state(
    service: &CoreService,
    task: &TaskId,
    want: ProcessState,
    timeout: Duration,
) -> ProcessState {
    let deadline = Instant::now() + timeout;
    loop {
        let state = service.runner().state_of(task);
        if state == want || Instant::now() >= deadline {
            return state;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn a_layered_start_brings_up_every_task_in_the_graph() {
    let harness = Harness::new();
    // db: log readiness on a line the fixture prints immediately.
    let app = harness.make_app(
        "layered",
        custom(format!("sh {}", fixture("steady-log.sh"))),
    );
    let db = harness
        .service
        .storage_call(|storage| {
            neohub_core::AppCatalog::new(storage)
                .list_tasks(&app)
                .expect("tasks")
                .remove(0)
                .id
        })
        .expect("storage");
    harness
        .service
        .storage_call(|storage| {
            let catalog = neohub_core::AppCatalog::new(storage);
            catalog
                .update_task(
                    &db,
                    neohub_core::catalog::UpdateTaskInput {
                        readiness: Some(ReadinessSignal::Log {
                            regex: "tick".into(),
                            flags: None,
                        }),
                        ..Default::default()
                    },
                )
                .expect("readiness");
        })
        .expect("storage");

    let api = harness.add_task(
        &app,
        "api",
        &format!("sh {}", fixture("steady-log.sh")),
        ReadinessSignal::Log {
            regex: "tick".into(),
            flags: None,
        },
        vec![db.clone()],
        true,
    );
    let web = harness.add_task(
        &app,
        "web",
        &format!("sh {}", fixture("steady-log.sh")),
        ReadinessSignal::Delay { ms: 100 },
        vec![api.clone()],
        true,
    );

    let handle = harness.service.handle();
    handle
        .request(CoreCommand::StartApp {
            app_id: app.clone(),
        })
        .await
        .expect("start app");

    for task in [&db, &api, &web] {
        assert_eq!(
            harness.service.runner().state_of(task),
            ProcessState::Running,
            "task {task} should be running"
        );
    }
    assert_eq!(
        harness.service.readiness_of(&db).map(|(_, ok)| ok),
        Some(true)
    );
    assert_eq!(
        harness.service.readiness_of(&web).map(|(_, ok)| ok),
        Some(true),
        "delay readiness is verified"
    );

    handle
        .request(CoreCommand::StopApp {
            app_id: app.clone(),
        })
        .await
        .expect("stop app");
    for task in [&db, &api, &web] {
        let state = wait_state(
            &harness.service,
            task,
            ProcessState::Exited,
            Duration::from_secs(5),
        )
        .await;
        assert!(!state.is_live(), "task {task} left in {state:?}");
    }
    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn a_readiness_failure_rolls_back_only_this_start() {
    let harness = Harness::new();
    let app = harness.make_app(
        "rollback",
        custom(format!("sh {}", fixture("steady-log.sh"))),
    );
    let user_started = harness
        .service
        .storage_call(|storage| {
            neohub_core::AppCatalog::new(storage)
                .list_tasks(&app)
                .expect("tasks")
                .remove(0)
                .id
        })
        .expect("storage");

    // A task whose readiness never fires.
    let doomed = harness.add_task(
        &app,
        "doomed",
        &format!("sh {}", fixture("steady-log.sh")),
        ReadinessSignal::Log {
            regex: "NEVER-PRINTED-MARKER".into(),
            flags: None,
        },
        vec![],
        true,
    );

    // The user starts one task by hand first.
    let handle = harness.service.handle();
    handle
        .request(CoreCommand::StartTask {
            task_id: user_started.clone(),
        })
        .await
        .expect("manual start");

    // Seed a short readiness timeout so the test does not wait a minute.
    harness
        .service
        .storage_call(|storage| {
            let mut settings = storage.settings().load().expect("settings");
            settings.readiness_timeout_ms = 800;
            storage.settings().save(&settings).expect("save settings");
        })
        .expect("storage");

    let result = handle
        .request(CoreCommand::StartApp {
            app_id: app.clone(),
        })
        .await;
    assert!(result.is_err(), "the doomed task must fail the app start");

    assert_eq!(
        harness.service.runner().state_of(&doomed),
        ProcessState::Exited,
        "the failed task is rolled back"
    );
    assert_eq!(
        harness.service.runner().state_of(&user_started),
        ProcessState::Running,
        "a task the user started earlier must not be stopped by the rollback"
    );

    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn disabled_tasks_are_never_started() {
    let harness = Harness::new();
    let app = harness.make_app(
        "disabled",
        custom(format!("sh {}", fixture("steady-log.sh"))),
    );
    let off = harness.add_task(
        &app,
        "off",
        &format!("sh {}", fixture("steady-log.sh")),
        ReadinessSignal::None,
        vec![],
        false,
    );

    let handle = harness.service.handle();
    handle
        .request(CoreCommand::StartApp {
            app_id: app.clone(),
        })
        .await
        .expect("start app");
    assert_eq!(
        harness.service.runner().state_of(&off),
        ProcessState::Idle,
        "a disabled task must stay idle"
    );
    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn none_readiness_is_unverified_not_ready() {
    let harness = Harness::new();
    let app = harness.make_app(
        "unverified",
        custom(format!("sh {}", fixture("steady-log.sh"))),
    );
    let task = harness
        .service
        .storage_call(|storage| {
            neohub_core::AppCatalog::new(storage)
                .list_tasks(&app)
                .expect("tasks")
                .remove(0)
                .id
        })
        .expect("storage");

    let handle = harness.service.handle();
    handle
        .request(CoreCommand::StartApp {
            app_id: app.clone(),
        })
        .await
        .expect("start app");
    assert_eq!(
        harness.service.readiness_of(&task),
        Some(("none".to_string(), false)),
        "readiness 'none' means unverified — never 'ready'"
    );
    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn restart_produces_a_new_run_and_keeps_the_app_running() {
    let harness = Harness::new();
    let app = harness.make_app(
        "restart",
        custom(format!("sh {}", fixture("steady-log.sh"))),
    );
    let task = harness
        .service
        .storage_call(|storage| {
            neohub_core::AppCatalog::new(storage)
                .list_tasks(&app)
                .expect("tasks")
                .remove(0)
                .id
        })
        .expect("storage");
    let handle = harness.service.handle();

    handle
        .request(CoreCommand::StartApp {
            app_id: app.clone(),
        })
        .await
        .expect("start");
    let first_run = harness
        .service
        .runner()
        .running_tasks()
        .into_iter()
        .find(|info| info.task_id == task)
        .expect("running")
        .run_id;

    handle
        .request(CoreCommand::RestartApp {
            app_id: app.clone(),
        })
        .await
        .expect("restart");

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(info) = harness
            .service
            .runner()
            .running_tasks()
            .into_iter()
            .find(|info| info.task_id == task)
        {
            assert_ne!(info.run_id, first_run, "a restart is a new run");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the task never came back after restart"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn a_port_readiness_probe_sees_a_real_listening_fixture() {
    let harness = Harness::new();
    let port = 4432u16;
    let app = harness.make_app(
        "port-ready",
        custom(format!("sh {} {port}", fixture("listens-on-port.sh"))),
    );
    let task = harness
        .service
        .storage_call(|storage| {
            let catalog = neohub_core::AppCatalog::new(storage);
            let task = catalog.list_tasks(&app).expect("tasks").remove(0).id;
            catalog
                .update_task(
                    &task,
                    neohub_core::catalog::UpdateTaskInput {
                        readiness: Some(ReadinessSignal::Port { port, host: None }),
                        ..Default::default()
                    },
                )
                .expect("readiness");
            task
        })
        .expect("storage");

    let handle = harness.service.handle();
    handle
        .request(CoreCommand::StartApp {
            app_id: app.clone(),
        })
        .await
        .expect("start app");
    assert_eq!(
        harness.service.readiness_of(&task).map(|(_, ok)| ok),
        Some(true),
        "the port probe must confirm the listener"
    );
    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn an_http_readiness_probe_waits_for_the_endpoint_to_serve() {
    let harness = Harness::new();
    let port = 4433u16;
    let app = harness.make_app(
        "http-ready",
        custom(format!("sh {} {port}", fixture("http-ready.sh"))),
    );
    let task = harness
        .service
        .storage_call(|storage| {
            let catalog = neohub_core::AppCatalog::new(storage);
            let task = catalog.list_tasks(&app).expect("tasks").remove(0).id;
            catalog
                .update_task(
                    &task,
                    neohub_core::catalog::UpdateTaskInput {
                        readiness: Some(ReadinessSignal::Http {
                            url: format!("http://127.0.0.1:{port}/health"),
                            status: None,
                        }),
                        ..Default::default()
                    },
                )
                .expect("readiness");
            task
        })
        .expect("storage");

    harness
        .service
        .storage_call(|storage| {
            let mut settings = storage.settings().load().expect("settings");
            settings.readiness_timeout_ms = 15_000;
            storage.settings().save(&settings).expect("save");
        })
        .expect("storage");

    let handle = harness.service.handle();
    handle
        .request(CoreCommand::StartApp {
            app_id: app.clone(),
        })
        .await
        .expect("start app");
    assert_eq!(
        harness.service.readiness_of(&task).map(|(_, ok)| ok),
        Some(true),
        "the http probe must wait through the initial 503"
    );
    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}
