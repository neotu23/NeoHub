//! The main window shell: sidebar + dashboard, rendered from the core snapshot.
//!
//! Threading contract (see `native/docs/interfaces.md`): the render function only reads the
//! view model and never blocks — commands go to the core over a background Tokio runtime and
//! results come back as events, which the pump applies before the next frame.

use crate::state::session::{Session, SessionStore};
use crate::state::store::UiStore;
use crate::theme::{DARK, LIGHT, Palette, metrics};
use gpui_kit::*;
use neohub_core::commands::CoreCommand;
use neohub_core::events::CoreEvent;
use neohub_core::service::{CoreHandle, CoreService};
use neohub_domain::{AppId, ProcessState, TaskId};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Everything the view needs from the outside world.
pub struct HostBridge {
    runtime: Arc<tokio::runtime::Runtime>,
    handle: CoreHandle,
    store: Mutex<UiStore>,
    events: Mutex<std::sync::mpsc::Receiver<CoreEvent>>,
    session_store: SessionStore,
}

impl HostBridge {
    /// Wire the view to a running core: take one snapshot, then stream events.
    ///
    /// The runtime is supplied by the host so the core (which spawns its own tasks in
    /// `CoreService::open`) and the UI share exactly one reactor.
    pub fn new(
        runtime: Arc<tokio::runtime::Runtime>,
        service: Arc<CoreService>,
        data_dir: PathBuf,
    ) -> Arc<Self> {
        let session_store = SessionStore::new(&data_dir);
        let session = session_store.load();
        let handle = service.handle();

        let (tx, rx) = std::sync::mpsc::channel();
        let mut subscription = handle.subscribe();
        runtime.spawn(async move {
            while let Some(event) = subscription.recv().await {
                if tx.send(event).is_err() {
                    break;
                }
            }
        });

        let bridge = Arc::new(HostBridge {
            runtime,
            handle: handle.clone(),
            store: Mutex::new(UiStore::new(session)),
            events: Mutex::new(rx),
            session_store,
        });
        bridge.refresh_snapshot();
        bridge
    }

    /// Take a fresh snapshot synchronously (startup and resync only — it is a memory read).
    pub fn refresh_snapshot(&self) {
        let reply = self
            .runtime
            .block_on(self.handle.request(CoreCommand::GetSnapshot));
        if let Ok(neohub_core::commands::CoreReply::Snapshot(snapshot)) = reply {
            self.store
                .lock()
                .expect("store lock")
                .apply_snapshot(*snapshot);
        }
    }

    fn send(&self, command: CoreCommand) {
        let handle = self.handle.clone();
        self.runtime.spawn(async move {
            let _ = handle.request(command).await;
        });
    }

    pub fn start_task(&self, task_id: TaskId) {
        self.send(CoreCommand::StartTask { task_id });
    }

    pub fn stop_task(&self, task_id: TaskId) {
        self.send(CoreCommand::StopTask { task_id });
    }

    pub fn start_app(&self, app_id: AppId) {
        self.send(CoreCommand::StartApp { app_id });
    }

    pub fn stop_app(&self, app_id: AppId) {
        self.send(CoreCommand::StopApp { app_id });
    }

    /// Apply queued events; returns true when the visible state changed.
    fn drain(&self) -> bool {
        let events: Vec<CoreEvent> = {
            let receiver = self.events.lock().expect("events lock");
            let mut drained = Vec::new();
            while let Ok(event) = receiver.try_recv() {
                drained.push(event);
                if drained.len() >= 512 {
                    break; // bound the work per frame; the rest arrives next tick
                }
            }
            drained
        };
        if events.is_empty() {
            return false;
        }
        let mut store = self.store.lock().expect("store lock");
        let mut changed = false;
        for event in events {
            let resync = matches!(
                event.kind,
                neohub_core::events::CoreEventKind::ResyncRequired { .. }
            );
            changed |= store.apply_event(event);
            if resync {
                drop(store);
                self.refresh_snapshot();
                store = self.store.lock().expect("store lock");
            }
        }
        changed
    }

    /// Persist the navigation session after a user action.
    fn persist_session(&self) {
        let session = self.store.lock().expect("store lock").session().clone();
        if let Err(err) = self.session_store.save(&session) {
            eprintln!("[ui] could not save the session: {err}");
        }
    }

    fn with_session(&self, f: impl FnOnce(&mut Session)) {
        {
            let mut store = self.store.lock().expect("store lock");
            f(store.session_mut());
        }
        self.persist_session();
    }
}

/// The window root.
pub struct ShellView {
    bridge: Arc<HostBridge>,
}

impl ShellView {
    pub fn new(bridge: Arc<HostBridge>) -> Self {
        ShellView { bridge }
    }

    /// Spawn the event pump that turns core events into repaints.
    pub fn start_event_pump(&self, cx: &mut Context<Self>) {
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx: &mut AsyncApp| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let changed = bridge.drain();
                if !changed {
                    continue;
                }
                if this
                    .update(cx, |_, cx| {
                        cx.notify();
                    })
                    .is_err()
                {
                    break; // window gone: the core keeps running
                }
            }
        })
        .detach();
    }

    fn palette(&self) -> Palette {
        let mode = self
            .bridge
            .store
            .lock()
            .expect("store lock")
            .snapshot()
            .map(|snapshot| snapshot.settings.theme)
            .unwrap_or(neohub_domain::ThemeSetting::Dark);
        match mode {
            neohub_domain::ThemeSetting::Light => LIGHT,
            neohub_domain::ThemeSetting::Dark => DARK,
            neohub_domain::ThemeSetting::System => DARK,
        }
    }
}

fn state_color(palette: &Palette, state: ProcessState) -> Rgba {
    match state {
        ProcessState::Running => palette.success_strong,
        ProcessState::Starting | ProcessState::Exiting => palette.warn_strong,
        ProcessState::Crashed => palette.danger_strong,
        _ => palette.fg_subtle,
    }
}

fn state_label(state: ProcessState) -> &'static str {
    match state {
        ProcessState::Idle => "空闲",
        ProcessState::Starting => "启动中",
        ProcessState::Running => "运行中",
        ProcessState::Exiting => "停止中",
        ProcessState::Exited => "已停止",
        ProcessState::Crashed => "已崩溃",
    }
}

impl Render for ShellView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.palette();
        let (sections, cards, totals, tags) = {
            let store = self.bridge.store.lock().expect("store lock");
            (
                store.sidebar_sections(),
                store.dashboard_cards(),
                store.totals(),
                store.all_tags(),
            )
        };
        let (view, selected) = {
            let store = self.bridge.store.lock().expect("store lock");
            (
                store.session().view,
                store.session().selected_app_id.clone(),
            )
        };

        let mut sidebar = div()
            .flex()
            .flex_col()
            .w(px(metrics::SIDEBAR_WIDTH))
            .h_full()
            .bg(palette.bg_surface)
            .border_r_1()
            .border_color(palette.border)
            .p(px(metrics::PAD_3))
            .gap(px(metrics::GAP_2));

        sidebar = sidebar.child(
            div()
                .text_size(px(metrics::TEXT_XS))
                .text_color(palette.fg_subtle)
                .child("NEOHUB"),
        );

        // Dashboard entry.
        {
            let bridge = self.bridge.clone();
            sidebar = sidebar.child(
                div()
                    .id("nav-dashboard")
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(metrics::PAD_2))
                    .py(px(metrics::GAP_1))
                    .rounded(px(metrics::RADIUS_MD))
                    .bg(if view == crate::state::session::View::Dashboard {
                        palette.bg_elevated
                    } else {
                        palette.bg_surface
                    })
                    .text_size(px(metrics::TEXT_BASE))
                    .text_color(palette.fg)
                    .cursor_pointer()
                    .child("仪表盘")
                    .child(
                        div()
                            .text_size(px(metrics::TEXT_XS))
                            .text_color(palette.fg_subtle)
                            .child(format!("{}", totals.running)),
                    )
                    .on_click(move |_, _, _| {
                        bridge.with_session(|session| session.go_dashboard());
                    }),
            );
        }

        for section in &sections {
            sidebar = sidebar.child(
                div()
                    .mt(px(metrics::GAP_2))
                    .text_size(px(metrics::TEXT_XS))
                    .text_color(palette.fg_subtle)
                    .child(section.title.clone()),
            );
            for row in &section.apps {
                let bridge = self.bridge.clone();
                let app_id = row.app_id.clone();
                let is_selected = selected.as_ref() == Some(&row.app_id);
                sidebar = sidebar.child(
                    div()
                        .id(format!("nav-app-{}", row.app_id))
                        .flex()
                        .items_center()
                        .gap(px(metrics::GAP_2))
                        .px(px(metrics::PAD_2))
                        .py(px(metrics::GAP_1))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(if is_selected {
                            palette.bg_elevated
                        } else {
                            palette.bg_surface
                        })
                        .cursor_pointer()
                        .child(
                            div()
                                .w(px(8.0))
                                .h(px(8.0))
                                .rounded_full()
                                .bg(state_color(&palette, row.state)),
                        )
                        .child(
                            div()
                                .text_size(px(metrics::TEXT_BASE))
                                .text_color(palette.fg)
                                .child(row.name.clone()),
                        )
                        .on_click(move |_, _, _| {
                            bridge.with_session(|session| session.select_app(app_id.clone(), None));
                        }),
                );
            }
        }

        // Main pane.
        let mut main = div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .bg(palette.bg_base)
            .text_color(palette.fg)
            .p(px(metrics::PAD_6))
            .gap(px(metrics::GAP_3));

        main = main.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(div().text_size(px(metrics::TEXT_LG)).child(
                    if view == crate::state::session::View::Dashboard {
                        "仪表盘"
                    } else {
                        "项目"
                    },
                ))
                .child(
                    div()
                        .flex()
                        .gap(px(metrics::GAP_3))
                        .text_size(px(metrics::TEXT_SM))
                        .text_color(palette.fg_muted)
                        .child(format!("运行中 {} / {}", totals.running, totals.apps))
                        .child(format!("CPU {:.1}%", totals.cpu_percent))
                        .child(format!("内存 {:.0} MB", totals.mem_mb))
                        .child(format!("端口 {}", totals.ports)),
                ),
        );

        if !tags.is_empty() {
            let mut tag_row = div().flex().gap(px(metrics::GAP_1));
            for tag in tags.iter().take(8) {
                tag_row = tag_row.child(
                    div()
                        .px(px(metrics::PAD_2))
                        .py(px(1.0))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(palette.bg_surface)
                        .text_size(px(metrics::TEXT_XS))
                        .text_color(palette.fg_muted)
                        .child(format!("#{tag}")),
                );
            }
            main = main.child(tag_row);
        }

        let mut grid = div().flex().flex_wrap().gap(px(metrics::GAP_3));
        for card in cards {
            let bridge = self.bridge.clone();
            let app_id = card.app_id.clone();
            let running = card.state.is_live();
            let mut card_view = div()
                .id(format!("card-{}", card.app_id))
                .flex()
                .flex_col()
                .gap(px(metrics::GAP_2))
                .w(px(280.0))
                .p(px(metrics::PAD_4))
                .rounded(px(metrics::RADIUS_LG))
                .border_1()
                .border_color(if running {
                    palette.success_border
                } else {
                    palette.border
                })
                .bg(palette.bg_surface)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(metrics::GAP_2))
                                .child(
                                    div()
                                        .w(px(8.0))
                                        .h(px(8.0))
                                        .rounded_full()
                                        .bg(state_color(&palette, card.state)),
                                )
                                .child(
                                    div()
                                        .text_size(px(metrics::TEXT_BASE))
                                        .child(card.name.clone()),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(metrics::TEXT_XS))
                                .text_color(state_color(&palette, card.state))
                                .child(state_label(card.state)),
                        ),
                )
                .child(
                    div()
                        .text_size(px(metrics::TEXT_XS))
                        .text_color(palette.fg_subtle)
                        .child(format!(
                            "{} 个任务 · {} 运行中",
                            card.total_tasks, card.running_tasks
                        )),
                )
                .child(
                    div()
                        .text_size(px(metrics::TEXT_XS))
                        .text_color(palette.fg_muted)
                        .child(format!(
                            "CPU {:.1}%   {:.0} MB",
                            card.cpu_percent, card.mem_mb
                        )),
                );

            if !card.ports.is_empty() {
                let mut ports = div().flex().gap(px(metrics::GAP_1));
                for port in &card.ports {
                    ports = ports.child(
                        div()
                            .px(px(metrics::PAD_2))
                            .py(px(1.0))
                            .rounded(px(metrics::RADIUS_MD))
                            .bg(palette.bg_elevated)
                            .text_size(px(metrics::TEXT_XS))
                            .text_color(palette.accent)
                            .child(format!(":{port}")),
                    );
                }
                card_view = card_view.child(ports);
            }

            let toggle_bridge = bridge.clone();
            let entity = cx.entity().downgrade();
            card_view = card_view.child(
                div()
                    .id(format!("toggle-{}", card.app_id))
                    .px(px(metrics::PAD_3))
                    .py(px(metrics::GAP_1))
                    .rounded(px(metrics::RADIUS_MD))
                    .bg(if running {
                        palette.danger_bg
                    } else {
                        palette.bg_elevated
                    })
                    .text_size(px(metrics::TEXT_SM))
                    .text_color(if running {
                        palette.danger_fg
                    } else {
                        palette.fg
                    })
                    .cursor_pointer()
                    .child(if running { "停止" } else { "启动" })
                    .on_click(move |_, _, cx| {
                        if running {
                            toggle_bridge.stop_app(app_id.clone());
                        } else {
                            toggle_bridge.start_app(app_id.clone());
                        }
                        let _ = entity.update(cx, |_, cx| cx.notify());
                    }),
            );

            grid = grid.child(card_view);
        }
        main = main.child(grid);

        div()
            .flex()
            .size_full()
            .bg(palette.bg_base)
            .child(sidebar)
            .child(main)
    }
}
