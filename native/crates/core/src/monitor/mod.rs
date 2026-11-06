//! Monitoring: CPU/memory sampling and listening-port discovery.

pub mod ports;
pub mod stats;

pub use ports::{HINT_TTL_SWEEPS, PortSample, PortTracker, extract_port_hints, listening_ports_of};
pub use stats::{
    DEFAULT_INTERVAL_MS, MIN_INTERVAL_MS, ProcRow, StatsSample, clamp_interval, read_process_table,
    sample_from_table, sample_tasks,
};
