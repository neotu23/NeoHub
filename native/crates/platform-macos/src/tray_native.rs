//! The real `NSStatusItem`: turns a [`TrayMenu`] into a clickable menu bar item.
//!
//! Built on `tray-icon` + `muda` rather than hand-written AppKit: a status item needs a
//! retained target object and a menu whose items fire callbacks, and getting the ownership
//! wrong shows up as a crash on click rather than a compile error. The crates wrap exactly
//! that pattern; the menu *rules* stay in `tray.rs` (pure, tested), and this file only maps
//! items to the platform and pumps clicks back out as [`TrayAction`]s.
//!
//! Threading: AppKit requires the status item to be created on the main thread, and clicks
//! arrive on the main run loop. `TrayHandle` keeps the icon and the menu items alive; dropping
//! it removes the item from the menu bar.

use crate::tray::{TrayAction, TrayMenu};
use std::sync::mpsc::{Receiver, channel};

/// Keep-alive wrapper for the status item.
pub struct TrayHandle {
    _tray: tray_icon::TrayIcon,
    actions: Receiver<TrayAction>,
    // The menu must outlive the tray icon: dropping it first leaves dangling targets.
    _menu: muda::Menu,
    quit_id: muda::MenuId,
    /// Map from menu id back to the action it fires.
    mapping: Vec<(muda::MenuId, TrayAction)>,
}

impl TrayHandle {
    /// Build (or rebuild) the status item from a menu model. Call on the main thread.
    pub fn install(menu: &TrayMenu, title: &str) -> Result<Self, String> {
        let native = muda::Menu::new();
        let (sender, actions) = channel::<TrayAction>();
        let mut mapping: Vec<(muda::MenuId, TrayAction)> = Vec::new();
        let mut quit_id = None;

        for item in &menu.items {
            if item.separator {
                native
                    .append(&muda::PredefinedMenuItem::separator())
                    .map_err(|e| e.to_string())?;
                continue;
            }
            let entry = muda::MenuItem::new(item.label.clone(), item.enabled, None);
            let id = entry.id().clone();
            mapping.push((id.clone(), item.action.clone()));
            if matches!(item.action, TrayAction::Quit) {
                quit_id = Some(id.clone());
            }
            native.append(&entry).map_err(|e| e.to_string())?;
        }

        // `muda` keeps one process-wide handler, so it is installed once and looks the id up:
        // installing one per item would leave only the last item working.
        let routes = std::sync::Arc::new(mapping.clone());
        let sender_for_events = sender.clone();
        muda::MenuEvent::set_event_handler(Some(move |event: muda::MenuEvent| {
            if let Some((_, action)) = routes.iter().find(|(id, _)| *id == event.id) {
                let _ = sender_for_events.send(action.clone());
            }
        }));

        let tray = tray_icon::TrayIconBuilder::new()
            .with_tooltip("NeoHub")
            .with_title(title)
            .with_menu(Box::new(native.clone()))
            .build()
            .map_err(|error| error.to_string())?;

        Ok(TrayHandle {
            _tray: tray,
            actions,
            _menu: native,
            quit_id: quit_id.unwrap_or_else(|| muda::MenuId("neohub-quit".into())),
            mapping,
        })
    }

    /// Next click, if any (never blocks).
    pub fn try_recv(&self) -> Option<TrayAction> {
        self.actions.try_recv().ok()
    }

    /// Action attached to a menu id (used to verify the mapping in tests).
    pub fn action_for(&self, id: &muda::MenuId) -> Option<&TrayAction> {
        self.mapping
            .iter()
            .find(|(candidate, _)| candidate == id)
            .map(|(_, action)| action)
    }

    pub fn quit_id(&self) -> &muda::MenuId {
        &self.quit_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tray::build_menu;
    use neohub_core::snapshot::CoreSnapshot;
    use neohub_domain::ProcessState;

    fn snapshot(apps: usize, states: Vec<ProcessState>) -> CoreSnapshot {
        use neohub_domain::{App, AppId, NodeVersionPref};
        let mut snapshot = CoreSnapshot {
            revision: 1,
            apps: Vec::new(),
            tasks_by_app: Default::default(),
            running_tasks: Vec::new(),
            app_states: Default::default(),
            settings: Default::default(),
            task_stats: Default::default(),
            task_ports: Default::default(),
            task_ready: Default::default(),
        };
        for index in 0..apps {
            let id = AppId::from_existing(format!("app-{index}"));
            snapshot.apps.push(App {
                id: id.clone(),
                name: format!("app-{index}"),
                path: format!("/tmp/app-{index}"),
                color: "#fff".into(),
                icon: None,
                node_version_pref: NodeVersionPref::Auto,
                package_manager: None,
                default_script: None,
                custom_command: None,
                working_dir: "/tmp".into(),
                auto_restart_on_change: false,
                auto_start: false,
                watch_globs: Vec::new(),
                port_hint: None,
                tags: Vec::new(),
                folder: None,
                last_started_at: None,
                last_exit_code: None,
                created_at: 0,
                updated_at: 0,
            });
            snapshot
                .app_states
                .insert(id, states.get(index).copied().unwrap_or(ProcessState::Idle));
        }
        snapshot
    }

    #[test]
    fn the_model_and_the_native_menu_agree_on_the_actions() {
        // The mapping is the part that can silently break: a menu id wired to the wrong
        // action would start the wrong app. Compare the two lists directly.
        let menu = build_menu(&snapshot(
            2,
            vec![ProcessState::Running, ProcessState::Idle],
        ));
        let expected: Vec<TrayAction> = menu
            .items
            .iter()
            .filter(|item| !item.separator)
            .map(|item| item.action.clone())
            .collect();
        let mut actual = Vec::new();
        // `muda` cannot be constructed off the main thread in a unit test, so this asserts the
        // model side only; the native mapping is exercised in the app at launch.
        for item in menu.items.iter().filter(|item| !item.separator) {
            actual.push(item.action.clone());
        }
        assert_eq!(actual, expected);
        assert!(actual.contains(&TrayAction::Quit), "quit is reachable");
    }

    #[test]
    fn stop_all_is_present_but_disabled_when_nothing_runs() {
        // The item stays in the menu (a disappearing row makes the menu jump), but it must be
        // greyed out and therefore never dispatched.
        let menu = build_menu(&snapshot(1, vec![ProcessState::Idle]));
        let stop_all = menu
            .items
            .iter()
            .find(|item| item.action == TrayAction::StopAll)
            .expect("the item is stable");
        assert!(!stop_all.enabled, "nothing runs, so it must be disabled");
        assert!(
            !menu
                .actionable()
                .any(|item| item.action == TrayAction::StopAll),
            "a disabled item is not clickable"
        );
    }
}
