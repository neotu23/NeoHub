//! NeoHub native desktop host (macOS first).
//!
//! Responsibilities: build the Tokio runtime, open the core (database + runner + MCP later),
//! create the GPUI window, and hand the window a bridge to the core. The window is a *view*
//! over the core — destroying it never stops the core, which is what makes "close the window,
//! keep the servers running" work.

use gpui_kit::*;
use neohub_core::service::CoreService;
use neohub_storage::db::DataDir;
use neohub_ui::theme::metrics;
use neohub_ui::views::{HostBridge, ShellView};
use std::sync::Arc;

fn window_options() -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(
                px(metrics::DEFAULT_WINDOW_WIDTH / 8.0),
                px(metrics::DEFAULT_WINDOW_HEIGHT / 8.0),
            ),
            size(
                px(metrics::DEFAULT_WINDOW_WIDTH),
                px(metrics::DEFAULT_WINDOW_HEIGHT),
            ),
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

fn main() {
    // One runtime for everything: the core spawns its pumps/monitors on it, the UI sends
    // commands through it. Nothing is `block_on`-ed on the GPUI thread except the initial
    // snapshot read.
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("tokio runtime"),
    );
    let _enter = runtime.enter();

    let data_dir = DataDir::from_args(std::env::args()).expect("resolve data dir");
    let service = CoreService::open(data_dir.clone()).expect("open the NeoHub core");
    let bridge = HostBridge::new(runtime.clone(), service, data_dir.path().to_path_buf());

    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        let bridge = bridge.clone();
        gpui_kit::open_window(window_options(), cx, move |_, cx| {
            let view = cx.new(|_| ShellView::new(bridge.clone()));
            view.update(cx, |view, cx| view.start_event_pump(cx));
            view
        })
        .expect("failed to open the NeoHub window");
    });
}
