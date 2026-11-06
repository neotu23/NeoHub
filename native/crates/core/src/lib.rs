//! NeoHub background core.
//!
//! The core owns the database thread, the process/PTY workers and the orchestration
//! state. GPUI types must never appear here (`native/docs/interfaces.md`).

pub mod catalog;
pub mod commands;
pub mod database;
pub mod environment;
pub mod events;
pub mod logs;
pub mod monitor;
pub mod orchestrator;
pub mod process;
pub mod service;
pub mod snapshot;
pub mod update;
pub mod watch;

pub use catalog::{AppCatalog, CoreError, CoreResult, CreateAppInput, CreateTaskSpec};
