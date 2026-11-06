//! Importing an Electron NeoHub database into the native build.
//!
//! Two steps, always separate: [`legacy_db::inspect`] (read-only, produces a preview with
//! counts only) and [`import_legacy`] (writes into a staging database, verifies, then publishes
//! atomically). The UI shows the preview first; nothing is written before the user confirms.

pub mod legacy_db;
pub mod legacy_import;
pub mod manifest;

pub use legacy_db::{
    KNOWN_SETTINGS, KNOWN_VERSIONS, LEGACY_DB_FILE, LEGACY_DIR_CANDIDATES, MigrationPreview,
    TableCounts, copy_source, discover_legacy_dir, file_sha256, inspect,
};
pub use legacy_import::{ImportOutcome, import_legacy};
pub use manifest::{MigrationManifest, MigrationReport};
