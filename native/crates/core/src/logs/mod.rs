//! Bounded logs: raw bytes, parsed terminal state, subscription bootstrap and search.

pub mod buffer;
pub mod search;
pub mod subscription;
pub mod terminal_state;

pub use buffer::{
    AppendOutcome, CHUNK_LIMIT, EXITED_TTL_MS, GLOBAL_BUDGET, LogError, LogStore, LogUsage,
    PER_TASK_BUDGET, RAW_TAIL_BUDGET, TaskLog,
};
pub use search::{LineMatch, SearchError, SearchOptions, compile, search_text};
pub use subscription::{LogBootstrap, LogCursor, LogSubscriptionOutcome};
pub use terminal_state::{
    AnsiColor, CellStyle, SCROLLBACK_LINES, TerminalSnapshot, TerminalState, TextRun, apply_sgr,
};
