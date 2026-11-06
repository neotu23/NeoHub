//! NeoHub desktop host: window lifecycle, single-instance lock and platform wiring.
//!
//! The binary is a thin wrapper; everything testable lives here.

pub mod session_lock;
pub mod window_session;

pub use session_lock::{LockOutcome, SessionLock, activation_path, activation_stamp};
pub use window_session::{AppHost, CloseDecision, ReopenPlan};
