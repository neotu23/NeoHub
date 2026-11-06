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
use crate::process::runner::{RunnerEvent, TaskRunner, wait_for_settled};
use crate::process::spawn_spec::{DEFAULT_COLS, DEFAULT_ROWS, SpawnSpec};
use crate::snapshot::CoreSnapshot;
use neohub_domain::{App, AppId, DomainError, ProcessState, RunHistoryRow, Settings, Task, TaskId};
use neohub_storage::db::now_ms;
use neohub_storage::secrets::{FakeSecretStore, SecretStore, read_secret_value};
use neohub_storage::{DataDir, Storage};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;

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
        });
        Self::spawn_pump(&service);
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

        Ok(CoreSnapshot {
            revision,
            apps,
            tasks_by_app,
            running_tasks,
            app_states,
            settings,
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

    /// Stop every task of one app (phase 7 replaces this with reverse-topological order).
    async fn stop_app(&self, app_id: &AppId) -> CoreResult<()> {
        let tasks = self.with_storage(|storage| Ok(storage.catalog().list_tasks(app_id)?))?;
        for task in tasks {
            if self.runner.state_of(&task.id).is_live() {
                let _ = self.stop_task(&task.id).await;
            }
        }
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
        match command {
            CoreCommand::GetSnapshot => Ok(CoreReply::Snapshot(Box::new(self.service.snapshot()?))),
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
            CoreCommand::GetLogs { task_id } => {
                Ok(CoreReply::Logs(self.service.runner.logs_of(&task_id)))
            }
            CoreCommand::StopApp { app_id } => {
                self.service.stop_app(&app_id).await?;
                Ok(CoreReply::Ack)
            }
            CoreCommand::Shutdown => {
                self.service.shutdown().await?;
                Ok(CoreReply::Ack)
            }
        }
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
