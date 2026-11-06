//! The core service: one owner of the database, the process runner and the event fan-out.
//!
//! Ordering rules implemented here (`native/docs/interfaces.md` §5):
//!
//! * `Shutdown` first refuses new starts (via the runner), then stops every managed task,
//!   then writes the run history, then checkpoints the database. A window closing never
//!   calls this — only an explicit quit does.
//! * Run history is written **after** the spawn is registered, and a history failure never
//!   un-registers a live process: the process is still reaped, only the row is missing.
//! * Events carry the snapshot revision; a subscriber that lags gets `ResyncRequired`.

use crate::catalog::{CoreError, CoreResult};
use crate::commands::{CoreCommand, CoreReply};
use crate::environment::env_builder::{BuildEnvInput, build_env};
use crate::environment::node_resolver::NodeResolver;
use crate::environment::path_probe::PathProbe;
use crate::events::{CoreEvent, CoreEventKind};
use crate::monitor::ports::{PortSample, PortTracker, listening_ports_of};
use crate::monitor::stats::{StatsSample, clamp_interval, read_process_table, sample_from_table};
use crate::orchestrator::readiness::{ProbeContext, ReadinessOutcome, wait_for_readiness};
use crate::orchestrator::state::StickyOutcomes;
use crate::orchestrator::topology::{TopologyError, plan_start, plan_stop};
use crate::process::runner::{RunnerEvent, TaskRunner, wait_for_settled};
use crate::process::spawn_spec::{DEFAULT_COLS, DEFAULT_ROWS, SpawnSpec};
use crate::snapshot::CoreSnapshot;
use neohub_domain::{App, AppId, DomainError, ProcessState, RunHistoryRow, Settings, Task, TaskId};
use neohub_storage::db::now_ms;
use neohub_storage::secrets::{FakeSecretStore, SecretStore, read_secret_value};
use neohub_storage::{DataDir, Storage};
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{Notify, broadcast};

const EVENT_CAPACITY: usize = 2048;

/// Owns everything that must survive a window being destroyed.
pub struct CoreService {
    storage: Mutex<Option<Storage>>,
    runner: TaskRunner,
    events: broadcast::Sender<CoreEvent>,
    revision: Mutex<u64>,
    shutting_down: AtomicBool,
    node_resolver: Arc<NodeResolver>,
    path_probe: PathProbe,
    secrets: Arc<dyn SecretStore>,
    /// Sticky per-app outcome (a stopped app keeps reading stopped, not idle).
    outcomes: Mutex<StickyOutcomes>,
    /// In-flight start cancellation flags, one per app.
    start_cancels: Mutex<HashMap<AppId, Arc<AtomicBool>>>,
    /// Readiness result per task for the current run.
    ready_flags: Mutex<HashMap<TaskId, (String, bool)>>,
    /// Wakes the monitor loop when a task starts (idle loops must not poll).
    monitor_notify: Arc<Notify>,
    /// How many sampling sweeps ran (test/observability hook: idle must stay at 0 growth).
    monitor_sweeps: Arc<AtomicU64>,
    port_tracker: Mutex<PortTracker>,
    stats_cache: Mutex<BTreeMap<TaskId, StatsSample>>,
    ports_cache: Mutex<BTreeMap<TaskId, PortSample>>,
}

/// Handle given to the UI and to MCP.
#[derive(Clone)]
pub struct CoreHandle {
    service: Arc<CoreService>,
}

impl CoreService {
    /// Open the database in `data_dir` and start the event pump.
    pub fn open(data_dir: DataDir) -> CoreResult<Arc<Self>> {
        let storage = Storage::open(data_dir).map_err(CoreError::Storage)?;
        Ok(Self::build(storage))
    }

    /// In-memory service for tests.
    pub fn in_memory() -> CoreResult<Arc<Self>> {
        let storage = Storage::open_in_memory().map_err(CoreError::Storage)?;
        Ok(Self::build(storage))
    }

    fn build(storage: Storage) -> Arc<Self> {
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let service = Arc::new(CoreService {
            storage: Mutex::new(Some(storage)),
            runner: TaskRunner::new(),
            events,
            revision: Mutex::new(0),
            shutting_down: AtomicBool::new(false),
            node_resolver: Arc::new(NodeResolver::new()),
            path_probe: PathProbe::new(),
            secrets: Arc::new(FakeSecretStore::new()),
            outcomes: Mutex::new(StickyOutcomes::new()),
            start_cancels: Mutex::new(HashMap::new()),
            ready_flags: Mutex::new(HashMap::new()),
            monitor_notify: Arc::new(Notify::new()),
            monitor_sweeps: Arc::new(AtomicU64::new(0)),
            port_tracker: Mutex::new(PortTracker::new()),
            stats_cache: Mutex::new(BTreeMap::new()),
            ports_cache: Mutex::new(BTreeMap::new()),
        });
        Self::spawn_pump(&service);
        Self::spawn_monitor(&service);
        service
    }

    /// Forward runner events as core events, and keep the DB's run history in step.
    fn spawn_pump(service: &Arc<Self>) {
        let weak = Arc::downgrade(service);
        let mut rx = service.runner.subscribe();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        let Some(service) = weak.upgrade() else {
                            return;
                        };
                        service.on_runner_event(event);
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        let Some(service) = weak.upgrade() else {
                            return;
                        };
                        let revision = service.bump_revision();
                        let _ = service.events.send(CoreEvent {
                            revision,
                            kind: CoreEventKind::ResyncRequired { revision },
                        });
                        eprintln!(
                            "[core] event subscriber lagged by {skipped} events; resync sent"
                        );
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
    }

    fn on_runner_event(&self, event: RunnerEvent) {
        match event {
            RunnerEvent::Status {
                task_id,
                app_id,
                state,
                pid,
                exit,
                run_id,
            } => {
                // Settle history on a terminal state. A failure here is logged, never fatal:
                // the process is already reaped, only the row would be missing.
                if matches!(state, ProcessState::Exited | ProcessState::Crashed) {
                    self.finish_history(&task_id, &run_id, exit.as_ref());
                    self.ready_flags
                        .lock()
                        .expect("ready flags")
                        .remove(&task_id);
                    // Sticky outcome: an app whose tasks all end must not read as idle.
                    self.outcomes
                        .lock()
                        .expect("outcomes")
                        .record(&app_id, state);
                }
                let revision = self.bump_revision();
                let _ = self.events.send(CoreEvent {
                    revision,
                    kind: CoreEventKind::TaskStatus {
                        task_id,
                        app_id,
                        state,
                        pid,
                        run_id,
                        exit,
                    },
                });
            }
            RunnerEvent::Log {
                task_id,
                run_id,
                seq,
                bytes,
            } => {
                let revision = self.bump_revision();
                let _ = self.events.send(CoreEvent {
                    revision,
                    kind: CoreEventKind::Log {
                        task_id,
                        run_id,
                        seq,
                        bytes,
                    },
                });
            }
        }
    }

    pub fn handle(self: &Arc<Self>) -> CoreHandle {
        CoreHandle {
            service: self.clone(),
        }
    }

    pub fn runner(&self) -> &TaskRunner {
        &self.runner
    }

    /// Run a storage operation while the database is still open.
    fn with_storage<T>(&self, f: impl FnOnce(&Storage) -> CoreResult<T>) -> CoreResult<T> {
        let guard = self.storage.lock().expect("storage lock");
        let storage = guard
            .as_ref()
            .ok_or(CoreError::Domain(DomainError::InvalidInput {
                field: "core",
                message: "the database is closed (shutdown already ran)".into(),
            }))?;
        f(storage)
    }

    /// Direct database access for tests and for the (future) migration inspector.
    /// Returns `None` once the shutdown has closed the connection.
    #[doc(hidden)]
    pub fn storage_call<T>(&self, f: impl FnOnce(&Storage) -> T) -> Option<T> {
        let guard = self.storage.lock().ok()?;
        guard.as_ref().map(f)
    }

    fn bump_revision(&self) -> u64 {
        let mut revision = self.revision.lock().expect("revision lock");
        *revision += 1;
        *revision
    }

    pub fn revision(&self) -> u64 {
        *self.revision.lock().expect("revision lock")
    }

    pub fn snapshot(&self) -> CoreResult<CoreSnapshot> {
        let revision = self.revision();
        let mut apps: Vec<App> = Vec::new();
        let mut tasks_by_app: BTreeMap<AppId, Vec<Task>> = BTreeMap::new();
        let mut settings = Settings::default();
        if !self.shutting_down.load(Ordering::SeqCst) {
            self.with_storage(|storage| {
                apps = storage.catalog().list_apps()?;
                for app in &apps {
                    let tasks = storage.catalog().list_tasks(&app.id)?;
                    tasks_by_app.insert(app.id.clone(), tasks);
                }
                settings = storage.settings().load()?;
                Ok(())
            })?;
        }

        let running_tasks = self.runner.running_tasks();
        let mut app_states: BTreeMap<AppId, ProcessState> = BTreeMap::new();
        for app in &apps {
            let mut state = ProcessState::Idle;
            for task in tasks_by_app.get(&app.id).into_iter().flatten() {
                let task_state = self.runner.state_of(&task.id);
                state = worst_state(state, task_state);
            }
            app_states.insert(app.id.clone(), state);
        }

        let task_stats = self.stats_cache.lock().expect("stats cache").clone();
        let task_ports = self.ports_cache.lock().expect("ports cache").clone();
        Ok(CoreSnapshot {
            revision,
            apps,
            tasks_by_app,
            running_tasks,
            app_states,
            settings,
            task_stats,
            task_ports,
        })
    }

    /// Start one task end-to-end: resolve Node, build env, spawn, then record history.
    pub async fn start_task(
        &self,
        task_id: &TaskId,
    ) -> CoreResult<crate::process::runner::RunningTaskInfo> {
        if self.shutting_down.load(Ordering::SeqCst) {
            return Err(DomainError::InvalidInput {
                field: "shutdown",
                message: "the core is shutting down".into(),
            }
            .into());
        }
        let (task, app) = self.with_storage(|storage| {
            let catalog = storage.catalog();
            let task = catalog
                .get_task(task_id)?
                .ok_or_else(|| DomainError::NotFound {
                    entity: "task",
                    id: task_id.to_string(),
                })?;
            let app = catalog
                .get_app(&task.app_id)?
                .ok_or_else(|| DomainError::NotFound {
                    entity: "app",
                    id: task.app_id.to_string(),
                })?;
            Ok((task, app))
        })?;

        // Node resolution + env building are blocking (directory scans, one `node --version`);
        // keep them off the reactor.
        let tree = app.path.clone();
        let project_dir = std::path::PathBuf::from(&tree);
        let pref = task
            .node_version_pref_override
            .clone()
            .unwrap_or_else(|| app.node_version_pref.clone());
        let resolver = self.node_resolver.clone();
        let login_path = self.login_path();
        let env_vars = self.load_env_vars(&app.id, &task.id);
        let working_dir_override = task.working_dir_override.clone();
        let command_kind = task.command_kind;
        let script = task.script.clone();
        let custom = task.custom_command.clone();
        let package_manager = task.package_manager_override.or(app.package_manager);
        let task_id_owned = task.id.clone();
        let app_id_owned = app.id.clone();

        let node_info = tokio::task::spawn_blocking(move || resolver.resolve(&pref, &project_dir))
            .await
            .ok()
            .and_then(|result| result.ok());

        let node_bin_dir = node_info
            .as_ref()
            .map(|info| info.bin_dir.clone())
            .unwrap_or_else(|| std::path::PathBuf::from("/usr/bin"));
        let node_version = node_info.map(|info| info.version);

        let os_env: BTreeMap<String, String> = std::env::vars().collect();
        let env = build_env(&BuildEnvInput {
            os_env: &os_env,
            node_bin_dir: &node_bin_dir,
            login_path: &login_path,
            project_dir: std::path::Path::new(&tree),
            global: &env_vars.global,
            app: &env_vars.app,
            task: &env_vars.task,
        });

        // Secrets that cannot be decrypted block the start instead of being passed through.
        for var in env_vars.all_secret_keys() {
            if let Some(value) = env.get(&var)
                && read_secret_value(self.secrets.as_ref(), value).is_err()
            {
                // Never hand an undecryptable value to a child process.
                return Err(DomainError::SecretUnavailable { key: var }.into());
            }
        }

        let spec_task = Task {
            command_kind,
            script,
            custom_command: custom,
            working_dir_override,
            ..task.clone()
        };
        let spec = SpawnSpec::for_task(
            &spec_task,
            std::path::Path::new(&tree),
            env,
            package_manager,
            DEFAULT_COLS,
            DEFAULT_ROWS,
        )
        .map_err(|err| DomainError::InvalidInput {
            field: "command",
            message: err.to_string(),
        })?;

        let info = self
            .runner
            .start(task_id_owned.clone(), app_id_owned.clone(), spec.clone())
            .await
            .map_err(|err| DomainError::InvalidInput {
                field: "process",
                message: err.to_string(),
            })?;

        // History is written after the process is registered. If it fails, the process is
        // still tracked and will be reaped — we only lose the row, and say so.
        let row = RunHistoryRow {
            id: info.run_id.to_string(),
            app_id: app_id_owned,
            task_id: Some(task_id_owned.clone()),
            task_name: Some(spec_task.name.clone()),
            script: spec_task.script.clone(),
            custom_command: spec_task.custom_command.clone(),
            node_version,
            package_manager,
            started_at: info.started_at,
            ended_at: None,
            exit_code: None,
            exit_signal: None,
            was_killed_by_user: false,
        };
        if let Err(err) = self.with_storage(|storage| {
            storage.history().start_run(&row)?;
            Ok(())
        }) {
            eprintln!("[core] could not record run history for {task_id_owned}: {err}");
        }
        // Sampling only runs while something is live; wake the (otherwise parked) loop.
        self.monitor_notify.notify_one();
        Ok(info)
    }

    async fn stop_task(&self, task_id: &TaskId) -> CoreResult<()> {
        self.runner
            .stop(task_id)
            .await
            .map_err(|err| DomainError::InvalidInput {
                field: "process",
                message: err.to_string(),
            })?;
        Ok(())
    }

    /// Full shutdown: refuse starts → stop everything → write history → checkpoint → close.
    pub async fn shutdown(&self) -> CoreResult<()> {
        self.shutting_down.store(true, Ordering::SeqCst);
        self.runner.shutdown().await;

        // Make sure every teardown is settled before touching the database.
        let task_ids: Vec<TaskId> = self
            .with_storage(|storage| {
                let mut ids = Vec::new();
                for app in storage.catalog().list_apps()? {
                    for task in storage.catalog().list_tasks(&app.id)? {
                        ids.push(task.id);
                    }
                }
                Ok(ids)
            })
            .unwrap_or_default();
        for task_id in task_ids {
            let _ = wait_for_settled(&self.runner, &task_id, Duration::from_secs(2)).await;
        }

        let mut guard = self.storage.lock().expect("storage lock");
        if let Some(storage) = guard.as_ref() {
            {
                let history = storage.history();
                if let Err(err) = history.close_all_open(now_ms()) {
                    eprintln!("[core] closing open run rows failed: {err}");
                }
                if let Ok(apps) = storage.catalog().list_apps() {
                    let limit = storage
                        .settings()
                        .load()
                        .map(|s| s.run_history_limit)
                        .unwrap_or(500);
                    for app in apps {
                        let _ = history.prune(&app.id, limit);
                    }
                }
            }
            if let Err(err) = storage.checkpoint() {
                eprintln!("[core] wal checkpoint failed: {err}");
            }
        }
        // Dropping the connection is the "database closed" step of the shutdown order.
        *guard = None;
        Ok(())
    }

    /// Global search across every retained task.
    pub fn search_logs_all(
        &self,
        query: &str,
        options: &crate::logs::SearchOptions,
    ) -> Result<Vec<(TaskId, crate::logs::LineMatch)>, crate::logs::LogError> {
        self.runner.search_logs_all(query, options)
    }

    /// Number of sampling sweeps performed (idle periods must not increase this).
    pub fn monitor_sweeps(&self) -> u64 {
        self.monitor_sweeps.load(Ordering::SeqCst)
    }

    /// Sampling interval from settings, clamped to the legacy floor.
    fn dashboard_interval(&self) -> Duration {
        let millis = self
            .with_storage(|storage| {
                Ok(storage
                    .settings()
                    .load()
                    .map(|settings| settings.dashboard_refresh_ms)
                    .unwrap_or(1_000))
            })
            .unwrap_or(1_000);
        Duration::from_millis(clamp_interval(millis))
    }

    /// One sampling sweep: process table once, listening ports once, events per task.
    pub async fn monitor_tick(&self) {
        let running = self.runner.running_tasks();
        if running.is_empty() {
            return; // nothing to sample: no shell processes, no CPU
        }
        self.monitor_sweeps.fetch_add(1, Ordering::SeqCst);

        let table = tokio::task::spawn_blocking(read_process_table)
            .await
            .unwrap_or_default();

        let groups: Vec<(TaskId, AppId, i32, Vec<i32>)> = running
            .iter()
            .map(|info| {
                (
                    info.task_id.clone(),
                    info.app_id.clone(),
                    info.pid as i32,
                    crate::process::process_tree::group_members(info.pgid),
                )
            })
            .collect();

        let ports = groups
            .iter()
            .map(|(_, _, _, members)| listening_ports_of(members))
            .collect::<Vec<_>>();

        for ((task_id, app_id, pid, _), listening) in groups.iter().zip(ports) {
            let sample = sample_from_table(&table, *pid);
            self.stats_cache
                .lock()
                .expect("stats cache")
                .insert(task_id.clone(), sample);
            let revision = self.bump_revision();
            let _ = self.events.send(CoreEvent {
                revision,
                kind: CoreEventKind::TaskStats {
                    task_id: task_id.clone(),
                    app_id: app_id.clone(),
                    cpu_percent: sample.cpu_percent,
                    mem_mb: sample.mem_mb,
                    tree_cpu_percent: sample.tree_cpu_percent,
                    tree_mem_mb: sample.tree_mem_mb,
                    process_count: sample.process_count,
                },
            });

            // Hints come from the tail of the output only (cheap per sweep).
            let tail = self.runner.logs_tail(task_id, 4 * 1024);
            let mut tracker = self.port_tracker.lock().expect("port tracker");
            tracker.observe_logs(task_id, &tail);
            let port_sample = tracker.sweep(task_id, &listening);
            drop(tracker);
            self.ports_cache
                .lock()
                .expect("ports cache")
                .insert(task_id.clone(), port_sample.clone());
            let revision = self.bump_revision();
            let _ = self.events.send(CoreEvent {
                revision,
                kind: CoreEventKind::TaskPorts {
                    task_id: task_id.clone(),
                    app_id: app_id.clone(),
                    confirmed: port_sample.confirmed,
                    hinted: port_sample.hinted,
                },
            });
        }

        // Drop samples for tasks that are no longer running.
        let live: std::collections::HashSet<TaskId> =
            running.iter().map(|info| info.task_id.clone()).collect();
        self.stats_cache
            .lock()
            .expect("stats cache")
            .retain(|task_id, _| live.contains(task_id));
        self.ports_cache
            .lock()
            .expect("ports cache")
            .retain(|task_id, _| live.contains(task_id));
    }

    /// Background sampling loop. Parks on a notification while nothing is running, so an
    /// idle NeoHub starts no timers and runs no shell commands.
    fn spawn_monitor(service: &Arc<Self>) {
        let weak = Arc::downgrade(service);
        let notify = service.monitor_notify.clone();
        tokio::spawn(async move {
            loop {
                // Wait until at least one task is live (not a spin: this is a real await).
                loop {
                    let Some(service) = weak.upgrade() else {
                        return;
                    };
                    if !service.runner.running_tasks().is_empty() {
                        break;
                    }
                    drop(service);
                    notify.notified().await;
                }
                let Some(service) = weak.upgrade() else {
                    return;
                };
                let interval = service.dashboard_interval();
                service.monitor_tick().await;
                tokio::time::sleep(interval).await;
            }
        });
    }

    fn finish_history(
        &self,
        task_id: &TaskId,
        run_id: &neohub_domain::RunId,
        exit: Option<&crate::process::runner::ExitInfo>,
    ) {
        let Some(exit) = exit else { return };
        let result = self.with_storage(|storage| {
            storage.history().finish_run(
                run_id.as_str(),
                now_ms(),
                exit.code,
                exit.signal.as_deref(),
                exit.killed_by_user,
            )?;
            Ok(())
        });
        if let Err(err) = result {
            eprintln!("[core] could not finish run history for {task_id}: {err}");
        }
    }

    /// Scan a folder for importable projects, marking the ones already registered.
    pub fn scan_folder(
        &self,
        dir: &std::path::Path,
    ) -> CoreResult<Vec<crate::environment::ImportCandidate>> {
        let mut candidates = crate::environment::scan_folder(dir);
        self.with_storage(|storage| {
            let catalog = storage.catalog();
            for candidate in &mut candidates {
                // Registered projects are flagged so the UI can grey them out; the path and
                // the suggested script are left untouched (the UI uses the path as its key,
                // and canonicalising it here would make the row stop matching).
                let canonical = std::fs::canonicalize(&candidate.path)
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| candidate.path.clone());
                candidate.already_registered = catalog.get_app_by_path(&canonical)?.is_some();
            }
            Ok(())
        })?;
        Ok(candidates)
    }

    /// Create an app through the same atomic path the UI uses.
    pub fn create_app(&self, input: crate::catalog::CreateAppInput) -> CoreResult<App> {
        self.with_storage(|storage| {
            let catalog = crate::catalog::AppCatalog::new(storage);
            catalog.create_app(input)
        })
    }

    /// Read one scope's environment variables, masking secrets.
    pub fn list_env_vars(&self, scope: &EnvScope) -> CoreResult<Vec<EnvVarView>> {
        let vars = self.with_storage(|storage| {
            let env = storage.env_store();
            Ok(match scope {
                EnvScope::Global => env.list_global()?,
                EnvScope::App(app_id) => env.list_app(app_id)?,
                EnvScope::Task(task_id) => env.list_task(task_id)?,
            })
        })?;
        Ok(vars
            .into_iter()
            .map(|var| EnvVarView {
                key: var.key,
                // A secret never leaves the core as text; the UI only learns that it is set.
                value: if var.is_secret { None } else { Some(var.value) },
                is_secret: var.is_secret,
                enabled: var.enabled,
                note: var.note,
            })
            .collect())
    }

    /// Replace one scope's variables. A `None` value keeps the stored secret as-is, which is
    /// what makes it impossible for the UI to write a mask character back as a password.
    pub fn set_env_vars(&self, scope: &EnvScope, inputs: Vec<EnvVarInput>) -> CoreResult<()> {
        let now = neohub_storage::db::now_ms();
        self.with_storage(|storage| {
            let existing = {
                let env = storage.env_store();
                match scope {
                    EnvScope::Global => env.list_global()?,
                    EnvScope::App(app_id) => env.list_app(app_id)?,
                    EnvScope::Task(task_id) => env.list_task(task_id)?,
                }
            };
            let mut vars: Vec<neohub_domain::EnvVar> = Vec::new();
            for input in inputs {
                if input.key.trim().is_empty() {
                    continue;
                }
                let previous = existing.iter().find(|var| var.key == input.key);
                let value = match input.value {
                    Some(value) if input.is_secret => {
                        // Encrypt on the way in; the store never holds a plaintext secret.
                        self.secrets.encrypt(&value).map_err(|err| {
                            DomainError::SecretUnavailable {
                                key: format!("{} ({err})", input.key),
                            }
                        })?
                    }
                    Some(value) => value,
                    None => match previous {
                        Some(previous) => previous.value.clone(),
                        None => continue, // nothing stored and nothing typed: skip the row
                    },
                };
                let mut var = neohub_storage::env::new_var(
                    match scope {
                        EnvScope::Global => None,
                        EnvScope::App(_) | EnvScope::Task(_) => scope.app_id(),
                    },
                    match scope {
                        EnvScope::Task(task_id) => Some(task_id.clone()),
                        _ => None,
                    },
                    &input.key,
                    &value,
                    input.is_secret,
                );
                var.enabled = input.enabled;
                var.note = input.note;
                vars.push(var);
            }
            let mut env = storage.env_store();
            match scope {
                EnvScope::Global => env.set_global(&vars, now)?,
                EnvScope::App(app_id) => env.set_app(app_id, &vars, now)?,
                EnvScope::Task(task_id) => {
                    env.set_task(scope.app_id().as_ref(), task_id, &vars, now)?
                }
            }
            Ok(())
        })
    }

    /// Read the current settings.
    pub fn settings(&self) -> CoreResult<neohub_domain::Settings> {
        self.with_storage(|storage| Ok(storage.settings().load()?))
    }

    /// Apply a settings patch, persist it, and tell subscribers.
    pub fn update_settings(
        &self,
        patch: &neohub_domain::SettingsPatch,
    ) -> CoreResult<neohub_domain::Settings> {
        let mut settings = self.settings()?;
        settings.apply(patch);
        self.with_storage(|storage| Ok(storage.settings().save(&settings)?))?;
        let revision = self.bump_revision();
        let _ = self.events.send(CoreEvent {
            revision,
            kind: CoreEventKind::SettingsChanged(Box::new(settings.clone())),
        });
        Ok(settings)
    }

    /// Update an app through the catalog (validation lives there).
    pub fn update_app(
        &self,
        app_id: &AppId,
        input: crate::catalog::UpdateAppInput,
    ) -> CoreResult<App> {
        self.with_storage(|storage| {
            crate::catalog::AppCatalog::new(storage).update_app(app_id, input)
        })
    }

    /// Remove an app registration. Refused while any task of it is live, so a running stack
    /// cannot be orphaned by a tool call.
    pub fn remove_app(&self, app_id: &AppId) -> CoreResult<()> {
        let tasks = self.with_storage(|storage| Ok(storage.catalog().list_tasks(app_id)?))?;
        if let Some(live) = tasks
            .iter()
            .find(|task| self.runner.state_of(&task.id).is_live())
        {
            return Err(DomainError::AppIsLive {
                state: self.runner.state_of(&live.id).as_str(),
            }
            .into());
        }
        self.ready_flags
            .lock()
            .expect("ready flags")
            .retain(|task_id, _| !tasks.iter().any(|task| &task.id == task_id));
        self.with_storage(|storage| Ok(storage.catalog().remove_app(app_id)?))
    }

    /// Every Node installation the resolver knows about (highest first).
    pub fn node_versions(&self) -> Vec<crate::environment::NodeInstallation> {
        self.node_resolver.list()
    }

    /// Task mutations always go through the catalog so validation stays in one place.
    pub fn add_task(
        &self,
        app_id: &AppId,
        input: crate::catalog::AddTaskInput,
    ) -> CoreResult<Task> {
        self.with_storage(|storage| {
            crate::catalog::AppCatalog::new(storage).add_task(app_id, input)
        })
    }

    pub fn update_task(
        &self,
        task_id: &TaskId,
        input: crate::catalog::UpdateTaskInput,
    ) -> CoreResult<Task> {
        self.with_storage(|storage| {
            crate::catalog::AppCatalog::new(storage).update_task(task_id, input)
        })
    }

    pub fn remove_task(&self, task_id: &TaskId) -> CoreResult<()> {
        self.with_storage(|storage| crate::catalog::AppCatalog::new(storage).remove_task(task_id))
    }

    pub fn reorder_tasks(&self, app_id: &AppId, task_ids: &[TaskId]) -> CoreResult<()> {
        self.with_storage(|storage| {
            crate::catalog::AppCatalog::new(storage).reorder_tasks(app_id, task_ids)
        })
    }

    /// Run history for one app, newest first.
    pub fn run_history(
        &self,
        app_id: &AppId,
        limit: Option<u32>,
    ) -> CoreResult<Vec<RunHistoryRow>> {
        self.with_storage(|storage| Ok(storage.history().list_for_app(app_id, limit)?))
    }

    /// Readiness result recorded for a task's current run.
    pub fn readiness_of(&self, task_id: &TaskId) -> Option<(String, bool)> {
        self.ready_flags
            .lock()
            .expect("ready flags")
            .get(task_id)
            .cloned()
    }

    fn set_ready(&self, task_id: &TaskId, kind: &str, verified: bool) {
        self.ready_flags
            .lock()
            .expect("ready flags")
            .insert(task_id.clone(), (kind.to_string(), verified));
    }

    /// Sticky app state (used by the snapshot when no task is live).
    pub fn sticky_state(&self, app_id: &AppId) -> Option<ProcessState> {
        self.outcomes.lock().expect("outcomes").get(app_id)
    }

    /// Build the probe context for a running task.
    fn probe_context(&self, task_id: &TaskId, pid: u32, pgid: i32) -> ProbeContext {
        let runner_for_state = self.runner.clone();
        let runner_for_logs = self.runner.clone();
        let runner_for_exit = self.runner.clone();
        let id_state = task_id.clone();
        let id_logs = task_id.clone();
        let id_exit = task_id.clone();
        ProbeContext {
            pid,
            pgid,
            state: Arc::new(move || runner_for_state.state_of(&id_state)),
            raw_logs: Arc::new(move || runner_for_logs.logs_of(&id_logs)),
            exit_code: Arc::new(move || {
                runner_for_exit
                    .last_exit_of(&id_exit)
                    .and_then(|exit| exit.code)
            }),
        }
    }

    fn cancellation_flag(&self, app_id: &AppId) -> Arc<AtomicBool> {
        let mut cancels = self.start_cancels.lock().expect("cancel flags");
        cancels
            .entry(app_id.clone())
            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .clone()
    }

    /// Cancel an in-flight start for this app (called by stop/restart before taking the lock).
    fn cancel_start(&self, app_id: &AppId) {
        let cancels = self.start_cancels.lock().expect("cancel flags");
        if let Some(flag) = cancels.get(app_id) {
            flag.store(true, Ordering::SeqCst);
        }
    }

    fn readiness_timeout(&self) -> Duration {
        let millis = self
            .with_storage(|storage| {
                Ok(storage
                    .settings()
                    .load()
                    .map(|settings| settings.readiness_timeout_ms)
                    .unwrap_or(60_000))
            })
            .unwrap_or(60_000);
        Duration::from_millis(millis)
    }

    /// Start an app: topo levels, parallel within a level, readiness per task, rollback on
    /// failure. Only tasks that *this* call started are rolled back — anything the user had
    /// already running stays untouched.
    pub async fn start_app(&self, app_id: &AppId) -> CoreResult<()> {
        if self.shutting_down.load(Ordering::SeqCst) {
            return Err(DomainError::InvalidInput {
                field: "shutdown",
                message: "the core is shutting down".into(),
            }
            .into());
        }
        let tasks = self.with_storage(|storage| Ok(storage.catalog().list_tasks(app_id)?))?;
        let levels = plan_start(&tasks).map_err(|err| match err {
            TopologyError::UnknownDependency { task, missing } => DomainError::UnknownDependency {
                task_id: task,
                missing,
            },
            TopologyError::Cycle { tasks } => DomainError::DependencyCycle { tasks },
        })?;
        if levels.is_empty() {
            return Err(DomainError::InvalidInput {
                field: "tasks",
                message: "no enabled tasks for this app".into(),
            }
            .into());
        }

        self.outcomes.lock().expect("outcomes").clear(app_id);
        let cancel = self.cancellation_flag(app_id);
        cancel.store(false, Ordering::SeqCst);
        let timeout = self.readiness_timeout();

        let mut started_here: Vec<TaskId> = Vec::new();
        for level in &levels {
            if cancel.load(Ordering::SeqCst) {
                self.rollback(&started_here).await;
                return Err(DomainError::InvalidInput {
                    field: "process",
                    message: "start cancelled".into(),
                }
                .into());
            }
            // Start everything in this level in parallel.
            let mut launch = Vec::new();
            for task in level {
                if self.runner.state_of(&task.id).is_live() {
                    continue; // already running (user started it manually) — leave it alone
                }
                let task_id = task.id.clone();
                let signal = task.readiness.clone();
                let fut = async move {
                    let info = self.start_task(&task_id).await?;
                    let ctx = self.probe_context(&task_id, info.pid, info.pgid);
                    let outcome = wait_for_readiness(&signal, &ctx, timeout).await;
                    Ok::<_, CoreError>((task_id, signal.kind().to_string(), outcome))
                };
                launch.push(fut);
            }
            let results = futures::future::join_all(launch).await;
            for result in results {
                match result {
                    Ok((task_id, kind, ReadinessOutcome::Ready)) => {
                        self.set_ready(&task_id, &kind, true);
                        started_here.push(task_id);
                    }
                    Ok((task_id, kind, ReadinessOutcome::Unverified)) => {
                        // `none`: started, but nothing was verified. Never reported as ready.
                        self.set_ready(&task_id, &kind, false);
                        started_here.push(task_id);
                    }
                    Ok((task_id, kind, outcome)) => {
                        self.set_ready(&task_id, &kind, false);
                        started_here.push(task_id.clone());
                        let message = match outcome {
                            ReadinessOutcome::Failed(reason) => reason,
                            ReadinessOutcome::TimedOut => {
                                format!("readiness timed out after {} ms", timeout.as_millis())
                            }
                            _ => "readiness failed".to_string(),
                        };
                        self.rollback(&started_here).await;
                        self.outcomes
                            .lock()
                            .expect("outcomes")
                            .record(app_id, ProcessState::Crashed);
                        eprintln!("[core] readiness failure: {message}");
                        return Err(DomainError::ReadinessTimeout {
                            task_id,
                            timeout_ms: timeout.as_millis() as u64,
                        }
                        .into());
                    }
                    Err(err) => {
                        self.rollback(&started_here).await;
                        return Err(err);
                    }
                }
            }
        }
        Ok(())
    }

    /// Stop only the tasks this start added (failure rollback). Never touches pre-existing runs.
    async fn rollback(&self, started: &[TaskId]) {
        for task_id in started {
            let _ = self.runner.stop(task_id).await;
            self.ready_flags
                .lock()
                .expect("ready flags")
                .remove(task_id);
        }
    }

    /// Stop an app: reverse-topological, level by level.
    pub async fn stop_app(&self, app_id: &AppId) -> CoreResult<()> {
        // Cancel an in-flight start first, so the stop is not queued behind readiness waits.
        self.cancel_start(app_id);
        let tasks = self.with_storage(|storage| Ok(storage.catalog().list_tasks(app_id)?))?;
        let levels = plan_start(&tasks).unwrap_or_default();
        let stop_plan = plan_stop(&levels);
        for level in stop_plan {
            for task_id in level {
                if self.runner.state_of(&task_id).is_live() {
                    let _ = self.runner.stop(&task_id).await;
                }
                self.ready_flags
                    .lock()
                    .expect("ready flags")
                    .remove(&task_id);
            }
        }
        self.outcomes
            .lock()
            .expect("outcomes")
            .record(app_id, ProcessState::Exited);
        Ok(())
    }

    /// Stop then start, holding the app's cancellation flag across both.
    pub async fn restart_app(&self, app_id: &AppId) -> CoreResult<()> {
        self.stop_app(app_id).await?;
        tokio::time::sleep(Duration::from_millis(100)).await;
        self.start_app(app_id).await
    }

    /// Shared resolver: the scan cache lives for the process lifetime, so Node upgrades
    /// appear only when something explicitly refreshes it.
    pub fn node_resolver(&self) -> Arc<NodeResolver> {
        self.node_resolver.clone()
    }

    fn login_path(&self) -> String {
        self.path_probe.get()
    }

    fn load_env_vars(&self, app_id: &AppId, task_id: &TaskId) -> EnvVars {
        let loaded = self.with_storage(|storage| {
            let env = storage.env_store();
            Ok((
                env.list_global()?,
                env.list_app(app_id)?,
                env.list_task(task_id)?,
            ))
        });
        match loaded {
            Ok((global, app, task)) => EnvVars { global, app, task },
            Err(err) => {
                eprintln!("[core] env lookup failed: {err}");
                EnvVars::default()
            }
        }
    }
}

/// Which scope an env operation applies to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvScope {
    Global,
    App(AppId),
    Task(TaskId),
}

impl EnvScope {
    /// The owning app, when the scope has one (task scopes carry it for cascade purposes).
    pub fn app_id(&self) -> Option<AppId> {
        match self {
            EnvScope::Global => None,
            EnvScope::App(app_id) => Some(app_id.clone()),
            EnvScope::Task(_) => None,
        }
    }
}

/// One row as the UI sees it: secrets are reported as "set" without their value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvVarView {
    pub key: String,
    /// `None` for a secret: the core never sends secret text to a view.
    pub value: Option<String>,
    pub is_secret: bool,
    pub enabled: bool,
    pub note: Option<String>,
}

/// One row to write. `value: None` means "keep whatever is stored".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvVarInput {
    pub key: String,
    pub value: Option<String>,
    pub is_secret: bool,
    pub enabled: bool,
    pub note: Option<String>,
}

#[derive(Default)]
struct EnvVars {
    global: Vec<neohub_domain::EnvVar>,
    app: Vec<neohub_domain::EnvVar>,
    task: Vec<neohub_domain::EnvVar>,
}

impl EnvVars {
    fn all_secret_keys(&self) -> Vec<String> {
        self.global
            .iter()
            .chain(self.app.iter())
            .chain(self.task.iter())
            .filter(|var| var.enabled && var.is_secret)
            .map(|var| var.key.clone())
            .collect()
    }
}

fn worst_state(current: ProcessState, next: ProcessState) -> ProcessState {
    let rank = |state: ProcessState| match state {
        ProcessState::Crashed => 6,
        ProcessState::Running => 5,
        ProcessState::Exiting => 4,
        ProcessState::Starting => 3,
        ProcessState::Exited => 2,
        ProcessState::Idle => 1,
    };
    if rank(next) > rank(current) {
        next
    } else {
        current
    }
}

impl CoreHandle {
    /// The only business entry point.
    pub async fn request(&self, command: CoreCommand) -> CoreResult<CoreReply> {
        let mutated = command.mutates_catalog();
        let reply = match command {
            CoreCommand::GetSnapshot => Ok(CoreReply::Snapshot(Box::new(self.service.snapshot()?))),
            CoreCommand::ListEnvVars { scope } => {
                Ok(CoreReply::EnvVars(self.service.list_env_vars(&scope)?))
            }
            CoreCommand::SetEnvVars { scope, vars } => {
                self.service.set_env_vars(&scope, vars)?;
                let rows = self.service.list_env_vars(&scope)?;
                Ok(CoreReply::EnvVars(rows))
            }
            CoreCommand::GetSettings => {
                let settings = self.service.settings()?;
                Ok(CoreReply::Settings(Box::new(settings)))
            }
            CoreCommand::UpdateSettings { patch } => {
                let settings = self.service.update_settings(&patch)?;
                Ok(CoreReply::Settings(Box::new(settings)))
            }
            CoreCommand::ScanFolder { dir } => {
                let candidates = self.service.scan_folder(&dir)?;
                Ok(CoreReply::ImportCandidates(candidates))
            }
            CoreCommand::CreateApp { input } => {
                let app = self.service.create_app(*input)?;
                Ok(CoreReply::App(Box::new(app)))
            }
            CoreCommand::StartTask { task_id } => {
                let info = self.service.start_task(&task_id).await?;
                Ok(CoreReply::Started(Box::new(info)))
            }
            CoreCommand::StopTask { task_id } => {
                self.service.stop_task(&task_id).await?;
                Ok(CoreReply::Ack)
            }
            CoreCommand::ResizeTask {
                task_id,
                cols,
                rows,
            } => {
                self.service
                    .runner
                    .resize(&task_id, cols, rows)
                    .await
                    .map_err(|err| DomainError::InvalidInput {
                        field: "grid",
                        message: err.to_string(),
                    })?;
                Ok(CoreReply::Ack)
            }
            CoreCommand::UpdateApp { app_id, input } => {
                let app = self.service.update_app(&app_id, *input)?;
                Ok(CoreReply::App(Box::new(app)))
            }
            CoreCommand::RemoveApp { app_id } => {
                self.service.remove_app(&app_id)?;
                Ok(CoreReply::Ack)
            }
            CoreCommand::ListNodeVersions => {
                Ok(CoreReply::NodeVersions(self.service.node_versions()))
            }
            CoreCommand::AddTask { app_id, input } => {
                let task = self.service.add_task(&app_id, *input)?;
                Ok(CoreReply::Task(Box::new(task)))
            }
            CoreCommand::UpdateTask { task_id, input } => {
                let task = self.service.update_task(&task_id, *input)?;
                Ok(CoreReply::Task(Box::new(task)))
            }
            CoreCommand::RemoveTask { task_id } => {
                self.service.remove_task(&task_id)?;
                Ok(CoreReply::Ack)
            }
            CoreCommand::ReorderTasks { app_id, task_ids } => {
                self.service.reorder_tasks(&app_id, &task_ids)?;
                Ok(CoreReply::Ack)
            }
            CoreCommand::ListRunHistory { app_id, limit } => {
                let rows = self.service.run_history(&app_id, limit)?;
                Ok(CoreReply::RunHistory(rows))
            }
            CoreCommand::GetLogs { task_id } => {
                Ok(CoreReply::Logs(self.service.runner.logs_of(&task_id)))
            }
            CoreCommand::StartApp { app_id } => {
                self.service.start_app(&app_id).await?;
                Ok(CoreReply::Ack)
            }
            CoreCommand::BootstrapLogs { task_id, after } => {
                let outcome = self
                    .service
                    .runner
                    .bootstrap_logs(&task_id, after.as_ref())
                    .ok_or_else(|| DomainError::NotFound {
                        entity: "logs",
                        id: task_id.to_string(),
                    })?;
                Ok(CoreReply::LogBootstrap(outcome))
            }
            CoreCommand::SearchLogs {
                task_id,
                query,
                options,
            } => {
                let matches = match task_id {
                    Some(task_id) => self
                        .service
                        .runner
                        .search_logs(&task_id, &query, &options)
                        .map_err(|err| DomainError::InvalidInput {
                            field: "query",
                            message: err.to_string(),
                        })?
                        .into_iter()
                        .map(|hit| (task_id.clone(), hit))
                        .collect(),
                    None => self
                        .service
                        .search_logs_all(&query, &options)
                        .map_err(|err| DomainError::InvalidInput {
                            field: "query",
                            message: err.to_string(),
                        })?,
                };
                Ok(CoreReply::LogMatches(matches))
            }
            CoreCommand::GetTerminalText { task_id } => {
                Ok(CoreReply::Logs(self.service.runner.terminal_text(&task_id)))
            }
            CoreCommand::ExportLogs { task_id, path, raw } => {
                let contents = if raw {
                    self.service.runner.logs_of(&task_id)
                } else {
                    self.service.runner.terminal_text(&task_id)
                };
                let write_to = path.clone();
                tokio::task::spawn_blocking(move || std::fs::write(&write_to, contents))
                    .await
                    .map_err(|err| DomainError::InvalidInput {
                        field: "path",
                        message: err.to_string(),
                    })?
                    .map_err(|err| DomainError::InvalidInput {
                        field: "path",
                        message: err.to_string(),
                    })?;
                Ok(CoreReply::Exported(path))
            }
            CoreCommand::ClearLogs { task_id } => {
                self.service.runner.clear_logs(&task_id);
                Ok(CoreReply::Ack)
            }
            CoreCommand::StopApp { app_id } => {
                self.service.stop_app(&app_id).await?;
                Ok(CoreReply::Ack)
            }
            CoreCommand::RestartApp { app_id } => {
                self.service.restart_app(&app_id).await?;
                Ok(CoreReply::Ack)
            }
            CoreCommand::Shutdown => {
                self.service.shutdown().await?;
                Ok(CoreReply::Ack)
            }
        };
        // One broadcast point for every mutation, whoever asked for it.
        if mutated && reply.is_ok() {
            let revision = self.service.bump_revision();
            let _ = self.service.events.send(CoreEvent {
                revision,
                kind: CoreEventKind::CatalogChanged,
            });
        }
        reply
    }

    /// Subscribe before taking a snapshot, then drop events at or below the snapshot
    /// revision.
    pub fn subscribe(&self) -> CoreSubscription {
        CoreSubscription {
            rx: self.service.events.subscribe(),
        }
    }

    pub fn service(&self) -> &Arc<CoreService> {
        &self.service
    }
}

/// Event stream for one window. Dropping it must not affect the core.
pub struct CoreSubscription {
    rx: broadcast::Receiver<CoreEvent>,
}

impl CoreSubscription {
    /// Next event, or `ResyncRequired` when this subscriber fell behind.
    pub async fn recv(&mut self) -> Option<CoreEvent> {
        match self.rx.recv().await {
            Ok(event) => Some(event),
            Err(broadcast::error::RecvError::Lagged(_)) => Some(CoreEvent {
                revision: 0,
                kind: CoreEventKind::ResyncRequired { revision: 0 },
            }),
            Err(broadcast::error::RecvError::Closed) => None,
        }
    }

    pub fn try_recv(&mut self) -> Option<CoreEvent> {
        match self.rx.try_recv() {
            Ok(event) => Some(event),
            Err(broadcast::error::TryRecvError::Lagged(_)) => Some(CoreEvent {
                revision: 0,
                kind: CoreEventKind::ResyncRequired { revision: 0 },
            }),
            Err(_) => None,
        }
    }
}
