//! NeoHub MCP server.
//!
//! Layout: `auth` gates requests, `redaction` scrubs everything on the way out, and the tool
//! layer (next) is a thin shell over `CoreHandle` — MCP never touches the database, the PTY or
//! the filesystem directly, so the UI and MCP cannot drift apart.

pub mod auth;
pub mod dispatch;
pub mod redaction;
pub mod server;
pub mod tools;

pub use auth::{bearer_matches, host_allowed, is_loopback_hostname, origin_allowed};
pub use dispatch::{ToolContext, ToolError, dispatch};
pub use redaction::{MASK, Redactor};
pub use server::{McpConfig, ServerHandle, start};
pub use tools::{ALL as ALL_TOOLS, ToolName, ToolSpec, tool_specs};
