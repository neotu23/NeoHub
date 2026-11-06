//! The application menu in the menu bar.
//!
//! Built with `muda` like the status item, and routed through the same single event handler
//! (`menu_router`) — `muda` only supports one handler per process, so a second menu must share
//! the router rather than install its own.
//!
//! What is here is what the app can actually do today. Menu items that would only look useful
//! (an "About" panel with no licence text, an "Edit" menu for fields that handle keys
//! themselves) are left out, because a menu row that does nothing is worse than no row.

use crate::menu_router::{self, MenuSource};
use crate::tray::TrayAction;

/// One row of the application menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppMenuRow {
    pub label: String,
    pub action: TrayAction,
    /// macOS accelerator, e.g. `Cmd+Comma`. `None` for rows without a shortcut.
    pub accelerator: Option<&'static str>,
}

/// A menu (a top-level entry such as "NeoHub" or "文件").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppMenuGroup {
    pub title: &'static str,
    pub rows: Vec<AppMenuRow>,
}

/// The menu bar as data, so the shape can be asserted without AppKit.
pub fn model() -> Vec<AppMenuGroup> {
    vec![
        AppMenuGroup {
            title: "NeoHub",
            rows: vec![
                AppMenuRow {
                    label: "设置…".to_string(),
                    action: TrayAction::OpenSettings,
                    accelerator: Some("Cmd+Comma"),
                },
                AppMenuRow {
                    label: "打开日志目录".to_string(),
                    action: TrayAction::OpenLogs,
                    accelerator: None,
                },
                AppMenuRow {
                    label: "退出 NeoHub".to_string(),
                    action: TrayAction::Quit,
                    accelerator: Some("Cmd+Q"),
                },
            ],
        },
        AppMenuGroup {
            title: "文件",
            rows: vec![
                AppMenuRow {
                    label: "打开窗口".to_string(),
                    action: TrayAction::Open(None),
                    accelerator: Some("Cmd+O"),
                },
                AppMenuRow {
                    label: "检查更新…".to_string(),
                    action: TrayAction::CheckUpdates,
                    accelerator: None,
                },
            ],
        },
    ]
}

/// Install the application menu. Call on the main thread, after AppKit is up.
pub fn install() -> Result<muda::Menu, String> {
    let menu = muda::Menu::new();
    let mut routing = Vec::new();
    for group in model() {
        let submenu = muda::Submenu::new(group.title, true);
        for row in group.rows {
            let item = match row.accelerator {
                Some(accelerator) => muda::MenuItem::with_id(
                    muda::MenuId(format!("app-menu-{}", row.label)),
                    row.label.clone(),
                    true,
                    accelerator.parse().ok(),
                ),
                None => muda::MenuItem::with_id(
                    muda::MenuId(format!("app-menu-{}", row.label)),
                    row.label.clone(),
                    true,
                    None,
                ),
            };
            routing.push((item.id().clone(), row.action));
            submenu.append(&item).map_err(|error| error.to_string())?;
        }
        menu.append(&submenu).map_err(|error| error.to_string())?;
    }
    menu.init_for_nsapp();
    menu_router::register(MenuSource::AppMenu, routing);
    Ok(menu)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_offers_settings_quit_and_the_log_folder() {
        let model = model();
        let actions: Vec<&TrayAction> = model
            .iter()
            .flat_map(|group| group.rows.iter())
            .map(|row| &row.action)
            .collect();
        assert!(actions.contains(&&TrayAction::OpenSettings));
        assert!(actions.contains(&&TrayAction::Quit));
        assert!(actions.contains(&&TrayAction::OpenLogs));
    }

    #[test]
    fn every_row_has_a_label_and_a_reason_to_exist() {
        for group in model() {
            assert!(!group.title.is_empty());
            assert!(!group.rows.is_empty(), "{} is empty", group.title);
            for row in group.rows {
                assert!(!row.label.is_empty());
                // A row whose action the app cannot perform would be a dead click.
                assert!(!matches!(
                    row.action,
                    TrayAction::StartApp(_) | TrayAction::StopApp(_)
                ));
            }
        }
    }

    #[test]
    fn accelerators_are_parseable() {
        for group in model() {
            for row in group.rows {
                if let Some(accelerator) = row.accelerator {
                    assert!(
                        accelerator
                            .parse::<muda::accelerator::Accelerator>()
                            .is_ok(),
                        "{} has an invalid accelerator: {accelerator}",
                        row.label
                    );
                }
            }
        }
    }
}
