//! Process statistics per task.
//!
//! One batched `ps` call per sweep (never one shell per task per second), which keeps the
//! cost flat no matter how many tasks run. Two shapes are reported:
//!
//! * **own** — the task's root pid (CPU %, RSS MB). This preserves the old dashboard's
//!   `cpu`/`memMB` meaning, so numbers stay comparable.
//! * **tree** — the root plus every descendant (multi-level). Reported in separate fields
//!   so the UI does not silently change what "memory" means.

use std::collections::{BTreeMap, HashMap};

/// Default stats interval (matches `dashboard_refresh_ms`).
pub const DEFAULT_INTERVAL_MS: u64 = 1_000;
/// Lowest interval accepted (the old sampler's floor).
pub const MIN_INTERVAL_MS: u64 = 200;

/// One row of the process table we care about.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProcRow {
    pub pid: i32,
    pub ppid: i32,
    pub cpu_percent: f32,
    /// Resident set size in bytes.
    pub rss_bytes: u64,
}

/// Aggregated view for one task.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StatsSample {
    /// Root process CPU (legacy `cpu`).
    pub cpu_percent: f32,
    /// Root process RSS in MB (legacy `memMB`).
    pub mem_mb: f64,
    /// Root + descendants CPU (new, separate field).
    pub tree_cpu_percent: f32,
    /// Root + descendants RSS in MB (new, separate field).
    pub tree_mem_mb: f64,
    /// How many processes the tree contains (1 when only the root is alive).
    pub process_count: usize,
}

/// Read the whole process table once.
pub fn read_process_table() -> Vec<ProcRow> {
    // `-A` all processes, `-o` only the columns we need, `-r` no sorting work.
    let output = std::process::Command::new("/bin/ps")
        .args(["-A", "-o", "pid=,ppid=,pcpu=,rss="])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let pid = parts.next()?.parse::<i32>().ok()?;
            let ppid = parts.next()?.parse::<i32>().ok()?;
            let cpu = parts.next()?.parse::<f32>().unwrap_or(0.0);
            // ps reports RSS in KiB.
            let rss_kb = parts.next()?.parse::<u64>().unwrap_or(0);
            Some(ProcRow {
                pid,
                ppid,
                cpu_percent: cpu,
                rss_bytes: rss_kb * 1024,
            })
        })
        .collect()
}

/// Sample one task's root pid from a process table that was read once.
pub fn sample_from_table(table: &[ProcRow], root_pid: i32) -> StatsSample {
    let by_pid: HashMap<i32, &ProcRow> = table.iter().map(|row| (row.pid, row)).collect();
    let mut children: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
    for row in table {
        children.entry(row.ppid).or_default().push(row.pid);
    }

    let mut sample = StatsSample::default();
    if let Some(root) = by_pid.get(&root_pid) {
        sample.cpu_percent = root.cpu_percent;
        sample.mem_mb = root.rss_bytes as f64 / (1024.0 * 1024.0);
    }
    let mut stack = vec![root_pid];
    while let Some(pid) = stack.pop() {
        if let Some(row) = by_pid.get(&pid) {
            sample.tree_cpu_percent += row.cpu_percent;
            sample.tree_mem_mb += row.rss_bytes as f64 / (1024.0 * 1024.0);
            sample.process_count += 1;
        }
        if let Some(kids) = children.get(&pid) {
            stack.extend(kids.iter().copied());
        }
    }
    if sample.process_count == 0 {
        sample.cpu_percent = 0.0;
        sample.mem_mb = 0.0;
    }
    sample
}

/// Sample many tasks from a single table read.
pub fn sample_tasks(table: &[ProcRow], roots: &[(i32, i32)]) -> BTreeMap<i32, StatsSample> {
    roots
        .iter()
        .map(|(task_pid, _)| (*task_pid, sample_from_table(table, *task_pid)))
        .collect()
}

/// Clamp a configured interval to the allowed range.
pub fn clamp_interval(ms: u64) -> u64 {
    ms.max(MIN_INTERVAL_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pid: i32, ppid: i32, cpu: f32, rss_kb: u64) -> ProcRow {
        ProcRow {
            pid,
            ppid,
            cpu_percent: cpu,
            rss_bytes: rss_kb * 1024,
        }
    }

    #[test]
    fn own_stats_are_the_root_process_and_tree_stats_include_descendants() {
        let table = vec![
            row(100, 1, 5.0, 1024),
            row(101, 100, 2.5, 2048),
            row(102, 101, 1.0, 512),
        ];
        let sample = sample_from_table(&table, 100);
        assert_eq!(sample.cpu_percent, 5.0, "legacy cpu is the root's");
        assert!(
            (sample.mem_mb - 1.0).abs() < 0.001,
            "legacy memMB is the root's"
        );
        assert_eq!(sample.process_count, 3);
        assert!((sample.tree_cpu_percent - 8.5).abs() < 0.001);
        assert!((sample.tree_mem_mb - 3.5).abs() < 0.001);
    }

    #[test]
    fn a_dead_root_samples_as_zero_without_panicking() {
        let table = vec![row(101, 100, 2.5, 2048)];
        let sample = sample_from_table(&table, 999);
        assert_eq!(sample.process_count, 0);
        assert_eq!(sample.cpu_percent, 0.0);
        assert_eq!(sample.mem_mb, 0.0);
    }

    #[test]
    fn sampling_the_real_table_finds_this_process() {
        let table = read_process_table();
        assert!(!table.is_empty(), "ps must return a process table");
        let me = std::process::id() as i32;
        let sample = sample_from_table(&table, me);
        assert!(
            sample.process_count >= 1,
            "this test process is in its own tree"
        );
        assert!(sample.mem_mb > 0.0, "rss is reported");
    }

    #[test]
    fn several_tasks_share_one_table_read() {
        let table = vec![row(100, 1, 1.0, 1024), row(200, 1, 2.0, 2048)];
        let samples = sample_tasks(&table, &[(100, 0), (200, 0)]);
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[&100].cpu_percent, 1.0);
        assert_eq!(samples[&200].cpu_percent, 2.0);
    }

    #[test]
    fn the_interval_never_drops_below_the_floor() {
        assert_eq!(clamp_interval(10), MIN_INTERVAL_MS);
        assert_eq!(clamp_interval(1000), 1000);
    }
}
