//! File watching: `.env` changes and restart-on-source-change.

pub mod environment;
pub mod restart;

pub use environment::{EnvEventKind, EnvFileEvent, EnvWatch, is_env_file, watch_env_files};
pub use restart::{IGNORED_DIRS, RestartWatch, is_ignored, watch_sources};
