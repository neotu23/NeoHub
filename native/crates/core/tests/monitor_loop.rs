//! The background sampling loop: stats/ports events reach subscribers, and an idle core does
//! no sampling at all.

use neohub_core::catalog::{CreateAppInput, CreateTaskSpec};
use neohub_core::commands::{CoreCommand, CoreReply};
use neohub_core::events::CoreEventKind;
use neohub_core::service::CoreService;
use neohub_domain::{AppId, CommandKind, TaskId};
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

fn seed(service: &CoreService, dir: &std::path::Path, command: String) -> (AppId, TaskId) {
    let project = dir.join("synthetic-project");
    std::fs::create_dir_all(&project).unwrap();
    let path = project.to_string_lossy().into_owned();
    service
        .storage_call(move |storage| {
            let catalog = neohub_core::AppCatalog::new(storage);
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
            let task = catalog.list_tasks(&app.id).expect("tasks").remove(0).id;
            (app.id, task)
        })
        .expect("storage")
}

#[tokio::test]
async fn stats_and_ports_events_reach_a_subscriber() {
    let dir = tempfile::tempdir().unwrap();
    let service = CoreService::open(DataDir::new(dir.path())).expect("service");
    let port = 4436u16;
    // A task that listens on a port and prints an unrelated URL.
    let (_, task) = seed(
        &service,
        dir.path(),
        format!("sh {} {port}", fixture("listens-on-port.sh")),
    );
    let handle = service.handle();
    let mut events = handle.subscribe();

    handle
        .request(CoreCommand::StartTask {
            task_id: task.clone(),
        })
        .await
        .expect("start");

    let mut saw_stats = false;
    let mut saw_ports = false;
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline && !(saw_stats && saw_ports) {
        let Some(event) = tokio::time::timeout(Duration::from_millis(500), events.recv())
            .await
            .ok()
            .flatten()
        else {
            continue;
        };
        match event.kind {
            CoreEventKind::TaskStats {
                task_id,
                process_count,
                mem_mb,
                ..
            } => {
                if task_id == task {
                    assert!(process_count >= 1, "a live task has at least one process");
                    assert!(mem_mb > 0.0, "the sample carries memory");
                    saw_stats = true;
                }
            }
            CoreEventKind::TaskPorts {
                task_id, confirmed, ..
            } if task_id == task && confirmed.contains(&port) => {
                saw_ports = true;
            }
            _ => {}
        }
    }
    assert!(saw_stats, "a TaskStats event must arrive");
    assert!(
        saw_ports,
        "a TaskPorts event with the confirmed port must arrive"
    );

    // The snapshot carries the same caches.
    match handle.request(CoreCommand::GetSnapshot).await.unwrap() {
        CoreReply::Snapshot(snapshot) => {
            assert!(snapshot.task_stats.contains_key(&task));
            assert!(
                snapshot
                    .task_ports
                    .get(&task)
                    .map(|sample| sample.confirmed.contains(&port))
                    .unwrap_or(false)
            );
        }
        other => panic!("unexpected reply {other:?}"),
    }

    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn an_idle_core_does_not_sample() {
    let dir = tempfile::tempdir().unwrap();
    let service = CoreService::open(DataDir::new(dir.path())).expect("service");
    let baseline = service.monitor_sweeps();
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(
        service.monitor_sweeps(),
        baseline,
        "an idle core must not run sampling sweeps (no polling, no shell processes)"
    );

    // Starting a task wakes the loop.
    let (_, task) = seed(
        &service,
        dir.path(),
        format!("sh {}", fixture("steady-log.sh")),
    );
    let handle = service.handle();
    handle
        .request(CoreCommand::StartTask {
            task_id: task.clone(),
        })
        .await
        .expect("start");
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && service.monitor_sweeps() <= baseline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        service.monitor_sweeps() > baseline,
        "starting a task must start sampling"
    );

    // Stopping returns it to idle: sweep count stops growing.
    handle
        .request(CoreCommand::StopTask {
            task_id: task.clone(),
        })
        .await
        .expect("stop");
    tokio::time::sleep(Duration::from_millis(300)).await;
    let after_stop = service.monitor_sweeps();
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(
        service.monitor_sweeps(),
        after_stop,
        "once nothing runs, sampling stops again"
    );

    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn a_hinted_port_is_reported_separately_from_a_confirmed_one() {
    let dir = tempfile::tempdir().unwrap();
    let service = CoreService::open(DataDir::new(dir.path())).expect("service");
    let port = 4437u16;
    // Prints a URL for a port it never binds; the listener is on `port`.
    let script = format!(
        "printf 'ready on http://localhost:4998\\n'; exec sh {} {port}",
        fixture("listens-on-port.sh")
    );
    let (_, task) = seed(&service, dir.path(), script);
    let handle = service.handle();
    let mut events = handle.subscribe();
    handle
        .request(CoreCommand::StartTask {
            task_id: task.clone(),
        })
        .await
        .expect("start");

    let mut confirmed_hit = false;
    let mut hinted_hit = false;
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline && !(confirmed_hit && hinted_hit) {
        let Some(event) = tokio::time::timeout(Duration::from_millis(500), events.recv())
            .await
            .ok()
            .flatten()
        else {
            continue;
        };
        if let CoreEventKind::TaskPorts {
            task_id,
            confirmed,
            hinted,
            ..
        } = event.kind
        {
            if task_id != task {
                continue;
            }
            confirmed_hit |= confirmed.contains(&port);
            hinted_hit |= hinted.contains(&4998);
        }
    }
    assert!(confirmed_hit, "the bound port is confirmed");
    assert!(hinted_hit, "the printed URL stays a hint");

    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}
