//! NeoHub native desktop host (macOS first).
//!
//! Responsibilities: take the single-instance lock, build the Tokio runtime, open the core
//! (database + runner + monitor), then hand GPUI a window over it.
//!
//! The window is created through `app_host::open_main_window`, which means it can be destroyed
//! and rebuilt while the core keeps running — the behaviour "close the window, keep the
//! servers up, re-open from the tray" depends on.

use gpui_kit::*;
use neohub_core::commands::CoreCommand;
use neohub_core::service::CoreService;
use neohub_desktop::app_host::{ActivationWatcher, SharedHost, open_main_window, reopen_or_focus};
use neohub_desktop::session_lock::{LockOutcome, SessionLock};
use neohub_desktop::window_session::AppHost;
use neohub_platform_macos::tray::{TrayAction, build_menu};
use neohub_platform_macos::tray_native::TrayHandle;
use neohub_storage::db::DataDir;
use neohub_ui::views::HostBridge;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Fire one command on the core without blocking the UI thread.
fn run_command(shared: &SharedHost, runtime: &Arc<tokio::runtime::Runtime>, command: CoreCommand) {
    let handle = shared.app.lock().expect("host").service().handle();
    runtime.spawn(async move {
        if let Err(error) = handle.request(command).await {
            eprintln!("[host] tray command failed: {error}");
        }
    });
}

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

        // Menu bar item. It must be created after AppKit is up (i.e. inside `run`) and on the
        // main thread; the handle keeps the status item and its menu alive for the process.
        let mut tray: Option<TrayHandle> =
            match shared.app.lock().expect("host").service().snapshot() {
                Ok(snapshot) => {
                    let menu = build_menu(&snapshot);
                    match TrayHandle::install(&menu, &menu.title) {
                        Ok(handle) => Some(handle),
                        Err(error) => {
                            eprintln!("[host] could not install the menu bar item: {error}");
                            None
                        }
                    }
                }
                Err(error) => {
                    eprintln!("[host] no snapshot for the menu bar item: {error}");
                    None
                }
            };

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

                // Menu bar clicks. Drained on the same tick as the activation file, so a
                // click never waits on the window.
                // Keep the menu in step with the core. The snapshot is cheap (cached stats and
                // ports); `refresh` is a no-op unless the rows actually changed.
                if let Some(handle) = tray.as_mut() {
                    let snapshot = shared.app.lock().expect("host").service().snapshot();
                    if let Ok(snapshot) = snapshot {
                        let menu = build_menu(&snapshot);
                        if let Err(error) = handle.refresh(&menu, &menu.title) {
                            eprintln!("[host] could not refresh the menu bar item: {error}");
                        }
                    }
                }

                if let Some(handle) = tray.as_ref() {
                    while let Some(action) = handle.try_recv() {
                        let shared = shared.clone();
                        let runtime = runtime.clone();
                        cx.update(move |cx| match action {
                            TrayAction::Open(_) => {
                                if let Err(err) = reopen_or_focus(cx, &shared) {
                                    eprintln!("[host] tray could not open the window: {err}");
                                }
                            }
                            TrayAction::StartApp(app_id) => {
                                run_command(&shared, &runtime, CoreCommand::StartApp { app_id });
                            }
                            TrayAction::StopApp(app_id) => {
                                run_command(&shared, &runtime, CoreCommand::StopApp { app_id });
                            }
                            TrayAction::StopAll => {
                                // No StopAll command exists; stop every live app through the
                                // same per-app path the UI uses.
                                let handle = shared.app.lock().expect("host").service().handle();
                                runtime.spawn(async move {
                                    if let Ok(neohub_core::commands::CoreReply::Snapshot(
                                        snapshot,
                                    )) = handle.request(CoreCommand::GetSnapshot).await
                                    {
                                        for app in snapshot.apps {
                                            let live = snapshot
                                                .app_states
                                                .get(&app.id)
                                                .map(|state| state.is_live())
                                                .unwrap_or(false);
                                            if live {
                                                let _ = handle
                                                    .request(CoreCommand::StopApp {
                                                        app_id: app.id.clone(),
                                                    })
                                                    .await;
                                            }
                                        }
                                    }
                                });
                            }
                            TrayAction::Quit => {
                                run_command(&shared, &runtime, CoreCommand::Shutdown);
                                cx.quit();
                            }
                        });
                    }
                }
            }
        })
        .detach();
    });
}
