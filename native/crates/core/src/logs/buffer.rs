//! Bounded per-task log storage: the raw byte ring plus the parsed terminal state.
//!
//! Budgets (ported from the Electron build's intent, now counted in **real UTF-8 bytes**
//! rather than JS string length):
//!
//! * 5 MiB of raw bytes per task (`PER_TASK_BUDGET`);
//! * 100 MiB across every task (`GLOBAL_BUDGET`) — over that, the oldest chunk anywhere is
//!   evicted, and the owning task is flagged `truncated`;
//! * one emitted event carries at most 10 KiB (`CHUNK_LIMIT`), so a single huge write cannot
//!   create an unbounded event;
//! * exited tasks are dropped after ~10 minutes (`EXITED_TTL_MS`).
//!
//! Nothing here drops bytes the parser has not consumed: `append` feeds the parser before it
//! trims raw history, so eviction can never desynchronise the terminal.

use super::search::{LineMatch, SearchError, SearchOptions, search_text};
use super::subscription::{LogBootstrap, LogCursor, LogSubscriptionOutcome};
use super::terminal_state::TerminalState;
use neohub_domain::{RunId, TaskId};
use std::collections::{HashMap, VecDeque};

/// Raw bytes kept per task.
pub const PER_TASK_BUDGET: usize = 5 * 1024 * 1024;
/// Raw bytes kept across all tasks.
pub const GLOBAL_BUDGET: usize = 100 * 1024 * 1024;
/// Maximum bytes in one emitted chunk.
pub const CHUNK_LIMIT: usize = 10 * 1024;
/// Raw tail handed to a new subscriber.
pub const RAW_TAIL_BUDGET: usize = 256 * 1024;
/// Retain an exited task's logs for this long before eviction.
pub const EXITED_TTL_MS: i64 = 10 * 60 * 1000;
/// Default terminal geometry (matches the PTY's initial grid).
pub const DEFAULT_COLS: u16 = 120;
/// Default terminal geometry.
pub const DEFAULT_ROWS: u16 = 30;

#[derive(Clone, Debug)]
struct Chunk {
    seq: u64,
    bytes: Vec<u8>,
}

/// One task run's logs.
pub struct TaskLog {
    pub run_id: RunId,
    /// Sequence number of the most recent chunk.
    pub seq: u64,
    chunks: VecDeque<Chunk>,
    bytes: usize,
    /// Total bytes ever received for this run (reporting only).
    pub total_bytes: u64,
    /// True once raw history has been dropped for this run.
    pub truncated: bool,
    pub terminal: TerminalState,
    /// Set when the task leaves the live states.
    pub exited_at: Option<i64>,
}

impl TaskLog {
    fn new(run_id: RunId) -> Self {
        TaskLog {
            run_id,
            seq: 0,
            chunks: VecDeque::new(),
            bytes: 0,
            total_bytes: 0,
            truncated: false,
            terminal: TerminalState::new(DEFAULT_COLS, DEFAULT_ROWS),
            exited_at: None,
        }
    }

    fn first_seq(&self) -> u64 {
        self.chunks
            .front()
            .map(|chunk| chunk.seq)
            .unwrap_or(self.seq)
    }

    /// Push raw bytes; returns the chunks to emit (each at most `CHUNK_LIMIT`).
    fn append(&mut self, bytes: &[u8], chunk_limit: usize) -> Vec<(u64, Vec<u8>)> {
        // Parse first: the terminal must see every byte even if raw history is trimmed.
        self.terminal.feed(bytes);

        let mut emitted = Vec::new();
        for piece in bytes.chunks(chunk_limit) {
            self.seq += 1;
            emitted.push((self.seq, piece.to_vec()));
            self.bytes += piece.len();
            self.total_bytes += piece.len() as u64;
            self.chunks.push_back(Chunk {
                seq: self.seq,
                bytes: piece.to_vec(),
            });
        }
        emitted
    }

    /// Trim to a byte budget, dropping the oldest chunks. Returns how many bytes went away
    /// so the store-level total stays truthful.
    fn trim_to(&mut self, budget: usize) -> usize {
        let mut dropped = 0;
        while self.bytes > budget {
            let Some(front) = self.chunks.pop_front() else {
                break;
            };
            dropped += front.bytes.len();
            self.bytes = self.bytes.saturating_sub(front.bytes.len());
            self.truncated = true;
        }
        dropped
    }

    fn raw_tail(&self, limit: usize) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        for chunk in self.chunks.iter().rev() {
            if out.len() + chunk.bytes.len() > limit {
                let take = limit.saturating_sub(out.len());
                let mut piece: Vec<u8> =
                    chunk.bytes[chunk.bytes.len().saturating_sub(take)..].to_vec();
                piece.extend_from_slice(&out);
                return piece;
            }
            out.splice(0..0, chunk.bytes.iter().copied());
        }
        out
    }

    /// All retained raw bytes (for export/search).
    pub fn raw_text(&self) -> String {
        let mut bytes = Vec::with_capacity(self.bytes);
        for chunk in &self.chunks {
            bytes.extend_from_slice(&chunk.bytes);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }

    pub fn raw_len(&self) -> usize {
        self.bytes
    }
}

/// What `append` produced.
pub struct AppendOutcome {
    /// Chunks to broadcast, in order (seq, bytes).
    pub chunks: Vec<(u64, Vec<u8>)>,
    /// True when this append started a new run (previous run's logs were dropped).
    pub new_run: bool,
    /// True when this append evicted older raw history.
    pub truncated: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum LogError {
    #[error("no logs for task {0}")]
    UnknownTask(String),
    #[error(transparent)]
    Search(#[from] SearchError),
}

/// Every task's logs, with global memory accounting.
pub struct LogStore {
    logs: HashMap<TaskId, TaskLog>,
    total: usize,
    per_task_budget: usize,
    global_budget: usize,
    chunk_limit: usize,
}

impl Default for LogStore {
    fn default() -> Self {
        Self::new()
    }
}

impl LogStore {
    pub fn new() -> Self {
        LogStore {
            logs: HashMap::new(),
            total: 0,
            per_task_budget: PER_TASK_BUDGET,
            global_budget: GLOBAL_BUDGET,
            chunk_limit: CHUNK_LIMIT,
        }
    }

    /// Custom budgets (tests use tiny ones to prove eviction without allocating 100 MiB).
    pub fn with_budgets(per_task: usize, global: usize, chunk_limit: usize) -> Self {
        LogStore {
            logs: HashMap::new(),
            total: 0,
            per_task_budget: per_task,
            global_budget: global,
            chunk_limit,
        }
    }

    /// Make sure a run has a log entry even before it prints anything, so a re-opened window
    /// gets an (empty) bootstrap instead of "no logs".
    pub fn ensure_run(&mut self, task_id: &TaskId, run_id: &RunId) {
        let entry = self
            .logs
            .entry(task_id.clone())
            .or_insert_with(|| TaskLog::new(run_id.clone()));
        if &entry.run_id != run_id {
            self.total = self.total.saturating_sub(entry.bytes);
            *entry = TaskLog::new(run_id.clone());
        }
    }

    /// Append raw PTY bytes for one run.
    pub fn append(&mut self, task_id: &TaskId, run_id: &RunId, bytes: &[u8]) -> AppendOutcome {
        let mut new_run = false;
        let chunk_limit = self.chunk_limit;
        let per_task_budget = self.per_task_budget;
        let (chunks, truncated) = {
            let entry = self
                .logs
                .entry(task_id.clone())
                .or_insert_with(|| TaskLog::new(run_id.clone()));
            if &entry.run_id != run_id {
                // A restart is a new stream: drop the previous run's bytes and grid so a
                // cursor from the old run can never be mistaken for continuity.
                self.total = self.total.saturating_sub(entry.bytes);
                *entry = TaskLog::new(run_id.clone());
                new_run = true;
            }
            let chunks = entry.append(bytes, chunk_limit);
            self.total += bytes.len();
            let dropped = entry.trim_to(per_task_budget);
            self.total = self.total.saturating_sub(dropped);
            (chunks, entry.truncated)
        };
        self.enforce_global_budget();
        AppendOutcome {
            chunks,
            new_run,
            truncated,
        }
    }

    /// Drop the oldest chunk anywhere until the global budget is respected.
    fn enforce_global_budget(&mut self) {
        while self.total > self.global_budget {
            let victim = self
                .logs
                .iter()
                .filter_map(|(task_id, log)| {
                    log.chunks
                        .front()
                        .map(|chunk| (task_id.clone(), chunk.bytes.len(), chunk.seq))
                })
                .min_by_key(|(_, _, seq)| *seq);
            let Some((task_id, len, _)) = victim else {
                break;
            };
            if let Some(log) = self.logs.get_mut(&task_id) {
                log.chunks.pop_front();
                log.bytes = log.bytes.saturating_sub(len);
                log.truncated = true;
                self.total = self.total.saturating_sub(len);
            } else {
                break;
            }
        }
    }

    /// Subscribe: cursor + terminal + bounded raw tail, taken atomically.
    pub fn bootstrap(
        &self,
        task_id: &TaskId,
        after: Option<&LogCursor>,
    ) -> Option<LogSubscriptionOutcome> {
        let log = self.logs.get(task_id)?;
        let cursor = LogCursor {
            run_id: log.run_id.clone(),
            seq: log.seq,
        };
        let bootstrap = LogBootstrap {
            cursor: cursor.clone(),
            terminal: log.terminal.snapshot(),
            raw_tail: log.raw_tail(RAW_TAIL_BUDGET),
            truncated: log.truncated,
        };
        match after {
            None => Some(LogSubscriptionOutcome::Ok(bootstrap)),
            Some(after) => {
                if after.run_id != log.run_id {
                    // Restart happened while this subscriber was away.
                    return Some(LogSubscriptionOutcome::ResetRequired(bootstrap));
                }
                if after.seq + 1 < log.first_seq() {
                    // The requested position has been evicted.
                    return Some(LogSubscriptionOutcome::ResetRequired(bootstrap));
                }
                Some(LogSubscriptionOutcome::Ok(bootstrap))
            }
        }
    }

    /// Search one task's retained raw text.
    pub fn search_task(
        &self,
        task_id: &TaskId,
        query: &str,
        options: &SearchOptions,
    ) -> Result<Vec<LineMatch>, LogError> {
        let log = self
            .logs
            .get(task_id)
            .ok_or_else(|| LogError::UnknownTask(task_id.to_string()))?;
        Ok(search_text(&log.raw_text(), query, options)?)
    }

    /// Search every retained task (global search).
    pub fn search_all(
        &self,
        query: &str,
        options: &SearchOptions,
    ) -> Result<Vec<(TaskId, LineMatch)>, LogError> {
        let mut out = Vec::new();
        let mut task_ids: Vec<&TaskId> = self.logs.keys().collect();
        task_ids.sort_by_key(|id| id.as_str().to_owned());
        for task_id in task_ids {
            for hit in self.search_task(task_id, query, options)? {
                out.push((task_id.clone(), hit));
                if out.len() >= options.limit {
                    return Ok(out);
                }
            }
        }
        Ok(out)
    }

    /// Drop one task's logs. Never touches the process.
    pub fn clear(&mut self, task_id: &TaskId) -> bool {
        match self.logs.remove(task_id) {
            Some(log) => {
                self.total = self.total.saturating_sub(log.bytes);
                true
            }
            None => false,
        }
    }

    /// Mark a task as no longer live (starts the retention clock).
    pub fn mark_exited(&mut self, task_id: &TaskId, now_ms: i64) {
        if let Some(log) = self.logs.get_mut(task_id) {
            log.exited_at = Some(now_ms);
        }
    }

    /// Evict logs of tasks that have been exited longer than the TTL.
    pub fn evict_expired(&mut self, now_ms: i64) -> usize {
        let expired: Vec<TaskId> = self
            .logs
            .iter()
            .filter_map(|(task_id, log)| match log.exited_at {
                Some(at) if now_ms - at >= EXITED_TTL_MS => Some(task_id.clone()),
                _ => None,
            })
            .collect();
        let mut evicted = 0;
        for task_id in expired {
            if self.clear(&task_id) {
                evicted += 1;
            }
        }
        evicted
    }

    /// Terminal geometry follows the visible terminal.
    pub fn resize(&mut self, task_id: &TaskId, cols: u16, rows: u16) {
        if let Some(log) = self.logs.get_mut(task_id) {
            log.terminal.resize(cols, rows);
        }
    }

    /// Parsed terminal text (scrollback + grid, escape sequences already consumed).
    pub fn terminal_text(&self, task_id: &TaskId) -> Option<String> {
        self.logs.get(task_id).map(|log| log.terminal.to_text())
    }

    pub fn terminal_snapshot(
        &self,
        task_id: &TaskId,
    ) -> Option<super::terminal_state::TerminalSnapshot> {
        self.logs.get(task_id).map(|log| log.terminal.snapshot())
    }

    pub fn raw_text(&self, task_id: &TaskId) -> Option<String> {
        self.logs.get(task_id).map(|log| log.raw_text())
    }

    pub fn usage(&self) -> LogUsage {
        LogUsage {
            total_bytes: self.total,
            per_task: self
                .logs
                .iter()
                .map(|(id, log)| (id.clone(), log.raw_len()))
                .collect(),
            truncated_tasks: self
                .logs
                .iter()
                .filter(|(_, log)| log.truncated)
                .map(|(id, _)| id.clone())
                .collect(),
        }
    }

    pub fn tracked_tasks(&self) -> usize {
        self.logs.len()
    }

    pub fn run_id_of(&self, task_id: &TaskId) -> Option<RunId> {
        self.logs.get(task_id).map(|log| log.run_id.clone())
    }
}

/// Reported memory usage, so "bounded" is measurable rather than asserted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogUsage {
    pub total_bytes: usize,
    pub per_task: Vec<(TaskId, usize)>,
    pub truncated_tasks: Vec<TaskId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str) -> TaskId {
        TaskId::from_existing(id)
    }

    #[test]
    fn chunks_are_capped_and_sequence_numbers_are_contiguous() {
        let mut store = LogStore::with_budgets(PER_TASK_BUDGET, GLOBAL_BUDGET, 16);
        let id = task("t1");
        let run = RunId::from_existing("run1");
        let outcome = store.append(&id, &run, &[b'x'; 40]);
        assert_eq!(outcome.chunks.len(), 3, "40 bytes at 16 bytes/chunk");
        assert_eq!(
            outcome
                .chunks
                .iter()
                .map(|(seq, _)| *seq)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert!(outcome.chunks.iter().all(|(_, bytes)| bytes.len() <= 16));
        assert!(!outcome.new_run);
    }

    #[test]
    fn a_per_task_budget_evicts_old_bytes_and_flags_truncation() {
        let mut store = LogStore::with_budgets(32, GLOBAL_BUDGET, 8);
        let id = task("t1");
        let run = RunId::from_existing("run1");
        for _ in 0..10 {
            store.append(&id, &run, b"01234567");
        }
        let usage = store.usage();
        assert!(usage.total_bytes <= 32, "per-task budget respected");
        assert!(usage.truncated_tasks.contains(&id));
        let outcome = store.append(&id, &run, b"z");
        assert!(outcome.truncated);
    }

    #[test]
    fn the_global_budget_evicts_across_tasks() {
        let mut store = LogStore::with_budgets(64, 64, 8);
        let run = RunId::from_existing("run1");
        for index in 0..4 {
            let id = task(&format!("t{index}"));
            store.append(&id, &run, b"abcdefgh");
            store.append(&id, &run, b"abcdefgh");
            store.append(&id, &run, b"abcdefgh");
        }
        let usage = store.usage();
        assert!(usage.total_bytes <= 64, "global budget respected");
        assert!(
            usage.truncated_tasks.len() >= 2,
            "evictions must be visible: {usage:?}"
        );
    }

    #[test]
    fn a_new_run_drops_the_previous_stream() {
        let mut store = LogStore::with_budgets(PER_TASK_BUDGET, GLOBAL_BUDGET, 8);
        let id = task("t1");
        store.append(&id, &RunId::from_existing("run1"), b"old output");
        let outcome = store.append(&id, &RunId::from_existing("run2"), b"new output");
        assert!(outcome.new_run, "a restart is a new stream");
        assert_eq!(outcome.chunks[0].0, 1, "sequence restarts for the new run");
        assert_eq!(store.raw_text(&id).unwrap(), "new output");
    }

    #[test]
    fn bootstrap_reports_a_reset_after_a_restart() {
        let mut store = LogStore::with_budgets(PER_TASK_BUDGET, GLOBAL_BUDGET, 8);
        let id = task("t1");
        store.append(&id, &RunId::from_existing("run1"), b"hello");
        let stale = LogCursor {
            run_id: RunId::from_existing("run1"),
            seq: 1,
        };
        store.append(&id, &RunId::from_existing("run2"), b"world");
        match store.bootstrap(&id, Some(&stale)).expect("logs exist") {
            LogSubscriptionOutcome::ResetRequired(bootstrap) => {
                assert_eq!(bootstrap.cursor.run_id.as_str(), "run2");
            }
            other => panic!("expected a reset, got {other:?}"),
        }
    }

    #[test]
    fn bootstrap_reports_a_reset_when_the_cursor_was_evicted() {
        let mut store = LogStore::with_budgets(16, GLOBAL_BUDGET, 8);
        let id = task("t1");
        let run = RunId::from_existing("run1");
        for _ in 0..6 {
            store.append(&id, &run, b"01234567");
        }
        let old = LogCursor {
            run_id: run.clone(),
            seq: 1,
        };
        match store.bootstrap(&id, Some(&old)).expect("logs exist") {
            LogSubscriptionOutcome::ResetRequired(bootstrap) => assert!(bootstrap.truncated),
            other => panic!("expected a reset, got {other:?}"),
        }
    }

    #[test]
    fn a_valid_cursor_gets_a_plain_bootstrap() {
        let mut store = LogStore::with_budgets(PER_TASK_BUDGET, GLOBAL_BUDGET, 8);
        let id = task("t1");
        let run = RunId::from_existing("run1");
        store.append(&id, &run, b"abc");
        let cursor = LogCursor {
            run_id: run.clone(),
            seq: 1,
        };
        match store.bootstrap(&id, Some(&cursor)).expect("logs exist") {
            LogSubscriptionOutcome::Ok(bootstrap) => {
                assert_eq!(bootstrap.cursor.seq, 1);
                assert!(bootstrap.raw_tail.starts_with(b"abc"));
            }
            other => panic!("expected ok, got {other:?}"),
        }
    }

    #[test]
    fn the_bootstrap_raw_tail_is_bounded() {
        let mut store = LogStore::with_budgets(PER_TASK_BUDGET, GLOBAL_BUDGET, CHUNK_LIMIT);
        let id = task("t1");
        let run = RunId::from_existing("run1");
        let big = vec![b'a'; RAW_TAIL_BUDGET * 2];
        store.append(&id, &run, &big);
        match store.bootstrap(&id, None).expect("logs exist") {
            LogSubscriptionOutcome::Ok(bootstrap) => {
                assert!(bootstrap.raw_tail.len() <= RAW_TAIL_BUDGET);
            }
            other => panic!("expected ok, got {other:?}"),
        }
    }

    #[test]
    fn clearing_one_task_leaves_the_others_alone() {
        let mut store = LogStore::with_budgets(PER_TASK_BUDGET, GLOBAL_BUDGET, 8);
        let run = RunId::from_existing("run1");
        store.append(&task("a"), &run, b"aaa");
        store.append(&task("b"), &run, b"bbb");
        assert!(store.clear(&task("a")));
        assert!(store.raw_text(&task("a")).is_none());
        assert_eq!(store.raw_text(&task("b")).unwrap(), "bbb");
        assert!(!store.clear(&task("a")), "clearing twice is a no-op");
    }

    #[test]
    fn exited_logs_are_evicted_after_the_ttl_only() {
        let mut store = LogStore::with_budgets(PER_TASK_BUDGET, GLOBAL_BUDGET, 8);
        let id = task("t1");
        store.append(&id, &RunId::from_existing("run1"), b"hello");
        store.mark_exited(&id, 1_000);
        assert_eq!(store.evict_expired(1_000 + EXITED_TTL_MS - 1), 0);
        assert!(
            store.raw_text(&id).is_some(),
            "still inside the retention window"
        );
        assert_eq!(store.evict_expired(1_000 + EXITED_TTL_MS), 1);
        assert!(store.raw_text(&id).is_none());
    }

    #[test]
    fn terminal_state_tracks_the_grid_for_the_same_run() {
        let mut store = LogStore::with_budgets(PER_TASK_BUDGET, GLOBAL_BUDGET, 8);
        let id = task("t1");
        store.append(&id, &RunId::from_existing("run1"), b"hello\r\nworld");
        let snapshot = store.terminal_snapshot(&id).expect("terminal");
        assert_eq!(snapshot.lines[0].trim_end(), "hello");
        assert_eq!(snapshot.parsed_bytes, 12);
    }
}
