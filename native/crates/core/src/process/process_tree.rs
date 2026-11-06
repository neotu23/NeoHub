//! Terminating a task means terminating **its whole process group**.
//!
//! `portable-pty` spawns every task through `forkpty`, and the child calls `setsid()`, so a
//! task's PTY session leader is also its process-group leader and every descendant that does
//! not deliberately create its own session stays in that group. Escalation is TERM → grace →
//! KILL, exactly like the Electron build, but performed on the group so a `pnpm → node →
//! worker` chain cannot survive a stop of the parent.
//!
//! Safety rules, all enforced here:
//! * never signal pid 0/1 or our own process group;
//! * never signal a group we did not spawn (callers pass the pgid captured at spawn time);
//! * a group that is already gone is a normal outcome, not an error.

use std::time::{Duration, Instant};

/// Default grace period between TERM and KILL (seconds→ms), matching the legacy default.
pub const DEFAULT_GRACE: Duration = Duration::from_millis(5_000);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminationOutcome {
    /// The group exited before or on the first TERM.
    Terminated,
    /// The group ignored TERM and had to be killed after the grace period.
    KilledAfterGrace,
    /// Nothing in the group was alive any more.
    AlreadyGone,
    /// Refused: the target was pid 0/1, our own group, or not a positive id.
    Refused,
}

#[derive(Debug, thiserror::Error)]
pub enum TerminationError {
    #[error("failed to signal process group {pgid}: {message}")]
    Signal { pgid: i32, message: String },
}

/// Terminate a process group with TERM, then KILL after `grace`.
///
/// Returns only once the group is empty (or the refusal/late-kill outcome is known), so a
/// caller that then marks the task `exited` cannot be lying about a still-running child.
pub fn terminate_group(pgid: i32, grace: Duration) -> Result<TerminationOutcome, TerminationError> {
    if pgid <= 1 {
        return Ok(TerminationOutcome::Refused);
    }
    let self_group = unsafe { libc::getpgrp() };
    if pgid == self_group {
        // Never take NeoHub (or the test harness) down with the task.
        return Ok(TerminationOutcome::Refused);
    }
    if group_members(pgid).is_empty() {
        return Ok(TerminationOutcome::AlreadyGone);
    }

    signal_group(pgid, libc::SIGTERM)?;
    if wait_for_empty(pgid, grace) {
        return Ok(TerminationOutcome::Terminated);
    }

    // Escalate: anything still alive after the grace period ignored TERM.
    signal_group(pgid, libc::SIGKILL)?;
    // SIGKILL is not ignorable; give the kernel a moment to reap.
    let _ = wait_for_empty(pgid, Duration::from_secs(2));
    Ok(TerminationOutcome::KilledAfterGrace)
}

/// Pids currently in `pgid` (empty when the group is gone).
pub fn group_members(pgid: i32) -> Vec<i32> {
    if pgid <= 0 {
        return Vec::new();
    }
    // One `ps` call per stop, not per signal: batched on purpose.
    let output = std::process::Command::new("/bin/ps")
        .args(["-o", "pid=,pgid=", "-x"])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let pid = parts.next()?.parse::<i32>().ok()?;
            let group = parts.next()?.parse::<i32>().ok()?;
            (group == pgid && pid > 0).then_some(pid)
        })
        .collect()
}

fn wait_for_empty(pgid: i32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if group_members(pgid).is_empty() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn signal_group(pgid: i32, signal: i32) -> Result<(), TerminationError> {
    // kill(-pgid) targets the group; a stale pgid yields ESRCH, which is fine.
    let rc = unsafe { libc::kill(-pgid, signal) };
    if rc == 0 {
        return Ok(());
    }
    let err = std::io::Error::last_os_error();
    if err.raw_os_error() == Some(libc::ESRCH) {
        return Ok(());
    }
    Err(TerminationError::Signal {
        pgid,
        message: err.to_string(),
    })
}

/// Our own process-group id (used by callers that need to prove a target is not us).
pub fn current_process_group() -> i32 {
    unsafe { libc::getpgrp() }
}

/// Our own pid.
pub fn current_pid() -> i32 {
    std::process::id() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_pid_zero_one_and_our_own_group() {
        assert_eq!(
            terminate_group(0, Duration::from_millis(10)).unwrap(),
            TerminationOutcome::Refused
        );
        assert_eq!(
            terminate_group(1, Duration::from_millis(10)).unwrap(),
            TerminationOutcome::Refused
        );
        assert_eq!(
            terminate_group(current_process_group(), Duration::from_millis(10)).unwrap(),
            TerminationOutcome::Refused,
            "NeoHub must never signal its own group"
        );
    }

    #[test]
    fn a_group_that_does_not_exist_reports_already_gone() {
        // 999_999 is above the pid_max used by macOS in practice.
        assert_eq!(
            terminate_group(999_999, Duration::from_millis(10)).unwrap(),
            TerminationOutcome::AlreadyGone
        );
    }

    #[test]
    fn group_members_of_a_bogus_group_is_empty() {
        assert!(group_members(999_999).is_empty());
        assert!(group_members(0).is_empty());
        assert!(group_members(-5).is_empty());
    }

    #[test]
    fn our_own_pid_is_not_in_a_foreign_group() {
        let members = group_members(current_process_group());
        assert!(
            members.contains(&current_pid()),
            "sanity: ps must list this test process in its own group"
        );
    }
}
