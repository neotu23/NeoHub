//! The ⌘K command palette.
//!
//! Three things here are worth testing without a window: which rows exist for a given snapshot,
//! how typing narrows them, and what selecting one does. The view only draws this and forwards
//! keys.
//!
//! Ordering matters more than it looks: rows are grouped (apps, then actions) and inside a group
//! they keep the sidebar's order, so the same project sits in the same place every time the
//! palette opens. A palette that reshuffles makes muscle memory useless.

use crate::state::text_input::{KeyOutcome, TextField};
use neohub_core::snapshot::CoreSnapshot;
use neohub_domain::{AppId, ProcessState};

/// What a row does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteAction {
    /// Jump to an app's detail page.
    OpenApp(AppId),
    StartApp(AppId),
    StopApp(AppId),
    RestartApp(AppId),
    /// Open the settings panel.
    Settings,
    /// Open the import panel.
    Import,
    /// Open the global log search.
    GlobalSearch,
}

impl PaletteAction {
    /// Whether the row makes sense for the app's current state.
    ///
    /// Offering "start" for something already running is not just noise: it is the row a user
    /// will hit by accident.
    fn is_available(&self, state: ProcessState) -> bool {
        match self {
            PaletteAction::StartApp(_) => !state.is_live(),
            PaletteAction::StopApp(_) | PaletteAction::RestartApp(_) => state.is_live(),
            _ => true,
        }
    }
}

/// One row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteItem {
    /// Group heading it belongs to (`应用` / `操作`).
    pub group: &'static str,
    pub label: String,
    /// Right-hand hint: state, or the key that does the same thing.
    pub detail: String,
    pub action: PaletteAction,
}

/// The palette's state.
pub struct CommandPalette {
    query: TextField,
    items: Vec<PaletteItem>,
    /// Indices into `items` after filtering, in display order.
    matches: Vec<usize>,
    cursor: usize,
}

impl CommandPalette {
    /// Build from a snapshot: one row per app action plus the global actions.
    pub fn new(snapshot: Option<&CoreSnapshot>) -> Self {
        let mut items = Vec::new();
        if let Some(snapshot) = snapshot {
            let mut apps = snapshot.apps.clone();
            apps.sort_by(|left, right| {
                left.name
                    .to_lowercase()
                    .cmp(&right.name.to_lowercase())
                    .then_with(|| left.id.as_str().cmp(right.id.as_str()))
            });
            for app in apps {
                let state = snapshot
                    .app_states
                    .get(&app.id)
                    .copied()
                    .unwrap_or(ProcessState::Idle);
                let state_label = state.as_str().to_string();
                let mut push = |label: String, action: PaletteAction| {
                    if action.is_available(state) {
                        items.push(PaletteItem {
                            group: "应用",
                            label,
                            detail: state_label.clone(),
                            action,
                        });
                    }
                };
                push(app.name.clone(), PaletteAction::OpenApp(app.id.clone()));
                push(
                    format!("启动 {}", app.name),
                    PaletteAction::StartApp(app.id.clone()),
                );
                push(
                    format!("停止 {}", app.name),
                    PaletteAction::StopApp(app.id.clone()),
                );
                push(
                    format!("重启 {}", app.name),
                    PaletteAction::RestartApp(app.id.clone()),
                );
            }
        }
        for (label, detail, action) in [
            ("打开设置", "⌘,", PaletteAction::Settings),
            ("导入项目", "⌘N", PaletteAction::Import),
            ("全局日志搜索", "⌘⇧F", PaletteAction::GlobalSearch),
        ] {
            items.push(PaletteItem {
                group: "操作",
                label: label.to_string(),
                detail: detail.to_string(),
                action,
            });
        }

        let mut palette = CommandPalette {
            query: TextField::new(""),
            items,
            matches: Vec::new(),
            cursor: 0,
        };
        palette.refilter();
        palette
    }

    pub fn query(&self) -> &str {
        self.query.text()
    }

    pub fn is_empty(&self) -> bool {
        self.matches.is_empty()
    }

    /// Visible rows in display order.
    pub fn visible(&self) -> Vec<&PaletteItem> {
        self.matches
            .iter()
            .map(|index| &self.items[*index])
            .collect()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The row the cursor is on.
    pub fn selected(&self) -> Option<&PaletteItem> {
        self.matches
            .get(self.cursor)
            .map(|index| &self.items[*index])
    }

    /// Feed a keystroke: typing narrows, arrows move, Enter picks.
    pub fn apply_key(&mut self, key: &str, characters: Option<&str>) -> PaletteOutcome {
        match key {
            "up" => {
                self.move_by(-1);
                PaletteOutcome::Moved
            }
            "down" => {
                self.move_by(1);
                PaletteOutcome::Moved
            }
            "enter" => match self.selected() {
                Some(item) => PaletteOutcome::Run(item.action.clone()),
                None => PaletteOutcome::Moved,
            },
            "escape" => PaletteOutcome::Close,
            _ => {
                let outcome = self.query.apply_key(key, characters);
                if outcome == KeyOutcome::Changed {
                    self.refilter();
                    PaletteOutcome::Moved
                } else {
                    PaletteOutcome::Ignored
                }
            }
        }
    }

    /// Recompute the visible rows for the current query and reset the cursor.
    fn refilter(&mut self) {
        let needle = self.query.text().trim().to_lowercase();
        self.matches = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| needle.is_empty() || fuzzy_match(&item.label, &needle))
            .map(|(index, _)| index)
            .collect();
        self.cursor = 0;
    }

    fn move_by(&mut self, delta: i32) {
        if self.matches.is_empty() {
            self.cursor = 0;
            return;
        }
        let len = self.matches.len() as i32;
        let next = (self.cursor as i32 + delta).rem_euclid(len);
        self.cursor = next as usize;
    }
}

/// What the caller should do after a key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteOutcome {
    /// The palette needs a repaint (typing or moving).
    Moved,
    /// Close it.
    Close,
    /// Run this action and close.
    Run(PaletteAction),
    /// The key was not ours.
    Ignored,
}

/// Subsequence match: `dst` matches `ovr` because the letters appear in order.
///
/// Subsequence (not prefix) is what makes a palette feel fast: `ovr` finds "启动 overview".
fn fuzzy_match(label: &str, needle: &str) -> bool {
    let mut chars = label.to_lowercase().chars().collect::<Vec<_>>().into_iter();
    needle
        .chars()
        .all(|wanted| chars.any(|candidate| candidate == wanted))
}

#[cfg(test)]
mod tests {
    use super::*;
    use neohub_domain::{App, NodeVersionPref};

    fn app(id: &str, name: &str) -> App {
        App {
            id: AppId::from_existing(id),
            name: name.into(),
            path: format!("/tmp/{id}"),
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
        }
    }

    fn snapshot(apps: Vec<App>, running: &[&str]) -> CoreSnapshot {
        let mut snapshot = CoreSnapshot {
            revision: 1,
            apps: apps.clone(),
            tasks_by_app: Default::default(),
            running_tasks: Vec::new(),
            app_states: Default::default(),
            settings: Default::default(),
            task_stats: Default::default(),
            task_ports: Default::default(),
            task_ready: Default::default(),
        };
        for app in apps {
            let state = if running.contains(&app.id.as_str()) {
                ProcessState::Running
            } else {
                ProcessState::Idle
            };
            snapshot.app_states.insert(app.id, state);
        }
        snapshot
    }

    #[test]
    fn rows_cover_both_apps_and_global_actions() {
        let palette = CommandPalette::new(Some(&snapshot(vec![app("a", "web")], &[])));
        let labels: Vec<&str> = palette
            .visible()
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        assert!(labels.contains(&"web"), "the app itself");
        assert!(labels.contains(&"启动 web"), "start");
        assert!(labels.contains(&"打开设置"), "settings");
        assert_eq!(palette.selected().map(|item| item.group), Some("应用"));
    }

    #[test]
    fn rows_that_would_do_nothing_are_not_offered() {
        let idle = CommandPalette::new(Some(&snapshot(vec![app("a", "web")], &[])));
        let labels: Vec<&str> = idle
            .visible()
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        assert!(labels.contains(&"启动 web"));
        assert!(!labels.contains(&"停止 web"), "nothing to stop");
        assert!(!labels.contains(&"重启 web"), "nothing to restart");

        let running = CommandPalette::new(Some(&snapshot(vec![app("a", "web")], &["a"])));
        let labels: Vec<&str> = running
            .visible()
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        assert!(labels.contains(&"停止 web"));
        assert!(!labels.contains(&"启动 web"), "already running");
    }

    #[test]
    fn typing_narrows_and_the_cursor_resets() {
        let mut palette =
            CommandPalette::new(Some(&snapshot(vec![app("a", "web"), app("b", "api")], &[])));
        let before = palette.visible().len();
        palette.apply_key("down", None);
        assert_eq!(palette.cursor(), 1);

        for character in ["a", "p", "i"] {
            palette.apply_key(character, Some(character));
        }
        assert!(palette.visible().len() < before, "typing narrows");
        assert_eq!(palette.cursor(), 0, "the cursor goes back to the top");
        assert!(
            palette
                .visible()
                .iter()
                .all(|item| item.label.to_lowercase().contains("api")),
            "only matching rows: {:?}",
            palette
                .visible()
                .iter()
                .map(|i| &i.label)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn matching_is_a_subsequence_so_short_queries_still_find_rows() {
        let mut palette = CommandPalette::new(Some(&snapshot(vec![app("a", "overview")], &[])));
        for character in ["o", "v", "r"] {
            palette.apply_key(character, Some(character));
        }
        assert!(
            palette
                .visible()
                .iter()
                .any(|item| item.label == "overview"),
            "ovr should match overview"
        );
    }

    #[test]
    fn arrows_wrap_and_never_leave_the_list() {
        let mut palette = CommandPalette::new(Some(&snapshot(vec![app("a", "web")], &[])));
        let len = palette.visible().len();
        assert!(len > 1);
        palette.apply_key("up", None);
        assert_eq!(
            palette.cursor(),
            len - 1,
            "up from the top wraps to the bottom"
        );
        palette.apply_key("down", None);
        assert_eq!(palette.cursor(), 0);
    }

    #[test]
    fn enter_runs_the_row_under_the_cursor() {
        let mut palette = CommandPalette::new(Some(&snapshot(vec![app("a", "web")], &[])));
        let expected = palette.selected().expect("a row").action.clone();
        assert_eq!(
            palette.apply_key("enter", None),
            PaletteOutcome::Run(expected)
        );
    }

    #[test]
    fn escape_closes_and_unknown_keys_change_nothing() {
        let mut palette = CommandPalette::new(None);
        assert_eq!(palette.apply_key("escape", None), PaletteOutcome::Close);
        assert_eq!(palette.apply_key("f13", None), PaletteOutcome::Ignored);
    }

    #[test]
    fn an_empty_snapshot_still_offers_the_global_actions() {
        let palette = CommandPalette::new(None);
        let labels: Vec<&str> = palette
            .visible()
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        assert_eq!(labels.len(), 3);
        assert!(labels.contains(&"打开设置"));
    }

    #[test]
    fn a_query_with_no_match_leaves_nothing_selected() {
        let mut palette = CommandPalette::new(Some(&snapshot(vec![app("a", "web")], &[])));
        for character in ["z", "z", "z"] {
            palette.apply_key(character, Some(character));
        }
        assert!(palette.is_empty());
        assert!(palette.selected().is_none());
        assert_eq!(
            palette.apply_key("enter", None),
            PaletteOutcome::Moved,
            "enter on nothing is not a crash"
        );
    }
}
