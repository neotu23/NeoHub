//! SQLite persistence for NeoHub.

pub mod catalog;
pub mod db;
pub mod env;
pub mod history;
pub mod migrations;
pub mod settings;

pub use catalog::{AppPatch, Catalog, NewApp, NewTask, TaskPatch};
pub use db::{DataDir, Storage};
pub use env::EnvStore;
pub use history::{HistoryRow, RunHistoryStore};
pub use migrations::MIGRATIONS;
pub use settings::SettingsStore;

/// Crate-wide error type. Storage failures must be actionable: the UI/MCP layer
/// maps these onto field-level errors instead of showing a raw sqlite message.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("io error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("corrupt JSON in column {column}: {message}")]
    CorruptJson {
        column: &'static str,
        message: String,
    },

    #[error("{entity} not found: {id}")]
    NotFound { entity: &'static str, id: String },

    #[error("an app already uses path {path} (id {existing})")]
    DuplicatePath { path: String, existing: String },
}

pub type Result<T> = std::result::Result<T, StorageError>;
