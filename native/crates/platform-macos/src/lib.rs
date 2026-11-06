//! NeoHub macOS platform services.
//!
//! Narrow, macOS-only bindings: keychain secrets, "open in" helpers, deep links,
//! notifications, login item and the tray/Dock bridges. Nothing here depends on the UI
//! layer; the desktop crate assembles these into the app host.

#[cfg(target_os = "macos")]
pub mod keychain;

#[cfg(target_os = "macos")]
pub use keychain::{KeychainSecretStore, delete_master_key};
