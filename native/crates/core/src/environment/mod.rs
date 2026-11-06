//! Environment resolution: `.env` parsing, layering and the login-shell PATH probe.

pub mod detect;
pub mod dotenv;
pub mod env_builder;
pub mod node_resolver;
pub mod path_probe;

pub use detect::{DetectionResult, WorkspaceCandidate, detect_project};
pub use dotenv::{is_secret_key, parse_dotenv};
pub use env_builder::{
    BuildEnvInput, ENV_FILE_PRECEDENCE, build_env, is_process_control_key, read_env_files,
};
pub use node_resolver::{NodeError, NodeInstallation, NodeResolver, NodeSource};
pub use path_probe::PathProbe;
