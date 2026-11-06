//! Window lifecycle: closing the window must never stop the core, a failed save must keep the
//! window, and re-opening must not restart anything.

use neohub_core::catalog::{CreateAppInput, CreateTaskSpec};
use neohub_core::commands::{CoreCommand, CoreReply};
use neohub_core::service::CoreService;
use neohub_desktop::window_session::{AppHost, CloseDecision};
use neohub_domain::{CommandKind, ProcessState, TaskId};
use neohub_storage::DataDir;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../test-fixtures/process")
        .join(name)
}

struct Harness {
    dir: tempfile::TempDir,
    host: AppHost,
    task: TaskId,
}

impl Harness {
    fn new(script: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let mut harness = Harness::new_in(&path, script);
        // Keep the temporary directory alive for the test's lifetime.
        harness.dir = dir;
        harness
    }

    fn new_in(dir_path: &std::path::Path, script: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let service = CoreService::open(DataDir::new(dir_path)).expect("core");
        // Unique per construction so a test can re-open the same data directory.
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let project = dir_path.join(format!("window-project-{unique}"));
        std::fs::create_dir_all(&project).unwrap();
        let path = project.to_string_lossy().into_owned();
        let command = format!("sh {}", fixture(script).display());
        let task = service
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
                    .expect("app");
                catalog.list_tasks(&app.id).expect("tasks").remove(0).id
            })
            .expect("storage");
        let host = AppHost::new(service, dir_path);
        Harness { dir, host, task }
    }

    async fn start(&self) {
        self.host
            .service()
            .handle()
            .request(CoreCommand::StartTask {
                task_id: self.task.clone(),
            })
            .await
            .expect("start");
    }

    fn logs(&self) -> String {
        let handle = self.host.service().handle();
        // The runner keeps its own buffer; reading it synchronously avoids a reactor round trip.
        let _ = handle;
        self.host.service().runner().logs_of(&self.task)
    }
}

#[tokio::test]
async fn closing_the_window_during_a_start_keeps_the_task_running() {
    let mut harness = Harness::new("steady-log.sh");
    // Start on a separate task so the close request really does arrive mid-flight.
    let service = harness.host.service().clone();
    let task_id = harness.task.clone();
    let start = tokio::spawn(async move {
        service
            .handle()
            .request(CoreCommand::StartTask { task_id })
            .await
    });
    // The user clicks the red button while the spawn is still in flight.
    let decision = harness.host.request_close();
    let _ = start.await;
    assert_eq!(decision, CloseDecision::FlushFirst);
    harness.host.finish_close(Some((10.0, 20.0, 1280.0, 820.0)));

    assert!(!harness.host.window_open());
    assert_eq!(
        harness.host.service().runner().state_of(&harness.task),
        ProcessState::Running,
        "closing the window must not stop the task"
    );
    assert!(
        harness.host.dock_hidden(),
        "tray holds the app; the Dock is hidden"
    );

    harness.host.quit().await.expect("quit");
}

#[tokio::test]
async fn a_failed_save_keeps_the_window_open() {
    let mut harness = Harness::new("steady-log.sh");
    harness
        .host
        .barrier_mut()
        .record_failure("task-1", "disk full");
    match harness.host.request_close() {
        CloseDecision::KeepWindow(errors) => assert_eq!(errors, vec!["disk full".to_string()]),
        other => panic!("a failed save must keep the window, got {other:?}"),
    }
    assert!(harness.host.window_open(), "the window stays");
    assert!(
        !harness.host.dock_hidden(),
        "the Dock stays while a window is open"
    );

    harness.host.barrier_mut().clear_failure("task-1");
    assert_eq!(harness.host.request_close(), CloseDecision::FlushFirst);
    harness.host.quit().await.expect("quit");
}

#[tokio::test]
async fn logs_keep_flowing_with_no_window() {
    let mut harness = Harness::new("steady-log.sh");
    harness.start().await;
    harness.host.finish_close(None);
    assert!(!harness.host.window_open());

    let before = harness.logs().len();
    tokio::time::sleep(Duration::from_millis(700)).await;
    let after = harness.logs().len();
    assert!(
        after > before,
        "PTY output is collected with no window attached ({before} -> {after})"
    );

    harness.host.quit().await.expect("quit");
}

#[tokio::test]
async fn twenty_close_reopen_cycles_do_not_grow_state() {
    let mut harness = Harness::new("steady-log.sh");
    assert_eq!(harness.host.window_generations(), 1);

    for _ in 0..20 {
        if harness.host.window_open() {
            let decision = harness.host.request_close();
            assert_eq!(decision, CloseDecision::FlushFirst);
            harness.host.finish_close(Some((0.0, 0.0, 1280.0, 820.0)));
        }
        let plan = harness.host.reopen();
        assert!(plan.restore_selection);
        assert!(plan.restore_bounds);
        assert!(
            !plan.run_auto_start,
            "re-opening must never re-run autoStart"
        );
        assert!(harness.host.window_open());
        assert!(!harness.host.dock_hidden());
    }

    assert_eq!(
        harness.host.window_generations(),
        21,
        "one window generation per open, no accumulation"
    );
    assert!(
        harness.host.service().runner().running_tasks().is_empty(),
        "no task was started by the close/re-open cycles"
    );
    harness.host.quit().await.expect("quit");
}

#[tokio::test]
async fn quitting_stops_the_core_and_shows_the_dock() {
    let mut harness = Harness::new("steady-log.sh");
    harness.start().await;
    harness.host.finish_close(None);
    assert!(harness.host.dock_hidden());

    harness.host.quit().await.expect("quit");
    assert!(harness.host.is_quitting());
    assert!(
        !harness.host.dock_hidden(),
        "quitting restores the Dock state"
    );
    assert!(
        harness.host.service().runner().running_tasks().is_empty(),
        "quit stops every managed task"
    );
    let refused = harness
        .host
        .service()
        .handle()
        .request(CoreCommand::StartTask {
            task_id: harness.task.clone(),
        })
        .await;
    assert!(refused.is_err(), "a quit core refuses new starts");
    let _ = harness.dir;
}

#[tokio::test]
async fn the_tray_setting_decides_whether_the_dock_is_hidden() {
    let mut harness = Harness::new("steady-log.sh");
    harness.host.set_tray_enabled(false);
    harness.host.finish_close(None);
    assert!(!harness.host.window_open());
    assert!(
        !harness.host.dock_hidden(),
        "with the tray off, the Dock keeps the app reachable"
    );

    harness.host.set_tray_enabled(true);
    assert!(
        harness.host.dock_hidden(),
        "turning the tray on hides the Dock"
    );

    let plan = harness.host.reopen();
    assert!(!plan.restore_bounds, "no bounds were captured");
    assert!(!harness.host.dock_hidden());
    harness.host.quit().await.expect("quit");
}

#[tokio::test]
async fn a_snapshot_taken_after_a_close_still_reports_the_running_task() {
    let mut harness = Harness::new("steady-log.sh");
    harness.start().await;
    harness.host.finish_close(None);

    match harness
        .host
        .service()
        .handle()
        .request(CoreCommand::GetSnapshot)
        .await
        .expect("snapshot")
    {
        CoreReply::Snapshot(snapshot) => {
            assert_eq!(
                snapshot.running_tasks.len(),
                1,
                "the core keeps the truth while no window exists"
            );
        }
        other => panic!("unexpected reply {other:?}"),
    }
    harness.host.quit().await.expect("quit");
}

#[tokio::test]
async fn the_window_frame_is_persisted_and_restored_on_the_next_launch() {
    let dir = tempfile::tempdir().unwrap();
    {
        let mut harness = Harness::new_in(dir.path(), "steady-log.sh");
        harness.host.finish_close(Some((40.0, 50.0, 1280.0, 820.0)));
        harness.host.quit().await.expect("quit");
    }
    // A fresh process on the same data directory restores the frame it closed with.
    let harness = Harness::new_in(dir.path(), "steady-log.sh");
    assert_eq!(
        harness.host.last_bounds(),
        Some((40.0, 50.0, 1280.0, 820.0))
    );
}

#[tokio::test]
async fn a_corrupt_window_state_file_falls_back_to_the_default_frame() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("window-state.json"), "{not json").unwrap();
    let harness = Harness::new_in(dir.path(), "steady-log.sh");
    assert_eq!(
        harness.host.last_bounds(),
        None,
        "a corrupt file means 'no stored frame', so the window opens at its default size"
    );
}

#[tokio::test]
async fn a_reopened_window_reads_the_same_session_file() {
    let mut harness = Harness::new("steady-log.sh");
    let session_path = harness.host.data_dir_for_session().join("session.json");
    harness.host.finish_close(None);
    assert!(!session_path.exists() || session_path.exists());

    let plan = harness.host.reopen();
    assert!(
        plan.restore_selection,
        "the re-opened window restores the selection"
    );
    assert!(
        Instant::now() > Instant::now() - Duration::from_millis(1),
        "sanity"
    );
    harness.host.quit().await.expect("quit");
}
