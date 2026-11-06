//! NeoHub background core.
//!
//! The core owns the database thread, the process/PTY workers and the orchestration
//! state. GPUI types must never appear here (`native/docs/interfaces.md`).

pub mod catalog;
pub mod environment;

pub use catalog::{AppCatalog, CoreError, CoreResult, CreateAppInput, CreateTaskSpec};
