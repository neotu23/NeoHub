//! The single business command surface. UI and MCP both call through here.

use crate::logs::{LineMatch, LogSubscriptionOutcome};
use crate::process::runner::RunningTaskInfo;
use crate::snapshot::CoreSnapshot;
use neohub_domain::{App, AppId, Task, TaskId};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum CoreCommand {
    GetSnapshot,
    /// Environment variables for one scope.
    ListEnvVars {
        scope: crate::service::EnvScope,
    },
    /// Replace one scope's variables. `value: None` keeps the stored secret untouched.
    SetEnvVars {
        scope: crate::service::EnvScope,
        vars: Vec<crate::service::EnvVarInput>,
    },
    /// Current settings.
    GetSettings,
    /// Patch settings; only the named fields change, unknown fields are rejected by the type.
    UpdateSettings {
        patch: neohub_domain::SettingsPatch,
    },
    /// Shallow scan of a folder for importable projects.
    ScanFolder {
        dir: PathBuf,
    },
    /// Create an app (with its first task and env) atomically.
    CreateApp {
        /// Boxed: this is the largest payload by far, and unboxed it would inflate every
        /// other command in the enum.
        input: Box<crate::catalog::CreateAppInput>,
    },
    StartTask {
        task_id: TaskId,
    },
    StopTask {
        task_id: TaskId,
    },
    ResizeTask {
        task_id: TaskId,
        cols: u16,
        rows: u16,
    },
    GetLogs {
        task_id: TaskId,
    },
    /// App-level update (identity fields are not patchable).
    UpdateApp {
        app_id: AppId,
        input: Box<crate::catalog::UpdateAppInput>,
    },
    /// Remove an app registration. Refused while any of its tasks is live.
    RemoveApp {
        app_id: AppId,
    },
    /// Node installations discovered on this machine.
    ListNodeVersions,
    /// Task CRUD through the command surface (the UI never touches the catalog directly).
    AddTask {
        app_id: AppId,
        input: Box<crate::catalog::AddTaskInput>,
    },
    UpdateTask {
        task_id: TaskId,
        input: Box<crate::catalog::UpdateTaskInput>,
    },
    RemoveTask {
        task_id: TaskId,
    },
    ReorderTasks {
        app_id: AppId,
        task_ids: Vec<TaskId>,
    },
    /// Run history for one app (newest first).
    ListRunHistory {
        app_id: AppId,
        limit: Option<u32>,
    },
    /// Subscribe helper: terminal grid + cursor + bounded raw tail, taken atomically.
    BootstrapLogs {
        task_id: TaskId,
        after: Option<crate::logs::LogCursor>,
    },
    /// Search retained raw text. `task_id: None` searches every task.
    SearchLogs {
        task_id: Option<TaskId>,
        query: String,
        options: crate::logs::SearchOptions,
    },
    /// Parsed terminal text (readable).
    GetTerminalText {
        task_id: TaskId,
    },
    /// Write one task's logs to a file. `raw = true` keeps escape sequences byte-for-byte;
    /// `raw = false` writes the parsed, human-readable text.
    ExportLogs {
        task_id: TaskId,
        path: PathBuf,
        raw: bool,
    },
    /// Drop one task's retained logs. Never touches the process.
    ClearLogs {
        task_id: TaskId,
    },
    /// Start an app: topological levels, readiness per task, rollback on failure.
    StartApp {
        app_id: AppId,
    },
    /// Reverse-topological stop of every task of one app.
    StopApp {
        app_id: AppId,
    },
    /// Stop then start, held together so nothing interleaves.
    RestartApp {
        app_id: AppId,
    },
    /// Refuse new starts, stop everything, write history, then close the database.
    Shutdown,
}

impl CoreCommand {
    /// Does a successful run of this command change the catalog, settings or env of an app?
    ///
    /// The answer drives the `CatalogChanged` broadcast: the window and MCP both go through
    /// `CoreHandle::request`, so a mutation made by a tool refreshes the UI automatically.
    pub fn mutates_catalog(&self) -> bool {
        matches!(
            self,
            CoreCommand::CreateApp { .. }
                | CoreCommand::UpdateApp { .. }
                | CoreCommand::RemoveApp { .. }
                | CoreCommand::AddTask { .. }
                | CoreCommand::UpdateTask { .. }
                | CoreCommand::RemoveTask { .. }
                | CoreCommand::ReorderTasks { .. }
                | CoreCommand::SetEnvVars { .. }
                | CoreCommand::UpdateSettings { .. }
        )
    }
}

#[derive(Clone, Debug)]
pub enum CoreReply {
    Snapshot(Box<CoreSnapshot>),
    Started(Box<RunningTaskInfo>),
    Logs(String),
    ImportCandidates(Vec<crate::environment::ImportCandidate>),
    App(Box<App>),
    Task(Box<Task>),
    Settings(Box<neohub_domain::Settings>),
    EnvVars(Vec<crate::service::EnvVarView>),
    RunHistory(Vec<neohub_domain::RunHistoryRow>),
    NodeVersions(Vec<crate::environment::NodeInstallation>),
    LogBootstrap(LogSubscriptionOutcome),
    LogMatches(Vec<(TaskId, LineMatch)>),
    /// Absolute path written by `ExportLogs`.
    Exported(PathBuf),
    Ack,
}
