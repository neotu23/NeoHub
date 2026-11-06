//! Log cursor and bootstrap types.
//!
//! A subscriber never receives an unbounded backlog: it asks for a `LogCursor`
//! (`run_id` + `seq`) and gets either the events *after* that cursor or an explicit
//! `ResetRequired` with a fresh bootstrap. That is what keeps a re-opened window from
//! replaying a half-parsed ANSI fragment.

use super::terminal_state::TerminalSnapshot;
use neohub_domain::RunId;

/// Where a subscriber left off.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogCursor {
    pub run_id: RunId,
    pub seq: u64,
}

/// Atomically-taken starting point: terminal grid plus the raw tail.
#[derive(Clone, Debug)]
pub struct LogBootstrap {
    /// Continue from *after* this cursor.
    pub cursor: LogCursor,
    pub terminal: TerminalSnapshot,
    /// Bounded raw bytes (may be empty for a task that has produced nothing yet).
    pub raw_tail: Vec<u8>,
    /// True when older raw history has been evicted — the UI shows a truncation notice.
    pub truncated: bool,
}

/// Result of subscribing.
#[derive(Clone, Debug)]
pub enum LogSubscriptionOutcome {
    /// The cursor was still valid: events after it will be delivered.
    Ok(LogBootstrap),
    /// The cursor was from an older run or had been evicted: use this bootstrap and expect a
    /// discontinuity (never pretend the log is continuous).
    ResetRequired(LogBootstrap),
}
