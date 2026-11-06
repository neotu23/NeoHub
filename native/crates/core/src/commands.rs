//! The single business command surface. UI and MCP both call through here.

use crate::logs::{LineMatch, LogSubscriptionOutcome};
use crate::process::runner::RunningTaskInfo;
use crate::snapshot::CoreSnapshot;
use neohub_domain::{App, AppId, TaskId};
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
    LogBootstrap(LogSubscriptionOutcome),
    LogMatches(Vec<(TaskId, LineMatch)>),
    Ack,
}
