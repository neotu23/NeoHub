//! The single business command surface. UI and MCP both call through here.

use crate::logs::{LineMatch, LogSubscriptionOutcome};
use crate::process::runner::RunningTaskInfo;
use crate::snapshot::CoreSnapshot;
use neohub_domain::{App, AppId, Task, TaskId};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum CoreCommand {
    GetSnapshot,
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

#[derive(Clone, Debug)]
pub enum CoreReply {
    Snapshot(Box<CoreSnapshot>),
    Started(Box<RunningTaskInfo>),
    Logs(String),
    ImportCandidates(Vec<crate::environment::ImportCandidate>),
    App(Box<App>),
    Task(Box<Task>),
    RunHistory(Vec<neohub_domain::RunHistoryRow>),
    LogBootstrap(LogSubscriptionOutcome),
    LogMatches(Vec<(TaskId, LineMatch)>),
    Ack,
}
