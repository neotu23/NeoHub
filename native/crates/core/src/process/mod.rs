//! Process execution: what to spawn, how to stop the whole tree, and the PTY runner.

pub mod process_tree;
pub mod runner;
pub mod spawn_spec;

pub use process_tree::{
    TerminationError, TerminationOutcome, current_process_group, group_members, terminate_group,
};
pub use runner::{
    ExitInfo, LOG_BUDGET_BYTES, RunnerError, RunnerEvent, RunningTaskInfo, TaskRunner,
    wait_for_settled,
};
pub use spawn_spec::{
    DEFAULT_COLS, DEFAULT_ROWS, MIN_COLS, MIN_ROWS, SpawnSpec, SpawnSpecError, clamp_grid,
    resize_grid,
};
