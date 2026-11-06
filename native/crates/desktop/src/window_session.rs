//! Window lifetime, independent of GPUI.
//!
//! The rule the whole product rests on: **closing the window never stops the core**. This
//! module decides what a close means (hide the window, keep the process; or quit), what a
//! re-open restores, and refuses to close when an edit could not be saved.
//!
//! Everything here is plain state so it can be tested without a window: the GPUI layer only
//! calls `request_close`, then either drops the window or shows the error.

use neohub_core::service::CoreService;
use neohub_ui::state::SaveBarrier;
use neohub_ui::state::session::SessionStore;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// What the window should do after a close request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CloseDecision {
    /// Everything is saved: destroy the window. The core keeps running.
    HideWindow,
    /// A save is still in flight; the caller waits for the core, then closes.
    FlushFirst,
    /// Saves failed: keep the window and show these errors.
    KeepWindow(Vec<String>),
}

/// What a re-open has to restore.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReopenPlan {
    /// Restore the sidebar/detail selection from the session file.
    pub restore_selection: bool,
    /// Restore the last window bounds.
    pub restore_bounds: bool,
    /// Never re-run autoStart when re-opening (the core already has the truth).
    pub run_auto_start: bool,
}

/// Window + tray + dock state for one process.
pub struct AppHost {
    service: Arc<CoreService>,
    session_store: SessionStore,
    barrier: SaveBarrier,
    window_open: bool,
    /// Tray is enabled: closing hides the Dock and the window can be re-opened from the menu.
    tray_enabled: bool,
    /// True while the Dock icon is hidden because the window is closed.
    dock_hidden: bool,
    /// How many windows have been created in this process (growth check for tests).
    window_generations: u64,
    /// Bounds captured on the last close.
    last_bounds: Option<(f64, f64, f64, f64)>,
    quitting: bool,
}

impl AppHost {
    pub fn new(service: Arc<CoreService>, data_dir: &Path) -> Self {
        AppHost {
            service,
            session_store: SessionStore::new(data_dir),
            barrier: SaveBarrier::new(),
            window_open: true,
            tray_enabled: true,
            dock_hidden: false,
            window_generations: 1,
            last_bounds: None,
            quitting: false,
        }
    }

    pub fn service(&self) -> &Arc<CoreService> {
        &self.service
    }

    pub fn window_open(&self) -> bool {
        self.window_open
    }

    pub fn dock_hidden(&self) -> bool {
        self.dock_hidden
    }

    pub fn window_generations(&self) -> u64 {
        self.window_generations
    }

    pub fn tray_enabled(&self) -> bool {
        self.tray_enabled
    }

    pub fn set_tray_enabled(&mut self, enabled: bool) {
        self.tray_enabled = enabled;
        self.reconcile_dock();
    }

    pub fn barrier_mut(&mut self) -> &mut SaveBarrier {
        &mut self.barrier
    }

    pub fn data_dir_for_session(&self) -> PathBuf {
        self.session_store
            .path()
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default()
    }

    /// The user asked to close the window (red button or ⌘W).
    ///
    /// Never shuts the core down: that only happens on an explicit quit.
    pub fn request_close(&mut self) -> CloseDecision {
        if !self.barrier.failures().is_empty() {
            return CloseDecision::KeepWindow(self.barrier.failures().values().cloned().collect());
        }
        if self.window_open {
            // The window layer flushes its editors first; this call is the gate, and the
            // window is only destroyed once it reports success.
            return CloseDecision::FlushFirst;
        }
        CloseDecision::HideWindow
    }

    /// Called after the save barrier reported success (or when there was nothing to save).
    pub fn finish_close(&mut self, bounds: Option<(f64, f64, f64, f64)>) {
        self.window_open = false;
        self.last_bounds = bounds;
        self.reconcile_dock();
    }

    /// Re-open from the tray, Finder or a second launch.
    pub fn reopen(&mut self) -> ReopenPlan {
        if !self.window_open {
            self.window_generations += 1;
        }
        self.window_open = true;
        self.reconcile_dock();
        ReopenPlan {
            restore_selection: true,
            restore_bounds: self.last_bounds.is_some(),
            // Re-opening must not start anything: the core already owns the process state.
            run_auto_start: false,
        }
    }

    pub fn last_bounds(&self) -> Option<(f64, f64, f64, f64)> {
        self.last_bounds
    }

    /// The mac goes to sleep with the last window closed: the Dock comes back so the app is
    /// reachable, unless the tray is there to hold it.
    fn reconcile_dock(&mut self) {
        self.dock_hidden = !self.window_open && self.tray_enabled;
    }

    /// An explicit quit (⌘Q or the tray's Quit): the core stops everything first.
    pub async fn quit(&mut self) -> Result<(), String> {
        self.quitting = true;
        // Closing the window is part of quitting, but the reverse is not true.
        self.window_open = false;
        self.dock_hidden = false;
        self.service.shutdown().await.map_err(|err| err.to_string())
    }

    pub fn is_quitting(&self) -> bool {
        self.quitting
    }
}

#[cfg(feature = "test-support")]
mod test_support {
    // Reserved for a headless window double; the lifecycle tests use plain state instead.
}
