//! The import → start → ports → stop round trip that phase 9's UI drives, exercised through
//! the same commands the window sends (no GUI automation needed for the semantics).

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

/// A synthetic "folder of repositories" to import from.
fn synth_repos() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, scripts) in [
        (
            "demo-web",
            r#"{"dev":"node server.js","build":"node build.js"}"#,
        ),
        ("demo-worker", r#"{"start":"node worker.js"}"#),
    ] {
        let project = dir.path().join(name);
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("package.json"),
            format!(r#"{{"name":"{name}","scripts":{scripts}}}"#),
        )
        .unwrap();
        std::fs::write(project.join("pnpm-lock.yaml"), "lockfileVersion: 9\n").unwrap();
    }
    // Not a project: no manifest, must not be offered.
    std::fs::create_dir_all(dir.path().join("notes")).unwrap();
    dir
}

#[tokio::test]
async fn scan_import_start_ports_stop_round_trip() {
    let repos = synth_repos();
    let data = tempfile::tempdir().unwrap();
    let service = CoreService::open(DataDir::new(data.path())).expect("service");
    let handle = service.handle();

    // 1. Scan the folder the user picked.
    let reply = handle
        .request(CoreCommand::ScanFolder {
            dir: repos.path().to_path_buf(),
        })
        .await
        .expect("scan");
    let candidates = match reply {
        CoreReply::ImportCandidates(candidates) => candidates,
        other => panic!("unexpected reply {other:?}"),
    };
    let names: Vec<&str> = candidates.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["demo-web", "demo-worker"], "only real projects");
    assert_eq!(candidates[0].suggested_script.as_deref(), Some("dev"));
    assert_eq!(candidates[1].suggested_script.as_deref(), Some("start"));

    // 2. Import one of them, with a task that actually listens on a port so the port event
    //    can be observed end-to-end (the real UI would use the detected script).
    let port = 4441u16;
    let chosen = candidates[0].path.clone();
    let reply = handle
        .request(CoreCommand::CreateApp {
            input: Box::new(CreateAppInput {
                path: chosen.clone(),
                name: Some("demo-web".into()),
                node_version_pref: None,
                package_manager: candidates[0].package_manager,
                default_script: candidates[0].suggested_script.clone(),
                first_task: Some(CreateTaskSpec {
                    name: "listener".into(),
                    command_kind: Some(CommandKind::Custom),
                    script: None,
                    custom_command: Some(format!("sh {} {port}", fixture("listens-on-port.sh"))),
                    working_dir_override: None,
                }),
                tasks: vec![],
                env_vars: vec![("DEMO_FLAG".into(), "synthetic".into(), false)],
            }),
        })
        .await
        .expect("create app");
    let app = match reply {
        CoreReply::App(app) => *app,
        other => panic!("unexpected reply {other:?}"),
    };
    assert_eq!(app.name, "demo-web");

    // A second scan now reports the project as already registered (greyed out in the UI).
    let reply = handle
        .request(CoreCommand::ScanFolder {
            dir: repos.path().to_path_buf(),
        })
        .await
        .expect("rescan");
    if let CoreReply::ImportCandidates(candidates) = reply {
        let imported = candidates
            .iter()
            .find(|c| c.path == chosen)
            .expect("the imported project is still listed");
        assert!(imported.already_registered, "marked as registered");
        assert_eq!(
            imported.suggested_script.as_deref(),
            Some("dev"),
            "the suggestion is still shown so the row stays useful"
        );
    }

    // 3. Start the app through the same command the card's Start button sends.
    let mut events = handle.subscribe();
    handle
        .request(CoreCommand::StartApp {
            app_id: app.id.clone(),
        })
        .await
        .expect("start app");

    let snapshot = match handle.request(CoreCommand::GetSnapshot).await.unwrap() {
        CoreReply::Snapshot(snapshot) => snapshot,
        other => panic!("unexpected reply {other:?}"),
    };
    assert_eq!(
        snapshot.app_states.get(&app.id),
        Some(&ProcessState::Running),
        "the dashboard card would render as running"
    );

    // 4. Wait for the confirmed port to reach the UI.
    let mut saw_port = false;
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline && !saw_port {
        let Some(event) = tokio::time::timeout(Duration::from_millis(500), events.recv())
            .await
            .ok()
            .flatten()
        else {
            continue;
        };
        if let CoreEventKind::TaskPorts { confirmed, .. } = event.kind {
            saw_port = confirmed.contains(&port);
        }
    }
    assert!(
        saw_port,
        "the card's port chip comes from a confirmed listener"
    );

    // 5. Stop, and confirm the app is no longer live.
    handle
        .request(CoreCommand::StopApp {
            app_id: app.id.clone(),
        })
        .await
        .expect("stop app");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let state = service
            .snapshot()
            .map(|snapshot| {
                snapshot
                    .app_states
                    .get(&app.id)
                    .copied()
                    .unwrap_or(ProcessState::Idle)
            })
            .unwrap_or(ProcessState::Idle);
        if !state.is_live() {
            break;
        }
        assert!(Instant::now() < deadline, "app did not stop: {state:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn importing_the_same_folder_twice_is_refused() {
    let repos = synth_repos();
    let data = tempfile::tempdir().unwrap();
    let service = CoreService::open(DataDir::new(data.path())).expect("service");
    let handle = service.handle();
    let path = repos
        .path()
        .join("demo-worker")
        .to_string_lossy()
        .into_owned();

    let input = || {
        Box::new(CreateAppInput {
            path: path.clone(),
            name: None,
            node_version_pref: None,
            package_manager: None,
            default_script: None,
            first_task: Some(CreateTaskSpec {
                name: "worker".into(),
                command_kind: Some(CommandKind::Script),
                script: Some("start".into()),
                custom_command: None,
                working_dir_override: None,
            }),
            tasks: vec![],
            env_vars: vec![],
        })
    };

    handle
        .request(CoreCommand::CreateApp { input: input() })
        .await
        .expect("first import");
    let second = handle
        .request(CoreCommand::CreateApp { input: input() })
        .await;
    assert!(second.is_err(), "a duplicate import must be refused");

    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}
