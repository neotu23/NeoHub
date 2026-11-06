//! NeoHub native interface crate.
//!
//! Nothing here touches the database, the PTY or the filesystem: views receive
//! `CoreSnapshot`/`CoreEvent` data and emit `CoreCommand`s through a handle. See
//! `native/docs/interfaces.md` for the contract.

pub mod i18n;
pub mod state;
pub mod terminal;
pub mod theme;
pub mod views;

pub use i18n::Locales;
pub use state::{AppSort, GroupBy, Session, SessionStore, UiStore, View};
pub use terminal::{MatchList, Selection, TerminalView};
pub use theme::{DARK, LIGHT, Palette, ThemeMode, metrics};
pub use views::{HostBridge, ShellView};
