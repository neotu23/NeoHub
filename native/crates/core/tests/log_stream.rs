//! Log streaming: high-water marks, no duplication/gaps across a late subscribe, eviction
//! reporting, retention after exit, and "closing a window does not stop collection".
//!
//! Every process is a synthetic fixture.

use neohub_core::logs::search::SearchOptions;
use neohub_core::logs::{CHUNK_LIMIT, LogCursor, LogStore, LogSubscriptionOutcome};
use neohub_core::process::runner::{RunnerEvent, TaskRunner};
use neohub_core::process::spawn_spec::{DEFAULT_COLS, DEFAULT_ROWS, SpawnSpec};
use neohub_domain::{AppId, RunId, TaskId};
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
        display: format!("sh {name}"),
    }
}

fn task(id: &str) -> TaskId {
    TaskId::from_existing(id)
}

/// Collect `line NNNN` numbers from a text blob, in order.
fn numbered_lines(text: &str) -> Vec<u32> {
    text.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            trimmed
                .strip_prefix("line ")
                .and_then(|rest| rest.trim().parse::<u32>().ok())
        })
        .collect()
}

#[tokio::test]
async fn subscribing_late_neither_duplicates_nor_loses_numbered_output() {
    let runner = TaskRunner::with_grace(Duration::from_millis(300));
    let id = task("task_watermark");
    let mut events = runner.subscribe();

    runner
        .start(
            id.clone(),
            AppId::from_existing("app"),
            spec_for("numbered-output.sh", &[]),
        )
        .await
        .expect("start");

    // Wait until output has started, then subscribe mid-stream (the "mount" moment).
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && runner.logs_of(&id).is_empty() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let bootstrap = match runner.bootstrap_logs(&id, None) {
        Some(LogSubscriptionOutcome::Ok(bootstrap)) => bootstrap,
        Some(LogSubscriptionOutcome::ResetRequired(_)) => {
            panic!("a fresh subscribe must not reset")
        }
        None => panic!("no logs for a running task"),
    };
    let cursor_after_mount = bootstrap.cursor.clone();
    let mut text_at_mount = String::from_utf8_lossy(&bootstrap.raw_tail).into_owned();
    let parsed_at_mount = bootstrap.terminal.parsed_bytes;

    // Keep every chunk that arrives after the mount cursor.
    let mut live = String::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match tokio::time::timeout(Duration::from_millis(200), events.recv()).await {
            Ok(Ok(RunnerEvent::Log {
                task_id,
                run_id,
                seq,
                bytes,
            })) => {
                if task_id == id
                    && run_id == cursor_after_mount.run_id
                    && seq > cursor_after_mount.seq
                {
                    live.push_str(&String::from_utf8_lossy(&bytes));
                }
            }
            Ok(Ok(_)) => {}
            Ok(Err(_)) | Err(_) => {}
        }
        if numbered_lines(&text_at_mount).len() >= 1000 {
            break;
        }
        // Pull the authoritative tail too, so a slow event consumer cannot hide a gap.
        text_at_mount = runner.logs_of(&id);
        if numbered_lines(&text_at_mount).len() >= 1000 && Instant::now() > deadline {
            break;
        }
        if Instant::now() >= deadline {
            break;
        }
    }

    // Combine what the late subscriber saw: bootstrap tail + post-cursor events + the tail
    // it read afterwards. Numbers must appear in a strictly increasing, gap-free run.
    let combined = format!("{text_at_mount}{live}");
    let numbers = numbered_lines(&combined);
    assert!(
        numbers.len() >= 1000,
        "expected all 1000 numbered lines, saw {} ({:?}..)",
        numbers.len(),
        numbers.first()
    );
    let unique: std::collections::BTreeSet<u32> = numbers.iter().copied().collect();
    assert_eq!(
        unique.len(),
        numbers.len(),
        "no line may be delivered twice (duplicates in the stream)"
    );
    for (index, number) in unique.iter().take(1000).enumerate() {
        assert_eq!(
            *number,
            index as u32 + 1,
            "the numbered output must be gap-free"
        );
    }
    assert!(
        parsed_at_mount > 0,
        "the mount snapshot knows where parsing stopped"
    );

    runner.stop(&id).await.expect("stop");
    runner.shutdown().await;
}

#[tokio::test]
async fn a_cursor_from_a_previous_run_requires_a_reset() {
    let runner = TaskRunner::with_grace(Duration::from_millis(300));
    let id = task("task_reset");
    let first = runner
        .start(
            id.clone(),
            AppId::from_existing("app"),
            spec_for("steady-log.sh", &[]),
        )
        .await
        .expect("first start");
    tokio::time::sleep(Duration::from_millis(300)).await;
    runner.stop(&id).await.expect("stop");

    let stale = LogCursor {
        run_id: first.run_id.clone(),
        seq: 1,
    };
    runner
        .start(
            id.clone(),
            AppId::from_existing("app"),
            spec_for("steady-log.sh", &[]),
        )
        .await
        .expect("restart");
    tokio::time::sleep(Duration::from_millis(200)).await;

    match runner.bootstrap_logs(&id, Some(&stale)).expect("logs") {
        LogSubscriptionOutcome::ResetRequired(bootstrap) => {
            assert_ne!(
                bootstrap.cursor.run_id, stale.run_id,
                "the new run has its own id"
            );
        }
        other => panic!("expected ResetRequired, got {other:?}"),
    }
    runner.shutdown().await;
}

#[tokio::test]
async fn closing_the_subscription_does_not_stop_collection() {
    let runner = TaskRunner::with_grace(Duration::from_millis(300));
    let id = task("task_detached");
    runner
        .start(
            id.clone(),
            AppId::from_existing("app"),
            spec_for("steady-log.sh", &[]),
        )
        .await
        .expect("start");

    // A window "closes": its subscription is dropped.
    {
        let events = runner.subscribe();
        drop(events);
    }
    let before = runner.logs_of(&id).len();
    tokio::time::sleep(Duration::from_millis(600)).await;
    let after = runner.logs_of(&id).len();
    assert!(
        after > before,
        "PTY collection continues with no subscriber attached ({before} -> {after})"
    );
    runner.shutdown().await;
}

#[tokio::test]
async fn an_exited_task_keeps_searchable_logs_until_evicted() {
    let runner = TaskRunner::with_grace(Duration::from_millis(300));
    let id = task("task_exited_logs");
    runner
        .start(
            id.clone(),
            AppId::from_existing("app"),
            spec_for("exit-code.sh", &["4"]),
        )
        .await
        .expect("start");

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && runner.state_of(&id).is_live() {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let options = SearchOptions::default();
    let hits = runner
        .search_logs(&id, "exiting with 4", &options)
        .expect("search");
    assert_eq!(hits.len(), 1, "an exited task can still be searched");
    assert!(runner.logs_of(&id).contains("exiting with 4"));
    assert_eq!(
        runner.evict_expired_logs(neohub_storage::db::now_ms()),
        0,
        "not yet expired"
    );
    runner.shutdown().await;
}

#[tokio::test]
async fn clearing_logs_does_not_touch_the_running_process() {
    let runner = TaskRunner::with_grace(Duration::from_millis(300));
    let id = task("task_clear");
    let info = runner
        .start(
            id.clone(),
            AppId::from_existing("app"),
            spec_for("steady-log.sh", &[]),
        )
        .await
        .expect("start");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!runner.logs_of(&id).is_empty());

    assert!(runner.clear_logs(&id));
    assert!(runner.logs_of(&id).is_empty());
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(
        !runner.logs_of(&id).is_empty(),
        "new output after a clear is collected again"
    );
    assert_eq!(runner.state_of(&id), neohub_domain::ProcessState::Running);
    assert!(
        neohub_core::process::group_members(info.pgid).contains(&(info.pid as i32)),
        "the process is untouched by clearing logs"
    );
    runner.shutdown().await;
}

#[test]
fn the_log_store_reports_memory_usage_and_truncation() {
    let mut store = LogStore::with_budgets(1024, 4096, CHUNK_LIMIT);
    let id = task("t1");
    let run = RunId::from_existing("run1");
    for _ in 0..10 {
        store.append(&id, &run, &vec![b'a'; 512]);
    }
    let usage = store.usage();
    assert!(
        usage.total_bytes <= 4096,
        "global budget respected: {usage:?}"
    );
    assert!(
        usage.truncated_tasks.contains(&id),
        "truncation is reported"
    );
    assert_eq!(store.tracked_tasks(), 1);
}
