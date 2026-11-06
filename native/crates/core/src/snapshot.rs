//! State snapshot handed to a (re)connecting window.

use crate::monitor::ports::PortSample;
use crate::monitor::stats::StatsSample;
use crate::process::runner::RunningTaskInfo;
use neohub_domain::{App, AppId, ProcessState, Settings, Task};
use std::collections::BTreeMap;

/// Everything a fresh window needs to render, taken atomically.
#[derive(Clone, Debug)]
pub struct CoreSnapshot {
    /// Monotonic revision; events at or below it are already reflected here.
    pub revision: u64,
    pub apps: Vec<App>,
    pub tasks_by_app: BTreeMap<AppId, Vec<Task>>,
    pub running_tasks: Vec<RunningTaskInfo>,
    pub app_states: BTreeMap<AppId, ProcessState>,
    pub settings: Settings,
    /// Latest CPU/memory sample per running task.
    pub task_stats: BTreeMap<neohub_domain::TaskId, StatsSample>,
    /// Latest port picture per running task.
    pub task_ports: BTreeMap<neohub_domain::TaskId, PortSample>,
    /// Readiness verdict per task. Absent means "no probe has reported anything yet", which
    /// clients render as `null` — an idle task must not look verified.
    pub task_ready: BTreeMap<neohub_domain::TaskId, bool>,
}
