//! The main window shell: sidebar + dashboard, rendered from the core snapshot.
//!
//! Threading contract (see `native/docs/interfaces.md`): the render function only reads the
//! view model and never blocks — commands go to the core over a background Tokio runtime and
//! results come back as events, which the pump applies before the next frame.

use crate::state::env_editor::{EnvEditor, ScopeTab, effective, scope_for};
use crate::state::session::{Session, SessionStore};
use crate::state::settings_form::{NumberField, SettingsForm, pending_native_fields};
use crate::state::store::UiStore;
use crate::state::task_form::{Field, ReadinessChoice, TaskForm};
use crate::state::text_input::{KeyOutcome, TextField};
use crate::theme::{DARK, LIGHT, Palette, metrics};
use gpui_kit::*;
use neohub_core::commands::{CoreCommand, CoreReply};
use neohub_core::environment::ImportCandidate;
use neohub_core::events::CoreEvent;
use neohub_core::service::{CoreHandle, CoreService};
use neohub_domain::{AppId, ProcessState, TaskId};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// One environment-variable load/save result: which scope, and its rows.
type EnvUpdate = (
    neohub_core::service::EnvScope,
    Vec<neohub_core::service::EnvVarView>,
);

/// Messages from the core to the window: streamed events plus command replies.
enum HostUpdate {
    Event(CoreEvent),
    Candidates(Vec<ImportCandidate>),
    Imported,
    /// One candidate of a batch finished: (path, ok).
    ImportedOne(String, bool),
    /// A whole batch finished.
    BatchImported {
        imported: usize,
        failures: Vec<String>,
    },
    History(AppId, Vec<neohub_domain::RunHistoryRow>),
    TaskSaved(Box<neohub_domain::Task>),
    TaskError(String),
    UpdateChecked(neohub_core::update::UpdateStatus),
    UpdateFailed(String),
    DatabaseExported(std::path::PathBuf),
    DatabaseReset(usize),
    DatabaseError(String),
    AppSaved(Box<neohub_domain::App>),
    AppSaveError(String),
    SettingsSaved(Box<neohub_domain::Settings>),
    SettingsError(String),
    EnvVars(
        neohub_core::service::EnvScope,
        Vec<neohub_core::service::EnvVarView>,
    ),
    EnvError(String),
    Terminal(TaskId, String),
    LogMatches(Vec<(TaskId, usize, String)>),
    Exported(std::path::PathBuf),
    ExportError(String),
    Error(String),
}

/// Everything the view needs from the outside world.
pub struct HostBridge {
    runtime: Arc<tokio::runtime::Runtime>,
    handle: CoreHandle,
    store: Mutex<UiStore>,
    updates: Mutex<std::sync::mpsc::Receiver<HostUpdate>>,
    /// Command replies are pushed onto the same channel the event stream uses.
    reply_sink: Arc<std::sync::mpsc::Sender<HostUpdate>>,
    /// Outcome of the last task save, picked up by the editor.
    last_save: Mutex<(Option<neohub_domain::Task>, Option<String>)>,
    /// Outcome of the last app-configuration save: (saved app, error).
    last_app_save: Mutex<(Option<neohub_domain::App>, Option<String>)>,
    /// Message from the last database operation (export/reset/error).
    last_db_notice: Mutex<Option<String>>,
    /// Outcome of the last update check: (status summary, has update).
    last_update: Mutex<Option<(String, bool)>>,
    /// Outcome of the last settings save, picked up by the settings panel.
    last_settings: Mutex<(Option<neohub_domain::Settings>, Option<String>)>,
    /// Environment-variable loads/saves waiting to be folded into the editor.
    last_env: Mutex<(Vec<EnvUpdate>, Option<String>)>,
    session_store: SessionStore,
    /// Data directory (log exports land in `<data>/logs`).
    data_dir: PathBuf,
}

impl HostBridge {
    /// Wire the view to a running core: take one snapshot, then stream events.
    ///
    /// The runtime is supplied by the host so the core (which spawns its own tasks in
    /// `CoreService::open`) and the UI share exactly one reactor.
    pub fn new(
        runtime: Arc<tokio::runtime::Runtime>,
        service: Arc<CoreService>,
        data_dir: PathBuf,
    ) -> Arc<Self> {
        let session_store = SessionStore::new(&data_dir);
        let session = session_store.load();
        let handle = service.handle();

        let (tx, rx) = std::sync::mpsc::channel();
        let mut subscription = handle.subscribe();
        let event_tx = tx.clone();
        runtime.spawn(async move {
            while let Some(event) = subscription.recv().await {
                if event_tx.send(HostUpdate::Event(event)).is_err() {
                    break;
                }
            }
        });
        let command_tx = tx.clone();
        let reply_sink = Arc::new(command_tx);

        let bridge = Arc::new(HostBridge {
            runtime,
            handle: handle.clone(),
            store: Mutex::new(UiStore::new(session)),
            updates: Mutex::new(rx),
            reply_sink,
            last_save: Mutex::new((None, None)),
            last_app_save: Mutex::new((None, None)),
            last_db_notice: Mutex::new(None),
            last_update: Mutex::new(None),
            last_settings: Mutex::new((None, None)),
            last_env: Mutex::new((Vec::new(), None)),
            session_store,
            data_dir: data_dir.clone(),
        });
        bridge.refresh_snapshot();
        bridge
    }

    /// Take a fresh snapshot synchronously (startup and resync only — it is a memory read).
    pub fn refresh_snapshot(&self) {
        let reply = self
            .runtime
            .block_on(self.handle.request(CoreCommand::GetSnapshot));
        if let Ok(neohub_core::commands::CoreReply::Snapshot(snapshot)) = reply {
            self.store
                .lock()
                .expect("store lock")
                .apply_snapshot(*snapshot);
        }
    }

    fn send(&self, command: CoreCommand) {
        let handle = self.handle.clone();
        self.runtime.spawn(async move {
            let _ = handle.request(command).await;
        });
    }

    /// Where the import panel looks by default: `--import-dir`-style override, else `~/Code`.
    pub fn default_import_dir() -> String {
        if let Ok(dir) = std::env::var("NEOHUB_IMPORT_DIR")
            && !dir.trim().is_empty()
        {
            return dir;
        }
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
        format!("{home}/Code")
    }

    /// Open the import panel and immediately scan the default folder.
    pub fn begin_import(&self, dir: String) {
        self.store
            .lock()
            .expect("store lock")
            .open_import(dir.clone());
        self.scan_folder(dir);
    }

    /// Scan a folder for importable projects.
    pub fn scan_folder(&self, dir: String) {
        let handle = self.handle.clone();
        let sink = self.reply_sink.clone();
        self.runtime.spawn(async move {
            let reply = handle
                .request(CoreCommand::ScanFolder { dir: dir.into() })
                .await;
            let message = match reply {
                Ok(CoreReply::ImportCandidates(candidates)) => HostUpdate::Candidates(candidates),
                Ok(other) => HostUpdate::Error(format!("unexpected scan reply: {other:?}")),
                Err(err) => HostUpdate::Error(format!("scan failed: {err}")),
            };
            let _ = sink.send(message);
        });
    }

    /// Save one task from the editor; the reply feeds back through the update channel.
    pub fn save_task(&self, task_id: TaskId, input: neohub_core::catalog::UpdateTaskInput) {
        let handle = self.handle.clone();
        let sink = self.reply_sink.clone();
        self.runtime.spawn(async move {
            let reply = handle
                .request(CoreCommand::UpdateTask {
                    task_id,
                    input: Box::new(input),
                })
                .await;
            let message = match reply {
                Ok(CoreReply::Task(task)) => HostUpdate::TaskSaved(task),
                Ok(other) => HostUpdate::TaskError(format!("unexpected save reply: {other:?}")),
                Err(err) => HostUpdate::TaskError(err.to_string()),
            };
            let _ = sink.send(message);
        });
    }

    /// Where exported logs are written.
    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    /// Search every retained task's logs.
    pub fn search_logs(&self, query: String) {
        let handle = self.handle.clone();
        let sink = self.reply_sink.clone();
        self.runtime.spawn(async move {
            let reply = handle
                .request(CoreCommand::SearchLogs {
                    task_id: None,
                    query,
                    options: neohub_core::logs::SearchOptions::default(),
                })
                .await;
            let message = match reply {
                Ok(CoreReply::LogMatches(hits)) => HostUpdate::LogMatches(
                    hits.into_iter()
                        .map(|(task_id, hit)| (task_id, hit.line_no, hit.line))
                        .collect(),
                ),
                Ok(_) => HostUpdate::ExportError("unexpected search reply".into()),
                Err(err) => HostUpdate::ExportError(err.to_string()),
            };
            let _ = sink.send(message);
        });
    }

    /// Load one task's parsed terminal text.
    pub fn load_terminal(&self, task_id: TaskId) {
        let handle = self.handle.clone();
        let sink = self.reply_sink.clone();
        self.runtime.spawn(async move {
            let reply = handle
                .request(CoreCommand::GetTerminalText {
                    task_id: task_id.clone(),
                })
                .await;
            let message = match reply {
                Ok(CoreReply::Logs(text)) => HostUpdate::Terminal(task_id, text),
                Ok(_) => HostUpdate::ExportError("unexpected terminal reply".into()),
                Err(err) => HostUpdate::ExportError(err.to_string()),
            };
            let _ = sink.send(message);
        });
    }

    /// Export one task's logs into the data directory (`logs/`), returning the written path
    /// through the update channel.
    pub fn export_logs(&self, task_id: TaskId, logs_dir: std::path::PathBuf, raw: bool) {
        let handle = self.handle.clone();
        let sink = self.reply_sink.clone();
        self.runtime.spawn(async move {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_millis())
                .unwrap_or(0);
            let name = format!("{}-{stamp}{}.log", task_id, if raw { "-raw" } else { "" });
            let path = logs_dir.join(name);
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let reply = handle
                .request(CoreCommand::ExportLogs {
                    task_id,
                    path: path.clone(),
                    raw,
                })
                .await;
            let message = match reply {
                Ok(CoreReply::Exported(written)) => HostUpdate::Exported(written),
                Ok(_) => HostUpdate::ExportError("unexpected export reply".into()),
                Err(err) => HostUpdate::ExportError(err.to_string()),
            };
            let _ = sink.send(message);
        });
    }

    /// Load one scope's environment variables.
    pub fn load_env_vars(&self, scope: neohub_core::service::EnvScope) {
        let handle = self.handle.clone();
        let sink = self.reply_sink.clone();
        self.runtime.spawn(async move {
            let reply = handle
                .request(CoreCommand::ListEnvVars {
                    scope: scope.clone(),
                })
                .await;
            let message = match reply {
                Ok(CoreReply::EnvVars(rows)) => HostUpdate::EnvVars(scope, rows),
                Ok(_) => HostUpdate::EnvError("unexpected env reply".into()),
                Err(err) => HostUpdate::EnvError(err.to_string()),
            };
            let _ = sink.send(message);
        });
    }

    /// Persist one scope's variables; masked rows arrive as `None` and are kept as-is.
    pub fn save_env_vars(
        &self,
        scope: neohub_core::service::EnvScope,
        vars: Vec<neohub_core::service::EnvVarInput>,
    ) {
        let handle = self.handle.clone();
        let sink = self.reply_sink.clone();
        self.runtime.spawn(async move {
            let reply = handle
                .request(CoreCommand::SetEnvVars {
                    scope: scope.clone(),
                    vars,
                })
                .await;
            let message = match reply {
                Ok(CoreReply::EnvVars(rows)) => HostUpdate::EnvVars(scope, rows),
                Ok(_) => HostUpdate::EnvError("unexpected env reply".into()),
                Err(err) => HostUpdate::EnvError(err.to_string()),
            };
            let _ = sink.send(message);
        });
    }

    /// Persist a settings patch; the reply comes back through the update channel.
    pub fn save_settings(&self, patch: neohub_domain::SettingsPatch) {
        let handle = self.handle.clone();
        let sink = self.reply_sink.clone();
        self.runtime.spawn(async move {
            let reply = handle.request(CoreCommand::UpdateSettings { patch }).await;
            let message = match reply {
                Ok(CoreReply::Settings(settings)) => HostUpdate::SettingsSaved(settings),
                Ok(_) => HostUpdate::SettingsError("unexpected settings reply".into()),
                Err(err) => HostUpdate::SettingsError(err.to_string()),
            };
            let _ = sink.send(message);
        });
    }

    /// Save the app configuration form through the same command the MCP tool uses.
    pub fn save_app_config(
        &self,
        app_id: neohub_domain::AppId,
        patch: neohub_core::catalog::UpdateAppInput,
    ) {
        let handle = self.handle.clone();
        let sink = self.reply_sink.clone();
        self.runtime.spawn(async move {
            let reply = handle
                .request(CoreCommand::UpdateApp {
                    app_id,
                    input: Box::new(patch),
                })
                .await;
            let message = match reply {
                Ok(CoreReply::App(app)) => HostUpdate::AppSaved(app),
                Ok(_) => HostUpdate::AppSaveError("unexpected app reply".into()),
                Err(err) => HostUpdate::AppSaveError(err.to_string()),
            };
            let _ = sink.send(message);
        });
    }

    /// Apply a `neohub://` link: select the app it names (and its path, if given).
    ///
    /// A link may only *select*. It cannot start anything by itself: a URL arriving from a
    /// browser is not consent to run someone's dev server.
    pub fn apply_deep_link(&self, link: neohub_platform_macos::deep_link::DeepLink) {
        use neohub_platform_macos::deep_link::DeepLink;
        let mut store = self.store.lock().expect("store lock");
        // Resolve to an app id first, then select: the session owns navigation.
        let target = match &link {
            DeepLink::Open { app_id, path } => store
                .apps()
                .into_iter()
                .find(|app| {
                    app_id
                        .as_deref()
                        .is_some_and(|wanted| app.id.as_str() == wanted)
                        || path.as_deref().is_some_and(|wanted| app.path == wanted)
                })
                .map(|app| app.id),
            DeepLink::Start { .. } => None,
        };
        if let Some(app_id) = target {
            store.session_mut().select_app(app_id, None);
        }
        match link {
            DeepLink::Open { .. } => {}
            DeepLink::Start { .. } => {
                // Deliberately ignored: see the note above.
                eprintln!("[ui] start links are not honoured; select the app instead");
            }
        }
    }

    /// Ask GitHub whether a newer release exists (blocking call, off the UI thread).
    pub fn check_for_updates(&self) {
        let sink = self.reply_sink.clone();
        let current = env!("CARGO_PKG_VERSION").to_string();
        self.runtime.spawn_blocking(move || {
            let message = match neohub_core::update::check_blocking(&current) {
                Ok(status) => HostUpdate::UpdateChecked(status),
                Err(error) => HostUpdate::UpdateFailed(error),
            };
            let _ = sink.send(message);
        });
    }

    /// Import every ticked candidate, one at a time.
    ///
    /// Sequential on purpose: each import writes apps, tasks and env vars in one transaction, and
    /// running them in parallel would make the failure report depend on scheduling. Each success
    /// is marked in the panel so a retry after a partial failure re-imports only what is left.
    pub fn import_selected(&self) {
        let candidates = self
            .store
            .lock()
            .expect("store lock")
            .selected_import_candidates();
        if candidates.is_empty() {
            return;
        }
        let handle = self.handle.clone();
        let sink = self.reply_sink.clone();
        self.runtime.spawn(async move {
            let mut imported = 0usize;
            let mut failures: Vec<String> = Vec::new();
            for candidate in candidates {
                let input = neohub_core::catalog::CreateAppInput {
                    path: candidate.path.clone(),
                    name: Some(candidate.name.clone()),
                    node_version_pref: None,
                    package_manager: candidate.package_manager,
                    default_script: candidate.suggested_script.clone(),
                    first_task: candidate.suggested_script.clone().map(|script| {
                        neohub_core::catalog::CreateTaskSpec {
                            name: script.clone(),
                            command_kind: Some(neohub_domain::CommandKind::Script),
                            script: Some(script),
                            custom_command: None,
                            working_dir_override: None,
                        }
                    }),
                    tasks: Vec::new(),
                    env_vars: Vec::new(),
                };
                match handle
                    .request(CoreCommand::CreateApp {
                        input: Box::new(input),
                    })
                    .await
                {
                    Ok(_) => {
                        imported += 1;
                        // Marked as it lands, so a retry after a partial failure only re-imports
                        // what is left.
                        let _ = sink.send(HostUpdate::ImportedOne(candidate.path.clone(), true));
                    }
                    Err(error) => {
                        failures.push(format!("{}：{error}", candidate.name));
                        let _ = sink.send(HostUpdate::ImportedOne(candidate.path.clone(), false));
                    }
                }
            }
            let _ = sink.send(HostUpdate::BatchImported { imported, failures });
        });
    }

    /// Reveal the app's own log file in Finder (creates the folder first).
    pub fn open_log_dir(&self) {
        let dir = self.data_dir.join("logs");
        // The process log lives under ~/Library/Logs; fall back to the data dir if unavailable.
        let target = neohub_platform_macos::log_file::log_dir().unwrap_or(dir);
        if let Err(error) = neohub_platform_macos::log_file::reveal(&target) {
            eprintln!("[ui] could not open the log folder: {error}");
        }
    }

    /// Where an export goes: `<data-dir>/exports/neohub-<stamp>.db`.
    pub fn export_target(&self) -> std::path::PathBuf {
        self.data_dir.join("exports").join(format!(
            "neohub-{}.db",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_millis())
                .unwrap_or(0)
        ))
    }

    /// Export the database to `path` (refuses to overwrite).
    pub fn export_database(&self, path: std::path::PathBuf) {
        let handle = self.handle.clone();
        let sink = self.reply_sink.clone();
        self.runtime.spawn(async move {
            let message = match handle.request(CoreCommand::ExportDatabase { path }).await {
                Ok(CoreReply::DatabaseExported(path)) => HostUpdate::DatabaseExported(path),
                Ok(_) => HostUpdate::DatabaseError("unexpected export reply".into()),
                Err(error) => HostUpdate::DatabaseError(error.to_string()),
            };
            let _ = sink.send(message);
        });
    }

    /// Reset the database (the old file is moved aside, not deleted).
    pub fn reset_database(&self) {
        let handle = self.handle.clone();
        let sink = self.reply_sink.clone();
        self.runtime.spawn(async move {
            let message = match handle.request(CoreCommand::ResetDatabase).await {
                Ok(CoreReply::DatabaseReset(outcome)) => {
                    HostUpdate::DatabaseReset(outcome.moved.len())
                }
                Ok(_) => HostUpdate::DatabaseError("unexpected reset reply".into()),
                Err(error) => HostUpdate::DatabaseError(error.to_string()),
            };
            let _ = sink.send(message);
        });
    }

    /// Load the run history the detail pane renders.
    pub fn load_history(&self, app_id: AppId) {
        let handle = self.handle.clone();
        let sink = self.reply_sink.clone();
        self.runtime.spawn(async move {
            let reply = handle
                .request(CoreCommand::ListRunHistory {
                    app_id: app_id.clone(),
                    limit: Some(10),
                })
                .await;
            let message = match reply {
                Ok(CoreReply::RunHistory(rows)) => HostUpdate::History(app_id, rows),
                Ok(_) => HostUpdate::History(app_id, Vec::new()),
                Err(err) => HostUpdate::Error(format!("history failed: {err}")),
            };
            let _ = sink.send(message);
        });
    }

    /// Import one scanned candidate using the detection the scan already produced.
    pub fn import_candidate(&self, candidate: ImportCandidate) {
        let name = candidate.name.clone();
        let input = neohub_core::catalog::CreateAppInput {
            path: candidate.path.clone(),
            name: Some(candidate.name.clone()),
            node_version_pref: None,
            package_manager: candidate.package_manager,
            default_script: candidate.suggested_script.clone(),
            first_task: candidate.suggested_script.clone().map(|script| {
                neohub_core::catalog::CreateTaskSpec {
                    name: script.clone(),
                    command_kind: Some(neohub_domain::CommandKind::Script),
                    script: Some(script),
                    custom_command: None,
                    working_dir_override: None,
                }
            }),
            tasks: vec![],
            env_vars: vec![],
        };
        let handle = self.handle.clone();
        let sink = self.reply_sink.clone();
        self.runtime.spawn(async move {
            let reply = handle
                .request(CoreCommand::CreateApp {
                    input: Box::new(input),
                })
                .await;
            let message = match reply {
                Ok(CoreReply::App(_)) => HostUpdate::Imported,
                Ok(other) => HostUpdate::Error(format!("unexpected import reply: {other:?}")),
                Err(err) => HostUpdate::Error(format!("import {name} failed: {err}")),
            };
            let _ = sink.send(message);
        });
    }

    pub fn start_task(&self, task_id: TaskId) {
        self.send(CoreCommand::StartTask { task_id });
    }

    pub fn stop_task(&self, task_id: TaskId) {
        self.send(CoreCommand::StopTask { task_id });
    }

    pub fn start_app(&self, app_id: AppId) {
        self.send(CoreCommand::StartApp { app_id });
    }

    pub fn stop_app(&self, app_id: AppId) {
        self.send(CoreCommand::StopApp { app_id });
    }

    /// Apply queued updates; returns true when the visible state changed.
    fn drain(&self) -> bool {
        let updates: Vec<HostUpdate> = {
            let receiver = self.updates.lock().expect("updates lock");
            let mut drained = Vec::new();
            while let Ok(update) = receiver.try_recv() {
                drained.push(update);
                if drained.len() >= 512 {
                    break; // bound the work per frame; the rest arrives next tick
                }
            }
            drained
        };
        if updates.is_empty() {
            return false;
        }
        let mut changed = false;
        let mut need_snapshot = false;
        let mut notices: Vec<neohub_platform_macos::notify::Notification> = Vec::new();
        let mut saved_task: Option<neohub_domain::Task> = None;
        let mut db_notice_out: Option<String> = None;
        let mut notices_out: Vec<(crate::state::toast::ToastKind, String)> = Vec::new();
        let mut update_out: Option<(String, bool)> = None;
        let mut app_saved_out: Option<neohub_domain::App> = None;
        let mut app_save_error_out: Option<String> = None;
        let mut save_error: Option<String> = None;
        let mut saved_settings_out: Option<neohub_domain::Settings> = None;
        let mut settings_error_out: Option<String> = None;
        let mut env_updates_out: Vec<EnvUpdate> = Vec::new();
        let mut env_error_out: Option<String> = None;

        {
            let mut store = self.store.lock().expect("store lock");
            for update in updates {
                match update {
                    HostUpdate::Event(event) => {
                        let resync = matches!(
                            event.kind,
                            neohub_core::events::CoreEventKind::ResyncRequired { .. }
                        );
                        // A crash is the one state change worth interrupting the user for; the
                        // decision itself lives in `notify::for_task_exit` so it is testable.
                        if let neohub_core::events::CoreEventKind::TaskStatus {
                            task_id,
                            state,
                            ..
                        } = &event.kind
                        {
                            let settings = store.settings();
                            if settings.notify_on_crash {
                                let (app_name, task_name) = store.labels_for(task_id);
                                if let Some(notification) =
                                    neohub_platform_macos::notify::for_task_exit(
                                        &app_name,
                                        &task_name,
                                        true,
                                        *state == neohub_domain::ProcessState::Crashed,
                                        false,
                                    )
                                {
                                    notices.push(notification);
                                }
                            }
                        }
                        changed |= store.apply_event(event);
                        need_snapshot |= resync;
                    }
                    HostUpdate::Candidates(candidates) => {
                        store.set_import_candidates(candidates);
                        changed = true;
                    }
                    HostUpdate::ImportedOne(path, ok) => {
                        if ok {
                            store.mark_candidate_registered(&path);
                        }
                        changed = true;
                    }
                    HostUpdate::BatchImported { imported, failures } => {
                        store.set_import_busy(false);
                        if failures.is_empty() {
                            notices_out.push((
                                crate::state::toast::ToastKind::Success,
                                format!("已导入 {imported} 个项目"),
                            ));
                        } else {
                            notices_out.push((
                                crate::state::toast::ToastKind::Error,
                                format!(
                                    "导入 {imported} 个，{} 个失败：{}",
                                    failures.len(),
                                    failures.join("；")
                                ),
                            ));
                        }
                        need_snapshot = true;
                        changed = true;
                    }
                    HostUpdate::Imported => {
                        store.set_import_busy(false);
                        need_snapshot = true;
                        changed = true;
                    }
                    HostUpdate::History(app_id, rows) => {
                        store.set_history(app_id, rows);
                        changed = true;
                    }
                    HostUpdate::TaskSaved(task) => {
                        store.upsert_task((*task).clone());
                        saved_task = Some(*task);
                        changed = true;
                    }
                    HostUpdate::TaskError(message) => {
                        save_error = Some(message);
                        changed = true;
                    }
                    HostUpdate::AppSaved(app) => {
                        app_saved_out = Some(*app);
                        need_snapshot = true;
                        changed = true;
                    }
                    HostUpdate::AppSaveError(message) => {
                        app_save_error_out = Some(message);
                        changed = true;
                    }
                    HostUpdate::SettingsSaved(settings) => {
                        store.set_settings((*settings).clone());
                        saved_settings_out = Some(*settings);
                        changed = true;
                    }
                    HostUpdate::SettingsError(message) => {
                        settings_error_out = Some(message);
                        changed = true;
                    }
                    HostUpdate::EnvVars(scope, rows) => {
                        env_updates_out.push((scope, rows));
                        changed = true;
                    }
                    HostUpdate::EnvError(message) => {
                        env_error_out = Some(message);
                        changed = true;
                    }
                    HostUpdate::Terminal(task_id, text) => {
                        store.set_terminal_text(task_id, text);
                        changed = true;
                    }
                    HostUpdate::LogMatches(results) => {
                        store.set_search_results(results);
                        changed = true;
                    }
                    HostUpdate::Exported(path) => {
                        eprintln!("[ui] exported logs to {}", path.display());
                        changed = true;
                    }
                    HostUpdate::ExportError(message) => {
                        eprintln!("[ui] log export failed: {message}");
                        changed = true;
                    }
                    HostUpdate::UpdateChecked(status) => {
                        update_out = Some((status.summary(), status.is_available()));
                        changed = true;
                    }
                    HostUpdate::UpdateFailed(message) => {
                        update_out = Some((format!("检查更新失败：{message}"), false));
                        changed = true;
                    }
                    HostUpdate::DatabaseExported(path) => {
                        db_notice_out = Some(format!("数据库已导出到 {}", path.display()));
                        changed = true;
                    }
                    HostUpdate::DatabaseReset(moved) => {
                        db_notice_out = Some(format!("数据库已重置（{moved} 个旧文件已改名保存）"));
                        need_snapshot = true;
                        changed = true;
                    }
                    HostUpdate::DatabaseError(message) => {
                        db_notice_out = Some(format!("数据库操作失败：{message}"));
                        changed = true;
                    }
                    HostUpdate::Error(message) => {
                        store.set_import_error(message);
                        changed = true;
                    }
                }
            }
        }
        if need_snapshot {
            // Taken outside the store lock: `refresh_snapshot` re-locks it.
            self.refresh_snapshot();
            changed = true;
        }
        // Hand the app form outcome back the same way the task editor gets its result.
        // Sent outside the store lock: `osascript` is a process spawn, and holding the lock
        // across it would stall every other update.
        for notice in notices {
            if let Err(error) = neohub_platform_macos::notify::send(&notice) {
                eprintln!("[ui] notification failed: {error}");
            }
        }
        if let Some(update) = update_out
            && let Ok(mut guard) = self.last_update.lock()
        {
            *guard = Some(update);
        }
        for (kind, message) in notices_out {
            if let Ok(mut store) = self.store.lock() {
                store.notify(kind, message);
            }
        }
        if let Some(notice) = db_notice_out {
            // Database results also surface as a toast: the panel may be closed by the time the
            // operation finishes. Errors stay until dismissed.
            let kind = if notice.contains("失败") {
                crate::state::toast::ToastKind::Error
            } else {
                crate::state::toast::ToastKind::Success
            };
            if let Ok(mut store) = self.store.lock() {
                store.notify(kind, notice.clone());
            }
            if let Ok(mut guard) = self.last_db_notice.lock() {
                *guard = Some(notice);
            }
        }
        if (app_saved_out.is_some() || app_save_error_out.is_some())
            && let Ok(mut guard) = self.last_app_save.lock()
        {
            *guard = (app_saved_out, app_save_error_out);
        }
        // Hand the save outcome back to the editor that is waiting for it.
        if (saved_task.is_some() || save_error.is_some())
            && let Ok(mut guard) = self.last_save.lock()
        {
            *guard = (saved_task, save_error);
        }
        if (!env_updates_out.is_empty() || env_error_out.is_some())
            && let Ok(mut guard) = self.last_env.lock()
        {
            *guard = (std::mem::take(&mut env_updates_out), env_error_out);
        }
        if (saved_settings_out.is_some() || settings_error_out.is_some())
            && let Ok(mut guard) = self.last_settings.lock()
        {
            *guard = (saved_settings_out, settings_error_out);
        }
        changed
    }

    /// Persist the navigation session after a user action.
    fn persist_session(&self) {
        let session = self.store.lock().expect("store lock").session().clone();
        if let Err(err) = self.session_store.save(&session) {
            eprintln!("[ui] could not save the session: {err}");
        }
    }

    /// Take the last save outcome (task or error), clearing it.
    pub fn take_save_outcome(&self) -> (Option<neohub_domain::Task>, Option<String>) {
        let mut guard = self.last_save.lock().expect("save outcome");
        std::mem::take(&mut *guard)
    }

    /// Take environment-variable updates (scope + rows) and any error, clearing them.
    pub fn take_env_updates(&self) -> (Vec<EnvUpdate>, Option<String>) {
        let mut guard = self.last_env.lock().expect("env outcome");
        std::mem::take(&mut *guard)
    }

    /// Take the last settings save outcome (settings or error), clearing it.
    pub fn take_settings_outcome(&self) -> (Option<neohub_domain::Settings>, Option<String>) {
        let mut guard = self.last_settings.lock().expect("settings outcome");
        std::mem::take(&mut *guard)
    }

    fn with_session(&self, f: impl FnOnce(&mut Session)) {
        {
            let mut store = self.store.lock().expect("store lock");
            f(store.session_mut());
        }
        self.persist_session();
    }
}

/// Which shortcut fired.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shortcut {
    Settings,
    Import,
    Filter,
    GlobalSearch,
    RestartSelected,
    Palette,
}

gpui_kit::actions!(
    neohub,
    [
        /// ⌘, - open the settings panel.
        OpenSettings,
        /// ⌘N - open the import panel and scan the default folder.
        OpenImport,
        /// ⌘P - focus the sidebar filter.
        FocusFilter,
        /// ⌘⇧F - open the global log search.
        GlobalSearch,
        /// ⌘R - restart the selected app.
        RestartSelected,
        /// ⌘K - open the command palette.
        OpenCommandPalette,
    ]
);

/// What the sidebar row menu offers for a given state.
///
/// The same rule as the command palette: no row that would do nothing. A stopped app has no
/// "stop", a running one has no "start" — those are the rows a user hits by accident.
fn row_menu_actions(state: neohub_domain::ProcessState) -> Vec<(&'static str, &'static str)> {
    let mut actions = Vec::new();
    if state.is_live() {
        actions.push(("停止", "stop"));
        actions.push(("重启", "restart"));
    } else {
        actions.push(("启动", "start"));
    }
    actions.push(("应用配置", "config"));
    actions.push(("在 Finder 中显示", "find"));
    actions
}

/// The window root.
pub struct ShellView {
    bridge: Arc<HostBridge>,
    /// Sidebar filter (the ⌘P search box).
    search: TextField,
    search_focus: FocusHandle,
    /// App whose history was last requested (avoids re-requesting every frame).
    history_requested: Option<AppId>,
    /// Open task editor, if any.
    editor: Option<TaskForm>,
    editor_focus: FocusHandle,
    /// Open app configuration panel, if any.
    app_form: Option<crate::state::app_form::AppForm>,
    app_form_focus: FocusHandle,
    /// Open settings panel, if any.
    settings: Option<SettingsForm>,
    settings_focus: FocusHandle,
    /// Open environment editor, if any.
    env: Option<EnvEditor>,
    env_tab: ScopeTab,
    /// Row being edited in the env panel: (index, editing_key).
    env_editing: Option<(usize, bool)>,
    /// Buffer for the row currently being edited (used once inline cell editing lands).
    #[allow(dead_code)]
    env_text: TextField,
    env_paste: TextField,
    env_focus: FocusHandle,
    /// Rendered terminal for the selected task.
    terminal: Option<crate::terminal::TerminalView>,
    terminal_follow: bool,
    terminal_height: usize,
    /// Task the rendered terminal belongs to (avoids rebuilding every frame).
    terminal_task: Option<TaskId>,
    /// Database reset needs two clicks: the first arms, the second acts.
    db_reset_armed: bool,
    /// Open command palette, if any.
    palette: Option<crate::state::command_palette::CommandPalette>,
    palette_focus: FocusHandle,
    /// Global log search input.
    search_input: TextField,
    log_search_focus: FocusHandle,
}

impl ShellView {
    /// Keyboard shortcuts, matching the legacy app's. Registered once per window.
    fn bind_shortcuts(cx: &mut Context<Self>) {
        cx.bind_keys([
            gpui_kit::KeyBinding::new("cmd-,", OpenSettings, None),
            gpui_kit::KeyBinding::new("cmd-n", OpenImport, None),
            gpui_kit::KeyBinding::new("cmd-p", FocusFilter, None),
            gpui_kit::KeyBinding::new("cmd-shift-f", GlobalSearch, None),
            gpui_kit::KeyBinding::new("cmd-r", RestartSelected, None),
            gpui_kit::KeyBinding::new("cmd-k", OpenCommandPalette, None),
        ]);
    }

    pub fn new(bridge: Arc<HostBridge>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::bind_shortcuts(cx);
        let search_focus = cx.focus_handle();
        let palette_focus = cx.focus_handle();
        let editor_focus = cx.focus_handle();
        let settings_focus = cx.focus_handle();
        let app_form_focus = cx.focus_handle();
        let env_focus = cx.focus_handle();
        let log_search_focus = cx.focus_handle();
        ShellView {
            bridge,
            search: TextField::new(""),
            search_focus,
            history_requested: None,
            editor: None,
            editor_focus,
            app_form: None,
            settings: None,
            app_form_focus,
            settings_focus,
            env: None,
            env_tab: ScopeTab::Global,
            env_editing: None,
            env_text: TextField::new(""),
            env_paste: TextField::new(""),
            env_focus,
            terminal: None,
            terminal_follow: true,
            terminal_height: 16,
            terminal_task: None,
            db_reset_armed: false,
            palette: None,
            palette_focus,
            search_input: TextField::new(""),
            log_search_focus,
        }
    }

    /// Spawn the event pump that turns core events into repaints.
    pub fn start_event_pump(&self, cx: &mut Context<Self>) {
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx: &mut AsyncApp| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let changed = bridge.drain();
                if !changed {
                    continue;
                }
                if this
                    .update(cx, |_, cx| {
                        cx.notify();
                    })
                    .is_err()
                {
                    break; // window gone: the core keeps running
                }
            }
        })
        .detach();
    }

    fn palette(&self) -> Palette {
        let mode = self
            .bridge
            .store
            .lock()
            .expect("store lock")
            .snapshot()
            .map(|snapshot| snapshot.settings.theme)
            .unwrap_or(neohub_domain::ThemeSetting::Dark);
        match mode {
            neohub_domain::ThemeSetting::Light => LIGHT,
            neohub_domain::ThemeSetting::Dark => DARK,
            neohub_domain::ThemeSetting::System => DARK,
        }
    }
}

/// A settings toggle: the label, its current value and how to flip it.
type ToggleFn = fn(&mut SettingsForm);

fn state_color(palette: &Palette, state: ProcessState) -> Rgba {
    match state {
        ProcessState::Running => palette.success_strong,
        ProcessState::Starting | ProcessState::Exiting => palette.warn_strong,
        ProcessState::Crashed => palette.danger_strong,
        _ => palette.fg_subtle,
    }
}

/// Zero-sized filler so the empty state can keep the same flex layout.
fn grid_placeholder(_palette: &Palette) -> impl IntoElement {
    div().flex().gap(px(0.0))
}

fn state_label(state: ProcessState) -> &'static str {
    match state {
        ProcessState::Idle => "空闲",
        ProcessState::Starting => "启动中",
        ProcessState::Running => "运行中",
        ProcessState::Exiting => "停止中",
        ProcessState::Exited => "已停止",
        ProcessState::Crashed => "已崩溃",
    }
}

impl ShellView {
    /// Run the palette row the user clicked (matched by its label).
    fn run_palette_row(&mut self, label: &str, window: &mut Window, cx: &mut Context<Self>) {
        let action = self.palette.as_ref().and_then(|state| {
            state
                .visible()
                .iter()
                .find(|item| item.label == label)
                .map(|item| item.action.clone())
        });
        self.palette = None;
        if let Some(action) = action {
            self.perform_palette_action(action, window, cx);
        }
        cx.notify();
    }

    /// Carry out a palette action through the same paths the buttons use.
    fn perform_palette_action(
        &mut self,
        action: crate::state::command_palette::PaletteAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::state::command_palette::PaletteAction;
        match action {
            PaletteAction::OpenApp(app_id) => {
                self.bridge
                    .store
                    .lock()
                    .expect("store lock")
                    .session_mut()
                    .select_app(app_id, None);
            }
            PaletteAction::StartApp(app_id) => {
                self.bridge.send(CoreCommand::StartApp { app_id });
            }
            PaletteAction::StopApp(app_id) => {
                self.bridge.send(CoreCommand::StopApp { app_id });
            }
            PaletteAction::RestartApp(app_id) => {
                self.bridge.send(CoreCommand::RestartApp { app_id });
            }
            PaletteAction::Settings => {
                self.on_shortcut(Shortcut::Settings, window, cx);
                return;
            }
            PaletteAction::Import => {
                self.on_shortcut(Shortcut::Import, window, cx);
                return;
            }
            PaletteAction::GlobalSearch => {
                self.on_shortcut(Shortcut::GlobalSearch, window, cx);
                return;
            }
        }
        cx.notify();
    }

    /// Run one row-menu action. Every branch reuses the path the buttons use.
    fn run_row_action(
        &mut self,
        action: &str,
        app_id: neohub_domain::AppId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            "start" => self.bridge.send(CoreCommand::StartApp { app_id }),
            "stop" => self.bridge.send(CoreCommand::StopApp { app_id }),
            "restart" => self.bridge.send(CoreCommand::RestartApp { app_id }),
            "config" => {
                let app = self
                    .bridge
                    .store
                    .lock()
                    .expect("store lock")
                    .apps()
                    .into_iter()
                    .find(|candidate| candidate.id == app_id);
                if let Some(app) = app {
                    self.app_form = Some(crate::state::app_form::AppForm::from_app(&app));
                    let focus = self.app_form_focus.clone();
                    window.focus(&focus, cx);
                }
            }
            "find" => {
                let path = self
                    .bridge
                    .store
                    .lock()
                    .expect("store lock")
                    .apps()
                    .into_iter()
                    .find(|candidate| candidate.id == app_id)
                    .map(|app| std::path::PathBuf::from(app.path));
                if let Some(path) = path
                    && let Err(error) = neohub_platform_macos::open_in::launch(
                        neohub_platform_macos::open_in::OpenInTarget::Finder,
                        &path,
                    )
                {
                    eprintln!("[ui] reveal failed: {error}");
                }
            }
            other => eprintln!("[ui] unknown row action: {other}"),
        }
        cx.notify();
    }

    /// Apply a shortcut. Kept as one method so the bindings and the behaviour are adjacent.
    fn on_shortcut(&mut self, action: Shortcut, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            Shortcut::Settings => {
                let settings_now = self.bridge.store.lock().expect("store lock").settings();
                self.settings = Some(SettingsForm::new(settings_now));
            }
            Shortcut::Import => {
                let dir = HostBridge::default_import_dir();
                self.bridge.begin_import(dir);
            }
            Shortcut::Filter => {
                // Focus has to be moved on the window, not just remembered: the filter row
                // tracks this handle, so this is what puts the caret in the box.
                window.focus(&self.search_focus, cx);
            }
            Shortcut::GlobalSearch => {
                self.bridge
                    .store
                    .lock()
                    .expect("store lock")
                    .open_search(String::new());
                self.search_input.clear();
            }
            Shortcut::Palette => {
                let snapshot = self
                    .bridge
                    .store
                    .lock()
                    .expect("store lock")
                    .snapshot_for_palette();
                self.palette = Some(crate::state::command_palette::CommandPalette::new(
                    snapshot.as_ref(),
                ));
                let focus = self.palette_focus.clone();
                window.focus(&focus, cx);
            }
            Shortcut::RestartSelected => {
                // Clone out of the lock: `send` must not run while the store is borrowed.
                let app_id = self
                    .bridge
                    .store
                    .lock()
                    .expect("store lock")
                    .selected_app()
                    .map(|app| app.id.clone());
                if let Some(app_id) = app_id {
                    self.bridge.send(CoreCommand::RestartApp { app_id });
                }
            }
        }
        cx.notify();
    }

    /// Update-check outcome: (summary, has update), read without clearing so the panel keeps
    /// showing the answer.
    fn update_status(&self) -> Option<(String, bool)> {
        self.bridge
            .last_update
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    /// Message from the last database operation, drained once per frame.
    fn take_db_notice(&self) -> Option<String> {
        self.bridge
            .last_db_notice
            .lock()
            .map(|mut guard| guard.take())
            .unwrap_or(None)
    }

    /// Outcome of the last app-configuration save, drained once per frame.
    fn take_app_save_outcome(&self) -> (Option<neohub_domain::App>, Option<String>) {
        self.bridge
            .last_app_save
            .lock()
            .map(|mut guard| std::mem::take(&mut *guard))
            .unwrap_or((None, None))
    }
}

impl Render for ShellView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.palette();
        // Fold a completed save back into the open editor.
        let (saved, save_error) = self.bridge.take_save_outcome();
        if let Some(editor) = self.editor.as_mut() {
            if let Some(saved) = saved.as_ref() {
                let revision = editor
                    .take_save()
                    .map(|request| request.revision)
                    .unwrap_or_default();
                editor.ack(saved, revision);
                self.editor = None;
            } else if let Some(message) = save_error.as_ref() {
                editor.apply_core_error(message);
            }
        }

        // Fold env loads/saves into the open editor.
        let (env_updates, env_error) = self.bridge.take_env_updates();
        for (scope, rows) in env_updates {
            if let Some(editor) = self.env.as_mut()
                && editor.scope() == &scope
            {
                editor.ack(rows);
            }
        }
        if let (Some(message), Some(_)) = (env_error, self.env.as_ref()) {
            eprintln!("[ui] env editor: {message}");
        }

        // Read before any panel borrows `self` mutably.
        let update_status_now = self.update_status();

        // Fold a completed app-configuration save back into the open panel.
        let (app_saved, app_error) = self.take_app_save_outcome();
        if let Some(form) = self.app_form.as_mut() {
            if let Some(saved) = app_saved {
                form.reset_from_app(&saved);
            } else if let Some(message) = app_error {
                form.apply_error("name", message);
            }
        }

        // Fold a completed settings save back into the open panel.
        let (settings_saved, settings_error) = self.bridge.take_settings_outcome();
        if let Some(panel) = self.settings.as_mut() {
            if let Some(saved) = settings_saved {
                panel.ack(saved);
            }
            let _ = &settings_error;
        }

        // Nothing registered yet: say what to do instead of showing an empty grid.
        let no_apps = self.bridge.store.lock().expect("store lock").has_no_apps();
        let empty_state_notice = if no_apps {
            let entity = cx.entity().downgrade();
            div()
                .id("empty-state")
                .flex()
                .flex_col()
                .items_center()
                .gap(px(metrics::GAP_2))
                .p(px(metrics::PAD_4))
                .rounded(px(metrics::RADIUS_LG))
                .border_1()
                .border_color(palette.border)
                .bg(palette.bg_surface)
                .child(div().text_size(px(metrics::TEXT_BASE)).child("还没有项目"))
                .child(
                    div()
                        .text_size(px(metrics::TEXT_SM))
                        .text_color(palette.fg_muted)
                        .child("扫描一个目录，把本地 Node 项目加进来"),
                )
                .child(
                    div()
                        .id("empty-state-import")
                        .px(px(metrics::PAD_3))
                        .py(px(metrics::GAP_1))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(palette.accent)
                        .text_color(palette.accent_fg)
                        .text_size(px(metrics::TEXT_SM))
                        .cursor_pointer()
                        .child("导入项目…")
                        .on_click(move |_, _, cx| {
                            let _ = entity.update(cx, |this, cx| {
                                let dir = HostBridge::default_import_dir();
                                this.bridge.begin_import(dir);
                                cx.notify();
                            });
                        }),
                )
        } else {
            div().id("empty-state-placeholder").w(px(0.0)).h(px(0.0))
        };

        // Expiry runs on the same tick as everything else; errors are exempt (see toast.rs).
        if let Ok(mut store) = self.bridge.store.lock() {
            store.expire_toasts(std::time::Instant::now());
        }

        let query = self.search.text().to_string();
        let (sections, cards, totals, tags, filter) = {
            let mut store = self.bridge.store.lock().expect("store lock");
            if store.filter() != query {
                store.set_filter(query.clone());
            }
            (
                store.sidebar_sections(),
                store.dashboard_cards(),
                store.totals(),
                store.all_tags(),
                store.filter().to_string(),
            )
        };
        let (view, selected, is_empty, import) = {
            let store = self.bridge.store.lock().expect("store lock");
            (
                store.session().view,
                store.session().selected_app_id.clone(),
                store.is_empty(),
                store.import().clone(),
            )
        };

        let mut sidebar = div()
            .flex()
            .flex_col()
            .w(px(metrics::SIDEBAR_WIDTH))
            .h_full()
            .bg(palette.bg_surface)
            .border_r_1()
            .border_color(palette.border)
            .p(px(metrics::PAD_3))
            .gap(px(metrics::GAP_2));

        sidebar = sidebar.child(
            div()
                .text_size(px(metrics::TEXT_XS))
                .text_color(palette.fg_subtle)
                .child("NEOHUB"),
        );
        sidebar = sidebar.child(
            div().flex().flex_col().gap(px(metrics::GAP_1)).child(
                div()
                    .id("sidebar-filter")
                    .track_focus(&self.search_focus)
                    .px(px(metrics::PAD_2))
                    .py(px(metrics::GAP_1))
                    .rounded(px(metrics::RADIUS_MD))
                    .border_1()
                    .border_color(palette.border)
                    .bg(palette.bg_base)
                    .text_size(px(metrics::TEXT_SM))
                    .text_color(if query.is_empty() {
                        palette.fg_subtle
                    } else {
                        palette.fg
                    })
                    .child(if query.is_empty() {
                        "筛选应用（⌘P）".to_string()
                    } else {
                        filter.clone()
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        window.focus(&this.search_focus, cx);
                        cx.notify();
                    }))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        let keystroke = &event.keystroke;
                        let characters = keystroke.key_char.as_deref();
                        match this.search.apply_key(&keystroke.key, characters) {
                            KeyOutcome::Changed => cx.notify(),
                            KeyOutcome::Ignored => {}
                            KeyOutcome::Escape => {
                                this.search.clear();
                                cx.notify();
                            }
                            KeyOutcome::Submit => {}
                        }
                    })),
            ),
        );

        // Dashboard entry.
        {
            let bridge = self.bridge.clone();
            sidebar = sidebar.child(
                div()
                    .id("nav-dashboard")
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(metrics::PAD_2))
                    .py(px(metrics::GAP_1))
                    .rounded(px(metrics::RADIUS_MD))
                    .bg(if view == crate::state::session::View::Dashboard {
                        palette.bg_elevated
                    } else {
                        palette.bg_surface
                    })
                    .text_size(px(metrics::TEXT_BASE))
                    .text_color(palette.fg)
                    .cursor_pointer()
                    .child("仪表盘")
                    .child(
                        div()
                            .text_size(px(metrics::TEXT_XS))
                            .text_color(palette.fg_subtle)
                            .child(format!("{}", totals.running)),
                    )
                    .on_click(move |_, _, _| {
                        bridge.with_session(|session| session.go_dashboard());
                    }),
            );
        }

        {
            let entity = cx.entity().downgrade();
            let settings_now = self.bridge.store.lock().expect("store lock").settings();
            sidebar = sidebar.child(
                div()
                    .id("nav-settings")
                    .mt(px(metrics::GAP_3))
                    .px(px(metrics::PAD_2))
                    .py(px(metrics::GAP_1))
                    .rounded(px(metrics::RADIUS_MD))
                    .bg(if self.settings.is_some() {
                        palette.bg_elevated
                    } else {
                        palette.bg_surface
                    })
                    .text_size(px(metrics::TEXT_SM))
                    .text_color(palette.fg_muted)
                    .cursor_pointer()
                    .child("设置")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if this.settings.is_some() {
                            this.settings = None;
                        } else {
                            this.settings = Some(SettingsForm::new(settings_now.clone()));
                            window.focus(&this.settings_focus, cx);
                        }
                        cx.notify();
                    })),
            );
            let _ = entity;
        }

        let row_menu_for = self.bridge.store.lock().expect("store lock").row_menu_app();
        for section in &sections {
            let entity = cx.entity().downgrade();
            let title = section.title.clone();
            let collapsed = section.collapsed;
            sidebar = sidebar.child(
                div()
                    .id(format!("nav-section-{title}"))
                    .mt(px(metrics::GAP_2))
                    .flex()
                    .items_center()
                    .gap(px(metrics::GAP_1))
                    .text_size(px(metrics::TEXT_XS))
                    .text_color(palette.fg_subtle)
                    .cursor_pointer()
                    // The marker says which way the click will go, so the header is readable
                    // without hovering it.
                    .child(if collapsed { "▸" } else { "▾" })
                    .child(title.clone())
                    .on_click(move |_, _, cx| {
                        let title = title.clone();
                        let _ = entity.update(cx, |this, cx| {
                            this.bridge
                                .store
                                .lock()
                                .expect("store lock")
                                .toggle_section(&title);
                            cx.notify();
                        });
                    }),
            );
            if collapsed {
                continue;
            }
            for row in &section.apps {
                let bridge = self.bridge.clone();
                let app_id = row.app_id.clone();
                let is_selected = selected.as_ref() == Some(&row.app_id);
                let menu_app_id = row.app_id.clone();
                let menu_entity = cx.entity().downgrade();
                let row_menu_open = row_menu_for.as_ref() == Some(&row.app_id);
                sidebar = sidebar.child(
                    div()
                        .id(format!("nav-app-{}", row.app_id))
                        .flex()
                        .items_center()
                        .gap(px(metrics::GAP_2))
                        .px(px(metrics::PAD_2))
                        .py(px(metrics::GAP_1))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(if is_selected {
                            palette.bg_elevated
                        } else {
                            palette.bg_surface
                        })
                        .cursor_pointer()
                        .child(
                            div()
                                .w(px(8.0))
                                .h(px(8.0))
                                .rounded_full()
                                .bg(state_color(&palette, row.state)),
                        )
                        .child(
                            div()
                                .text_size(px(metrics::TEXT_BASE))
                                .text_color(palette.fg)
                                .child(row.name.clone()),
                        )
                        .child(
                            div()
                                .id(format!("nav-menu-{}", row.app_id))
                                .px(px(2.0))
                                .rounded(px(metrics::RADIUS_MD))
                                .text_size(px(metrics::TEXT_XS))
                                .text_color(palette.fg_subtle)
                                .cursor_pointer()
                                .child("⋮")
                                .on_click(move |_, _, cx| {
                                    let app_id = menu_app_id.clone();
                                    let _ = menu_entity.update(cx, |this, cx| {
                                        if let Ok(mut store) = this.bridge.store.lock() {
                                            store.toggle_row_menu(&app_id);
                                        }
                                        cx.notify();
                                    });
                                }),
                        )
                        .on_click(move |_, _, _| {
                            bridge.with_session(|session| session.select_app(app_id.clone(), None));
                        }),
                );

                // Inline row menu: a small column of actions right under the row. Inline rather
                // than a floating popup because the sidebar is narrow and a popup would cover the
                // neighbouring rows it is meant to act on.
                if row_menu_open {
                    let mut menu = div()
                        .id(format!("nav-menu-panel-{}", row.app_id))
                        .flex()
                        .flex_col()
                        .gap(px(1.0))
                        .pl(px(metrics::PAD_3))
                        .pb(px(metrics::GAP_1));
                    for (label, action) in row_menu_actions(row.state) {
                        let bridge = self.bridge.clone();
                        let app_id = row.app_id.clone();
                        let entity = cx.entity().downgrade();
                        menu = menu.child(
                            div()
                                .id(format!("nav-action-{}-{label}", row.app_id))
                                .px(px(metrics::PAD_2))
                                .py(px(1.0))
                                .rounded(px(metrics::RADIUS_MD))
                                .bg(palette.bg_elevated)
                                .text_size(px(metrics::TEXT_XS))
                                .cursor_pointer()
                                .child(label)
                                .on_click(move |_, window, cx| {
                                    let bridge = bridge.clone();
                                    let app_id = app_id.clone();
                                    let _ = entity.update(cx, |this, cx| {
                                        this.run_row_action(action, app_id, window, cx);
                                        if let Ok(mut store) = bridge.store.lock() {
                                            store.close_row_menu();
                                        }
                                    });
                                }),
                        );
                    }
                    sidebar = sidebar.child(menu);
                }
            }
        }

        // Main pane.
        let mut main = div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .bg(palette.bg_base)
            .text_color(palette.fg)
            .p(px(metrics::PAD_6))
            .gap(px(metrics::GAP_3));

        main = main.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(div().text_size(px(metrics::TEXT_LG)).child(
                    if view == crate::state::session::View::Dashboard {
                        "仪表盘"
                    } else {
                        "项目"
                    },
                ))
                .child(
                    div()
                        .flex()
                        .gap(px(metrics::GAP_3))
                        .text_size(px(metrics::TEXT_SM))
                        .text_color(palette.fg_muted)
                        .child(format!("运行中 {} / {}", totals.running, totals.apps))
                        .child(format!("CPU {:.1}%", totals.cpu_percent))
                        .child(format!("内存 {:.0} MB", totals.mem_mb))
                        .child(format!("端口 {}", totals.ports)),
                )
                .child({
                    let entity = cx.entity().downgrade();
                    let focus = self.log_search_focus.clone();
                    div()
                        .id("nav-log-search")
                        .px(px(metrics::PAD_3))
                        .py(px(metrics::GAP_1))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(palette.bg_elevated)
                        .text_size(px(metrics::TEXT_SM))
                        .cursor_pointer()
                        .child("搜索日志")
                        .on_click(move |_, window, cx| {
                            let _ = entity.update(cx, |this, cx| {
                                this.bridge
                                    .store
                                    .lock()
                                    .expect("store lock")
                                    .open_search("");
                                window.focus(&focus, cx);
                                cx.notify();
                            });
                        })
                })
                .child({
                    let bridge = self.bridge.clone();
                    let entity = cx.entity().downgrade();
                    div()
                        .id("nav-import")
                        .px(px(metrics::PAD_3))
                        .py(px(metrics::GAP_1))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(palette.bg_elevated)
                        .text_size(px(metrics::TEXT_SM))
                        .cursor_pointer()
                        .child("+ 导入项目")
                        .on_click(move |_, _, cx| {
                            bridge.begin_import(HostBridge::default_import_dir());
                            let _ = entity.update(cx, |_, cx| cx.notify());
                        })
                }),
        );

        if !tags.is_empty() {
            let mut tag_row = div().flex().gap(px(metrics::GAP_1));
            for tag in tags.iter().take(8) {
                tag_row = tag_row.child(
                    div()
                        .px(px(metrics::PAD_2))
                        .py(px(1.0))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(palette.bg_surface)
                        .text_size(px(metrics::TEXT_XS))
                        .text_color(palette.fg_muted)
                        .child(format!("#{tag}")),
                );
            }
            main = main.child(tag_row);
        }

        if view == crate::state::session::View::App
            && let Some(app_id) = selected.clone()
            && self.history_requested.as_ref() != Some(&app_id)
        {
            self.history_requested = Some(app_id.clone());
            self.bridge.load_history(app_id);
        }

        let search_state = {
            let store = self.bridge.store.lock().expect("store lock");
            store.search().clone()
        };
        if search_state.open {
            let mut panel = div()
                .flex()
                .flex_col()
                .gap(px(metrics::GAP_2))
                .p(px(metrics::PAD_4))
                .rounded(px(metrics::RADIUS_LG))
                .border_1()
                .border_color(palette.border)
                .bg(palette.bg_surface)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(metrics::TEXT_BASE))
                                .child("全局日志搜索"),
                        )
                        .child({
                            let entity = cx.entity().downgrade();
                            div()
                                .id("log-search-close")
                                .px(px(metrics::PAD_2))
                                .rounded(px(metrics::RADIUS_MD))
                                .bg(palette.bg_elevated)
                                .text_size(px(metrics::TEXT_XS))
                                .cursor_pointer()
                                .child("关闭")
                                .on_click(move |_, _, cx| {
                                    let _ = entity.update(cx, |this, cx| {
                                        this.bridge
                                            .store
                                            .lock()
                                            .expect("store lock")
                                            .close_search();
                                        cx.notify();
                                    });
                                })
                        }),
                );

            let mut input_row = div().flex().items_center().gap(px(metrics::GAP_2));
            {
                let entity = cx.entity().downgrade();
                let focus = self.search_focus.clone();
                input_row = input_row.child(
                    div()
                        .id("log-search-input")
                        .track_focus(&self.log_search_focus)
                        .px(px(metrics::PAD_2))
                        .py(px(1.0))
                        .rounded(px(metrics::RADIUS_MD))
                        .border_1()
                        .border_color(palette.border)
                        .bg(palette.bg_base)
                        .text_size(px(metrics::TEXT_SM))
                        .cursor_pointer()
                        .child(if self.search_input.text().is_empty() {
                            "输入关键字…".to_string()
                        } else {
                            self.search_input.text().to_string()
                        })
                        .on_click(move |_, window, cx| {
                            window.focus(&focus, cx);
                            let _ = entity.update(cx, |_, cx| cx.notify());
                        }),
                );
            }
            {
                let bridge = self.bridge.clone();
                let entity = cx.entity().downgrade();
                input_row = input_row.child(
                    div()
                        .id("log-search-run")
                        .px(px(metrics::PAD_3))
                        .py(px(1.0))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(palette.accent)
                        .text_color(palette.accent_fg)
                        .text_size(px(metrics::TEXT_SM))
                        .cursor_pointer()
                        .child("搜索")
                        .on_click(move |_, _, cx| {
                            let _ = entity.update(cx, |this, cx| {
                                let query = this.search_input.text().to_string();
                                this.bridge
                                    .store
                                    .lock()
                                    .expect("store lock")
                                    .open_search(query.clone());
                                bridge.search_logs(query);
                                cx.notify();
                            });
                        }),
                );
            }
            panel = panel.child(input_row);

            if search_state.busy {
                panel = panel.child(
                    div()
                        .text_size(px(metrics::TEXT_XS))
                        .text_color(palette.fg_muted)
                        .child("搜索中…"),
                );
            } else if search_state.results.is_empty() {
                panel = panel.child(
                    div()
                        .text_size(px(metrics::TEXT_XS))
                        .text_color(palette.fg_muted)
                        .child("没有匹配（已退出但未淘汰的日志同样会被搜索）"),
                );
            }

            for (task_id, line_no, line) in search_state.results.iter().take(50) {
                let (app_name, task_name) = {
                    let store = self.bridge.store.lock().expect("store lock");
                    store.labels_for(task_id)
                };
                let entity = cx.entity().downgrade();
                let task_id = task_id.clone();
                panel = panel.child(
                    div()
                        .id(format!("log-hit-{task_id}-{line_no}"))
                        .flex()
                        .flex_col()
                        .px(px(metrics::PAD_2))
                        .py(px(1.0))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(palette.bg_base)
                        .text_size(px(metrics::TEXT_XS))
                        .cursor_pointer()
                        .child(
                            div()
                                .text_color(palette.fg_subtle)
                                .child(format!("{app_name} / {task_name} · 第 {line_no} 行")),
                        )
                        .child(div().text_color(palette.fg).child(line.clone()))
                        .on_click(move |_, _, cx| {
                            let _ = entity.update(cx, |this, cx| {
                                let app_id = {
                                    let store = this.bridge.store.lock().expect("store lock");
                                    store.locate_task(&task_id)
                                };
                                if let Some(app_id) = app_id {
                                    this.bridge.with_session(|session| {
                                        session.select_app(app_id.clone(), Some(task_id.clone()))
                                    });
                                    this.history_requested = None;
                                    this.terminal = None;
                                    this.terminal_task = None;
                                }
                                this.bridge.store.lock().expect("store lock").close_search();
                                cx.notify();
                            });
                        }),
                );
            }

            panel = panel.on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                let characters = event.keystroke.key_char.as_deref();
                match this
                    .search_input
                    .apply_key(&event.keystroke.key, characters)
                {
                    KeyOutcome::Changed => cx.notify(),
                    KeyOutcome::Escape => {
                        this.bridge.store.lock().expect("store lock").close_search();
                        cx.notify();
                    }
                    KeyOutcome::Submit => {
                        let query = this.search_input.text().to_string();
                        this.bridge
                            .store
                            .lock()
                            .expect("store lock")
                            .open_search(query.clone());
                        this.bridge.search_logs(query);
                        cx.notify();
                    }
                    KeyOutcome::Ignored => {}
                }
            }));
            main = main.child(panel);
        } else if let Some(palette_state) = self.palette.as_mut() {
            // `palette` (the colour palette) is already bound above; this is the command palette.
            let query = palette_state.query().to_string();
            let cursor = palette_state.cursor();
            let empty = palette_state.is_empty();
            let rows: Vec<(String, String)> = palette_state
                .visible()
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let label = if index == cursor {
                        format!("› {}", item.label)
                    } else {
                        format!("   {}", item.label)
                    };
                    (label, item.detail.clone())
                })
                .collect();

            let mut body = div()
                .id("command-palette")
                .track_focus(&self.palette_focus)
                .flex()
                .flex_col()
                .gap(px(metrics::GAP_1))
                .p(px(metrics::PAD_3))
                .rounded(px(metrics::RADIUS_LG))
                .border_1()
                .border_color(palette.border_strong)
                .bg(palette.bg_surface);

            body = body.child(
                div()
                    .px(px(metrics::PAD_2))
                    .py(px(metrics::GAP_1))
                    .rounded(px(metrics::RADIUS_MD))
                    .bg(palette.bg_base)
                    .text_size(px(metrics::TEXT_BASE))
                    .child(if query.is_empty() {
                        "输入命令…".to_string()
                    } else {
                        format!("{query}▌")
                    }),
            );

            if empty {
                body = body.child(
                    div()
                        .text_size(px(metrics::TEXT_XS))
                        .text_color(palette.fg_muted)
                        .child("没有匹配项"),
                );
            }

            for (label, detail) in rows {
                let entity = cx.entity().downgrade();
                // The label carries the cursor marker; strip it to find the row again.
                let plain = label
                    .trim_start_matches("› ")
                    .trim_start_matches("   ")
                    .to_string();
                body = body.child(
                    div()
                        .id(format!("palette-row-{plain}"))
                        .flex()
                        .items_center()
                        .justify_between()
                        .px(px(metrics::PAD_2))
                        .py(px(1.0))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(palette.bg_elevated)
                        .text_size(px(metrics::TEXT_SM))
                        .cursor_pointer()
                        .child(label)
                        .child(
                            div()
                                .text_size(px(metrics::TEXT_XS))
                                .text_color(palette.fg_subtle)
                                .child(detail),
                        )
                        .on_click(move |_, window, cx| {
                            let plain = plain.clone();
                            let _ = entity.update(cx, |this, cx| {
                                this.run_palette_row(&plain, window, cx);
                            });
                        }),
                );
            }

            body = body.on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let characters = event.keystroke.key_char.as_deref();
                let key = event.keystroke.key.clone();
                let outcome = match this.palette.as_mut() {
                    Some(state) => state.apply_key(&key, characters),
                    None => return,
                };
                match outcome {
                    crate::state::command_palette::PaletteOutcome::Run(action) => {
                        this.palette = None;
                        this.perform_palette_action(action, window, cx);
                    }
                    crate::state::command_palette::PaletteOutcome::Close => {
                        this.palette = None;
                    }
                    crate::state::command_palette::PaletteOutcome::Moved
                    | crate::state::command_palette::PaletteOutcome::Ignored => {}
                }
                cx.notify();
            }));

            main = main.child(body);
        } else if let Some(form) = self.app_form.as_mut() {
            // App configuration: nine fields, same layout language as the task editor.
            let fields = form.fields().clone();
            let errors = form.errors().clone();
            let dirty = form.is_dirty();
            let mut body = div()
                .flex()
                .flex_col()
                .gap(px(metrics::GAP_2))
                .p(px(metrics::PAD_4))
                .rounded(px(metrics::RADIUS_LG))
                .border_1()
                .border_color(palette.border)
                .bg(palette.bg_surface)
                .child(div().flex().items_center().justify_between().child(
                    div().text_size(px(metrics::TEXT_BASE)).child(if dirty {
                        "应用配置（未保存）"
                    } else {
                        "应用配置"
                    }),
                ));

            // Text rows: (label, field name, value, placeholder).
            for (label, field, value, placeholder) in [
                ("名称", "name", fields.name.clone(), "显示名称"),
                ("文件夹", "folder", fields.folder.clone(), "分组，可留空"),
                ("标签", "tags", fields.tags.clone(), "逗号分隔，如 api, web"),
                (
                    "监听文件",
                    "watch_globs",
                    fields.watch_globs.clone(),
                    "每行一个，相对路径",
                ),
                (
                    "端口提示",
                    "port_hint",
                    fields.port_hint.clone(),
                    "如 3000，可留空",
                ),
                (
                    "Node 版本",
                    "node_version",
                    fields.node_version.clone(),
                    "auto / system / 20.11.0",
                ),
            ] {
                let entity = cx.entity().downgrade();
                let focus = self.app_form_focus.clone();
                let editing = form.focused_field() == field;
                let current = if editing {
                    form.text().text().to_string()
                } else {
                    value.clone()
                };
                let error = errors.get(field).cloned();
                body = body.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .text_size(px(metrics::TEXT_XS))
                                .text_color(palette.fg_muted)
                                .child(label),
                        )
                        .child(
                            div()
                                .id(format!("app-field-{field}"))
                                .px(px(metrics::PAD_2))
                                .py(px(1.0))
                                .rounded(px(metrics::RADIUS_MD))
                                .border_1()
                                .border_color(if error.is_some() {
                                    palette.danger_fg
                                } else {
                                    palette.border
                                })
                                .bg(palette.bg_elevated)
                                .text_size(px(metrics::TEXT_SM))
                                .cursor_text()
                                .child(if current.is_empty() && !editing {
                                    placeholder.to_string()
                                } else if editing {
                                    format!("{current}▌")
                                } else {
                                    current.clone()
                                })
                                .on_click(move |_, window, cx| {
                                    let focus = focus.clone();
                                    let _ = entity.update(cx, |this, cx| {
                                        if let Some(form) = this.app_form.as_mut() {
                                            form.focus_field(field);
                                        }
                                        window.focus(&focus, cx);
                                        cx.notify();
                                    });
                                }),
                        )
                        .child(
                            div()
                                .text_size(px(metrics::TEXT_XS))
                                .text_color(palette.danger_fg)
                                .child(error.clone().unwrap_or_default()),
                        ),
                );
            }

            // Toggles.
            for (label, field, active) in [
                ("开机自启（应用）", "auto_start", fields.auto_start),
                (
                    "文件变化自动重启",
                    "auto_restart_on_change",
                    fields.auto_restart_on_change,
                ),
            ] {
                let entity = cx.entity().downgrade();
                body = body.child(
                    div()
                        .id(format!("app-toggle-{field}"))
                        .flex()
                        .items_center()
                        .gap(px(metrics::GAP_2))
                        .cursor_pointer()
                        .text_size(px(metrics::TEXT_SM))
                        .child(if active { "✓" } else { "·" })
                        .child(label)
                        .on_click(move |_, _, cx| {
                            let _ = entity.update(cx, |this, cx| {
                                if let Some(form) = this.app_form.as_mut() {
                                    let flipped = match field {
                                        "auto_start" => !form.fields().auto_start,
                                        _ => !form.fields().auto_restart_on_change,
                                    };
                                    form.set_toggle(field, flipped);
                                }
                                cx.notify();
                            });
                        }),
                );
            }

            // Save / cancel.
            let mut buttons = div().flex().gap(px(metrics::GAP_2));
            {
                let entity = cx.entity().downgrade();
                let bridge = self.bridge.clone();
                let app_id = form.app_id().to_string();
                buttons = buttons.child(
                    div()
                        .id("app-form-save")
                        .px(px(metrics::PAD_3))
                        .py(px(metrics::GAP_1))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(palette.accent)
                        .text_color(palette.accent_fg)
                        .text_size(px(metrics::TEXT_SM))
                        .cursor_pointer()
                        .child("保存")
                        .on_click(move |_, _, cx| {
                            let bridge = bridge.clone();
                            let app_id = app_id.clone();
                            let _ = entity.update(cx, |this, cx| {
                                let Some(form) = this.app_form.as_ref() else {
                                    return;
                                };
                                match form.to_patch() {
                                    Ok(patch) => bridge.save_app_config(
                                        neohub_domain::AppId::from_existing(app_id),
                                        patch,
                                    ),
                                    Err(errors) => {
                                        // Surface every field error at once; the model already
                                        // names the field, the view just shows them.
                                        if let Some(form) = this.app_form.as_mut() {
                                            for (field, message) in errors {
                                                form.apply_error(field, message);
                                            }
                                        }
                                    }
                                }
                                cx.notify();
                            });
                        }),
                );
            }
            {
                let entity = cx.entity().downgrade();
                buttons = buttons.child(
                    div()
                        .id("app-form-cancel")
                        .px(px(metrics::PAD_3))
                        .py(px(metrics::GAP_1))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(palette.bg_elevated)
                        .text_size(px(metrics::TEXT_SM))
                        .cursor_pointer()
                        .child("取消")
                        .on_click(move |_, _, cx| {
                            let _ = entity.update(cx, |this, cx| {
                                this.app_form = None;
                                cx.notify();
                            });
                        }),
                );
            }
            body = body.child(
                div()
                    .text_size(px(metrics::TEXT_XS))
                    .text_color(palette.fg_subtle)
                    .child("点字段后输入，Tab 切换，Esc 关闭"),
            );
            body = body.child(buttons);
            body = body.on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                let characters = event.keystroke.key_char.as_deref();
                let outcome = match this.app_form.as_mut() {
                    Some(form) => form.apply_key(&event.keystroke.key, characters),
                    None => return,
                };
                if outcome == KeyOutcome::Escape {
                    this.app_form = None;
                }
                cx.notify();
            }));

            main = main.child(body);
        } else if let Some(panel) = self.settings.as_mut() {
            let draft = panel.draft().clone();
            let dirty = panel.is_dirty();
            let mut body = div()
                .flex()
                .flex_col()
                .gap(px(metrics::GAP_2))
                .p(px(metrics::PAD_4))
                .rounded(px(metrics::RADIUS_LG))
                .border_1()
                .border_color(palette.border)
                .bg(palette.bg_surface)
                .child(div().flex().items_center().justify_between().child(
                    div().text_size(px(metrics::TEXT_BASE)).child(if dirty {
                        "设置（未保存）"
                    } else {
                        "设置"
                    }),
                ));

            // Theme + language.
            let mut theme_row = div().flex().gap(px(metrics::GAP_1));
            for (label, theme) in [
                ("深色", neohub_domain::ThemeSetting::Dark),
                ("浅色", neohub_domain::ThemeSetting::Light),
                ("跟随系统", neohub_domain::ThemeSetting::System),
            ] {
                let entity = cx.entity().downgrade();
                let active = draft.theme == theme;
                theme_row = theme_row.child(
                    div()
                        .id(format!("theme-{:?}", theme))
                        .px(px(metrics::PAD_2))
                        .py(px(1.0))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(if active {
                            palette.accent
                        } else {
                            palette.bg_elevated
                        })
                        .text_color(if active {
                            palette.accent_fg
                        } else {
                            palette.fg_muted
                        })
                        .text_size(px(metrics::TEXT_XS))
                        .cursor_pointer()
                        .child(label)
                        .on_click(move |_, _, cx| {
                            let _ = entity.update(cx, |this, cx| {
                                if let Some(panel) = this.settings.as_mut() {
                                    panel.set_theme(theme);
                                }
                                cx.notify();
                            });
                        }),
                );
            }
            body = body.child(theme_row);

            let mut lang_row = div().flex().gap(px(metrics::GAP_1));
            for (label, language) in [
                ("跟随系统", neohub_domain::LanguageSetting::System),
                ("中文", neohub_domain::LanguageSetting::ZhCn),
                ("English", neohub_domain::LanguageSetting::En),
            ] {
                let entity = cx.entity().downgrade();
                let active = draft.language == language;
                lang_row = lang_row.child(
                    div()
                        .id(format!("lang-{:?}", language))
                        .px(px(metrics::PAD_2))
                        .py(px(1.0))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(if active {
                            palette.accent
                        } else {
                            palette.bg_elevated
                        })
                        .text_color(if active {
                            palette.accent_fg
                        } else {
                            palette.fg_muted
                        })
                        .text_size(px(metrics::TEXT_XS))
                        .cursor_pointer()
                        .child(label)
                        .on_click(move |_, _, cx| {
                            let _ = entity.update(cx, |this, cx| {
                                if let Some(panel) = this.settings.as_mut() {
                                    panel.set_language(language);
                                }
                                cx.notify();
                            });
                        }),
                );
            }
            body = body.child(lang_row);

            // Booleans.
            #[allow(clippy::type_complexity)]
            let toggles: Vec<(&str, bool, ToggleFn)> = vec![
                ("自动检查更新", draft.auto_update, |form| {
                    form.toggle_auto_update()
                }),
                ("崩溃时通知", draft.notify_on_crash, |form| {
                    form.toggle_notify_on_crash()
                }),
                ("就绪时通知", draft.notify_on_ready, |form| {
                    form.toggle_notify_on_ready()
                }),
                ("登录时启动", draft.launch_at_login, |form| {
                    form.toggle_launch_at_login()
                }),
                ("显示菜单栏图标", draft.tray_enabled, |form| {
                    form.toggle_tray()
                }),
                ("启用 MCP 服务", draft.mcp_enabled, |form| {
                    form.toggle_mcp_enabled()
                }),
                ("MCP 要求 Token", draft.mcp_require_auth, |form| {
                    form.toggle_mcp_require_auth()
                }),
                ("MCP 使用 HTTPS", draft.mcp_https, |form| {
                    form.toggle_mcp_https()
                }),
            ];
            for (label, value, apply) in toggles {
                let entity = cx.entity().downgrade();
                body = body.child(
                    div()
                        .id(format!("toggle-{label}"))
                        .flex()
                        .items_center()
                        .justify_between()
                        .px(px(metrics::PAD_2))
                        .py(px(1.0))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(palette.bg_base)
                        .text_size(px(metrics::TEXT_XS))
                        .cursor_pointer()
                        .child(label)
                        .child(if value { "开" } else { "关" })
                        .on_click(move |_, _, cx| {
                            let _ = entity.update(cx, |this, cx| {
                                if let Some(panel) = this.settings.as_mut() {
                                    apply(panel);
                                }
                                cx.notify();
                            });
                        }),
                );
            }

            // Numeric settings with steppers (no free-form input needed; clamps stay central).
            let steps: [(NumberField, u64); 6] = [
                (NumberField::LogRingSize, 1_000),
                (NumberField::KillGraceMs, 500),
                (NumberField::DashboardRefreshMs, 100),
                (NumberField::RunHistoryLimit, 50),
                (NumberField::ReadinessTimeoutMs, 1_000),
                (NumberField::McpPort, 1),
            ];
            for (field, step) in steps {
                let value = field.value(&draft);
                let dec = cx.entity().downgrade();
                let inc = cx.entity().downgrade();
                body = body.child(
                    div()
                        .id(format!("num-{:?}", field))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(metrics::GAP_2))
                        .px(px(metrics::PAD_2))
                        .py(px(1.0))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(palette.bg_base)
                        .text_size(px(metrics::TEXT_XS))
                        .child(field.label())
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(metrics::GAP_1))
                                .child(
                                    div()
                                        .id(format!("num-dec-{:?}", field))
                                        .px(px(metrics::PAD_2))
                                        .rounded(px(metrics::RADIUS_MD))
                                        .bg(palette.bg_elevated)
                                        .cursor_pointer()
                                        .child("−")
                                        .on_click(move |_, _, cx| {
                                            let _ = dec.update(cx, |this, cx| {
                                                if let Some(panel) = this.settings.as_mut() {
                                                    let _ = panel.set_number(
                                                        field,
                                                        value.saturating_sub(step),
                                                    );
                                                }
                                                cx.notify();
                                            });
                                        }),
                                )
                                .child(format!("{value}"))
                                .child(
                                    div()
                                        .id(format!("num-inc-{:?}", field))
                                        .px(px(metrics::PAD_2))
                                        .rounded(px(metrics::RADIUS_MD))
                                        .bg(palette.bg_elevated)
                                        .cursor_pointer()
                                        .child("+")
                                        .on_click(move |_, _, cx| {
                                            let _ = inc.update(cx, |this, cx| {
                                                if let Some(panel) = this.settings.as_mut() {
                                                    let _ = panel.set_number(field, value + step);
                                                }
                                                cx.notify();
                                            });
                                        }),
                                ),
                        ),
                );
            }

            for pending in pending_native_fields(&draft) {
                body = body.child(
                    div()
                        .px(px(metrics::PAD_2))
                        .py(px(1.0))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(palette.warn_bg)
                        .text_size(px(metrics::TEXT_XS))
                        .text_color(palette.warn_fg)
                        .child(format!("待接入：{pending}")),
                );
            }

            let mut buttons = div().flex().gap(px(metrics::GAP_2));
            {
                let bridge = self.bridge.clone();
                let entity = cx.entity().downgrade();
                buttons = buttons.child(
                    div()
                        .id("settings-save")
                        .px(px(metrics::PAD_3))
                        .py(px(metrics::GAP_1))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(palette.success_strong)
                        .text_color(palette.accent_fg)
                        .text_size(px(metrics::TEXT_SM))
                        .cursor_pointer()
                        .child("保存")
                        .on_click(move |_, _, cx| {
                            let _ = entity.update(cx, |this, cx| {
                                if let Some(panel) = this.settings.as_ref() {
                                    bridge.save_settings(panel.patch());
                                }
                                cx.notify();
                            });
                        }),
                );
            }
            {
                let entity = cx.entity().downgrade();
                buttons = buttons.child(
                    div()
                        .id("settings-close")
                        .px(px(metrics::PAD_3))
                        .py(px(metrics::GAP_1))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(palette.bg_elevated)
                        .text_size(px(metrics::TEXT_SM))
                        .cursor_pointer()
                        .child("关闭")
                        .on_click(move |_, _, cx| {
                            let _ = entity.update(cx, |this, cx| {
                                this.settings = None;
                                cx.notify();
                            });
                        }),
                );
            }
            body = body.child(buttons);

            // Danger zone: diagnostics and the two operations that touch the database itself.
            let notice = self.take_db_notice();
            let armed = self.db_reset_armed;
            let bridge = self.bridge.clone();
            let entity = cx.entity().downgrade();
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(metrics::GAP_2))
                    .mt(px(metrics::GAP_3))
                    .pt(px(metrics::PAD_3))
                    .border_t_1()
                    .border_color(palette.border)
                    .child(
                        div()
                            .text_size(px(metrics::TEXT_XS))
                            .text_color(palette.fg_muted)
                            .child("诊断与危险区"),
                    )
                    .child({
                        let check_bridge = bridge.clone();
                        let (summary, has_update) = update_status_now.clone().unwrap_or_default();
                        let mut row = div().flex().items_center().gap(px(metrics::GAP_2));
                        row = row.child(
                            div()
                                .id("settings-check-update")
                                .px(px(metrics::PAD_2))
                                .py(px(metrics::GAP_1))
                                .rounded(px(metrics::RADIUS_MD))
                                .bg(palette.bg_elevated)
                                .text_size(px(metrics::TEXT_SM))
                                .cursor_pointer()
                                .child("检查更新")
                                .on_click(move |_, _, _| check_bridge.check_for_updates()),
                        );
                        if has_update {
                            row = row.child(
                                div()
                                    .id("settings-download-update")
                                    .px(px(metrics::PAD_2))
                                    .py(px(metrics::GAP_1))
                                    .rounded(px(metrics::RADIUS_MD))
                                    .bg(palette.accent)
                                    .text_color(palette.accent_fg)
                                    .text_size(px(metrics::TEXT_SM))
                                    .cursor_pointer()
                                    .child("打开下载页")
                                    .on_click(move |_, _, _| {
                                        let _ = neohub_core::update::open_releases_page();
                                    }),
                            );
                        }
                        row.child(
                            div()
                                .text_size(px(metrics::TEXT_XS))
                                .text_color(palette.fg_subtle)
                                .child(summary),
                        )
                    })
                    .child({
                        let log_bridge = bridge.clone();
                        div()
                            .id("settings-open-logs")
                            .px(px(metrics::PAD_2))
                            .py(px(metrics::GAP_1))
                            .rounded(px(metrics::RADIUS_MD))
                            .bg(palette.bg_elevated)
                            .text_size(px(metrics::TEXT_SM))
                            .cursor_pointer()
                            .child("打开日志目录")
                            .on_click(move |_, _, _| {
                                log_bridge.open_log_dir();
                            })
                    })
                    .child({
                        let export_bridge = bridge.clone();
                        let target = bridge.export_target();
                        let _ = &target;
                        div()
                            .id("settings-export-db")
                            .px(px(metrics::PAD_2))
                            .py(px(metrics::GAP_1))
                            .rounded(px(metrics::RADIUS_MD))
                            .bg(palette.bg_elevated)
                            .text_size(px(metrics::TEXT_SM))
                            .cursor_pointer()
                            .child("导出数据库…")
                            .on_click(move |_, _, _| {
                                export_bridge.export_database(export_bridge.export_target());
                            })
                    })
                    .child({
                        let reset_bridge = bridge.clone();
                        let entity = entity.clone();
                        div()
                            .id("settings-reset-db")
                            .px(px(metrics::PAD_2))
                            .py(px(metrics::GAP_1))
                            .rounded(px(metrics::RADIUS_MD))
                            .bg(if armed {
                                palette.danger_bg
                            } else {
                                palette.bg_elevated
                            })
                            .text_color(if armed { palette.danger_fg } else { palette.fg })
                            .text_size(px(metrics::TEXT_SM))
                            .cursor_pointer()
                            .child(if armed {
                                "再点一次确认重置（旧库会改名保存）"
                            } else {
                                "重置数据库…"
                            })
                            .on_click(move |_, _, cx| {
                                let _ = entity.update(cx, |this, cx| {
                                    if this.db_reset_armed {
                                        this.db_reset_armed = false;
                                        reset_bridge.reset_database();
                                    } else {
                                        this.db_reset_armed = true;
                                    }
                                    cx.notify();
                                });
                            })
                    })
                    .child(
                        div()
                            .text_size(px(metrics::TEXT_XS))
                            .text_color(palette.fg_subtle)
                            .child(notice.unwrap_or_default()),
                    ),
            );

            main = main.child(body);
        } else if import.open {
            let mut panel = div()
                .flex()
                .flex_col()
                .gap(px(metrics::GAP_2))
                .p(px(metrics::PAD_4))
                .rounded(px(metrics::RADIUS_LG))
                .border_1()
                .border_color(palette.border)
                .bg(palette.bg_surface)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(div().text_size(px(metrics::TEXT_BASE)).child("扫描导入"))
                        .child({
                            let bridge = self.bridge.clone();
                            let entity = cx.entity().downgrade();
                            div()
                                .id("import-close")
                                .px(px(metrics::PAD_2))
                                .rounded(px(metrics::RADIUS_MD))
                                .bg(palette.bg_elevated)
                                .text_size(px(metrics::TEXT_XS))
                                .cursor_pointer()
                                .child("关闭")
                                .on_click(move |_, _, cx| {
                                    bridge.store.lock().expect("store lock").close_import();
                                    let _ = entity.update(cx, |_, cx| cx.notify());
                                })
                        }),
                )
                .child(
                    div()
                        .text_size(px(metrics::TEXT_XS))
                        .text_color(palette.fg_subtle)
                        .child(format!("目录：{}", import.dir)),
                );

            if let Some(error) = import.error.clone() {
                panel = panel.child(
                    div()
                        .px(px(metrics::PAD_3))
                        .py(px(metrics::GAP_1))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(palette.danger_bg)
                        .text_size(px(metrics::TEXT_XS))
                        .text_color(palette.danger_fg)
                        .child(error),
                );
            }
            if import.busy {
                panel = panel.child(
                    div()
                        .text_size(px(metrics::TEXT_SM))
                        .text_color(palette.fg_muted)
                        .child("正在扫描…"),
                );
            } else if import.candidates.is_empty() {
                panel = panel.child(
                    div()
                        .text_size(px(metrics::TEXT_SM))
                        .text_color(palette.fg_muted)
                        .child("这个目录下没有找到含 package.json 的项目。"),
                );
            }

            // Batch controls: tick several and import them in one go.
            if !import.candidates.is_empty() {
                let entity = cx.entity().downgrade();
                let selected_count = import.selected.len();
                let importable = import
                    .candidates
                    .iter()
                    .filter(|c| !c.already_registered)
                    .count();
                let all_entity = cx.entity().downgrade();
                let batch_bridge = self.bridge.clone();
                let mut footer = div().flex().items_center().gap(px(metrics::GAP_2));
                footer = footer.child(
                    div()
                        .id("import-select-all")
                        .px(px(metrics::PAD_2))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(palette.bg_elevated)
                        .text_size(px(metrics::TEXT_XS))
                        .cursor_pointer()
                        .child(format!("全选（{importable}）"))
                        .on_click(move |_, _, cx| {
                            let _ = all_entity.update(cx, |this, cx| {
                                if let Ok(mut store) = this.bridge.store.lock() {
                                    store.toggle_select_all_imports();
                                }
                                cx.notify();
                            });
                        }),
                );
                footer = footer.child(
                    div()
                        .id("import-batch")
                        .px(px(metrics::PAD_3))
                        .py(px(metrics::GAP_1))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(if selected_count == 0 {
                            palette.bg_surface
                        } else {
                            palette.accent
                        })
                        .text_color(if selected_count == 0 {
                            palette.fg_subtle
                        } else {
                            palette.accent_fg
                        })
                        .text_size(px(metrics::TEXT_XS))
                        .cursor_pointer()
                        .child(format!("导入选中（{selected_count}）"))
                        .on_click(move |_, _, cx| {
                            if selected_count == 0 {
                                return;
                            }
                            batch_bridge.import_selected();
                            let _ = entity.update(cx, |_, cx| cx.notify());
                        }),
                );
                panel = panel.child(footer);
            }

            for candidate in &import.candidates {
                let bridge = self.bridge.clone();
                let entity = cx.entity().downgrade();
                let candidate = candidate.clone();
                let registered = candidate.already_registered;
                let checked = import.selected.contains(&candidate.path);
                let tick_path = candidate.path.clone();
                let tick_entity = cx.entity().downgrade();
                panel = panel.child(
                    div()
                        .id(format!("candidate-{}", candidate.path))
                        .flex()
                        .items_center()
                        .justify_between()
                        .px(px(metrics::PAD_3))
                        .py(px(metrics::GAP_1))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(palette.bg_elevated)
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(metrics::TEXT_SM))
                                        .child(candidate.name.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(metrics::TEXT_XS))
                                        .text_color(palette.fg_subtle)
                                        .child(candidate.path.clone()),
                                ),
                        )
                        .child(
                            div()
                                .id(format!("candidate-tick-{}", candidate.path))
                                .px(px(metrics::PAD_2))
                                .rounded(px(metrics::RADIUS_MD))
                                .bg(palette.bg_surface)
                                .text_size(px(metrics::TEXT_XS))
                                .text_color(if checked {
                                    palette.accent
                                } else {
                                    palette.fg_subtle
                                })
                                .cursor_pointer()
                                .child(if registered {
                                    "—"
                                } else if checked {
                                    "☑"
                                } else {
                                    "☐"
                                })
                                .on_click(move |_, _, cx| {
                                    let path = tick_path.clone();
                                    let _ = tick_entity.update(cx, |this, cx| {
                                        if let Ok(mut store) = this.bridge.store.lock() {
                                            store.toggle_import_candidate(&path);
                                        }
                                        cx.notify();
                                    });
                                }),
                        )
                        .child(
                            div()
                                .id(format!("candidate-add-{}", candidate.path))
                                .px(px(metrics::PAD_2))
                                .rounded(px(metrics::RADIUS_MD))
                                .bg(if registered {
                                    palette.bg_surface
                                } else {
                                    palette.accent
                                })
                                .text_size(px(metrics::TEXT_XS))
                                .text_color(if registered {
                                    palette.fg_subtle
                                } else {
                                    palette.accent_fg
                                })
                                .cursor_pointer()
                                .child(if registered { "已导入" } else { "添加" })
                                .on_click(move |_, _, cx| {
                                    if !registered {
                                        bridge
                                            .store
                                            .lock()
                                            .expect("store lock")
                                            .set_import_busy(true);
                                        bridge.import_candidate(candidate.clone());
                                    }
                                    let _ = entity.update(cx, |_, cx| cx.notify());
                                }),
                        ),
                );
            }
            main = main.child(panel);
        } else if view == crate::state::session::View::App {
            let (app, tasks, current, history, refresh) = {
                let store = self.bridge.store.lock().expect("store lock");
                let app_id = store.session().selected_app_id.clone();
                (
                    store.selected_app().cloned(),
                    store.tasks_of_selected_app(),
                    store.selected_task(),
                    app_id
                        .as_ref()
                        .map(|id| store.history_of(id).to_vec())
                        .unwrap_or_default(),
                    app_id,
                )
            };

            if let Some(app) = app.clone() {
                let mut header = div().flex().items_center().justify_between().child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(metrics::GAP_1))
                        .child(
                            div()
                                .text_size(px(metrics::TEXT_LG))
                                .child(app.name.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(metrics::TEXT_XS))
                                .text_color(palette.fg_subtle)
                                .child(app.path.clone()),
                        ),
                );

                let running = self
                    .bridge
                    .store
                    .lock()
                    .expect("store lock")
                    .app_state(&app.id)
                    .is_live();
                let mut actions = div().flex().gap(px(metrics::GAP_2));
                {
                    let bridge = self.bridge.clone();
                    let app_id = app.id.clone();
                    let entity = cx.entity().downgrade();
                    actions = actions.child(
                        div()
                            .id("detail-start-stop")
                            .px(px(metrics::PAD_3))
                            .py(px(metrics::GAP_1))
                            .rounded(px(metrics::RADIUS_MD))
                            .bg(if running {
                                palette.danger_bg
                            } else {
                                palette.success_strong
                            })
                            .text_color(if running {
                                palette.danger_fg
                            } else {
                                palette.accent_fg
                            })
                            .text_size(px(metrics::TEXT_SM))
                            .cursor_pointer()
                            .child(if running {
                                "停止应用"
                            } else {
                                "启动应用"
                            })
                            .on_click(move |_, _, cx| {
                                if running {
                                    bridge.stop_app(app_id.clone());
                                } else {
                                    bridge.start_app(app_id.clone());
                                }
                                let _ = entity.update(cx, |_, cx| cx.notify());
                            }),
                    );
                }
                {
                    let bridge = self.bridge.clone();
                    let app_id = app.id.clone();
                    let entity = cx.entity().downgrade();
                    actions = actions.child({
                        let entity = cx.entity().downgrade();
                        let current = current.clone();
                        let focus = self.editor_focus.clone();
                        div()
                            .id("detail-edit-task")
                            .px(px(metrics::PAD_3))
                            .py(px(metrics::GAP_1))
                            .rounded(px(metrics::RADIUS_MD))
                            .bg(palette.bg_elevated)
                            .text_size(px(metrics::TEXT_SM))
                            .cursor_pointer()
                            .child("编辑任务")
                            .on_click(move |_, window, cx| {
                                if let Some(task) = current.clone() {
                                    let _ = entity.update(cx, |this, cx| {
                                        this.editor = Some(TaskForm::new(&task));
                                        window.focus(&focus, cx);
                                        cx.notify();
                                    });
                                }
                            })
                    });
                    actions = actions.child({
                        let bridge = self.bridge.clone();
                        let entity = cx.entity().downgrade();
                        let app_id = app.id.clone();
                        div()
                            .id("detail-env")
                            .px(px(metrics::PAD_3))
                            .py(px(metrics::GAP_1))
                            .rounded(px(metrics::RADIUS_MD))
                            .bg(palette.bg_elevated)
                            .text_size(px(metrics::TEXT_SM))
                            .cursor_pointer()
                            .child("环境变量")
                            .on_click(move |_, _, cx| {
                                let scope = neohub_core::service::EnvScope::Global;
                                bridge.load_env_vars(scope.clone());
                                let _ = &app_id;
                                let _ = entity.update(cx, |this, cx| {
                                    this.env_tab = ScopeTab::Global;
                                    this.env = Some(EnvEditor::new(scope, Vec::new()));
                                    cx.notify();
                                });
                            })
                    });
                    for target in neohub_platform_macos::open_in::available_targets() {
                        let app_path = std::path::PathBuf::from(&app.path);
                        actions = actions.child(
                            div()
                                .id(format!("detail-open-in-{:?}", target))
                                .px(px(metrics::PAD_3))
                                .py(px(metrics::GAP_1))
                                .rounded(px(metrics::RADIUS_MD))
                                .bg(palette.bg_elevated)
                                .text_size(px(metrics::TEXT_SM))
                                .cursor_pointer()
                                .child(target.label().to_string())
                                .on_click(move |_, _, _| {
                                    // Detached by design: the target app outlives this click.
                                    if let Err(error) =
                                        neohub_platform_macos::open_in::launch(target, &app_path)
                                    {
                                        eprintln!("[ui] open in {:?} failed: {error}", target);
                                    }
                                }),
                        );
                    }
                    actions = actions.child({
                        let entity = cx.entity().downgrade();
                        let focus = self.app_form_focus.clone();
                        let app = app.clone();
                        div()
                            .id("detail-app-config")
                            .px(px(metrics::PAD_3))
                            .py(px(metrics::GAP_1))
                            .rounded(px(metrics::RADIUS_MD))
                            .bg(palette.bg_elevated)
                            .text_size(px(metrics::TEXT_SM))
                            .cursor_pointer()
                            .child("应用配置")
                            .on_click(move |_, window, cx| {
                                let form = crate::state::app_form::AppForm::from_app(&app);
                                let _ = entity.update(cx, |this, cx| {
                                    this.app_form = Some(form);
                                    window.focus(&focus, cx);
                                    cx.notify();
                                });
                            })
                    });
                    actions = actions.child(
                        div()
                            .id("detail-restart")
                            .px(px(metrics::PAD_3))
                            .py(px(metrics::GAP_1))
                            .rounded(px(metrics::RADIUS_MD))
                            .bg(palette.bg_elevated)
                            .text_size(px(metrics::TEXT_SM))
                            .cursor_pointer()
                            .child("重启应用")
                            .on_click(move |_, _, cx| {
                                bridge.send(CoreCommand::RestartApp {
                                    app_id: app_id.clone(),
                                });
                                let _ = entity.update(cx, |_, cx| cx.notify());
                            }),
                    );
                }
                header = header.child(actions);
                main = main.child(header);

                // Task tabs.
                let mut tabs = div().flex().gap(px(metrics::GAP_1));
                for task in &tasks {
                    let _bridge = self.bridge.clone();
                    let app_id = app.id.clone();
                    let task_id = task.id.clone();
                    let is_current = current.as_ref().map(|t| t.id == task.id).unwrap_or(false);
                    let state = self
                        .bridge
                        .store
                        .lock()
                        .expect("store lock")
                        .app_state(&app.id);
                    let _ = state;
                    let label = task.name.clone();
                    tabs = tabs.child(
                        div()
                            .id(format!("tab-{}", task.id))
                            .px(px(metrics::PAD_3))
                            .py(px(metrics::GAP_1))
                            .rounded(px(metrics::RADIUS_MD))
                            .bg(if is_current {
                                palette.bg_elevated
                            } else {
                                palette.bg_surface
                            })
                            .text_size(px(metrics::TEXT_SM))
                            .text_color(if task.enabled {
                                palette.fg
                            } else {
                                palette.fg_subtle
                            })
                            .cursor_pointer()
                            .child(label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.bridge.with_session(|session| {
                                    session.select_app(app_id.clone(), Some(task_id.clone()))
                                });
                                this.history_requested = None;
                                cx.notify();
                            })),
                    );
                }
                main = main.child(tabs);

                // Current task row + per-task controls.
                if let Some(task) = current.clone() {
                    let running = self
                        .bridge
                        .store
                        .lock()
                        .expect("store lock")
                        .snapshot()
                        .map(|snapshot| {
                            snapshot
                                .running_tasks
                                .iter()
                                .any(|info| info.task_id == task.id && info.state.is_live())
                        })
                        .unwrap_or(false);
                    let command = task
                        .custom_command
                        .clone()
                        .or_else(|| task.script.clone())
                        .unwrap_or_else(|| "(未配置命令)".to_string());
                    let mut row = div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .px(px(metrics::PAD_3))
                        .py(px(metrics::GAP_1))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(palette.bg_surface)
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(metrics::TEXT_SM))
                                        .child(format!("{} · {}", task.name, command)),
                                )
                                .child(
                                    div()
                                        .text_size(px(metrics::TEXT_XS))
                                        .text_color(palette.fg_subtle)
                                        .child(format!(
                                            "就绪：{} · {}",
                                            task.readiness.kind(),
                                            if task.enabled {
                                                "已启用"
                                            } else {
                                                "已停用"
                                            }
                                        )),
                                ),
                        );
                    {
                        let bridge = self.bridge.clone();
                        let task_id = task.id.clone();
                        let entity = cx.entity().downgrade();
                        row = row.child(
                            div()
                                .id(format!("task-toggle-{}", task.id))
                                .px(px(metrics::PAD_3))
                                .py(px(metrics::GAP_1))
                                .rounded(px(metrics::RADIUS_MD))
                                .bg(if running {
                                    palette.danger_bg
                                } else {
                                    palette.bg_elevated
                                })
                                .text_size(px(metrics::TEXT_SM))
                                .cursor_pointer()
                                .child(if running {
                                    "停止任务"
                                } else {
                                    "启动任务"
                                })
                                .on_click(move |_, _, cx| {
                                    if running {
                                        bridge.stop_task(task_id.clone());
                                    } else {
                                        bridge.start_task(task_id.clone());
                                    }
                                    let _ = entity.update(cx, |_, cx| cx.notify());
                                }),
                        );
                    }
                    main = main.child(row);

                    // Environment editor panel.
                    if let Some(editor) = self.env.as_mut() {
                        let tab = self.env_tab;
                        let rows = editor.rows().to_vec();
                        let mut panel =
                            div()
                                .id("env-editor")
                                .flex()
                                .flex_col()
                                .gap(px(metrics::GAP_1))
                                .p(px(metrics::PAD_3))
                                .rounded(px(metrics::RADIUS_LG))
                                .border_1()
                                .border_color(palette.border_strong)
                                .bg(palette.bg_surface)
                                .child(div().flex().items_center().justify_between().child(
                                    div().text_size(px(metrics::TEXT_SM)).child(
                                        if editor.is_dirty() {
                                            "环境变量（未保存）"
                                        } else {
                                            "环境变量"
                                        },
                                    ),
                                ));

                        // Scope tabs.
                        let mut tabs = div().flex().gap(px(metrics::GAP_1));
                        for scope_tab in [ScopeTab::Global, ScopeTab::App, ScopeTab::Task] {
                            let bridge = self.bridge.clone();
                            let entity = cx.entity().downgrade();
                            let app_id = app.id.clone();
                            let task_id = current.as_ref().map(|task| task.id.clone());
                            let active = tab == scope_tab;
                            let scope = scope_for(scope_tab, Some(&app_id), task_id.as_ref());
                            tabs = tabs.child(
                                div()
                                    .id(format!("env-tab-{:?}", scope_tab))
                                    .px(px(metrics::PAD_2))
                                    .py(px(1.0))
                                    .rounded(px(metrics::RADIUS_MD))
                                    .bg(if active {
                                        palette.accent
                                    } else {
                                        palette.bg_elevated
                                    })
                                    .text_color(if active {
                                        palette.accent_fg
                                    } else {
                                        palette.fg_muted
                                    })
                                    .text_size(px(metrics::TEXT_XS))
                                    .cursor_pointer()
                                    .child(scope_tab.label())
                                    .on_click(move |_, _, cx| {
                                        if let Some(scope) = scope.clone() {
                                            bridge.load_env_vars(scope.clone());
                                            let _ = entity.update(cx, |this, cx| {
                                                this.env_tab = scope_tab;
                                                this.env = Some(EnvEditor::new(scope, Vec::new()));
                                                this.env_editing = None;
                                                cx.notify();
                                            });
                                        }
                                    }),
                            );
                        }
                        panel = panel.child(tabs);

                        for (index, row) in rows.iter().enumerate() {
                            let toggle = cx.entity().downgrade();
                            let secret = cx.entity().downgrade();
                            let remove = cx.entity().downgrade();
                            panel = panel.child(
                                div()
                                    .id(format!("env-row-{index}"))
                                    .flex()
                                    .items_center()
                                    .gap(px(metrics::GAP_2))
                                    .px(px(metrics::PAD_2))
                                    .py(px(1.0))
                                    .rounded(px(metrics::RADIUS_MD))
                                    .bg(if row.enabled {
                                        palette.bg_base
                                    } else {
                                        palette.bg_surface
                                    })
                                    .text_size(px(metrics::TEXT_XS))
                                    .child(
                                        div()
                                            .w(px(140.0))
                                            .text_color(palette.fg)
                                            .child(if row.key.is_empty() {
                                                "(新变量)".to_string()
                                            } else {
                                                row.key.clone()
                                            }),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(120.0))
                                            .text_color(if row.enabled {
                                                palette.fg_muted
                                            } else {
                                                palette.fg_subtle
                                            })
                                            .child(row.value.display()),
                                    )
                                    .child(
                                        div()
                                            .id(format!("env-toggle-{index}"))
                                            .px(px(metrics::GAP_1))
                                            .rounded(px(metrics::RADIUS_MD))
                                            .bg(palette.bg_elevated)
                                            .cursor_pointer()
                                            .child(if row.enabled { "启用" } else { "停用" })
                                            .on_click(move |_, _, cx| {
                                                let _ = toggle.update(cx, |this, cx| {
                                                    if let Some(editor) = this.env.as_mut() {
                                                        editor.toggle_enabled(index);
                                                    }
                                                    cx.notify();
                                                });
                                            }),
                                    )
                                    .child(
                                        div()
                                            .id(format!("env-secret-{index}"))
                                            .px(px(metrics::GAP_1))
                                            .rounded(px(metrics::RADIUS_MD))
                                            .bg(palette.bg_elevated)
                                            .cursor_pointer()
                                            .child(if matches!(
                                                row.value,
                                                crate::state::env_editor::RowValue::Masked
                                                    | crate::state::env_editor::RowValue::NewSecret(_)
                                            ) {
                                                "秘密"
                                            } else {
                                                "明文"
                                            })
                                            .on_click(move |_, _, cx| {
                                                let _ = secret.update(cx, |this, cx| {
                                                    if let Some(editor) = this.env.as_mut() {
                                                        editor.toggle_secret(index);
                                                    }
                                                    cx.notify();
                                                });
                                            }),
                                    )
                                    .child(
                                        div()
                                            .id(format!("env-remove-{index}"))
                                            .px(px(metrics::GAP_1))
                                            .rounded(px(metrics::RADIUS_MD))
                                            .bg(palette.danger_bg)
                                            .text_color(palette.danger_fg)
                                            .cursor_pointer()
                                            .child("删除")
                                            .on_click(move |_, _, cx| {
                                                let _ = remove.update(cx, |this, cx| {
                                                    if let Some(editor) = this.env.as_mut() {
                                                        editor.remove_row(index);
                                                    }
                                                    cx.notify();
                                                });
                                            }),
                                    ),
                            );
                        }

                        panel = panel.child(
                            div()
                                .flex()
                                .gap(px(metrics::GAP_2))
                                .text_size(px(metrics::TEXT_XS))
                                .text_color(palette.fg_subtle)
                                .child("粘贴 .env 片段后点击「应用粘贴」；掩码行保存时不回写。"),
                        );

                        let mut paste_row = div().flex().gap(px(metrics::GAP_2)).items_center();
                        {
                            let entity = cx.entity().downgrade();
                            let focus = self.env_focus.clone();
                            paste_row = paste_row.child(
                                div()
                                    .id("env-paste")
                                    .track_focus(&self.env_focus)
                                    .px(px(metrics::PAD_2))
                                    .py(px(1.0))
                                    .rounded(px(metrics::RADIUS_MD))
                                    .border_1()
                                    .border_color(palette.border)
                                    .bg(palette.bg_base)
                                    .text_size(px(metrics::TEXT_XS))
                                    .cursor_pointer()
                                    .child(if self.env_paste.text().is_empty() {
                                        "KEY=VALUE KEY2=VALUE2".to_string()
                                    } else {
                                        self.env_paste.text().to_string()
                                    })
                                    .on_click(move |_, window, cx| {
                                        window.focus(&focus, cx);
                                        let _ = entity.update(cx, |_, cx| cx.notify());
                                    }),
                            );
                        }
                        {
                            let entity = cx.entity().downgrade();
                            paste_row = paste_row.child(
                                div()
                                    .id("env-paste-apply")
                                    .px(px(metrics::PAD_2))
                                    .py(px(1.0))
                                    .rounded(px(metrics::RADIUS_MD))
                                    .bg(palette.bg_elevated)
                                    .text_size(px(metrics::TEXT_XS))
                                    .cursor_pointer()
                                    .child("应用粘贴")
                                    .on_click(move |_, _, cx| {
                                        let _ = entity.update(cx, |this, cx| {
                                            let blob = this.env_paste.text().to_string();
                                            if let Some(editor) = this.env.as_mut() {
                                                editor.paste_dotenv(&blob);
                                            }
                                            this.env_paste.clear();
                                            cx.notify();
                                        });
                                    }),
                            );
                        }
                        panel = panel.child(paste_row);

                        let mut buttons = div().flex().gap(px(metrics::GAP_2));
                        {
                            let bridge = self.bridge.clone();
                            let entity = cx.entity().downgrade();
                            buttons = buttons.child(
                                div()
                                    .id("env-add")
                                    .px(px(metrics::PAD_3))
                                    .py(px(metrics::GAP_1))
                                    .rounded(px(metrics::RADIUS_MD))
                                    .bg(palette.bg_elevated)
                                    .text_size(px(metrics::TEXT_SM))
                                    .cursor_pointer()
                                    .child("新增变量")
                                    .on_click(move |_, _, cx| {
                                        let _ = entity.update(cx, |this, cx| {
                                            if let Some(editor) = this.env.as_mut() {
                                                editor.add_row();
                                            }
                                            cx.notify();
                                        });
                                    }),
                            );
                            let _ = &bridge;
                        }
                        {
                            let bridge = self.bridge.clone();
                            let entity = cx.entity().downgrade();
                            buttons = buttons.child(
                                div()
                                    .id("env-save")
                                    .px(px(metrics::PAD_3))
                                    .py(px(metrics::GAP_1))
                                    .rounded(px(metrics::RADIUS_MD))
                                    .bg(palette.success_strong)
                                    .text_color(palette.accent_fg)
                                    .text_size(px(metrics::TEXT_SM))
                                    .cursor_pointer()
                                    .child("保存")
                                    .on_click(move |_, _, cx| {
                                        let _ = entity.update(cx, |this, cx| {
                                            if let Some(editor) = this.env.as_ref() {
                                                bridge.save_env_vars(
                                                    editor.scope().clone(),
                                                    editor.to_inputs(),
                                                );
                                            }
                                            cx.notify();
                                        });
                                    }),
                            );
                        }
                        {
                            let entity = cx.entity().downgrade();
                            buttons = buttons.child(
                                div()
                                    .id("env-close")
                                    .px(px(metrics::PAD_3))
                                    .py(px(metrics::GAP_1))
                                    .rounded(px(metrics::RADIUS_MD))
                                    .bg(palette.bg_elevated)
                                    .text_size(px(metrics::TEXT_SM))
                                    .cursor_pointer()
                                    .child("关闭")
                                    .on_click(move |_, _, cx| {
                                        let _ = entity.update(cx, |this, cx| {
                                            this.env = None;
                                            cx.notify();
                                        });
                                    }),
                            );
                        }
                        panel = panel.child(buttons);
                        panel =
                            panel.on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                                let characters = event.keystroke.key_char.as_deref();
                                if this.env_paste.apply_key(&event.keystroke.key, characters)
                                    == KeyOutcome::Changed
                                {
                                    cx.notify();
                                }
                            }));
                        main = main.child(panel);
                        let _ = effective;
                    }

                    // Task editor panel.
                    if let Some(editor) = self.editor.as_mut() {
                        let fields = editor.fields().clone();
                        let focused = editor.focused();
                        let error = editor.error().cloned();
                        let dirty = editor.is_dirty();
                        let mut panel = div()
                            .id("task-editor")
                            .track_focus(&self.editor_focus)
                            .flex()
                            .flex_col()
                            .gap(px(metrics::GAP_1))
                            .p(px(metrics::PAD_3))
                            .rounded(px(metrics::RADIUS_LG))
                            .border_1()
                            .border_color(palette.border_strong)
                            .bg(palette.bg_surface)
                            .child(div().text_size(px(metrics::TEXT_SM)).child(if dirty {
                                "编辑任务（未保存）"
                            } else {
                                "编辑任务"
                            }));

                        let rows: Vec<(Field, String)> = vec![
                            (Field::Name, fields.name.clone()),
                            (Field::Command, fields.command.clone()),
                            (Field::WorkingDir, fields.working_dir.clone()),
                            (Field::ReadinessArg, fields.readiness_arg.clone()),
                        ];
                        for (field, value) in rows {
                            let entity = cx.entity().downgrade();
                            let focus = self.editor_focus.clone();
                            panel = panel.child(
                                div()
                                    .id(format!("editor-field-{:?}", field))
                                    .flex()
                                    .items_center()
                                    .gap(px(metrics::GAP_2))
                                    .px(px(metrics::PAD_2))
                                    .py(px(metrics::GAP_1))
                                    .rounded(px(metrics::RADIUS_MD))
                                    .bg(if focused == field {
                                        palette.bg_elevated
                                    } else {
                                        palette.bg_base
                                    })
                                    .cursor_pointer()
                                    .child(
                                        div()
                                            .w(px(76.0))
                                            .text_size(px(metrics::TEXT_XS))
                                            .text_color(palette.fg_subtle)
                                            .child(field.label()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(metrics::TEXT_SM))
                                            .text_color(if value.is_empty() {
                                                palette.fg_subtle
                                            } else {
                                                palette.fg
                                            })
                                            .child(if value.is_empty() {
                                                "（空）".to_string()
                                            } else {
                                                format!(
                                                    "{value}{}",
                                                    if focused == field { "▏" } else { "" }
                                                )
                                            }),
                                    )
                                    .on_click(move |_, window, cx| {
                                        let _ = entity.update(cx, |this, cx| {
                                            if let Some(form) = this.editor.as_mut() {
                                                form.focus(field);
                                            }
                                            cx.notify();
                                        });
                                        window.focus(&focus, cx);
                                    }),
                            );
                        }

                        // Readiness + toggles.
                        let mut toggles = div().flex().gap(px(metrics::GAP_1));
                        for choice in ReadinessChoice::ALL {
                            let entity = cx.entity().downgrade();
                            let active = fields.readiness == choice;
                            toggles = toggles.child(
                                div()
                                    .id(format!("readiness-{:?}", choice))
                                    .px(px(metrics::PAD_2))
                                    .py(px(1.0))
                                    .rounded(px(metrics::RADIUS_MD))
                                    .bg(if active {
                                        palette.accent
                                    } else {
                                        palette.bg_elevated
                                    })
                                    .text_color(if active {
                                        palette.accent_fg
                                    } else {
                                        palette.fg_muted
                                    })
                                    .text_size(px(metrics::TEXT_XS))
                                    .cursor_pointer()
                                    .child(choice.label())
                                    .on_click(move |_, _, cx| {
                                        let _ = entity.update(cx, |this, cx| {
                                            if let Some(form) = this.editor.as_mut() {
                                                form.set_readiness(choice);
                                            }
                                            cx.notify();
                                        });
                                    }),
                            );
                        }
                        panel = panel.child(toggles);
                        panel = panel.child(
                            div()
                                .flex()
                                .gap(px(metrics::GAP_2))
                                .text_size(px(metrics::TEXT_XS))
                                .text_color(palette.fg_muted)
                                .child(format!(
                                    "一次性任务：{} · 启用：{}",
                                    if fields.one_shot { "是" } else { "否" },
                                    if fields.enabled { "是" } else { "否" }
                                )),
                        );
                        if let Some(error) = error {
                            panel = panel.child(
                                div()
                                    .px(px(metrics::PAD_2))
                                    .py(px(1.0))
                                    .rounded(px(metrics::RADIUS_MD))
                                    .bg(palette.danger_bg)
                                    .text_size(px(metrics::TEXT_XS))
                                    .text_color(palette.danger_fg)
                                    .child(format!("{}：{}", error.field.label(), error.message)),
                            );
                        }

                        // Save / cancel.
                        let mut buttons = div().flex().gap(px(metrics::GAP_2));
                        {
                            let bridge = self.bridge.clone();
                            let entity = cx.entity().downgrade();
                            buttons = buttons.child(
                                div()
                                    .id("editor-save")
                                    .px(px(metrics::PAD_3))
                                    .py(px(metrics::GAP_1))
                                    .rounded(px(metrics::RADIUS_MD))
                                    .bg(palette.success_strong)
                                    .text_color(palette.accent_fg)
                                    .text_size(px(metrics::TEXT_SM))
                                    .cursor_pointer()
                                    .child("保存")
                                    .on_click(move |_, _, cx| {
                                        let _ = entity.update(cx, |this, cx| {
                                            let Some(form) = this.editor.as_mut() else {
                                                return;
                                            };
                                            match form.to_update_input() {
                                                Ok(input) => {
                                                    let task_id =
                                                        neohub_domain::TaskId::from_existing(
                                                            form.task_id(),
                                                        );
                                                    bridge.save_task(task_id, input);
                                                }
                                                Err(error) => {
                                                    form.apply_core_error(&error.message);
                                                }
                                            }
                                            cx.notify();
                                        });
                                    }),
                            );
                        }
                        {
                            let entity = cx.entity().downgrade();
                            buttons = buttons.child(
                                div()
                                    .id("editor-cancel")
                                    .px(px(metrics::PAD_3))
                                    .py(px(metrics::GAP_1))
                                    .rounded(px(metrics::RADIUS_MD))
                                    .bg(palette.bg_elevated)
                                    .text_size(px(metrics::TEXT_SM))
                                    .cursor_pointer()
                                    .child("取消")
                                    .on_click(move |_, _, cx| {
                                        let _ = entity.update(cx, |this, cx| {
                                            this.editor = None;
                                            cx.notify();
                                        });
                                    }),
                            );
                        }
                        panel = panel.child(buttons);
                        panel =
                            panel.on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                                let Some(form) = this.editor.as_mut() else {
                                    return;
                                };
                                if event.keystroke.key == "tab" {
                                    form.focus_next();
                                    cx.notify();
                                    return;
                                }
                                let characters = event.keystroke.key_char.as_deref();
                                match form.apply_key(&event.keystroke.key, characters) {
                                    KeyOutcome::Changed => cx.notify(),
                                    KeyOutcome::Escape => {
                                        this.editor = None;
                                        cx.notify();
                                    }
                                    _ => {}
                                }
                            }));
                        main = main.child(panel);
                    }

                    // Terminal pane: parsed rows from the core, with follow/scroll/export.
                    {
                        let source = {
                            let store = self.bridge.store.lock().expect("store lock");
                            store.terminal_text(&task.id).map(str::to_string)
                        };
                        if self.terminal_task.as_ref() != Some(&task.id) || self.terminal.is_none()
                        {
                            self.terminal_task = Some(task.id.clone());
                            self.terminal = Some(match &source {
                                Some(text) => crate::terminal::TerminalView::from_text(text),
                                None => crate::terminal::TerminalView::from_text(""),
                            });
                            self.bridge.load_terminal(task.id.clone());
                        } else if let Some(text) = &source {
                            let rebuilt = crate::terminal::TerminalView::from_text(text);
                            let stale = self
                                .terminal
                                .as_ref()
                                .map(|view| view.total_rows() != rebuilt.total_rows())
                                .unwrap_or(true);
                            if stale {
                                self.terminal = Some(rebuilt);
                            }
                        }

                        let mut tools = div().flex().gap(px(metrics::GAP_1)).items_center();
                        {
                            let entity = cx.entity().downgrade();
                            tools = tools.child(
                                div()
                                    .id("term-follow")
                                    .px(px(metrics::PAD_2))
                                    .py(px(1.0))
                                    .rounded(px(metrics::RADIUS_MD))
                                    .bg(if self.terminal_follow {
                                        palette.accent
                                    } else {
                                        palette.bg_elevated
                                    })
                                    .text_color(if self.terminal_follow {
                                        palette.accent_fg
                                    } else {
                                        palette.fg_muted
                                    })
                                    .text_size(px(metrics::TEXT_XS))
                                    .cursor_pointer()
                                    .child("跟随")
                                    .on_click(move |_, _, cx| {
                                        let _ = entity.update(cx, |this, cx| {
                                            this.terminal_follow = true;
                                            if let Some(view) = this.terminal.as_mut() {
                                                view.follow_tail();
                                            }
                                            cx.notify();
                                        });
                                    }),
                            );
                        }
                        {
                            let entity = cx.entity().downgrade();
                            tools = tools.child(
                                div()
                                    .id("term-up")
                                    .px(px(metrics::PAD_2))
                                    .py(px(1.0))
                                    .rounded(px(metrics::RADIUS_MD))
                                    .bg(palette.bg_elevated)
                                    .text_size(px(metrics::TEXT_XS))
                                    .cursor_pointer()
                                    .child("↑")
                                    .on_click(move |_, _, cx| {
                                        let _ = entity.update(cx, |this, cx| {
                                            let height = this.terminal_height;
                                            if let Some(view) = this.terminal.as_mut() {
                                                view.scroll_up(5, height);
                                            }
                                            this.terminal_follow = false;
                                            cx.notify();
                                        });
                                    }),
                            );
                        }
                        {
                            let entity = cx.entity().downgrade();
                            tools = tools.child(
                                div()
                                    .id("term-down")
                                    .px(px(metrics::PAD_2))
                                    .py(px(1.0))
                                    .rounded(px(metrics::RADIUS_MD))
                                    .bg(palette.bg_elevated)
                                    .text_size(px(metrics::TEXT_XS))
                                    .cursor_pointer()
                                    .child("↓")
                                    .on_click(move |_, _, cx| {
                                        let _ = entity.update(cx, |this, cx| {
                                            let height = this.terminal_height;
                                            let follow = this
                                                .terminal
                                                .as_mut()
                                                .map(|view| view.scroll_down(5, height).1)
                                                .unwrap_or(true);
                                            this.terminal_follow = follow;
                                            cx.notify();
                                        });
                                    }),
                            );
                        }
                        {
                            let bridge = self.bridge.clone();
                            let entity = cx.entity().downgrade();
                            let task_id = task.id.clone();
                            tools = tools.child(
                                div()
                                    .id("term-export")
                                    .px(px(metrics::PAD_2))
                                    .py(px(1.0))
                                    .rounded(px(metrics::RADIUS_MD))
                                    .bg(palette.bg_elevated)
                                    .text_size(px(metrics::TEXT_XS))
                                    .cursor_pointer()
                                    .child("导出可读")
                                    .on_click(move |_, _, cx| {
                                        bridge.export_logs(
                                            task_id.clone(),
                                            bridge.logs_dir(),
                                            false,
                                        );
                                        let _ = entity.update(cx, |_, cx| cx.notify());
                                    }),
                            );
                        }
                        {
                            let bridge = self.bridge.clone();
                            let entity = cx.entity().downgrade();
                            let task_id = task.id.clone();
                            tools = tools.child(
                                div()
                                    .id("term-export-raw")
                                    .px(px(metrics::PAD_2))
                                    .py(px(1.0))
                                    .rounded(px(metrics::RADIUS_MD))
                                    .bg(palette.bg_elevated)
                                    .text_size(px(metrics::TEXT_XS))
                                    .cursor_pointer()
                                    .child("导出原始")
                                    .on_click(move |_, _, cx| {
                                        bridge.export_logs(
                                            task_id.clone(),
                                            bridge.logs_dir(),
                                            true,
                                        );
                                        let _ = entity.update(cx, |_, cx| cx.notify());
                                    }),
                            );
                        }
                        main = main.child(tools);

                        let height = self.terminal_height;
                        let follow = self.terminal_follow;
                        let rows = self
                            .terminal
                            .as_ref()
                            .map(|view| view.visible(height, follow))
                            .unwrap_or_default();
                        let mut pane = div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .p(px(metrics::PAD_2))
                            .rounded(px(metrics::RADIUS_MD))
                            .bg(palette.bg_surface)
                            .text_size(px(metrics::TEXT_XS))
                            .text_color(palette.fg_muted);
                        if rows.is_empty() {
                            pane = pane.child("（还没有日志输出）".to_string());
                        }
                        for row in rows {
                            let mut line = div().flex().flex_row().gap(px(0.0));
                            if row.spans.is_empty() {
                                line = line.child(row.text);
                            } else {
                                for span in row.spans {
                                    let colour = crate::terminal::ansi_to_rgba(
                                        span.style.fg,
                                        palette.fg_muted,
                                    );
                                    line = line.child(div().text_color(colour).child(span.text));
                                }
                            }
                            pane = pane.child(line);
                        }
                        main = main.child(pane);
                    }
                }

                // Run history.
                if !history.is_empty() {
                    let mut rows = div().flex().flex_col().gap(px(1.0));
                    rows = rows.child(
                        div()
                            .text_size(px(metrics::TEXT_XS))
                            .text_color(palette.fg_subtle)
                            .child("最近运行"),
                    );
                    for row in history.iter().take(5) {
                        let code = row
                            .exit_code
                            .map(|code| format!("退出码 {code}"))
                            .unwrap_or_else(|| "运行中/无退出码".to_string());
                        let rerun = row.task_id.clone();
                        let rerun_bridge = self.bridge.clone();
                        let rerun_entity = cx.entity().downgrade();
                        let mut line = div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(metrics::GAP_2));
                        line = line.child(
                            div()
                                .text_size(px(metrics::TEXT_XS))
                                .text_color(if row.was_killed_by_user {
                                    palette.fg_muted
                                } else if row.exit_code == Some(0) {
                                    palette.success_fg
                                } else {
                                    palette.danger_fg
                                })
                                .child(format!(
                                    "{} · {} · {}{}",
                                    row.task_name.clone().unwrap_or_else(|| "(任务)".into()),
                                    code,
                                    if row.was_killed_by_user {
                                        "用户停止"
                                    } else {
                                        "自行退出"
                                    },
                                    row.ended_at.map(|_| "").unwrap_or(" · 未结束")
                                )),
                        );
                        // Rerun uses the recorded task id: the run happened with this task's
                        // configuration, so replaying it through the same task is the honest
                        // thing. A deleted task is reported, not silently recreated.
                        line = line.child(
                            div()
                                .id(format!("rerun-{}", row.id))
                                .px(px(metrics::PAD_2))
                                .rounded(px(metrics::RADIUS_MD))
                                .bg(palette.bg_elevated)
                                .text_size(px(metrics::TEXT_XS))
                                .text_color(palette.fg_muted)
                                .cursor_pointer()
                                .child("重跑")
                                .on_click(move |_, _, cx| {
                                    let bridge = rerun_bridge.clone();
                                    let task_id = rerun.clone();
                                    let _ = rerun_entity.update(cx, |_this, cx| {
                                        match task_id {
                                            Some(task_id) => {
                                                bridge.send(CoreCommand::StartTask {
                                                    task_id: task_id.clone(),
                                                });
                                                if let Ok(mut store) = bridge.store.lock() {
                                                    store.notify(
                                                        crate::state::toast::ToastKind::Info,
                                                        format!("已重新启动 {}", task_id.as_str()),
                                                    );
                                                }
                                            }
                                            None => {
                                                if let Ok(mut store) = bridge.store.lock() {
                                                    store.notify(
                                                        crate::state::toast::ToastKind::Error,
                                                        "这条历史没有关联任务，无法重跑"
                                                            .to_string(),
                                                    );
                                                }
                                            }
                                        }
                                        cx.notify();
                                    });
                                }),
                        );
                        rows = rows.child(line);
                    }
                    main = main.child(rows);
                }
                let _ = refresh;
            }
        } else if is_empty {
            let bridge = self.bridge.clone();
            let entity = cx.entity().downgrade();
            main = main.child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .flex_1()
                    .gap(px(metrics::GAP_3))
                    .child(
                        div()
                            .text_size(px(metrics::TEXT_LG))
                            .child("欢迎使用 NeoHub"),
                    )
                    .child(
                        div()
                            .text_size(px(metrics::TEXT_SM))
                            .text_color(palette.fg_muted)
                            .child("还没有项目。扫描一个目录，把本地 Node 项目导进来。"),
                    )
                    .child(
                        div()
                            .id("empty-import")
                            .px(px(metrics::PAD_4))
                            .py(px(metrics::GAP_2))
                            .rounded(px(metrics::RADIUS_MD))
                            .bg(palette.accent)
                            .text_color(palette.accent_fg)
                            .text_size(px(metrics::TEXT_SM))
                            .cursor_pointer()
                            .child("扫描文件夹导入")
                            .on_click(move |_, _, cx| {
                                bridge.begin_import(HostBridge::default_import_dir());
                                let _ = entity.update(cx, |_, cx| cx.notify());
                            }),
                    ),
            );
            main = main.child(grid_placeholder(&palette));
        } else {
            let mut grid = div().flex().flex_wrap().gap(px(metrics::GAP_3));
            for card in cards {
                let bridge = self.bridge.clone();
                let app_id = card.app_id.clone();
                let running = card.state.is_live();
                let mut card_view = div()
                    .id(format!("card-{}", card.app_id))
                    .flex()
                    .flex_col()
                    .gap(px(metrics::GAP_2))
                    .w(px(280.0))
                    .p(px(metrics::PAD_4))
                    .rounded(px(metrics::RADIUS_LG))
                    .border_1()
                    .border_color(if running {
                        palette.success_border
                    } else {
                        palette.border
                    })
                    .bg(palette.bg_surface)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(metrics::GAP_2))
                                    .child(
                                        div()
                                            .w(px(8.0))
                                            .h(px(8.0))
                                            .rounded_full()
                                            .bg(state_color(&palette, card.state)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(metrics::TEXT_BASE))
                                            .child(card.name.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(metrics::TEXT_XS))
                                    .text_color(state_color(&palette, card.state))
                                    .child(state_label(card.state)),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(metrics::TEXT_XS))
                            .text_color(palette.fg_subtle)
                            .child(format!(
                                "{} 个任务 · {} 运行中",
                                card.total_tasks, card.running_tasks
                            )),
                    )
                    .child(
                        div()
                            .text_size(px(metrics::TEXT_XS))
                            .text_color(palette.fg_muted)
                            .child(format!(
                                "CPU {:.1}%   {:.0} MB",
                                card.cpu_percent, card.mem_mb
                            )),
                    );

                // Trend, not just the current number: a card that only shows an instant cannot
                // tell "busy once" from "busy for a minute".
                if !card.cpu_sparkline.is_empty() {
                    card_view = card_view.child(
                        div()
                            .text_size(px(metrics::TEXT_SM))
                            .text_color(palette.accent)
                            .child(card.cpu_sparkline.clone()),
                    );
                }

                if !card.ports.is_empty() {
                    let mut ports = div().flex().gap(px(metrics::GAP_1));
                    for port in &card.ports {
                        ports = ports.child(
                            div()
                                .px(px(metrics::PAD_2))
                                .py(px(1.0))
                                .rounded(px(metrics::RADIUS_MD))
                                .bg(palette.bg_elevated)
                                .text_size(px(metrics::TEXT_XS))
                                .text_color(palette.accent)
                                .child(format!(":{port}")),
                        );
                    }
                    card_view = card_view.child(ports);
                }

                let toggle_bridge = bridge.clone();
                let entity = cx.entity().downgrade();
                card_view = card_view.child(
                    div()
                        .id(format!("toggle-{}", card.app_id))
                        .px(px(metrics::PAD_3))
                        .py(px(metrics::GAP_1))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(if running {
                            palette.danger_bg
                        } else {
                            palette.bg_elevated
                        })
                        .text_size(px(metrics::TEXT_SM))
                        .text_color(if running {
                            palette.danger_fg
                        } else {
                            palette.fg
                        })
                        .cursor_pointer()
                        .child(if running { "停止" } else { "启动" })
                        .on_click(move |_, _, cx| {
                            if running {
                                toggle_bridge.stop_app(app_id.clone());
                            } else {
                                toggle_bridge.start_app(app_id.clone());
                            }
                            let _ = entity.update(cx, |_, cx| cx.notify());
                        }),
                );

                grid = grid.child(card_view);
            }
            main = main.child(grid);
        }

        let toasts = self.bridge.store.lock().expect("store lock").toasts();
        let mut toast_layer = div().flex().flex_col().gap(px(metrics::GAP_1));
        for toast in toasts {
            let entity = cx.entity().downgrade();
            let id = toast.id;
            let color = match toast.kind {
                crate::state::toast::ToastKind::Error => palette.danger_fg,
                crate::state::toast::ToastKind::Success => palette.accent,
                crate::state::toast::ToastKind::Info => palette.fg_muted,
            };
            toast_layer = toast_layer.child(
                div()
                    .id(format!("toast-{id}"))
                    .px(px(metrics::PAD_3))
                    .py(px(metrics::GAP_1))
                    .rounded(px(metrics::RADIUS_MD))
                    // Errors speak louder: they are the ones that need a decision.
                    .bg(if toast.kind == crate::state::toast::ToastKind::Error {
                        palette.danger_bg
                    } else {
                        palette.bg_elevated
                    })
                    .text_size(px(metrics::TEXT_SM))
                    .text_color(color)
                    .cursor_pointer()
                    .child(format!("[{}] {}", toast.kind.as_str(), toast.message))
                    .on_click(move |_, _, cx| {
                        let _ = entity.update(cx, |this, cx| {
                            if let Ok(mut store) = this.bridge.store.lock() {
                                store.dismiss_toast(id);
                            }
                            cx.notify();
                        });
                    }),
            );
        }

        div()
            .flex()
            .size_full()
            .bg(palette.bg_base)
            .child(sidebar)
            .child(main)
            .child(empty_state_notice)
            .child(toast_layer)
            .on_action(cx.listener(|this, _: &OpenSettings, window, cx| {
                this.on_shortcut(Shortcut::Settings, window, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenImport, window, cx| {
                this.on_shortcut(Shortcut::Import, window, cx);
            }))
            .on_action(cx.listener(|this, _: &FocusFilter, window, cx| {
                this.on_shortcut(Shortcut::Filter, window, cx);
            }))
            .on_action(cx.listener(|this, _: &GlobalSearch, window, cx| {
                this.on_shortcut(Shortcut::GlobalSearch, window, cx);
            }))
            .on_action(cx.listener(|this, _: &RestartSelected, window, cx| {
                this.on_shortcut(Shortcut::RestartSelected, window, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenCommandPalette, window, cx| {
                this.on_shortcut(Shortcut::Palette, window, cx);
            }))
    }
}
