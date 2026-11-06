//! What the app-detail pane drives: per-task start/stop, project-level start/stop/restart,
//! editing a task through the command surface, and the run history it renders.

use neohub_core::catalog::{AddTaskInput, CreateAppInput, CreateTaskSpec, UpdateTaskInput};
use neohub_core::commands::{CoreCommand, CoreReply};
use neohub_core::service::CoreService;
use neohub_domain::{AppId, CommandKind, ProcessState, TaskId};
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
    app: AppId,
    task: TaskId,
}

impl Harness {
    fn new(command: String) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let service = CoreService::open(DataDir::new(dir.path())).expect("service");
        let project = dir.path().join("synthetic-project");
        std::fs::create_dir_all(&project).unwrap();
        let path = project.to_string_lossy().into_owned();
        let (app, task) = service
            .storage_call(move |storage| {
                let catalog = neohub_core::AppCatalog::new(storage);
                let app = catalog
                    .create_app(CreateAppInput {
                        path,
                        name: Some("demo".into()),
                        node_version_pref: None,
                        package_manager: None,
                        default_script: None,
                        first_task: Some(CreateTaskSpec {
                            name: "run".into(),
                            command_kind: Some(CommandKind::Custom),
                            script: None,
                            custom_command: Some(command),
                            working_dir_override: None,
                        }),
                        tasks: vec![],
                        env_vars: vec![],
                    })
                    .expect("create app");
                let task = catalog.list_tasks(&app.id).expect("tasks").remove(0).id;
                (app.id, task)
            })
            .expect("storage");
        Harness {
            _dir: dir,
            service,
            app,
            task,
        }
    }

    async fn wait_state(
        &self,
        task: &TaskId,
        want_terminal: bool,
        timeout: Duration,
    ) -> ProcessState {
        let deadline = Instant::now() + timeout;
        loop {
            let state = self.service.runner().state_of(task);
            if state.is_live() != want_terminal || Instant::now() >= deadline {
                return state;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

#[tokio::test]
async fn per_task_start_stop_and_project_level_controls_agree() {
    let harness = Harness::new(format!("sh {}", fixture("steady-log.sh")));
    let handle = harness.service.handle();

    // The detail pane's per-task Start button.
    handle
        .request(CoreCommand::StartTask {
            task_id: harness.task.clone(),
        })
        .await
        .expect("start task");
    assert_eq!(
        harness.service.runner().state_of(&harness.task),
        ProcessState::Running
    );

    // The project-level Stop stops it again.
    handle
        .request(CoreCommand::StopApp {
            app_id: harness.app.clone(),
        })
        .await
        .expect("stop app");
    let state = harness
        .wait_state(&harness.task, true, Duration::from_secs(5))
        .await;
    assert!(!state.is_live(), "state {state:?}");

    // Project-level Start brings it back.
    handle
        .request(CoreCommand::StartApp {
            app_id: harness.app.clone(),
        })
        .await
        .expect("start app");
    assert_eq!(
        harness.service.runner().state_of(&harness.task),
        ProcessState::Running
    );

    // Restart keeps it running with a new run id.
    let before = harness
        .service
        .runner()
        .running_tasks()
        .into_iter()
        .find(|info| info.task_id == harness.task)
        .expect("running")
        .run_id;
    handle
        .request(CoreCommand::RestartApp {
            app_id: harness.app.clone(),
        })
        .await
        .expect("restart app");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(info) = harness
            .service
            .runner()
            .running_tasks()
            .into_iter()
            .find(|info| info.task_id == harness.task)
        {
            assert_ne!(info.run_id, before, "restart is a new run");
            break;
        }
        assert!(Instant::now() < deadline, "task never came back");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn editing_a_task_through_the_command_surface_validates_and_applies() {
    let harness = Harness::new(format!("sh {}", fixture("steady-log.sh")));
    let handle = harness.service.handle();

    let reply = handle
        .request(CoreCommand::UpdateTask {
            task_id: harness.task.clone(),
            input: Box::new(UpdateTaskInput {
                name: Some("renamed".into()),
                ..Default::default()
            }),
        })
        .await
        .expect("rename");
    match reply {
        CoreReply::Task(task) => assert_eq!(task.name, "renamed"),
        other => panic!("unexpected reply {other:?}"),
    }

    // A blank name is rejected and the stored row is untouched.
    let error = handle
        .request(CoreCommand::UpdateTask {
            task_id: harness.task.clone(),
            input: Box::new(UpdateTaskInput {
                name: Some("   ".into()),
                ..Default::default()
            }),
        })
        .await
        .expect_err("blank names are refused");
    let message = error.to_string();
    assert!(
        message.contains("name") || message.contains("empty"),
        "the error must locate the field: {message}"
    );

    // Adding a second task and reordering both are visible in the snapshot order.
    let reply = handle
        .request(CoreCommand::AddTask {
            app_id: harness.app.clone(),
            input: Box::new(AddTaskInput {
                name: "second".into(),
                command_kind: Some(CommandKind::Custom),
                custom_command: Some("echo second".into()),
                ..Default::default()
            }),
        })
        .await
        .expect("add task");
    let second = match reply {
        CoreReply::Task(task) => task.id,
        other => panic!("unexpected reply {other:?}"),
    };
    handle
        .request(CoreCommand::ReorderTasks {
            app_id: harness.app.clone(),
            task_ids: vec![second.clone(), harness.task.clone()],
        })
        .await
        .expect("reorder");

    let names: Vec<String> = harness
        .service
        .storage_call(|storage| {
            neohub_core::AppCatalog::new(storage)
                .list_tasks(&harness.app)
                .expect("tasks")
                .into_iter()
                .map(|task| task.name)
                .collect::<Vec<_>>()
        })
        .expect("storage");
    assert_eq!(names, vec!["second".to_string(), "renamed".to_string()]);

    // Removing a task disappears from the detail tabs.
    handle
        .request(CoreCommand::RemoveTask { task_id: second })
        .await
        .expect("remove task");
    let remaining: usize = harness
        .service
        .storage_call(|storage| {
            neohub_core::AppCatalog::new(storage)
                .list_tasks(&harness.app)
                .expect("tasks")
                .len()
        })
        .expect("storage");
    assert_eq!(remaining, 1);

    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn run_history_is_readable_after_a_stop() {
    let harness = Harness::new(format!("sh {}", fixture("steady-log.sh")));
    let handle = harness.service.handle();

    handle
        .request(CoreCommand::StartApp {
            app_id: harness.app.clone(),
        })
        .await
        .expect("start");
    handle
        .request(CoreCommand::StopApp {
            app_id: harness.app.clone(),
        })
        .await
        .expect("stop");
    harness
        .wait_state(&harness.task, true, Duration::from_secs(5))
        .await;

    // Wait for the history row to be closed by the event pump.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let rows = match handle
            .request(CoreCommand::ListRunHistory {
                app_id: harness.app.clone(),
                limit: Some(5),
            })
            .await
            .expect("history")
        {
            CoreReply::RunHistory(rows) => rows,
            other => panic!("unexpected reply {other:?}"),
        };
        if let Some(row) = rows.first()
            && row.ended_at.is_some()
        {
            assert_eq!(row.task_name.as_deref(), Some("run"));
            assert!(row.was_killed_by_user, "a stop is attributed to the user");
            break;
        }
        assert!(Instant::now() < deadline, "history row never closed");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn the_logs_pane_can_bootstrap_after_reopening_the_detail_view() {
    let harness = Harness::new(format!("sh {}", fixture("steady-log.sh")));
    let handle = harness.service.handle();
    handle
        .request(CoreCommand::StartTask {
            task_id: harness.task.clone(),
        })
        .await
        .expect("start");

    // Wait for output, then behave like a re-opened detail pane: bootstrap from scratch.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match handle
            .request(CoreCommand::BootstrapLogs {
                task_id: harness.task.clone(),
                after: None,
            })
            .await
            .expect("bootstrap")
        {
            CoreReply::LogBootstrap(outcome) => {
                let bootstrap = match outcome {
                    neohub_core::logs::LogSubscriptionOutcome::Ok(bootstrap) => bootstrap,
                    neohub_core::logs::LogSubscriptionOutcome::ResetRequired(bootstrap) => {
                        bootstrap
                    }
                };
                if String::from_utf8_lossy(&bootstrap.raw_tail).contains("[fixture] tick") {
                    assert!(
                        !bootstrap.terminal.lines.is_empty(),
                        "grid snapshot included"
                    );
                    break;
                }
            }
            other => panic!("unexpected reply {other:?}"),
        }
        assert!(Instant::now() < deadline, "no log output arrived");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}
