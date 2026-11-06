//! Listening ports per task, distinguishing "confirmed" from "hinted".
//!
//! Two sources, kept apart on purpose (the Electron build learned this the hard way):
//!
//! * **confirmed** — a LISTEN socket owned by a process in the task's process group,
//!   discovered with one batched `lsof` call;
//! * **hinted** — a URL/port mentioned in the task's output (`http://localhost:3000`).
//!   A hint is not evidence that *this* task is listening, so the UI/MCP labels it
//!   differently, and a hint that stays unconfirmed for three sweeps is dropped.

use neohub_domain::TaskId;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// How many unconfirmed sweeps a log hint survives.
pub const HINT_TTL_SWEEPS: u8 = 3;

/// One task's port picture.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PortSample {
    /// Owned LISTEN sockets, ascending.
    pub confirmed: Vec<u16>,
    /// Ports only mentioned in the output, ascending.
    pub hinted: Vec<u16>,
}

/// Per-task hint bookkeeping.
#[derive(Default)]
pub struct PortTracker {
    hints: HashMap<TaskId, BTreeMap<u16, u8>>,
}

impl PortTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed the task's whole retained output; new ports become hints with a fresh TTL.
    pub fn observe_logs(&mut self, task_id: &TaskId, raw_logs: &str) {
        let found = extract_port_hints(raw_logs);
        if found.is_empty() {
            return;
        }
        let entry = self.hints.entry(task_id.clone()).or_default();
        for port in found {
            entry.entry(port).or_insert(HINT_TTL_SWEEPS);
        }
    }

    /// Resolve one sweep: confirmed ports come from the process group, hints age out.
    pub fn sweep(&mut self, task_id: &TaskId, listening: &[u16]) -> PortSample {
        let confirmed: Vec<u16> = listening.to_vec();
        let mut hinted = Vec::new();
        if let Some(entry) = self.hints.get_mut(task_id) {
            entry.retain(|port, ttl| {
                let still_open = !confirmed.contains(port);
                if !still_open {
                    // A hint that was confirmed is no longer a hint.
                    return false;
                }
                *ttl = ttl.saturating_sub(1);
                if *ttl > 0 {
                    hinted.push(*port);
                    true
                } else {
                    false // unconfirmed for HINT_TTL_SWEEPS sweeps → drop
                }
            });
        }
        hinted.sort_unstable();
        PortSample { confirmed, hinted }
    }

    /// Forget a task (stopped or removed).
    pub fn forget(&mut self, task_id: &TaskId) {
        self.hints.remove(task_id);
    }

    pub fn clear(&mut self) {
        self.hints.clear();
    }
}

/// Ports mentioned in output, from `scheme://host:port` and bare `:port` URLs.
pub fn extract_port_hints(raw_logs: &str) -> BTreeSet<u16> {
    let mut out = BTreeSet::new();
    let bytes: Vec<char> = raw_logs.chars().collect();
    let mut index = 0;
    while index < bytes.len() {
        // Look for "://" then host then ':' digits.
        if bytes[index] == ':'
            && index + 2 < bytes.len()
            && bytes[index + 1] == '/'
            && bytes[index + 2] == '/'
        {
            let mut cursor = index + 3;
            // Skip the host part. A bracketed IPv6 literal contains ':' characters, so it is
            // consumed up to its closing ']' before looking for the port separator.
            while cursor < bytes.len() {
                let c = bytes[cursor];
                if c == '[' {
                    while cursor < bytes.len() && bytes[cursor] != ']' {
                        cursor += 1;
                    }
                    if cursor < bytes.len() {
                        cursor += 1;
                    }
                    continue;
                }
                if c.is_alphanumeric() || ".-_".contains(c) {
                    cursor += 1;
                    continue;
                }
                break;
            }
            if cursor < bytes.len() && bytes[cursor] == ':' {
                cursor += 1;
                let start = cursor;
                while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                    cursor += 1;
                }
                if cursor > start {
                    let text: String = bytes[start..cursor].iter().collect();
                    if let Ok(port) = text.parse::<u16>()
                        && port >= 1
                    {
                        out.insert(port);
                    }
                }
            }
            index = cursor.max(index + 1);
            continue;
        }
        index += 1;
    }
    out
}

/// LISTEN ports owned by any of `pids`, in one batched `lsof` call.
pub fn listening_ports_of(pids: &[i32]) -> Vec<u16> {
    if pids.is_empty() {
        return Vec::new();
    }
    // `-Fpn` prints one `p<pid>` line per pid followed by `n<address>` lines.
    let output = std::process::Command::new("/usr/sbin/lsof")
        .args(["-nP", "-iTCP", "-sTCP:LISTEN", "-Fpn"])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() && output.status.code() != Some(1) {
        return Vec::new();
    }
    let wanted: BTreeSet<i32> = pids.iter().copied().collect();
    let mut current_pid: Option<i32> = None;
    let mut ports: BTreeSet<u16> = BTreeSet::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(pid) = line.strip_prefix('p') {
            current_pid = pid.trim().parse::<i32>().ok();
        } else if let Some(name) = line.strip_prefix('n')
            && let Some(pid) = current_pid
            && wanted.contains(&pid)
            && let Some(port) = name.rsplit(':').next().and_then(|p| p.parse::<u16>().ok())
        {
            ports.insert(port);
        }
    }
    ports.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_come_from_urls_only() {
        let text = "\
listening on http://127.0.0.1:4319
also http://localhost:3000/ready
time 12:30:45 and ratio 3:2 should not match
https://[::1]:8080/path
";
        let hints = extract_port_hints(text);
        assert!(hints.contains(&4319));
        assert!(hints.contains(&3000));
        assert!(hints.contains(&8080));
        assert_eq!(hints.len(), 3, "times and ratios are not ports: {hints:?}");
    }

    #[test]
    fn a_hint_survives_three_sweeps_then_disappears() {
        let mut tracker = PortTracker::new();
        let task = TaskId::from_existing("t1");
        tracker.observe_logs(&task, "http://localhost:5173");
        // Three consecutive unconfirmed sweeps, then the hint is gone: the legacy rule drops a
        // hint on the third miss, so it is visible during sweeps 1 and 2.
        assert_eq!(tracker.sweep(&task, &[]).hinted, vec![5173]);
        assert_eq!(tracker.sweep(&task, &[]).hinted, vec![5173]);
        assert!(
            tracker.sweep(&task, &[]).hinted.is_empty(),
            "third unconfirmed sweep drops the hint"
        );
        assert!(tracker.sweep(&task, &[]).hinted.is_empty());
    }

    #[test]
    fn a_confirmed_port_is_not_reported_as_a_hint() {
        let mut tracker = PortTracker::new();
        let task = TaskId::from_existing("t1");
        tracker.observe_logs(&task, "http://localhost:4319");
        let sample = tracker.sweep(&task, &[4319]);
        assert_eq!(sample.confirmed, vec![4319]);
        assert!(sample.hinted.is_empty());
    }

    #[test]
    fn a_port_that_changes_is_tracked_as_both_until_confirmed() {
        let mut tracker = PortTracker::new();
        let task = TaskId::from_existing("t1");
        tracker.observe_logs(&task, "http://localhost:3000");
        assert_eq!(tracker.sweep(&task, &[3000]).confirmed, vec![3000]);
        // The dev server restarts on another port and says so.
        tracker.observe_logs(&task, "http://localhost:3001");
        let sample = tracker.sweep(&task, &[3001]);
        assert_eq!(sample.confirmed, vec![3001]);
        assert!(sample.hinted.is_empty(), "3000 is no longer mentioned");
    }

    #[test]
    fn forgetting_a_task_drops_its_hints() {
        let mut tracker = PortTracker::new();
        let task = TaskId::from_existing("t1");
        tracker.observe_logs(&task, "http://localhost:1234");
        tracker.forget(&task);
        assert!(tracker.sweep(&task, &[]).hinted.is_empty());
    }

    #[test]
    fn no_pids_means_no_ports_and_no_command() {
        assert!(listening_ports_of(&[]).is_empty());
    }
}
