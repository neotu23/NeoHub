//! Core events pushed to every subscriber (UI window and MCP alike).

use crate::process::runner::ExitInfo;
use neohub_domain::{AppId, ProcessState, RunId, TaskId};

/// What happened. Lifecycle variants must never be dropped silently: a subscriber that
/// falls behind receives [`CoreEventKind::ResyncRequired`] and re-reads a snapshot.
#[derive(Clone, Debug)]
pub enum CoreEventKind {
    TaskStatus {
        task_id: TaskId,
        app_id: AppId,
        state: ProcessState,
        pid: Option<u32>,
        run_id: RunId,
        exit: Option<ExitInfo>,
    },
    Log {
        task_id: TaskId,
        run_id: RunId,
        seq: u64,
        bytes: Vec<u8>,
    },
    /// Catalog rows changed (app/task/env edited through the same command path).
    CatalogChanged,
    /// The subscriber missed events; take a fresh snapshot and continue after it.
    ResyncRequired { revision: u64 },
}

/// One event plus the revision it belongs to.
#[derive(Clone, Debug)]
pub struct CoreEvent {
    pub revision: u64,
    pub kind: CoreEventKind,
}
