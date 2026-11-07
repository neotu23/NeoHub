//! The pinned crash report: what a task printed right before it died.
//!
//! A crash is the one moment where the log buffer is about to be the only evidence, and it
//! expires (the ring has a TTL for exited tasks). Pinning copies the tail at the moment of the
//! crash, so the pane can still show it minutes later when the user comes back to the window.
//!
//! Deliberately a snapshot, not a live view: if it kept updating it would not be a record of the
//! crash, it would be the current log with a scary title.

use neohub_domain::{AppId, ProcessState, TaskId};

/// How many lines are kept.
pub const TAIL_LINES: usize = 200;

/// A frozen tail plus what we know about how it ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrashPin {
    pub task_id: TaskId,
    pub app_id: AppId,
    pub task_name: String,
    pub exit_code: Option<i32>,
    pub tail: String,
}

impl CrashPin {
    /// Build a pin from the last output of a task that just crashed.
    pub fn new(
        task_id: TaskId,
        app_id: AppId,
        task_name: String,
        exit_code: Option<i32>,
        output: &str,
    ) -> Self {
        CrashPin {
            task_id,
            app_id,
            task_name,
            exit_code,
            tail: tail_lines(output, TAIL_LINES),
        }
    }

    /// One line for the card header.
    pub fn headline(&self) -> String {
        match self.exit_code {
            Some(code) => format!("{} 崩溃了（退出码 {code}）", self.task_name),
            None => format!("{} 崩溃了", self.task_name),
        }
    }

    /// Does this crash look like a port fight?
    ///
    /// A dev server that cannot bind says so; recognising it turns "it crashed" into "something
    /// else is on 3000", which is the actual next step for the user.
    pub fn looks_like_port_conflict(&self, port_hint: Option<u16>) -> Option<u16> {
        let lower = self.tail.to_lowercase();
        let mentions = lower.contains("eaddrinuse")
            || lower.contains("address already in use")
            || lower.contains("port is already in use");
        if !mentions {
            return None;
        }
        port_hint.or_else(|| port_in_text(&self.tail))
    }
}

/// Keep the last `limit` lines (newest kept, order preserved).
pub fn tail_lines(output: &str, limit: usize) -> String {
    if limit == 0 {
        return String::new();
    }
    let lines: Vec<&str> = output.lines().collect();
    let start = lines.len().saturating_sub(limit);
    lines[start..].join("\n")
}

/// Find a port number mentioned near an "in use" complaint, if any.
fn port_in_text(text: &str) -> Option<u16> {
    text.split(|character: char| !character.is_ascii_digit())
        .filter_map(|word| word.parse::<u16>().ok())
        .find(|port| (1..=65535).contains(port))
}

/// Whether a state change should create a pin.
pub fn should_pin(state: ProcessState) -> bool {
    matches!(state, ProcessState::Crashed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin(output: &str, exit: Option<i32>) -> CrashPin {
        CrashPin::new(
            TaskId::from_existing("t1"),
            AppId::from_existing("a1"),
            "dev".into(),
            exit,
            output,
        )
    }

    #[test]
    fn only_the_tail_is_kept() {
        let mut output = String::new();
        for index in 0..500 {
            output.push_str(&format!("line {index}\n"));
        }
        let pin = pin(&output, Some(1));
        assert_eq!(pin.tail.lines().count(), TAIL_LINES);
        assert!(pin.tail.ends_with("line 499"), "the newest line survives");
        assert!(!pin.tail.contains("line 0\n"), "the oldest lines are gone");
    }

    #[test]
    fn a_short_log_is_kept_whole() {
        let pin = pin("one\ntwo", Some(1));
        assert_eq!(pin.tail, "one\ntwo");
    }

    #[test]
    fn an_empty_log_pins_nothing_rather_than_failing() {
        let pin = pin("", None);
        assert!(pin.tail.is_empty());
        assert_eq!(pin.headline(), "dev 崩溃了");
    }

    #[test]
    fn the_headline_carries_the_exit_code_when_there_is_one() {
        assert_eq!(pin("x", Some(2)).headline(), "dev 崩溃了（退出码 2）");
        assert_eq!(pin("x", None).headline(), "dev 崩溃了");
    }

    #[test]
    fn a_port_fight_is_recognised() {
        let output = "Error: listen EADDRINUSE: address already in use :::3000";
        assert_eq!(
            pin(output, Some(1)).looks_like_port_conflict(None),
            Some(3000)
        );
        // The configured hint wins when both are available: it is the port the user cares about.
        assert_eq!(
            pin(output, Some(1)).looks_like_port_conflict(Some(4000)),
            Some(4000)
        );
    }

    #[test]
    fn the_lowercase_spelling_also_counts() {
        let output = "error: address already in use (port 5173)";
        assert_eq!(
            pin(output, Some(1)).looks_like_port_conflict(None),
            Some(5173)
        );
    }

    #[test]
    fn an_unrelated_crash_is_not_reported_as_a_port_conflict() {
        let output = "TypeError: cannot read properties of undefined";
        assert_eq!(
            pin(output, Some(1)).looks_like_port_conflict(Some(3000)),
            None
        );
    }

    #[test]
    fn only_crashes_are_pinned() {
        assert!(should_pin(ProcessState::Crashed));
        assert!(
            !should_pin(ProcessState::Exited),
            "a clean exit is not a crash"
        );
        assert!(!should_pin(ProcessState::Running));
    }

    #[test]
    fn a_zero_limit_keeps_nothing() {
        assert_eq!(tail_lines("a\nb", 0), "");
    }
}
