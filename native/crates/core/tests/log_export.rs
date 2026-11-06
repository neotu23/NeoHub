//! Terminal text and log export: readable text is parsed, raw text keeps bytes, and both are
//! bounded to what the core retained.

use neohub_core::catalog::{CreateAppInput, CreateTaskSpec};
use neohub_core::commands::{CoreCommand, CoreReply};
use neohub_core::service::CoreService;
use neohub_domain::{CommandKind, TaskId};
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
    service: std::sync::Arc<CoreService>,
    task: TaskId,
}

impl Harness {
    fn new(script: &str, args: &[&str]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let service = CoreService::open(DataDir::new(dir.path())).expect("service");
        let project = dir.path().join("ansi-project");
        std::fs::create_dir_all(&project).unwrap();
        let path = project.to_string_lossy().into_owned();
        let mut command = format!("sh {}", fixture(script).display());
        for arg in args {
            command.push(' ');
            command.push_str(arg);
        }
        let task = service
            .storage_call(move |storage| {
                let catalog = neohub_core::AppCatalog::new(storage);
                let app = catalog
                    .create_app(CreateAppInput {
                        path,
                        name: Some("ansi".into()),
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
        Harness { dir, service, task }
    }

    async fn wait_for_output(&self, needle: &str) {
        let handle = self.service.handle();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let CoreReply::Logs(text) = handle
                .request(CoreCommand::GetTerminalText {
                    task_id: self.task.clone(),
                })
                .await
                .expect("terminal text")
                && text.contains(needle)
            {
                return;
            }
            assert!(Instant::now() < deadline, "output {needle:?} never arrived");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

#[tokio::test]
async fn readable_text_has_no_escape_sequences_and_raw_text_keeps_them() {
    let harness = Harness::new("ansi-output.sh", &[]);
    let handle = harness.service.handle();
    handle
        .request(CoreCommand::StartTask {
            task_id: harness.task.clone(),
        })
        .await
        .expect("start");
    harness.wait_for_output("progress 100%").await;

    let readable = match handle
        .request(CoreCommand::GetTerminalText {
            task_id: harness.task.clone(),
        })
        .await
        .expect("readable")
    {
        CoreReply::Logs(text) => text,
        other => panic!("unexpected reply {other:?}"),
    };
    assert!(readable.contains("starting green line"));
    assert!(
        readable.contains("progress 100%"),
        "CR overwrites leave the last frame"
    );
    assert!(
        !readable.contains("progress 0%"),
        "earlier progress frames are overwritten"
    );
    assert!(
        !readable.contains('\u{1b}'),
        "readable text carries no escapes"
    );

    // Raw export keeps the bytes (a client can replay them through its own terminal).
    let raw_path = harness.dir.path().join("raw.log");
    handle
        .request(CoreCommand::ExportLogs {
            task_id: harness.task.clone(),
            path: raw_path.clone(),
            raw: true,
        })
        .await
        .expect("raw export");
    let raw = std::fs::read(&raw_path).expect("raw file");
    assert!(
        raw.windows(2).any(|window| window == [0x1b, { b'[' }]),
        "raw export keeps ANSI escapes"
    );

    let readable_path = harness.dir.path().join("readable.log");
    handle
        .request(CoreCommand::ExportLogs {
            task_id: harness.task.clone(),
            path: readable_path.clone(),
            raw: false,
        })
        .await
        .expect("readable export");
    let readable_file = std::fs::read_to_string(&readable_path).expect("readable file");
    assert!(readable_file.contains("starting green line"));
    assert!(!readable_file.contains('\u{1b}'));

    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn exporting_an_unknown_task_writes_an_empty_file_instead_of_failing() {
    let harness = Harness::new("steady-log.sh", &[]);
    let handle = harness.service.handle();
    let path = harness.dir.path().join("empty.log");
    let reply = handle
        .request(CoreCommand::ExportLogs {
            task_id: TaskId::from_existing("no-such-task"),
            path: path.clone(),
            raw: false,
        })
        .await
        .expect("export of an unknown task is not an error");
    assert!(matches!(reply, CoreReply::Exported(_)));
    assert_eq!(std::fs::read_to_string(&path).expect("file"), "");
    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}
