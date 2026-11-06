//! Menubar menu model.
//!
//! The AppKit `NSStatusItem` binding needs a target/action object and must be created on the
//! main thread, so it is kept in the desktop layer. What lives here is the *model*: given a
//! core snapshot, which items does the menu show and what do they do? Keeping it separate
//! means the menu's rules ("Stop all" only when something runs, one row per app, a stable
//! order) are testable without a status bar.

use neohub_core::snapshot::CoreSnapshot;
use neohub_domain::{AppId, ProcessState};

/// What a menu item does when clicked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayAction {
    /// Show (or re-create) the window, optionally selecting an app.
    Open(Option<AppId>),
    StartApp(AppId),
    StopApp(AppId),
    StopAll,
    /// Open the settings panel.
    OpenSettings,
    /// Reveal the app's log folder.
    OpenLogs,
    /// Ask whether a newer release exists.
    CheckUpdates,
    /// Quit the app.
    Quit,
}

/// One row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayMenuItem {
    pub label: String,
    pub action: TrayAction,
    pub enabled: bool,
    /// True for separators (no action is dispatched).
    pub separator: bool,
}

/// The whole menu plus the status-bar title.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayMenu {
    /// Short aggregate label for the status item ("3" running, "" when idle).
    pub title: String,
    pub items: Vec<TrayMenuItem>,
}

impl TrayMenu {
    /// Items that dispatch an action (skips separators).
    pub fn actionable(&self) -> impl Iterator<Item = &TrayMenuItem> {
        self.items
            .iter()
            .filter(|item| !item.separator && item.enabled)
    }
}

fn separator() -> TrayMenuItem {
    TrayMenuItem {
        label: String::new(),
        action: TrayAction::Open(None),
        enabled: false,
        separator: true,
    }
}

/// Build the menubar menu from a snapshot. Works with no window open: it reads the core.
pub fn build_menu(snapshot: &CoreSnapshot) -> TrayMenu {
    let running = snapshot
        .running_tasks
        .iter()
        .filter(|task| task.state.is_live())
        .count();
    let title = if running == 0 {
        String::new()
    } else {
        running.to_string()
    };

    let mut items = vec![TrayMenuItem {
        label: "打开 NeoHub".to_string(),
        action: TrayAction::Open(None),
        enabled: true,
        separator: false,
    }];
    items.push(separator());

    // One row per app, ordered like the sidebar (name, case-insensitive).
    let mut apps: Vec<(String, AppId, ProcessState)> = snapshot
        .apps
        .iter()
        .map(|app| {
            (
                app.name.to_lowercase(),
                app.id.clone(),
                snapshot
                    .app_states
                    .get(&app.id)
                    .copied()
                    .unwrap_or(ProcessState::Idle),
            )
        })
        .collect();
    apps.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.as_str().cmp(b.1.as_str())));

    for (_, app_id, state) in apps {
        let name = snapshot
            .apps
            .iter()
            .find(|app| app.id == app_id)
            .map(|app| app.name.clone())
            .unwrap_or_else(|| app_id.to_string());
        if state.is_live() {
            items.push(TrayMenuItem {
                label: format!("停止 {name}"),
                action: TrayAction::StopApp(app_id),
                enabled: true,
                separator: false,
            });
        } else {
            items.push(TrayMenuItem {
                label: format!("启动 {name}"),
                action: TrayAction::StartApp(app_id),
                enabled: true,
                separator: false,
            });
        }
    }

    if !snapshot.apps.is_empty() {
        items.push(separator());
    }
    items.push(TrayMenuItem {
        label: "全部停止".to_string(),
        action: TrayAction::StopAll,
        enabled: running > 0,
        separator: false,
    });
    items.push(separator());
    items.push(TrayMenuItem {
        label: "退出 NeoHub".to_string(),
        action: TrayAction::Quit,
        enabled: true,
        separator: false,
    });

    TrayMenu { title, items }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neohub_core::monitor::ports::PortSample;
    use neohub_core::monitor::stats::StatsSample;
    use neohub_core::process::runner::RunningTaskInfo;
    use neohub_domain::{App, NodeVersionPref, RunId, Settings, TaskId};
    use std::collections::BTreeMap;

    fn app(id: &str, name: &str) -> App {
        App {
            id: AppId::from_existing(id),
            name: name.into(),
            path: format!("/tmp/{name}"),
            color: "#fff".into(),
            icon: None,
            node_version_pref: NodeVersionPref::Auto,
            package_manager: None,
            default_script: None,
            custom_command: None,
            working_dir: format!("/tmp/{name}"),
            auto_restart_on_change: false,
            auto_start: false,
            watch_globs: vec![],
            port_hint: None,
            tags: vec![],
            folder: None,
            last_started_at: None,
            last_exit_code: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn running(app_id: &str, task_id: &str) -> RunningTaskInfo {
        RunningTaskInfo {
            task_id: TaskId::from_existing(task_id),
            app_id: AppId::from_existing(app_id),
            pid: 1,
            pgid: 1,
            run_id: RunId::from_existing("run"),
            state: ProcessState::Running,
            started_at: 0,
            command: "cmd".into(),
        }
    }

    fn snapshot(apps: Vec<App>, running: Vec<RunningTaskInfo>) -> CoreSnapshot {
        let mut app_states = BTreeMap::new();
        for app in &apps {
            let live = running.iter().any(|info| info.app_id == app.id);
            app_states.insert(
                app.id.clone(),
                if live {
                    ProcessState::Running
                } else {
                    ProcessState::Idle
                },
            );
        }
        CoreSnapshot {
            revision: 1,
            apps,
            tasks_by_app: BTreeMap::new(),
            running_tasks: running,
            app_states,
            settings: Settings::default(),
            task_stats: BTreeMap::<TaskId, StatsSample>::new(),
            task_ports: BTreeMap::<TaskId, PortSample>::new(),
            task_ready: BTreeMap::<TaskId, bool>::new(),
        }
    }

    #[test]
    fn the_status_title_counts_running_tasks() {
        let idle = build_menu(&snapshot(vec![app("a1", "demo")], vec![]));
        assert_eq!(idle.title, "");
        let busy = build_menu(&snapshot(
            vec![app("a1", "demo")],
            vec![running("a1", "t1")],
        ));
        assert_eq!(busy.title, "1");
    }

    #[test]
    fn every_app_gets_one_row_with_the_right_verb() {
        let menu = build_menu(&snapshot(
            vec![app("a1", "beta"), app("a2", "alpha")],
            vec![running("a1", "t1")],
        ));
        let labels: Vec<&str> = menu
            .items
            .iter()
            .filter(|item| !item.separator)
            .map(|item| item.label.as_str())
            .collect();
        assert_eq!(
            labels,
            vec![
                "打开 NeoHub",
                "启动 alpha",
                "停止 beta",
                "全部停止",
                "退出 NeoHub"
            ],
            "apps are ordered by name and use start/stop by state"
        );
    }

    #[test]
    fn stop_all_is_disabled_when_nothing_runs() {
        let menu = build_menu(&snapshot(vec![app("a1", "demo")], vec![]));
        let stop_all = menu
            .items
            .iter()
            .find(|item| item.action == TrayAction::StopAll)
            .expect("stop all");
        assert!(!stop_all.enabled);

        let busy = build_menu(&snapshot(
            vec![app("a1", "demo")],
            vec![running("a1", "t1")],
        ));
        let stop_all = busy
            .items
            .iter()
            .find(|item| item.action == TrayAction::StopAll)
            .expect("stop all");
        assert!(stop_all.enabled);
    }

    #[test]
    fn an_empty_registry_still_offers_open_and_quit() {
        let menu = build_menu(&snapshot(vec![], vec![]));
        let actions: Vec<&TrayAction> = menu.actionable().map(|item| &item.action).collect();
        assert!(
            actions
                .iter()
                .any(|action| matches!(action, TrayAction::Open(None)))
        );
        assert!(
            actions
                .iter()
                .any(|action| matches!(action, TrayAction::Quit))
        );
        assert_eq!(menu.actionable().count(), 2);
    }

    #[test]
    fn the_menu_never_requires_a_window() {
        // The menu is built purely from the snapshot: opening it with no window is the case
        // that matters most (that is the whole reason it exists).
        let menu = build_menu(&snapshot(vec![app("a1", "demo")], vec![]));
        assert!(menu.actionable().count() >= 3);
    }
}
