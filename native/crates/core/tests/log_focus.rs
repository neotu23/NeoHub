//! Log push gating: the window says what it is showing, and only those tasks push events.
//!
//! The bytes are never dropped — the ring buffer keeps everything — so this test checks both
//! halves: fewer events, and the output still readable after the fact.

use neohub_core::catalog::{CreateAppInput, CreateTaskSpec};
use neohub_core::commands::{CoreCommand, CoreReply};
use neohub_core::events::CoreEventKind;
use neohub_core::service::CoreService;
use neohub_domain::CommandKind;
use neohub_storage::DataDir;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../test-fixtures/process")
        .join(name)
}

async fn service_with_two_tasks(
    dir: &std::path::Path,
) -> (std::sync::Arc<CoreService>, String, String) {
    let service = CoreService::open(DataDir::new(dir)).expect("core");
    let project = dir.join("demo");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("package.json"),
        r#"{"scripts":{"dev":"node ."}}"#,
    )
    .unwrap();
    let command = format!("sh {}", fixture("steady-log.sh").display());
    let app = service
        .create_app(CreateAppInput {
            path: project.to_string_lossy().into_owned(),
            name: Some("demo".into()),
            node_version_pref: None,
            package_manager: None,
            default_script: None,
            first_task: Some(CreateTaskSpec {
                name: "alpha".into(),
                command_kind: Some(CommandKind::Custom),
                script: None,
                custom_command: Some(command.clone()),
                working_dir_override: None,
            }),
            tasks: vec![CreateTaskSpec {
                name: "beta".into(),
                command_kind: Some(CommandKind::Custom),
                script: None,
                custom_command: Some(command),
                working_dir_override: None,
            }],
            env_vars: vec![],
        })
        .expect("app");
    let tasks = service
        .snapshot()
        .unwrap()
        .tasks_by_app
        .remove(&app.id)
        .unwrap();
    let alpha = tasks[0].id.as_str().to_string();
    let beta = tasks[1].id.as_str().to_string();
    (service, alpha, beta)
}

#[tokio::test]
async fn only_the_focused_tasks_push_log_events() {
    let dir = tempfile::tempdir().unwrap();
    let (service, alpha, beta) = service_with_two_tasks(dir.path()).await;
    let handle = service.handle();

    // Focus only alpha.
    handle
        .request(CoreCommand::SetLogFocus {
            tasks: Some(vec![neohub_domain::TaskId::from_existing(alpha.clone())]),
        })
        .await
        .expect("focus");

    for task in [&alpha, &beta] {
        handle
            .request(CoreCommand::StartTask {
                task_id: neohub_domain::TaskId::from_existing(task.clone()),
            })
            .await
            .expect("start");
    }

    // Collect events for a while and see whose logs arrive.
    let mut subscription = handle.subscribe();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut from_alpha = 0;
    let mut from_beta = 0;
    while Instant::now() < deadline {
        let Ok(Some(event)) =
            tokio::time::timeout(Duration::from_millis(300), subscription.recv()).await
        else {
            continue;
        };
        if let CoreEventKind::Log { task_id, .. } = &event.kind {
            if task_id.as_str() == alpha {
                from_alpha += 1;
            } else if task_id.as_str() == beta {
                from_beta += 1;
            }
        }
    }
    assert!(from_alpha > 0, "the focused task still pushes");
    assert_eq!(from_beta, 0, "an unfocused task must not push events");

    // The gated task's output is still there when someone asks for it.
    let text = match handle
        .request(CoreCommand::GetTerminalText {
            task_id: neohub_domain::TaskId::from_existing(beta.clone()),
        })
        .await
        .expect("text")
    {
        CoreReply::Logs(text) => text,
        other => panic!("unexpected reply: {other:?}"),
    };
    assert!(
        text.contains("[fixture] tick"),
        "gating events must not drop the bytes: {text}"
    );

    let _ = handle.request(CoreCommand::Shutdown).await;
}

#[tokio::test]
async fn no_focus_means_push_everything() {
    let dir = tempfile::tempdir().unwrap();
    let (service, alpha, beta) = service_with_two_tasks(dir.path()).await;
    let handle = service.handle();
    handle
        .request(CoreCommand::SetLogFocus { tasks: None })
        .await
        .expect("clear focus");

    for task in [&alpha, &beta] {
        handle
            .request(CoreCommand::StartTask {
                task_id: neohub_domain::TaskId::from_existing(task.clone()),
            })
            .await
            .expect("start");
    }

    let mut subscription = handle.subscribe();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut seen_beta = false;
    while Instant::now() < deadline && !seen_beta {
        let Ok(Some(event)) =
            tokio::time::timeout(Duration::from_millis(300), subscription.recv()).await
        else {
            continue;
        };
        if let CoreEventKind::Log { task_id, .. } = &event.kind
            && task_id.as_str() == beta
        {
            seen_beta = true;
        }
    }
    assert!(seen_beta, "with no focus, every task pushes");
    let _ = handle.request(CoreCommand::Shutdown).await;
}
