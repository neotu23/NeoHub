//! NeoHub macOS platform services.
//!
//! Narrow, macOS-only bindings: keychain secrets, "open in" helpers, deep links,
//! notifications, login item and the tray/Dock bridges. Nothing here depends on the UI
//! layer; the desktop crate assembles these into the app host.

#[cfg(target_os = "macos")]
pub mod app_menu;
pub mod deep_link;
#[cfg(target_os = "macos")]
pub mod deep_link_event;
#[cfg(target_os = "macos")]
pub mod dock;
#[cfg(target_os = "macos")]
pub mod keychain;
pub mod log_file;
pub mod login_item;
pub mod menu_router;
pub mod notify;
pub mod open_in;
pub mod tray;
#[cfg(target_os = "macos")]
pub mod tray_native;

#[cfg(target_os = "macos")]
pub use dock::{current_dock_visible, set_dock_visible, sync_dock};
#[cfg(target_os = "macos")]
pub use keychain::{KeychainSecretStore, delete_master_key};
#[cfg(target_os = "macos")]
pub use tray::{TrayAction, TrayMenu, TrayMenuItem, build_menu};
