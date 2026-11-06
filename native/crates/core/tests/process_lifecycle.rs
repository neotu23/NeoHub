//! PTY lifecycle and race tests.
//!
//! Every process here is a synthetic fixture from `native/test-fixtures/process/`; no user
//! project is ever executed. Each test asserts on the *live* process table as well as on the
//! runner state, so "reported exited" can never hide a surviving child.

use neohub_core::process::process_tree::{current_process_group, group_members};
use neohub_core::process::runner::{RunnerError, TaskRunner, wait_for_settled};
use neohub_core::process::spawn_spec::{DEFAULT_COLS, DEFAULT_ROWS, SpawnSpec};
use neohub_domain::{AppId, ProcessState, TaskId};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../test-fixtures/process")
        .join(name)
}

fn spec_for(name: &str, args: &[&str]) -> SpawnSpec {
    let script = fixture(name);
    assert!(script.exists(), "missing fixture {name}");
    let env: BTreeMap<String, String> = BTreeMap::from([
        (
            "PATH".to_string(),
            "/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin".to_string(),
        ),
        ("TERM".to_string(), "xterm-256color".to_string()),
        ("FORCE_COLOR".to_string(), "1".to_string()),
    ]);
    let mut argv = vec![script.to_string_lossy().into_owned()];
    argv.extend(args.iter().map(|a| a.to_string()));
    SpawnSpec {
        program: "/bin/sh".to_string(),
        args: argv,
        cwd: std::env::temp_dir(),
        env,
        initial_cols: DEFAULT_COLS,
        initial_rows: DEFAULT_ROWS,
        display: format!("sh {name} {}", args.join(" ")).trim().to_string(),
    }
}

fn task(id: &str) -> TaskId {
    TaskId::from_existing(id)
}

fn app() -> AppId {
    AppId::from_existing("app_fixture")
}

#[tokio::test]
async fn same_task_concurrent_start_spawns_once() {
    let runner = TaskRunner::with_grace(Duration::from_millis(300));
    let id = task("task_single_spawn");
    let spec = spec_for("steady-log.sh", &[]);

    let (a, b) = tokio::join!(
        runner.start(id.clone(), app(), spec.clone()),
        runner.start(id.clone(), app(), spec.clone())
    );

    let ok = [&a, &b].iter().filter(|r| r.is_ok()).count();
    let refused = [&a, &b]
        .iter()
        .filter(|r| matches!(r, Err(RunnerError::AlreadyActive { .. })))
        .count();
    assert_eq!(ok, 1, "exactly one start may spawn");
    assert_eq!(
        refused, 1,
        "the loser is refused, not silently double-spawned"
    );

    let info = match (&a, &b) {
        (Ok(info), _) => info.clone(),
        (_, Ok(info)) => info.clone(),
        _ => panic!("neither start succeeded: {a:?} / {b:?}"),
    };
    assert!(!group_members(info.pgid).is_empty(), "the process exists");

    runner.shutdown().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(runner.state_of(&id), ProcessState::Exited);
    assert!(
        group_members(info.pgid).is_empty(),
        "no process may survive a shutdown"
    );
}

#[tokio::test]
async fn stop_during_start_leaves_no_orphan() {
    let runner = TaskRunner::with_grace(Duration::from_millis(300));
    let id = task("task_stop_during_start");
    let spec = spec_for("delayed-start.sh", &[]);

    let start = runner.start(id.clone(), app(), spec);
    // Give the start a head start, then stop while it is still spawning/just running.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let stop = runner.stop(&id);
    let (start_result, stop_result) = tokio::join!(start, stop);
    stop_result.expect("stop must succeed");
    let info = start_result.expect("the start still completes");

    let state = wait_for_settled(&runner, &id, Duration::from_secs(5)).await;
    assert!(
        matches!(state, ProcessState::Exited | ProcessState::Crashed),
        "unexpected state {state:?}"
    );
    assert!(
        group_members(info.pgid).is_empty(),
        "stopping during start must not leave an orphan"
    );
}

#[tokio::test]
async fn start_during_exit_waits_for_teardown() {
    let runner = TaskRunner::with_grace(Duration::from_millis(300));
    let id = task("task_start_during_exit");

    let first = runner
        .start(id.clone(), app(), spec_for("steady-log.sh", &[]))
        .await
        .expect("first start");
    let stop = runner.stop(&id);
    let restart = async {
        // A restart request that lands while the stop is tearing down.
        tokio::time::sleep(Duration::from_millis(20)).await;
        runner
            .start(id.clone(), app(), spec_for("steady-log.sh", &[]))
            .await
    };
    let (stop_result, restart_result) = tokio::join!(stop, restart);
    stop_result.expect("stop");
    let second = restart_result.expect("start after teardown");

    assert_ne!(first.run_id, second.run_id, "a restart is a new run");
    assert_eq!(runner.state_of(&id), ProcessState::Running);
    assert!(
        group_members(first.pgid).is_empty() || first.pgid == second.pgid,
        "the previous run's group is gone before the new one runs"
    );

    runner.shutdown().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(group_members(second.pgid).is_empty());
}

#[tokio::test]
async fn quit_during_spawn_reaps_late_child() {
    let runner = TaskRunner::with_grace(Duration::from_millis(300));
    let id = task("task_quit_during_spawn");

    let start = runner.start(id.clone(), app(), spec_for("steady-log.sh", &[]));
    let shutdown = async {
        tokio::time::sleep(Duration::from_millis(30)).await;
        runner.shutdown().await
    };
    let (start_result, stopped) = tokio::join!(start, shutdown);

    // Either the start finished before shutdown (then it got stopped) or it was refused /
    // reaped. In both cases nothing may survive and the state is terminal.
    match start_result {
        Ok(info) => {
            let state = wait_for_settled(&runner, &id, Duration::from_secs(5)).await;
            assert!(matches!(
                state,
                ProcessState::Exited | ProcessState::Crashed
            ));
            assert!(
                group_members(info.pgid).is_empty(),
                "a late child must be reaped, never left running"
            );
        }
        Err(err) => {
            assert!(
                matches!(err, RunnerError::ShuttingDown),
                "unexpected start error: {err}"
            );
            assert_eq!(runner.state_of(&id), ProcessState::Crashed);
        }
    }
    let _ = stopped;

    // New starts are refused after shutdown.
    let after = runner
        .start(id.clone(), app(), spec_for("steady-log.sh", &[]))
        .await;
    assert!(matches!(after, Err(RunnerError::ShuttingDown)));
}

#[tokio::test]
async fn start_streams_the_fixture_output_through_the_pty() {
    let runner = TaskRunner::with_grace(Duration::from_millis(300));
    let id = task("task_stream");
    let info = runner
        .start(id.clone(), app(), spec_for("steady-log.sh", &[]))
        .await
        .expect("start");

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut logs = String::new();
    while Instant::now() < deadline {
        logs = runner.logs_of(&id);
        if logs.contains("[fixture] tick 3") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        logs.contains("[fixture] tick 1"),
        "expected PTY output, got: {logs:?}"
    );
    assert!(info.pid > 0);

    runner.stop(&id).await.expect("stop");
    wait_for_settled(&runner, &id, Duration::from_secs(5)).await;
    assert!(group_members(info.pgid).is_empty());
}

#[tokio::test]
async fn stopping_kills_the_whole_group_including_grandchildren() {
    let runner = TaskRunner::with_grace(Duration::from_millis(300));
    let id = task("task_tree");
    let info = runner
        .start(id.clone(), app(), spec_for("spawn-children.sh", &[]))
        .await
        .expect("start");

    // Wait until both grandchildren exist in the group.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && group_members(info.pgid).len() < 3 {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let before = group_members(info.pgid);
    assert!(
        before.len() >= 3,
        "fixture should have a parent and two children, saw {before:?}"
    );
    assert!(
        !before.contains(&(std::process::id() as i32)),
        "NeoHub's own process must never be in a task group"
    );

    runner.stop(&id).await.expect("stop");
    wait_for_settled(&runner, &id, Duration::from_secs(5)).await;

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && !group_members(info.pgid).is_empty() {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        group_members(info.pgid).is_empty(),
        "the whole process tree must be gone, still alive: {:?}",
        group_members(info.pgid)
    );
}

#[tokio::test]
async fn a_process_that_ignores_term_is_killed_after_the_grace_period() {
    let runner = TaskRunner::with_grace(Duration::from_millis(400));
    let id = task("task_ignore_term");
    let info = runner
        .start(id.clone(), app(), spec_for("ignores-term.sh", &[]))
        .await
        .expect("start");

    tokio::time::sleep(Duration::from_millis(200)).await;
    let started = Instant::now();
    runner.stop(&id).await.expect("stop");
    let elapsed = started.elapsed();
    wait_for_settled(&runner, &id, Duration::from_secs(5)).await;

    assert!(
        elapsed >= Duration::from_millis(350),
        "TERM was ignored, so the grace period must have elapsed first (took {elapsed:?})"
    );
    assert!(
        group_members(info.pgid).is_empty(),
        "SIGKILL must finish it"
    );
}

#[tokio::test]
async fn a_non_zero_exit_is_recorded_as_crashed_with_its_code() {
    let runner = TaskRunner::with_grace(Duration::from_millis(300));
    let id = task("task_crash");
    let info = runner
        .start(id.clone(), app(), spec_for("exit-code.sh", &["5"]))
        .await
        .expect("start");

    let state = wait_for_settled(&runner, &id, Duration::from_secs(5)).await;
    assert_eq!(state, ProcessState::Crashed);
    let exit = runner.last_exit_of(&id).expect("exit recorded");
    assert_eq!(exit.code, Some(5));
    assert!(!exit.killed_by_user, "the process failed on its own");
    assert!(group_members(info.pgid).is_empty());
}

#[tokio::test]
async fn a_clean_exit_is_recorded_as_exited() {
    let runner = TaskRunner::with_grace(Duration::from_millis(300));
    let id = task("task_clean_exit");
    runner
        .start(id.clone(), app(), spec_for("exit-code.sh", &["0"]))
        .await
        .expect("start");
    let state = wait_for_settled(&runner, &id, Duration::from_secs(5)).await;
    assert_eq!(state, ProcessState::Exited);
    assert_eq!(runner.last_exit_of(&id).unwrap().code, Some(0));
}

#[tokio::test]
async fn a_user_stop_is_attributed_to_the_user_not_a_crash() {
    let runner = TaskRunner::with_grace(Duration::from_millis(300));
    let id = task("task_user_stop");
    runner
        .start(id.clone(), app(), spec_for("steady-log.sh", &[]))
        .await
        .expect("start");
    runner.stop(&id).await.expect("stop");
    let state = wait_for_settled(&runner, &id, Duration::from_secs(5)).await;

    assert_eq!(state, ProcessState::Exited, "a user stop is not a crash");
    let exit = runner.last_exit_of(&id).expect("exit");
    assert!(exit.killed_by_user, "the exit must record who stopped it");
}

#[tokio::test]
async fn resize_clamps_to_the_two_by_two_floor() {
    let runner = TaskRunner::with_grace(Duration::from_millis(300));
    let id = task("task_resize");
    runner
        .start(id.clone(), app(), spec_for("steady-log.sh", &[]))
        .await
        .expect("start");

    runner.resize(&id, 100, 40).await.expect("resize");
    // 1×1 is clamped up to the 2×2 floor rather than rejected: the terminal may briefly
    // report a degenerate grid while a split is being dragged.
    runner.resize(&id, 1, 1).await.expect("clamped to 2x2");
    runner
        .resize(&id, 0, 40)
        .await
        .expect_err("a zero column count is not a usable grid");

    runner.shutdown().await;
}

#[tokio::test]
async fn shutdown_stops_every_live_task() {
    let runner = TaskRunner::with_grace(Duration::from_millis(300));
    let mut groups = Vec::new();
    for index in 0..3 {
        let id = task(&format!("task_multi_{index}"));
        let info = runner
            .start(id.clone(), app(), spec_for("steady-log.sh", &[]))
            .await
            .expect("start");
        groups.push(info.pgid);
    }
    assert_eq!(runner.running_tasks().len(), 3);

    let stopped = runner.shutdown().await;
    assert_eq!(stopped, 3, "every live task is stopped");
    tokio::time::sleep(Duration::from_millis(300)).await;
    for pgid in groups {
        assert!(
            group_members(pgid).is_empty(),
            "group {pgid} survived shutdown"
        );
    }
    assert!(runner.running_tasks().is_empty());
}

#[tokio::test]
async fn the_runner_never_targets_its_own_process_group() {
    let runner = TaskRunner::with_grace(Duration::from_millis(300));
    let id = task("task_self_safety");
    let info = runner
        .start(id.clone(), app(), spec_for("steady-log.sh", &[]))
        .await
        .expect("start");
    assert_ne!(
        info.pgid,
        current_process_group(),
        "a task group must never be NeoHub's own group"
    );
    runner.stop(&id).await.expect("stop");
    assert!(group_members(current_process_group()).contains(&(std::process::id() as i32)));
}
