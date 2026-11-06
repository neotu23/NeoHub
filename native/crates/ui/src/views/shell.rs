//! The main window shell: sidebar + dashboard, rendered from the core snapshot.
//!
//! Threading contract (see `native/docs/interfaces.md`): the render function only reads the
//! view model and never blocks — commands go to the core over a background Tokio runtime and
//! results come back as events, which the pump applies before the next frame.

use crate::state::session::{Session, SessionStore};
use crate::state::store::UiStore;
use crate::state::text_input::{KeyOutcome, TextField};
use crate::theme::{DARK, LIGHT, Palette, metrics};
use gpui_kit::*;
use neohub_core::commands::{CoreCommand, CoreReply};
use neohub_core::environment::ImportCandidate;
use neohub_core::events::CoreEvent;
use neohub_core::service::{CoreHandle, CoreService};
use neohub_domain::{AppId, ProcessState, TaskId};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Messages from the core to the window: streamed events plus command replies.
enum HostUpdate {
    Event(CoreEvent),
    Candidates(Vec<ImportCandidate>),
    Imported,
    Error(String),
}

/// Everything the view needs from the outside world.
pub struct HostBridge {
    runtime: Arc<tokio::runtime::Runtime>,
    handle: CoreHandle,
    store: Mutex<UiStore>,
    updates: Mutex<std::sync::mpsc::Receiver<HostUpdate>>,
    /// Command replies are pushed onto the same channel the event stream uses.
    reply_sink: Arc<std::sync::mpsc::Sender<HostUpdate>>,
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
        let event_tx = tx.clone();
        runtime.spawn(async move {
            while let Some(event) = subscription.recv().await {
                if event_tx.send(HostUpdate::Event(event)).is_err() {
                    break;
                }
            }
        });
        let command_tx = tx.clone();
        let reply_sink = Arc::new(command_tx);

        let bridge = Arc::new(HostBridge {
            runtime,
            handle: handle.clone(),
            store: Mutex::new(UiStore::new(session)),
            updates: Mutex::new(rx),
            reply_sink,
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

    /// Where the import panel looks by default: `--import-dir`-style override, else `~/Code`.
    pub fn default_import_dir() -> String {
        if let Ok(dir) = std::env::var("NEOHUB_IMPORT_DIR")
            && !dir.trim().is_empty()
        {
            return dir;
        }
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
        format!("{home}/Code")
    }

    /// Open the import panel and immediately scan the default folder.
    pub fn begin_import(&self, dir: String) {
        self.store
            .lock()
            .expect("store lock")
            .open_import(dir.clone());
        self.scan_folder(dir);
    }

    /// Scan a folder for importable projects.
    pub fn scan_folder(&self, dir: String) {
        let handle = self.handle.clone();
        let sink = self.reply_sink.clone();
        self.runtime.spawn(async move {
            let reply = handle
                .request(CoreCommand::ScanFolder { dir: dir.into() })
                .await;
            let message = match reply {
                Ok(CoreReply::ImportCandidates(candidates)) => HostUpdate::Candidates(candidates),
                Ok(other) => HostUpdate::Error(format!("unexpected scan reply: {other:?}")),
                Err(err) => HostUpdate::Error(format!("scan failed: {err}")),
            };
            let _ = sink.send(message);
        });
    }

    /// Import one scanned candidate using the detection the scan already produced.
    pub fn import_candidate(&self, candidate: ImportCandidate) {
        let name = candidate.name.clone();
        let input = neohub_core::catalog::CreateAppInput {
            path: candidate.path.clone(),
            name: Some(candidate.name.clone()),
            node_version_pref: None,
            package_manager: candidate.package_manager,
            default_script: candidate.suggested_script.clone(),
            first_task: candidate.suggested_script.clone().map(|script| {
                neohub_core::catalog::CreateTaskSpec {
                    name: script.clone(),
                    command_kind: Some(neohub_domain::CommandKind::Script),
                    script: Some(script),
                    custom_command: None,
                    working_dir_override: None,
                }
            }),
            tasks: vec![],
            env_vars: vec![],
        };
        let handle = self.handle.clone();
        let sink = self.reply_sink.clone();
        self.runtime.spawn(async move {
            let reply = handle
                .request(CoreCommand::CreateApp {
                    input: Box::new(input),
                })
                .await;
            let message = match reply {
                Ok(CoreReply::App(_)) => HostUpdate::Imported,
                Ok(other) => HostUpdate::Error(format!("unexpected import reply: {other:?}")),
                Err(err) => HostUpdate::Error(format!("import {name} failed: {err}")),
            };
            let _ = sink.send(message);
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

    /// Apply queued updates; returns true when the visible state changed.
    fn drain(&self) -> bool {
        let updates: Vec<HostUpdate> = {
            let receiver = self.updates.lock().expect("updates lock");
            let mut drained = Vec::new();
            while let Ok(update) = receiver.try_recv() {
                drained.push(update);
                if drained.len() >= 512 {
                    break; // bound the work per frame; the rest arrives next tick
                }
            }
            drained
        };
        if updates.is_empty() {
            return false;
        }
        let mut changed = false;
        let mut need_snapshot = false;
        {
            let mut store = self.store.lock().expect("store lock");
            for update in updates {
                match update {
                    HostUpdate::Event(event) => {
                        let resync = matches!(
                            event.kind,
                            neohub_core::events::CoreEventKind::ResyncRequired { .. }
                        );
                        changed |= store.apply_event(event);
                        need_snapshot |= resync;
                    }
                    HostUpdate::Candidates(candidates) => {
                        store.set_import_candidates(candidates);
                        changed = true;
                    }
                    HostUpdate::Imported => {
                        store.set_import_busy(false);
                        need_snapshot = true;
                        changed = true;
                    }
                    HostUpdate::Error(message) => {
                        store.set_import_error(message);
                        changed = true;
                    }
                }
            }
        }
        if need_snapshot {
            // Taken outside the store lock: `refresh_snapshot` re-locks it.
            self.refresh_snapshot();
            changed = true;
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
    /// Sidebar filter (the ⌘P search box).
    search: TextField,
    search_focus: FocusHandle,
}

impl ShellView {
    pub fn new(bridge: Arc<HostBridge>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search_focus = cx.focus_handle();
        ShellView {
            bridge,
            search: TextField::new(""),
            search_focus,
        }
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

/// Zero-sized filler so the empty state can keep the same flex layout.
fn grid_placeholder(_palette: &Palette) -> impl IntoElement {
    div().flex().gap(px(0.0))
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
        let query = self.search.text().to_string();
        let (sections, cards, totals, tags, filter) = {
            let mut store = self.bridge.store.lock().expect("store lock");
            if store.filter() != query {
                store.set_filter(query.clone());
            }
            (
                store.sidebar_sections(),
                store.dashboard_cards(),
                store.totals(),
                store.all_tags(),
                store.filter().to_string(),
            )
        };
        let (view, selected, is_empty, import) = {
            let store = self.bridge.store.lock().expect("store lock");
            (
                store.session().view,
                store.session().selected_app_id.clone(),
                store.is_empty(),
                store.import().clone(),
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
        sidebar = sidebar.child(
            div().flex().flex_col().gap(px(metrics::GAP_1)).child(
                div()
                    .id("sidebar-filter")
                    .track_focus(&self.search_focus)
                    .px(px(metrics::PAD_2))
                    .py(px(metrics::GAP_1))
                    .rounded(px(metrics::RADIUS_MD))
                    .border_1()
                    .border_color(palette.border)
                    .bg(palette.bg_base)
                    .text_size(px(metrics::TEXT_SM))
                    .text_color(if query.is_empty() {
                        palette.fg_subtle
                    } else {
                        palette.fg
                    })
                    .child(if query.is_empty() {
                        "筛选应用（⌘P）".to_string()
                    } else {
                        filter.clone()
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        window.focus(&this.search_focus, cx);
                        cx.notify();
                    }))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        let keystroke = &event.keystroke;
                        let characters = keystroke.key_char.as_deref();
                        match this.search.apply_key(&keystroke.key, characters) {
                            KeyOutcome::Changed => cx.notify(),
                            KeyOutcome::Ignored => {}
                            KeyOutcome::Escape => {
                                this.search.clear();
                                cx.notify();
                            }
                            KeyOutcome::Submit => {}
                        }
                    })),
            ),
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
                )
                .child({
                    let bridge = self.bridge.clone();
                    let entity = cx.entity().downgrade();
                    div()
                        .id("nav-import")
                        .px(px(metrics::PAD_3))
                        .py(px(metrics::GAP_1))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(palette.bg_elevated)
                        .text_size(px(metrics::TEXT_SM))
                        .cursor_pointer()
                        .child("+ 导入项目")
                        .on_click(move |_, _, cx| {
                            bridge.begin_import(HostBridge::default_import_dir());
                            let _ = entity.update(cx, |_, cx| cx.notify());
                        })
                }),
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

        if import.open {
            let mut panel = div()
                .flex()
                .flex_col()
                .gap(px(metrics::GAP_2))
                .p(px(metrics::PAD_4))
                .rounded(px(metrics::RADIUS_LG))
                .border_1()
                .border_color(palette.border)
                .bg(palette.bg_surface)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(div().text_size(px(metrics::TEXT_BASE)).child("扫描导入"))
                        .child({
                            let bridge = self.bridge.clone();
                            let entity = cx.entity().downgrade();
                            div()
                                .id("import-close")
                                .px(px(metrics::PAD_2))
                                .rounded(px(metrics::RADIUS_MD))
                                .bg(palette.bg_elevated)
                                .text_size(px(metrics::TEXT_XS))
                                .cursor_pointer()
                                .child("关闭")
                                .on_click(move |_, _, cx| {
                                    bridge.store.lock().expect("store lock").close_import();
                                    let _ = entity.update(cx, |_, cx| cx.notify());
                                })
                        }),
                )
                .child(
                    div()
                        .text_size(px(metrics::TEXT_XS))
                        .text_color(palette.fg_subtle)
                        .child(format!("目录：{}", import.dir)),
                );

            if let Some(error) = import.error.clone() {
                panel = panel.child(
                    div()
                        .px(px(metrics::PAD_3))
                        .py(px(metrics::GAP_1))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(palette.danger_bg)
                        .text_size(px(metrics::TEXT_XS))
                        .text_color(palette.danger_fg)
                        .child(error),
                );
            }
            if import.busy {
                panel = panel.child(
                    div()
                        .text_size(px(metrics::TEXT_SM))
                        .text_color(palette.fg_muted)
                        .child("正在扫描…"),
                );
            } else if import.candidates.is_empty() {
                panel = panel.child(
                    div()
                        .text_size(px(metrics::TEXT_SM))
                        .text_color(palette.fg_muted)
                        .child("这个目录下没有找到含 package.json 的项目。"),
                );
            }

            for candidate in &import.candidates {
                let bridge = self.bridge.clone();
                let entity = cx.entity().downgrade();
                let candidate = candidate.clone();
                let registered = candidate.already_registered;
                panel = panel.child(
                    div()
                        .id(format!("candidate-{}", candidate.path))
                        .flex()
                        .items_center()
                        .justify_between()
                        .px(px(metrics::PAD_3))
                        .py(px(metrics::GAP_1))
                        .rounded(px(metrics::RADIUS_MD))
                        .bg(palette.bg_elevated)
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(metrics::TEXT_SM))
                                        .child(candidate.name.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(metrics::TEXT_XS))
                                        .text_color(palette.fg_subtle)
                                        .child(candidate.path.clone()),
                                ),
                        )
                        .child(
                            div()
                                .id(format!("candidate-add-{}", candidate.path))
                                .px(px(metrics::PAD_2))
                                .rounded(px(metrics::RADIUS_MD))
                                .bg(if registered {
                                    palette.bg_surface
                                } else {
                                    palette.accent
                                })
                                .text_size(px(metrics::TEXT_XS))
                                .text_color(if registered {
                                    palette.fg_subtle
                                } else {
                                    palette.accent_fg
                                })
                                .cursor_pointer()
                                .child(if registered { "已导入" } else { "添加" })
                                .on_click(move |_, _, cx| {
                                    if !registered {
                                        bridge
                                            .store
                                            .lock()
                                            .expect("store lock")
                                            .set_import_busy(true);
                                        bridge.import_candidate(candidate.clone());
                                    }
                                    let _ = entity.update(cx, |_, cx| cx.notify());
                                }),
                        ),
                );
            }
            main = main.child(panel);
        } else if is_empty {
            let bridge = self.bridge.clone();
            let entity = cx.entity().downgrade();
            main = main.child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .flex_1()
                    .gap(px(metrics::GAP_3))
                    .child(
                        div()
                            .text_size(px(metrics::TEXT_LG))
                            .child("欢迎使用 NeoHub"),
                    )
                    .child(
                        div()
                            .text_size(px(metrics::TEXT_SM))
                            .text_color(palette.fg_muted)
                            .child("还没有项目。扫描一个目录，把本地 Node 项目导进来。"),
                    )
                    .child(
                        div()
                            .id("empty-import")
                            .px(px(metrics::PAD_4))
                            .py(px(metrics::GAP_2))
                            .rounded(px(metrics::RADIUS_MD))
                            .bg(palette.accent)
                            .text_color(palette.accent_fg)
                            .text_size(px(metrics::TEXT_SM))
                            .cursor_pointer()
                            .child("扫描文件夹导入")
                            .on_click(move |_, _, cx| {
                                bridge.begin_import(HostBridge::default_import_dir());
                                let _ = entity.update(cx, |_, cx| cx.notify());
                            }),
                    ),
            );
            main = main.child(grid_placeholder(&palette));
        } else {
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
        }

        div()
            .flex()
            .size_full()
            .bg(palette.bg_base)
            .child(sidebar)
            .child(main)
    }
}
