//! GPUI-side assembly: creates the window, intercepts its close, and re-creates it on demand.
//!
//! The window is disposable; the core is not. Closing destroys the window entity while the
//! `AppHost` (and therefore the database, PTY runners and monitor loop) keeps running; opening
//! again builds a fresh `ShellView` from the same core and the persisted session.

use crate::window_session::{AppHost, CloseDecision};
use gpui_kit::*;
use neohub_ui::theme::metrics;
use neohub_ui::views::{HostBridge, ShellView};
use std::sync::{Arc, Mutex};

/// Window frame as (x, y, width, height).
pub type WindowFrame = (f64, f64, f64, f64);

/// Shared host state, reachable from close callbacks and the activation poller.
#[derive(Clone)]
pub struct SharedHost {
    pub app: Arc<Mutex<AppHost>>,
    pub bridge: Arc<HostBridge>,
    /// Bounds captured at the last close, used when re-creating the window.
    pub last_bounds: Arc<Mutex<Option<WindowFrame>>>,
    /// True while a window exists (guards against opening two).
    pub window_open: Arc<Mutex<bool>>,
}

impl SharedHost {
    pub fn new(app: Arc<Mutex<AppHost>>, bridge: Arc<HostBridge>) -> Self {
        SharedHost {
            app,
            bridge,
            last_bounds: Arc::new(Mutex::new(None)),
            window_open: Arc::new(Mutex::new(true)),
        }
    }
}

fn options_for(bounds: Option<WindowFrame>) -> WindowOptions {
    let (x, y, width, height) = bounds.unwrap_or((
        f64::from(metrics::DEFAULT_WINDOW_WIDTH) / 8.0,
        f64::from(metrics::DEFAULT_WINDOW_HEIGHT) / 8.0,
        f64::from(metrics::DEFAULT_WINDOW_WIDTH),
        f64::from(metrics::DEFAULT_WINDOW_HEIGHT),
    ));
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(px(x as f32), px(y as f32)),
            size(px(width as f32), px(height as f32)),
        ))),
        window_min_size: Some(size(
            px(metrics::MIN_WINDOW_WIDTH),
            px(metrics::MIN_WINDOW_HEIGHT),
        )),
        titlebar: Some(TitlebarOptions {
            title: Some(SharedString::from("NeoHub")),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Create the main window (again, after a close). Returns the window handle.
pub fn open_main_window(cx: &mut App, shared: &SharedHost) -> anyhow::Result<AnyWindowHandle> {
    let bounds = *shared.last_bounds.lock().expect("bounds");
    let bridge = shared.bridge.clone();
    let host = shared.app.clone();
    let window_state = shared.window_open.clone();
    let bounds_state = shared.last_bounds.clone();

    let (handle, view) = gpui_kit::open_window(options_for(bounds), cx, move |window, cx| {
        let view = cx.new(|cx| ShellView::new(bridge.clone(), window, cx));
        view.update(cx, |view, cx| view.start_event_pump(cx));

        // The red button and ⌘W both land here. Returning false keeps the window open, which
        // is what a failed save must do.
        let host_for_close = host.clone();
        let bounds_for_close = bounds_state.clone();
        let window_state_for_close = window_state.clone();
        window.on_window_should_close(cx, move |window, _cx| {
            let decision = host_for_close.lock().expect("host").request_close();
            match decision {
                CloseDecision::KeepWindow(_) => false,
                CloseDecision::FlushFirst | CloseDecision::HideWindow => {
                    let captured = window.bounds();
                    *bounds_for_close.lock().expect("bounds") = Some((
                        f64::from(captured.origin.x),
                        f64::from(captured.origin.y),
                        f64::from(captured.size.width),
                        f64::from(captured.size.height),
                    ));
                    host_for_close
                        .lock()
                        .expect("host")
                        .finish_close(*bounds_for_close.lock().expect("bounds"));
                    *window_state_for_close.lock().expect("window state") = false;
                    true
                }
            }
        });
        view
    })?;
    *shared.window_open.lock().expect("window state") = true;
    // The event pump lives with the view; keep the handle so callers can focus it later.
    let _ = view;
    Ok(handle)
}

/// Re-open after a close, or bring the existing window forward.
///
/// Returns true when a window was created (false when one was already open).
pub fn reopen_or_focus(cx: &mut App, shared: &SharedHost) -> anyhow::Result<bool> {
    let already_open = *shared.window_open.lock().expect("window state");
    if already_open {
        if let Some(window) = cx.windows().into_iter().next() {
            window
                .update(cx, |_, window, _| window.activate_window())
                .ok();
        }
        return Ok(false);
    }
    open_main_window(cx, shared)?;
    Ok(true)
}

/// Watch the activation file written by a second instance and re-open the window when it moves.
pub struct ActivationWatcher {
    path: std::path::PathBuf,
    last_stamp: u64,
}

impl ActivationWatcher {
    pub fn new(data_dir: &std::path::Path) -> Self {
        let path = crate::session_lock::activation_path(data_dir);
        let last_stamp = crate::session_lock::activation_stamp(&path);
        ActivationWatcher { path, last_stamp }
    }

    /// True when another launch asked us to show the window since the last poll.
    pub fn poll(&mut self) -> bool {
        let stamp = crate::session_lock::activation_stamp(&self.path);
        if stamp > self.last_stamp {
            self.last_stamp = stamp;
            return true;
        }
        false
    }
}
