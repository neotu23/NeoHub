//! NeoHub native desktop host (macOS first).
//!
//! Responsibilities: take the single-instance lock, build the Tokio runtime, open the core
//! (database + runner + monitor), then hand GPUI a window over it.
//!
//! The window is created through `app_host::open_main_window`, which means it can be destroyed
//! and rebuilt while the core keeps running — the behaviour "close the window, keep the
//! servers up, re-open from the tray" depends on.

use gpui_kit::*;
use neohub_core::service::CoreService;
use neohub_desktop::app_host::{ActivationWatcher, SharedHost, open_main_window, reopen_or_focus};
use neohub_desktop::session_lock::{LockOutcome, SessionLock};
use neohub_desktop::window_session::AppHost;
use neohub_storage::db::DataDir;
use neohub_ui::views::HostBridge;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn main() {
    let data_dir = DataDir::from_args(std::env::args()).expect("resolve data dir");

    // Only one NeoHub may own the database. A second launch just raises the first window.
    let _lock = match SessionLock::acquire(data_dir.path()).expect("session lock") {
        LockOutcome::Acquired(lock) => lock,
        LockOutcome::AlreadyRunning { activation_file } => {
            println!(
                "NeoHub is already running; activation requested via {}",
                activation_file.display()
            );
            return;
        }
    };

    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("tokio runtime"),
    );
    let _enter = runtime.enter();

    let service = CoreService::open(data_dir.clone()).expect("open the NeoHub core");
    let host = Arc::new(Mutex::new(AppHost::new(service.clone(), data_dir.path())));
    let bridge = HostBridge::new(runtime.clone(), service, data_dir.path().to_path_buf());
    let shared = SharedHost::new(host, bridge);
    let mut watcher = ActivationWatcher::new(data_dir.path());

    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        open_main_window(cx, &shared).expect("open the NeoHub window");

        // Another launch (or, later, the tray) asks us to show the window again. The poll is
        // cheap: one file timestamp every 500 ms, no shell, no database.
        cx.spawn(async move |cx: &mut AsyncApp| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                if watcher.poll() {
                    let shared = shared.clone();
                    cx.update(move |cx| {
                        if let Err(err) = reopen_or_focus(cx, &shared) {
                            eprintln!("[host] could not re-open the window: {err}");
                        }
                    });
                }
            }
        })
        .detach();
    });
}
