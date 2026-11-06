//! Monitoring against real fixture processes: ports of a grandchild, confirmed vs hinted
//! ports, sampling a task that exits mid-sweep, and file watching on a synthetic project.

use neohub_core::monitor::ports::{PortTracker, listening_ports_of};
use neohub_core::monitor::stats::{read_process_table, sample_from_table};
use neohub_core::process::process_tree::group_members;
use neohub_core::process::runner::TaskRunner;
use neohub_core::process::spawn_spec::{DEFAULT_COLS, DEFAULT_ROWS, SpawnSpec};
use neohub_core::watch::{is_env_file, watch_env_files, watch_sources};
use neohub_domain::{AppId, ProcessState, TaskId};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../test-fixtures/process")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

fn spec_for(name: &str, args: &[&str]) -> SpawnSpec {
    let env: BTreeMap<String, String> = BTreeMap::from([
        (
            "PATH".to_string(),
            "/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin".to_string(),
        ),
        ("TERM".to_string(), "xterm-256color".to_string()),
    ]);
    let mut argv = vec![fixture(name)];
    argv.extend(args.iter().map(|a| a.to_string()));
    SpawnSpec {
        program: "/bin/sh".to_string(),
        args: argv,
        cwd: std::env::temp_dir(),
        env,
        initial_cols: DEFAULT_COLS,
        initial_rows: DEFAULT_ROWS,
        display: format!("sh {name}"),
    }
}

#[tokio::test]
async fn a_grandchild_listener_is_attributed_to_the_task() {
    let runner = TaskRunner::with_grace(Duration::from_millis(300));
    let id = TaskId::from_existing("task_tree_ports");
    let port = 4424u16;
    let info = runner
        .start(
            id.clone(),
            AppId::from_existing("app"),
            spec_for("spawn-listener.sh", &[&port.to_string()]),
        )
        .await
        .expect("start");

    // The listening socket belongs to a child of the task root, not the root itself.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut found = Vec::new();
    while Instant::now() < deadline {
        let members = group_members(info.pgid);
        found = listening_ports_of(&members);
        if found.contains(&port) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(
        found.contains(&port),
        "port {port} must be discovered through the process group, saw {found:?}"
    );
    assert!(
        !found.contains(&(std::process::id() as u16)),
        "NeoHub's own ports are never attributed to a task"
    );

    runner.shutdown().await;
}

#[tokio::test]
async fn confirmed_ports_and_log_hints_stay_separate() {
    let runner = TaskRunner::with_grace(Duration::from_millis(300));
    let id = TaskId::from_existing("task_ports_split");
    let port = 4425u16;
    let info = runner
        .start(
            id.clone(),
            AppId::from_existing("app"),
            spec_for("listens-on-port.sh", &[&port.to_string()]),
        )
        .await
        .expect("start");
    let mut tracker = PortTracker::new();

    // The task also prints a URL for a *different* port it never binds.
    tracker.observe_logs(
        &id,
        &format!("serving at http://localhost:{port}\nand http://localhost:4999"),
    );

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut sample = Default::default();
    while Instant::now() < deadline {
        let members = group_members(info.pgid);
        let listening = listening_ports_of(&members);
        sample = tracker.sweep(&id, &listening);
        if !sample.confirmed.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(sample.confirmed.contains(&port), "confirmed: {sample:?}");
    assert!(
        !sample.hinted.contains(&port),
        "a bound port is confirmed, not hinted: {sample:?}"
    );
    assert!(
        sample.hinted.contains(&4999),
        "an unbound URL stays a hint: {sample:?}"
    );

    // The unconfirmed hint ages out after three sweeps.
    for _ in 0..3 {
        sample = tracker.sweep(&id, &[]);
    }
    assert!(!sample.hinted.contains(&4999), "hints expire: {sample:?}");
    runner.shutdown().await;
}

#[tokio::test]
async fn sampling_a_task_that_just_exited_reports_zero_instead_of_panicking() {
    let runner = TaskRunner::with_grace(Duration::from_millis(300));
    let id = TaskId::from_existing("task_exit_sample");
    let info = runner
        .start(
            id.clone(),
            AppId::from_existing("app"),
            spec_for("steady-log.sh", &[]),
        )
        .await
        .expect("start");

    // A live task has a non-zero footprint.
    let table = read_process_table();
    let live = sample_from_table(&table, info.pid as i32);
    assert!(live.process_count >= 1);
    assert!(live.mem_mb > 0.0, "live task reports memory: {live:?}");

    runner.stop(&id).await.expect("stop");
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && runner.state_of(&id).is_live() {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(runner.state_of(&id), ProcessState::Exited);

    // Sampling the same pid right after exit is the "exit races the sampler" case.
    let table = read_process_table();
    let dead = sample_from_table(&table, info.pid as i32);
    assert_eq!(dead.process_count, 0);
    assert_eq!(dead.cpu_percent, 0.0);
    assert_eq!(dead.mem_mb, 0.0);
    runner.shutdown().await;
}

#[tokio::test]
async fn a_synthetic_project_reports_env_changes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = dir.path().join("synthetic-project");
    std::fs::create_dir_all(&project).unwrap();

    let env_watch = watch_env_files(&project).expect("env watch");
    std::thread::sleep(Duration::from_millis(400));

    std::fs::write(project.join(".env"), "DEMO=1\n").unwrap();
    std::fs::write(project.join("notes.txt"), "noise\n").unwrap();

    let deadline = Instant::now() + Duration::from_secs(6);
    let mut env_event = None;
    while Instant::now() < deadline && env_event.is_none() {
        env_event = env_watch.try_recv();
        std::thread::sleep(Duration::from_millis(50));
    }
    let env_event = env_event.expect("an .env change must be reported");
    assert!(is_env_file(&env_event.path), "got {env_event:?}");
    assert!(env_event.path.ends_with(".env"));
}

#[tokio::test]
async fn source_changes_are_reported_and_noise_is_ignored() {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = dir.path().join("synthetic-project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::create_dir_all(project.join("node_modules/pkg")).unwrap();

    let restart_watch = watch_sources(
        &project,
        vec!["src/**/*.js".into()],
        Duration::from_millis(250),
    )
    .expect("restart watch");
    std::thread::sleep(Duration::from_millis(400));

    std::fs::write(project.join("node_modules/pkg/index.js"), "x\n").unwrap();
    std::fs::write(project.join("README.md"), "x\n").unwrap();
    std::fs::write(project.join("src/app.js"), "console.log(1)\n").unwrap();

    let deadline = Instant::now() + Duration::from_secs(6);
    let mut batch = None;
    while Instant::now() < deadline && batch.is_none() {
        batch = restart_watch.try_recv();
        std::thread::sleep(Duration::from_millis(50));
    }
    let batch = batch.expect("a source change must be reported");
    assert!(batch.iter().any(|p| p.ends_with("src/app.js")), "{batch:?}");
    assert!(
        !batch
            .iter()
            .any(|p| p.to_string_lossy().contains("node_modules")),
        "generated files are ignored: {batch:?}"
    );
    assert!(
        !batch.iter().any(|p| p.ends_with("README.md")),
        "non-matching files are filtered by the glob: {batch:?}"
    );
}
