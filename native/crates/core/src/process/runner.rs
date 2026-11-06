//! The PTY runner: one live process per task, serialised state transitions.
//!
//! Invariants this module enforces (each has a test in `tests/process_lifecycle.rs`):
//!
//! * **one spawn per task** — concurrent `start` calls for the same task serialise on a
//!   per-task lock, and the second one observes `Starting`/`Running` and is refused;
//! * **no orphans** — a `stop` that arrives while a spawn is in flight waits for the spawn
//!   to finish and then stops the process it produced;
//! * **no re-entrancy** — `start` during `exiting` waits for the teardown to complete;
//! * **shutdown reaps late children** — once `shutdown` is set, a spawn that completes
//!   afterwards is terminated immediately instead of being left behind;
//! * **one settlement per run** — an exit is applied only for the current generation, so a
//!   stale waiter (or a double signal) cannot rewrite the state;
//! * **only our own processes** — the group is captured at spawn time; nothing is ever
//!   killed from a pid stored in the database.

use super::process_tree::{
    DEFAULT_GRACE, TerminationOutcome, current_process_group, terminate_group,
};
use super::spawn_spec::{SpawnSpec, resize_grid};
use crate::logs::{LogCursor, LogStore, LogSubscriptionOutcome, SearchOptions};
use neohub_domain::{AppId, ProcessState, RunId, TaskId};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::collections::HashMap;
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

/// Per-task raw log budget kept in memory (phase 6 adds the full bounded terminal state).
pub const LOG_BUDGET_BYTES: usize = 5 * 1024 * 1024;

/// How an event fan-out behaves when a subscriber falls behind.
pub const EVENT_CHANNEL_CAPACITY: usize = 1024;

/// Outcome of one run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExitInfo {
    pub code: Option<i32>,
    pub signal: Option<String>,
    /// True when the stop came from the user (stop/restart/shutdown) rather than the process.
    pub killed_by_user: bool,
}

/// Everything the core needs to know about one task's lifecycle.
#[derive(Clone, Debug)]
pub enum RunnerEvent {
    Status {
        task_id: TaskId,
        app_id: AppId,
        state: ProcessState,
        pid: Option<u32>,
        exit: Option<ExitInfo>,
        run_id: RunId,
    },
    Log {
        task_id: TaskId,
        run_id: RunId,
        seq: u64,
        bytes: Vec<u8>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum RunnerError {
    #[error("task {task_id} is already {state}")]
    AlreadyActive {
        task_id: String,
        state: &'static str,
    },

    #[error("the core is shutting down")]
    ShuttingDown,

    #[error("no such task: {task_id}")]
    UnknownTask { task_id: String },

    #[error("failed to start task {task_id}: {message}")]
    Spawn { task_id: String, message: String },

    #[error("failed to resize task {task_id}: {message}")]
    Resize { task_id: String, message: String },
}

/// Snapshot of one running task, for `GetSnapshot` and the UI.
#[derive(Clone, Debug)]
pub struct RunningTaskInfo {
    pub task_id: TaskId,
    pub app_id: AppId,
    pub pid: u32,
    pub pgid: i32,
    pub run_id: RunId,
    pub state: ProcessState,
    pub started_at: i64,
    pub command: String,
}

struct LiveTask {
    app_id: AppId,
    pid: u32,
    pgid: i32,
    run_id: RunId,
    started_at: i64,
    command: String,
    master: Mutex<Box<dyn MasterPty + Send>>,
}

struct Slot {
    state: ProcessState,
    generation: u64,
    op: Arc<tokio::sync::Mutex<()>>,
    live: Option<LiveTask>,
    last_exit: Option<ExitInfo>,
    /// Set while a user-initiated stop is in progress, so the exit is attributed correctly.
    stopping_by_user: bool,
}

impl Slot {
    fn new() -> Self {
        Slot {
            state: ProcessState::Idle,
            generation: 0,
            op: Arc::new(tokio::sync::Mutex::new(())),
            live: None,
            last_exit: None,
            stopping_by_user: false,
        }
    }
}

struct Inner {
    slots: Mutex<HashMap<TaskId, Slot>>,
    events: broadcast::Sender<RunnerEvent>,
    shutting_down: Mutex<bool>,
    revision: AtomicU64,
    grace: Duration,
    /// Bounded logs + parsed terminal state, shared by every task.
    logs: Mutex<LogStore>,
}

/// Owns every live task process.
#[derive(Clone)]
pub struct TaskRunner {
    inner: Arc<Inner>,
}

impl TaskRunner {
    pub fn new() -> Self {
        Self::with_grace(DEFAULT_GRACE)
    }

    /// Custom grace period (tests use a short one to keep the suite fast).
    pub fn with_grace(grace: Duration) -> Self {
        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        TaskRunner {
            inner: Arc::new(Inner {
                slots: Mutex::new(HashMap::new()),
                events,
                shutting_down: Mutex::new(false),
                revision: AtomicU64::new(0),
                grace,
                logs: Mutex::new(LogStore::new()),
            }),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RunnerEvent> {
        self.inner.events.subscribe()
    }

    pub fn revision(&self) -> u64 {
        self.inner.revision.load(Ordering::SeqCst)
    }

    /// Live tasks, for snapshots.
    pub fn running_tasks(&self) -> Vec<RunningTaskInfo> {
        let slots = self.inner.slots.lock().expect("runner lock");
        slots
            .iter()
            .filter_map(|(task_id, slot)| {
                slot.live.as_ref().map(|live| RunningTaskInfo {
                    task_id: task_id.clone(),
                    app_id: live.app_id.clone(),
                    pid: live.pid,
                    pgid: live.pgid,
                    run_id: live.run_id.clone(),
                    state: slot.state,
                    started_at: live.started_at,
                    command: live.command.clone(),
                })
            })
            .collect()
    }

    pub fn state_of(&self, task_id: &TaskId) -> ProcessState {
        self.inner
            .slots
            .lock()
            .expect("runner lock")
            .get(task_id)
            .map(|slot| slot.state)
            .unwrap_or(ProcessState::Idle)
    }

    pub fn last_exit_of(&self, task_id: &TaskId) -> Option<ExitInfo> {
        self.inner
            .slots
            .lock()
            .expect("runner lock")
            .get(task_id)
            .and_then(|slot| slot.last_exit.clone())
    }

    /// Retained raw text for one task (bounded).
    pub fn logs_of(&self, task_id: &TaskId) -> String {
        self.inner
            .logs
            .lock()
            .expect("logs lock")
            .raw_text(task_id)
            .unwrap_or_default()
    }

    /// Last `limit` bytes of retained output (cheap enough to call every sweep).
    pub fn logs_tail(&self, task_id: &TaskId, limit: usize) -> String {
        let text = self
            .inner
            .logs
            .lock()
            .expect("logs lock")
            .raw_text(task_id)
            .unwrap_or_default();
        if text.len() <= limit {
            return text;
        }
        let mut start = text.len() - limit;
        // Never split a UTF-8 character.
        while start < text.len() && !text.is_char_boundary(start) {
            start += 1;
        }
        text[start..].to_string()
    }

    /// Take a subscription bootstrap (terminal + cursor + bounded raw tail).
    pub fn bootstrap_logs(
        &self,
        task_id: &TaskId,
        after: Option<&LogCursor>,
    ) -> Option<LogSubscriptionOutcome> {
        self.inner
            .logs
            .lock()
            .expect("logs lock")
            .bootstrap(task_id, after)
    }

    /// Search the retained raw text of one task.
    pub fn search_logs(
        &self,
        task_id: &TaskId,
        query: &str,
        options: &SearchOptions,
    ) -> Result<Vec<crate::logs::LineMatch>, crate::logs::LogError> {
        self.inner
            .logs
            .lock()
            .expect("logs lock")
            .search_task(task_id, query, options)
    }

    /// Search every retained task (global log search).
    pub fn search_logs_all(
        &self,
        query: &str,
        options: &SearchOptions,
    ) -> Result<Vec<(TaskId, crate::logs::LineMatch)>, crate::logs::LogError> {
        self.inner
            .logs
            .lock()
            .expect("logs lock")
            .search_all(query, options)
    }

    /// Drop one task's logs (never touches the process).
    pub fn clear_logs(&self, task_id: &TaskId) -> bool {
        self.inner.logs.lock().expect("logs lock").clear(task_id)
    }

    /// Evict logs of tasks that exited longer than the retention window ago.
    pub fn evict_expired_logs(&self, now_ms: i64) -> usize {
        self.inner
            .logs
            .lock()
            .expect("logs lock")
            .evict_expired(now_ms)
    }

    /// Reported log memory usage ("bounded" is measurable, not asserted).
    pub fn log_usage(&self) -> crate::logs::LogUsage {
        self.inner.logs.lock().expect("logs lock").usage()
    }

    /// Start one task from a fully resolved spec.
    pub async fn start(
        &self,
        task_id: TaskId,
        app_id: AppId,
        spec: SpawnSpec,
    ) -> Result<RunningTaskInfo, RunnerError> {
        let (op, generation) = {
            let mut slots = self.inner.slots.lock().expect("runner lock");
            let slot = slots.entry(task_id.clone()).or_insert_with(Slot::new);
            (slot.op.clone(), slot.generation)
        };

        // Serialises start/stop for this task: a stop that arrives now waits here.
        let _guard = op.lock().await;

        if *self.inner.shutting_down.lock().expect("shutdown lock") {
            return Err(RunnerError::ShuttingDown);
        }
        {
            let mut slots = self.inner.slots.lock().expect("runner lock");
            let slot = slots.get_mut(&task_id).expect("slot exists");
            if slot.state.is_live() {
                return Err(RunnerError::AlreadyActive {
                    task_id: task_id.to_string(),
                    state: slot.state.as_str(),
                });
            }
            slot.state = ProcessState::Starting;
            slot.last_exit = None;
            slot.generation += 1;
        }
        let generation = {
            let slots = self.inner.slots.lock().expect("runner lock");
            slots
                .get(&task_id)
                .map(|slot| slot.generation)
                .unwrap_or(generation)
        };
        self.emit_status(
            &task_id,
            &app_id,
            ProcessState::Starting,
            None,
            None,
            generation,
        );

        // Opening a PTY and forking are blocking syscalls: keep them off the reactor.
        let spawned = tokio::task::spawn_blocking({
            let spec = spec.clone();
            move || open_pty(&spec)
        })
        .await;

        let (master, mut child, reader, pgid_hint) = match spawned {
            Ok(Ok(value)) => value,
            Ok(Err(message)) => {
                self.finish_start_failure(&task_id, &app_id, generation);
                return Err(RunnerError::Spawn {
                    task_id: task_id.to_string(),
                    message,
                });
            }
            Err(join_error) => {
                self.finish_start_failure(&task_id, &app_id, generation);
                return Err(RunnerError::Spawn {
                    task_id: task_id.to_string(),
                    message: join_error.to_string(),
                });
            }
        };

        let pid = child.process_id().unwrap_or(0);
        // The PTY child is a session leader (setsid in portable-pty), so its pid is the
        // process-group id. Prefer the master's own report when available.
        let pgid = pgid_hint.unwrap_or(pid as i32);
        let run_id = RunId::new();
        let started_at = neohub_storage::db::now_ms();
        let command = spec.display.clone();

        // A shutdown may have begun while we were forking: reap immediately.
        if *self.inner.shutting_down.lock().expect("shutdown lock") {
            let _ = tokio::task::spawn_blocking(move || {
                terminate_group(pgid, Duration::from_millis(500))
            })
            .await;
            drop(child);
            self.finish_start_failure(&task_id, &app_id, generation);
            return Err(RunnerError::ShuttingDown);
        }

        {
            let mut slots = self.inner.slots.lock().expect("runner lock");
            let slot = slots.get_mut(&task_id).expect("slot exists");
            slot.state = ProcessState::Running;
            slot.stopping_by_user = false;
            slot.live = Some(LiveTask {
                app_id: app_id.clone(),
                pid,
                pgid,
                run_id: run_id.clone(),
                started_at,
                command: command.clone(),
                master: Mutex::new(master),
            });
        }
        self.emit_status(
            &task_id,
            &app_id,
            ProcessState::Running,
            Some(pid),
            None,
            generation,
        );

        // Register the run so a subscriber can bootstrap before the first byte arrives.
        self.inner
            .logs
            .lock()
            .expect("logs lock")
            .ensure_run(&task_id, &run_id);

        // PTY reader: blocking by nature, so it gets its own thread.
        {
            let inner = self.inner.clone();
            let task_id = task_id.clone();
            let run_id = run_id.clone();
            std::thread::Builder::new()
                .name(format!("pty-read-{}", task_id))
                .spawn(move || {
                    let mut reader = reader;
                    let mut buffer = [0u8; 8192];
                    loop {
                        match reader.read(&mut buffer) {
                            Ok(0) => break,
                            Ok(n) => {
                                let chunk = &buffer[..n];
                                // Bound the work per lock hold: split into <=10 KiB events.
                                let outcome = inner
                                    .logs
                                    .lock()
                                    .expect("logs lock")
                                    .append(&task_id, &run_id, chunk);
                                for (seq, bytes) in outcome.chunks {
                                    let _ = inner.events.send(RunnerEvent::Log {
                                        task_id: task_id.clone(),
                                        run_id: run_id.clone(),
                                        seq,
                                        bytes,
                                    });
                                }
                            }
                            Err(_) => break,
                        }
                    }
                })
                .ok();
        }

        // Waiter: the single place an exit is settled.
        {
            let inner = self.inner.clone();
            let task_id = task_id.clone();
            let app_id = app_id.clone();
            let run_id = run_id.clone();
            std::thread::Builder::new()
                .name(format!("pty-wait-{}", task_id))
                .spawn(move || {
                    let status = child.wait();
                    let exit = match status {
                        Ok(status) => {
                            let code = if status.signal().is_some() {
                                None
                            } else {
                                Some(status.exit_code() as i32)
                            };
                            ExitInfo {
                                code,
                                signal: status.signal().map(str::to_string),
                                killed_by_user: false,
                            }
                        }
                        Err(err) => ExitInfo {
                            code: None,
                            signal: Some(format!("wait failed: {err}")),
                            killed_by_user: false,
                        },
                    };
                    settle_exit(&inner, &task_id, &app_id, &run_id, generation, exit);
                })
                .ok();
        }

        Ok(RunningTaskInfo {
            task_id,
            app_id,
            pid,
            pgid,
            run_id,
            state: ProcessState::Running,
            started_at,
            command,
        })
    }

    /// Stop one task: TERM the group, then KILL after the grace period.
    pub async fn stop(&self, task_id: &TaskId) -> Result<(), RunnerError> {
        let op = {
            let slots = self.inner.slots.lock().expect("runner lock");
            match slots.get(task_id) {
                Some(slot) => slot.op.clone(),
                None => return Ok(()), // never started: nothing to stop
            }
        };
        let _guard = op.lock().await;

        let (pgid, app_id, generation, run_id) = {
            let mut slots = self.inner.slots.lock().expect("runner lock");
            let Some(slot) = slots.get_mut(task_id) else {
                return Ok(());
            };
            if !slot.state.is_live() || slot.live.is_none() {
                return Ok(());
            }
            slot.state = ProcessState::Exiting;
            slot.stopping_by_user = true;
            let live = slot.live.as_ref().expect("live task");
            (
                live.pgid,
                AppId::from_existing(""),
                slot.generation,
                live.run_id.clone(),
            )
        };
        self.emit_status(
            task_id,
            &app_id,
            ProcessState::Exiting,
            None,
            None,
            generation,
        );

        let grace = self.inner.grace;
        let outcome = tokio::task::spawn_blocking(move || terminate_group(pgid, grace))
            .await
            .map_err(|err| RunnerError::Spawn {
                task_id: task_id.to_string(),
                message: err.to_string(),
            })?;

        // The waiter thread settles the state; if the process was already gone it has
        // already done so. Either way the task is no longer live.
        let inner = self.inner.clone();
        let task_id_owned = task_id.clone();
        let mut attempts = 0;
        while inner
            .slots
            .lock()
            .expect("runner lock")
            .get(&task_id_owned)
            .map(|slot| slot.state)
            .map(|state| state.is_live())
            .unwrap_or(false)
        {
            attempts += 1;
            if attempts > 400 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        // If no waiter ever settled it (e.g. the child vanished before wait was armed),
        // settle here so the UI cannot sit in "exiting" forever.
        {
            let mut slots = self.inner.slots.lock().expect("runner lock");
            if let Some(slot) = slots.get_mut(&task_id_owned)
                && slot.state.is_live()
            {
                slot.state = ProcessState::Exited;
                slot.live = None;
                slot.last_exit = Some(ExitInfo {
                    code: None,
                    signal: None,
                    killed_by_user: true,
                });
            }
        }
        if let Ok(TerminationOutcome::Refused) = outcome {
            // Should not happen for a group we spawned; surfaced rather than ignored.
            eprintln!("[runner] refused to terminate group for task {run_id}");
        }
        Ok(())
    }

    /// Resize a live task's PTY (min 2×2).
    pub async fn resize(&self, task_id: &TaskId, cols: u16, rows: u16) -> Result<(), RunnerError> {
        let (cols, rows) = resize_grid(cols, rows).map_err(|err| RunnerError::Resize {
            task_id: task_id.to_string(),
            message: err.to_string(),
        })?;
        // `MasterPty::resize` is a synchronous ioctl, so the lock is never held across an
        // await point.
        let slots = self.inner.slots.lock().expect("runner lock");
        let Some(live) = slots.get(task_id).and_then(|slot| slot.live.as_ref()) else {
            return Ok(()); // resizing a task that is not running is a no-op, not an error
        };
        let master = live.master.lock().expect("master lock");
        self.inner
            .logs
            .lock()
            .expect("logs lock")
            .resize(task_id, cols, rows);
        master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|err| RunnerError::Resize {
                task_id: task_id.to_string(),
                message: err.to_string(),
            })
    }

    /// Refuse new starts, then stop everything that is live. Idempotent.
    pub async fn shutdown(&self) -> usize {
        {
            let mut flag = match self.inner.shutting_down.lock() {
                Ok(flag) => flag,
                Err(poisoned) => poisoned.into_inner(),
            };
            *flag = true;
        }
        let task_ids: Vec<TaskId> = {
            let slots = self.inner.slots.lock().expect("runner lock");
            slots.keys().cloned().collect()
        };
        let mut stopped = 0;
        for task_id in task_ids {
            if self.state_of(&task_id).is_live() {
                let _ = self.stop(&task_id).await;
                stopped += 1;
            }
        }
        stopped
    }

    fn finish_start_failure(&self, task_id: &TaskId, app_id: &AppId, generation: u64) {
        {
            let mut slots = self.inner.slots.lock().expect("runner lock");
            if let Some(slot) = slots.get_mut(task_id) {
                slot.state = ProcessState::Crashed;
                slot.live = None;
                slot.last_exit = Some(ExitInfo {
                    code: None,
                    signal: Some("spawn failed".into()),
                    killed_by_user: false,
                });
            }
        }
        self.emit_status(
            task_id,
            app_id,
            ProcessState::Crashed,
            None,
            None,
            generation,
        );
    }

    fn emit_status(
        &self,
        task_id: &TaskId,
        app_id: &AppId,
        state: ProcessState,
        pid: Option<u32>,
        exit: Option<ExitInfo>,
        generation: u64,
    ) {
        self.inner.revision.fetch_add(1, Ordering::SeqCst);
        let run_id = {
            let slots = self.inner.slots.lock().expect("runner lock");
            slots
                .get(task_id)
                .and_then(|slot| slot.live.as_ref().map(|live| live.run_id.clone()))
                .unwrap_or_default()
        };
        let _ = self.inner.events.send(RunnerEvent::Status {
            task_id: task_id.clone(),
            app_id: app_id.clone(),
            state,
            pid,
            exit,
            run_id,
        });
        let _ = generation;
    }
}

impl Default for TaskRunner {
    fn default() -> Self {
        Self::new()
    }
}

/// Apply an exit exactly once, for the current generation of the task.
fn settle_exit(
    inner: &Arc<Inner>,
    task_id: &TaskId,
    app_id: &AppId,
    run_id: &RunId,
    generation: u64,
    mut exit: ExitInfo,
) {
    let emit;
    {
        let mut slots = inner.slots.lock().expect("runner lock");
        let Some(slot) = slots.get_mut(task_id) else {
            return;
        };
        if slot.generation != generation {
            // A stale waiter from a previous run: never rewrite the current state.
            return;
        }
        if slot.state == ProcessState::Exited || slot.state == ProcessState::Crashed {
            return; // already settled
        }
        exit.killed_by_user = slot.stopping_by_user;
        let crashed =
            !exit.killed_by_user && (exit.code.unwrap_or(1) != 0 || exit.signal.is_some());
        slot.state = if crashed {
            ProcessState::Crashed
        } else {
            ProcessState::Exited
        };
        slot.stopping_by_user = false;
        slot.last_exit = Some(exit.clone());
        slot.live = None;
        emit = Some(slot.state);
    }
    inner
        .logs
        .lock()
        .expect("logs lock")
        .mark_exited(task_id, neohub_storage::db::now_ms());
    if let Some(state) = emit {
        inner.revision.fetch_add(1, Ordering::SeqCst);
        let _ = inner.events.send(RunnerEvent::Status {
            task_id: task_id.clone(),
            app_id: app_id.clone(),
            state,
            pid: None,
            exit: Some(exit),
            run_id: run_id.clone(),
        });
    }
}

type SpawnedPty = (
    Box<dyn MasterPty + Send>,
    Box<dyn portable_pty::Child + Send + Sync>,
    Box<dyn Read + Send>,
    Option<i32>,
);

fn open_pty(spec: &SpawnSpec) -> Result<SpawnedPty, String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: spec.initial_rows,
            cols: spec.initial_cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|err| err.to_string())?;

    let mut builder = CommandBuilder::new(&spec.program);
    builder.args(&spec.args);
    builder.cwd(&spec.cwd);
    for (key, value) in &spec.env {
        builder.env(key, value);
    }

    let child = pair
        .slave
        .spawn_command(builder)
        .map_err(|err| err.to_string())?;
    // The slave handle must go away so the reader sees EOF when the child exits.
    drop(pair.slave);

    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|err| err.to_string())?;
    let pgid = pair.master.process_group_leader();
    if let Some(pgid) = pgid
        && pgid == current_process_group()
    {
        return Err("refusing to run a task in NeoHub's own process group".to_string());
    }
    Ok((pair.master, child, reader, pgid))
}

/// Wait until a task leaves the live states, or the deadline passes.
pub async fn wait_for_settled(
    runner: &TaskRunner,
    task_id: &TaskId,
    timeout: Duration,
) -> ProcessState {
    let deadline = Instant::now() + timeout;
    loop {
        let state = runner.state_of(task_id);
        if !state.is_live() || Instant::now() >= deadline {
            return state;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
