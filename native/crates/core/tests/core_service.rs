//! Core service wiring: command → snapshot → history, plus the shutdown order.
//!
//! All processes are synthetic fixtures; nothing touches a user project.

use neohub_core::catalog::AppCatalog;
use neohub_core::catalog::{CreateAppInput, CreateTaskSpec};
use neohub_core::commands::{CoreCommand, CoreReply};
use neohub_core::events::CoreEventKind;
use neohub_core::service::CoreService;
use neohub_domain::{CommandKind, ProcessState};
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

/// Create a synthetic app whose single task runs `sh <fixture>`.
fn seed_app(service: &CoreService, dir: &std::path::Path, command: String) -> (String, String) {
    let project = dir.join("synthetic-project");
    std::fs::create_dir_all(&project).unwrap();
    let path = project.to_string_lossy().into_owned();
    service
        .storage_call(|storage| {
            let catalog = AppCatalog::new(storage);
            let app = catalog
                .create_app(CreateAppInput {
                    path,
                    name: Some("synthetic".into()),
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
            let task = catalog.list_tasks(&app.id).expect("tasks").remove(0);
            (app.id.to_string(), task.id.to_string())
        })
        .expect("database must still be open")
}

#[tokio::test]
async fn start_records_history_and_stop_settles_it() {
    let dir = tempfile::tempdir().unwrap();
    let service = CoreService::open(DataDir::new(dir.path())).expect("service");
    let (app_id, task_id) = seed_app(
        &service,
        dir.path(),
        format!("sh {}", fixture("steady-log.sh")),
    );
    let handle = service.handle();

    let start = handle
        .request(CoreCommand::StartTask {
            task_id: task_id.as_str().into(),
        })
        .await
        .expect("start");
    let info = match start {
        CoreReply::Started(info) => info,
        other => panic!("unexpected reply {other:?}"),
    };
    assert!(info.pid > 0);

    let snapshot = match handle.request(CoreCommand::GetSnapshot).await.unwrap() {
        CoreReply::Snapshot(snapshot) => snapshot,
        other => panic!("unexpected reply {other:?}"),
    };
    assert_eq!(snapshot.running_tasks.len(), 1);
    assert_eq!(
        snapshot
            .app_states
            .get(&neohub_domain::AppId::from_existing(&app_id)),
        Some(&ProcessState::Running)
    );

    // Logs flow through the same command surface the UI will use.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut logs = String::new();
    while Instant::now() < deadline {
        if let CoreReply::Logs(text) = handle
            .request(CoreCommand::GetLogs {
                task_id: task_id.as_str().into(),
            })
            .await
            .unwrap()
        {
            logs = text;
            if logs.contains("[fixture] tick 2") {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        logs.contains("[fixture] tick"),
        "expected PTY output: {logs:?}"
    );

    handle
        .request(CoreCommand::StopTask {
            task_id: task_id.as_str().into(),
        })
        .await
        .expect("stop");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let reply = handle.request(CoreCommand::GetSnapshot).await.unwrap();
        if let CoreReply::Snapshot(snapshot) = reply
            && snapshot.running_tasks.is_empty()
        {
            break;
        }
        assert!(Instant::now() < deadline, "task did not stop in time");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // History: one row, closed, attributed to the user.
    let rows = service
        .storage_call(|storage| {
            storage
                .history()
                .list_for_app(&app_id.as_str().into(), Some(10))
                .expect("history")
        })
        .expect("database open");
    assert_eq!(rows.len(), 1, "one row per run");
    assert!(rows[0].ended_at.is_some(), "the run was closed");
    assert!(rows[0].was_killed_by_user);
    assert_eq!(rows[0].task_name.as_deref(), Some("run"));

    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn a_crashed_task_is_settled_as_crashed_and_history_keeps_the_code() {
    let dir = tempfile::tempdir().unwrap();
    let service = CoreService::open(DataDir::new(dir.path())).expect("service");
    let (app_id, task_id) = seed_app(
        &service,
        dir.path(),
        format!("sh {} 7", fixture("exit-code.sh")),
    );
    let handle = service.handle();

    handle
        .request(CoreCommand::StartTask {
            task_id: task_id.as_str().into(),
        })
        .await
        .expect("start");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let state = service.runner().state_of(&task_id.as_str().into());
        if !state.is_live() {
            assert_eq!(state, ProcessState::Crashed);
            break;
        }
        assert!(Instant::now() < deadline, "task never exited");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Give the history pump a moment to settle the row.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let rows = service
            .storage_call(|storage| {
                storage
                    .history()
                    .list_for_app(&app_id.as_str().into(), Some(5))
                    .expect("history")
            })
            .unwrap_or_default();
        if rows.first().map(|r| r.ended_at.is_some()).unwrap_or(false) {
            assert_eq!(rows[0].exit_code, Some(7));
            assert!(!rows[0].was_killed_by_user, "the process failed on its own");
            break;
        }
        assert!(Instant::now() < deadline, "history row was never closed");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn shutdown_stops_every_task_and_refuses_new_starts() {
    let dir = tempfile::tempdir().unwrap();
    let service = CoreService::open(DataDir::new(dir.path())).expect("service");
    let (_, task_id) = seed_app(
        &service,
        dir.path(),
        format!("sh {}", fixture("steady-log.sh")),
    );
    let handle = service.handle();

    handle
        .request(CoreCommand::StartTask {
            task_id: task_id.as_str().into(),
        })
        .await
        .expect("start");

    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
    assert!(service.runner().running_tasks().is_empty());

    let refused = handle
        .request(CoreCommand::StartTask {
            task_id: task_id.as_str().into(),
        })
        .await;
    assert!(refused.is_err(), "a shutdown core must refuse new starts");
}

#[tokio::test]
async fn a_subscriber_sees_lifecycle_events_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let service = CoreService::open(DataDir::new(dir.path())).expect("service");
    let (_, task_id) = seed_app(
        &service,
        dir.path(),
        format!("sh {}", fixture("steady-log.sh")),
    );
    let handle = service.handle();
    let mut subscription = handle.subscribe();

    handle
        .request(CoreCommand::StartTask {
            task_id: task_id.as_str().into(),
        })
        .await
        .expect("start");
    handle
        .request(CoreCommand::StopTask {
            task_id: task_id.as_str().into(),
        })
        .await
        .expect("stop");

    let mut states = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let Some(event) = subscription.recv().await else {
            break;
        };
        match event.kind {
            CoreEventKind::TaskStatus { state, .. } => {
                states.push(state);
                if matches!(state, ProcessState::Exited | ProcessState::Crashed) {
                    break;
                }
            }
            CoreEventKind::ResyncRequired { .. } => break,
            _ => {}
        }
    }
    assert!(
        states.contains(&ProcessState::Running),
        "states: {states:?}"
    );
    assert!(
        states.last().map(|s| !s.is_live()).unwrap_or(false),
        "the last observed state must be terminal: {states:?}"
    );

    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn an_undecryptable_secret_blocks_the_start() {
    use neohub_domain::EnvVar;
    use neohub_storage::env::new_var;

    let dir = tempfile::tempdir().unwrap();
    let service = CoreService::open(DataDir::new(dir.path())).expect("service");
    let (app_id, task_id) = seed_app(
        &service,
        dir.path(),
        format!("sh {}", fixture("steady-log.sh")),
    );

    // Legacy Electron ciphertext: readable by safeStorage, NOT by this build.
    let app: neohub_domain::AppId = app_id.as_str().into();
    let var: EnvVar = new_var(
        Some(app.clone()),
        None,
        "DB_PASSWORD",
        "enc1:v10:abcdef",
        true,
    );
    service
        .storage_call(|storage| {
            let mut env = storage.env_store();
            env.set_app(&app, &[var], neohub_storage::db::now_ms())
                .expect("seed secret");
        })
        .expect("database open");

    let handle = service.handle();
    let result = handle
        .request(CoreCommand::StartTask {
            task_id: task_id.as_str().into(),
        })
        .await;
    match result {
        Err(neohub_core::CoreError::Domain(neohub_domain::DomainError::SecretUnavailable {
            key,
        })) => {
            assert_eq!(key, "DB_PASSWORD");
        }
        other => panic!("expected SecretUnavailable, got {other:?}"),
    }
    assert!(
        service.runner().running_tasks().is_empty(),
        "no process may start when its secrets cannot be decrypted"
    );

    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn a_legacy_ciphertext_is_reported_as_needing_re_entry() {
    // Imported `enc1:` values cannot be decrypted, so the row must say "re-enter me" rather than
    // looking like a secret that is simply set. The ciphertext itself still never reaches a view.
    use neohub_core::commands::{CoreCommand, CoreReply};
    use neohub_core::service::EnvScope;
    use neohub_storage::db::DataDir;

    let dir = tempfile::tempdir().unwrap();
    // Write the legacy row the way the importer does - straight into the table, bypassing the
    // encryption that `SetEnvVars` applies (which would turn it into nhk1:).
    {
        let storage = neohub_storage::Storage::open(DataDir::new(dir.path())).expect("storage");
        let row = |key: &str, value: &str, secret: bool| neohub_domain::EnvVar {
            id: neohub_domain::EnvVarId::new(),
            app_id: None,
            task_id: None,
            key: key.to_string(),
            value: value.to_string(),
            enabled: true,
            is_secret: secret,
            note: None,
            created_at: 0,
            updated_at: 0,
        };
        storage
            .env_store()
            .set_global(
                &[
                    row("OLD_TOKEN", "enc1:v10:AAAA", true),
                    row("PLAIN", "visible", false),
                ],
                1,
            )
            .expect("legacy rows");
    }
    let service = neohub_core::service::CoreService::open(DataDir::new(dir.path())).expect("core");
    let handle = service.handle();

    let reply = handle
        .request(CoreCommand::ListEnvVars {
            scope: EnvScope::Global,
        })
        .await
        .expect("list");
    let CoreReply::EnvVars(rows) = reply else {
        panic!("unexpected reply");
    };
    let legacy = rows.iter().find(|row| row.key == "OLD_TOKEN").expect("row");
    let plain = rows.iter().find(|row| row.key == "PLAIN").expect("row");

    assert!(legacy.needs_reentry, "the enc1: row is flagged");
    assert!(legacy.value.is_none(), "and its text stays in the core");
    assert!(!plain.needs_reentry);
    assert_eq!(plain.value.as_deref(), Some("visible"));
}
