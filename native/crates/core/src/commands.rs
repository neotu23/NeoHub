//! The single business command surface. UI and MCP both call through here.

use crate::process::runner::RunningTaskInfo;
use crate::snapshot::CoreSnapshot;
use neohub_domain::{AppId, TaskId};

#[derive(Clone, Debug)]
pub enum CoreCommand {
    GetSnapshot,
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
    /// Reverse-topological stop of every task of one app (phase 7 adds the ordering).
    StopApp {
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
    Ack,
}
