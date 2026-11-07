//! Keeping one bad render from taking the window down.
//!
//! The UI builds its element tree from data that comes from elsewhere: a task's terminal grid, a
//! dashboard card computed from a snapshot. A panic in that arithmetic takes the whole window with
//! it, and the user loses every other pane over one project's odd state.
//!
//! What this does is narrow: it wraps the *computations* that feed the view — not the whole render
//! — in `catch_unwind`. A panic there yields `None`, the pane renders empty, and the panic goes to
//! the app log so it can be diagnosed. It deliberately does not try to catch a panic thrown while
//! GPUI itself is drawing: that is inside the framework's frame, where unwinding is not ours to
//! manage.
//!
//! This is not a substitute for not panicking. It is the difference between "one pane is blank
//! and there is a log line" and "the window vanished".

use std::panic::{AssertUnwindSafe, catch_unwind};

/// Run `f`, turning a panic into `None`.
///
/// `what` names the computation for the log line; keep it short and specific ("dashboard cards",
/// "terminal rows") so the entry points somewhere.
pub fn guard<T>(what: &str, f: impl FnOnce() -> T) -> Option<T> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => Some(value),
        Err(payload) => {
            let message = panic_message(&payload);
            // The log file is the only place this can be read later; stderr is gone when the app
            // was launched from Finder.
            neohub_platform_macos::log_file::log(
                "ui",
                &format!("render guard caught a panic in {what}: {message}"),
            );
            eprintln!("[ui] render guard caught a panic in {what}: {message}");
            None
        }
    }
}

/// Best-effort text from a panic payload.
fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_normal_computation_passes_through() {
        assert_eq!(guard("arithmetic", || 2 + 2), Some(4));
    }

    #[test]
    fn a_panic_becomes_none_instead_of_taking_the_caller_down() {
        // This is the property the module exists for: the caller keeps running.
        let result = guard::<i32>("deliberate", || panic!("boom"));
        assert!(result.is_none());
    }

    #[test]
    fn the_payload_is_reported_for_both_forms() {
        // `panic!("literal")` and `panic!("{}", value)` produce different payload types, and a
        // log line that says "non-string panic payload" for one of them would be useless.
        let literal = catch_unwind(|| panic!("literal message")).unwrap_err();
        assert_eq!(panic_message(&literal), "literal message");

        let formatted = catch_unwind(|| panic!("value {}", 7)).unwrap_err();
        assert_eq!(panic_message(&formatted), "value 7");
    }

    #[test]
    fn a_panic_inside_the_guard_does_not_lose_the_next_call() {
        // A caught panic must not poison anything: the next computation still runs.
        assert!(guard::<i32>("first", || panic!("boom")).is_none());
        assert_eq!(guard("second", || 1), Some(1));
    }
}
