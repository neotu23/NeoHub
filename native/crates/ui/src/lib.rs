//! NeoHub native interface crate.
//!
//! Nothing here touches the database, the PTY or the filesystem: views receive
//! `CoreSnapshot`/`CoreEvent` data and emit `CoreCommand`s through a handle. See
//! `native/docs/interfaces.md` for the contract.

pub mod theme;

pub use theme::{DARK, LIGHT, Palette, ThemeMode, metrics};
