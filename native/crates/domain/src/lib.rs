//! NeoHub domain: identifiers, models, settings and errors.
//!
//! No GUI, database, process or filesystem access lives here — this crate is the
//! vocabulary the storage/core/ui/mcp layers share.

pub mod error;
pub mod ids;
pub mod models;
pub mod settings;

pub use error::{DomainError, Result};
pub use ids::{AppId, EnvVarId, RunId, TaskId};
pub use models::{
    App, CommandKind, EnvVar, NodeVersionPref, PackageManager, ProcessState, ReadinessSignal,
    RunHistoryRow, Task,
};
pub use settings::{LanguageSetting, Settings, SettingsPatch, ThemeSetting};
