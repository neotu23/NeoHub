//! Domain models.
//!
//! Field names are Rust-side; the *stored* shapes that matter for compatibility are
//! the JSON columns (`node_version_pref`, `watch_globs`, `tags`, `depends_on`,
//! `readiness`, `env_overrides`) and their serde encodings, which must match what the
//! Electron build wrote — see `specs/rust-migration/FEATURE-PARITY.md` D1–D4.

use crate::ids::{AppId, TaskId};
use serde::{Deserialize, Serialize};

/// The six lifecycle states. Do not collapse these: the UI, the tray and MCP all
/// distinguish `exited` (clean stop/exit) from `crashed` (non-zero/abnormal).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProcessState {
    Idle,
    Starting,
    Running,
    Exiting,
    Exited,
    Crashed,
}

impl ProcessState {
    /// True while the process (or its spawn) is still ours to manage.
    pub fn is_live(self) -> bool {
        matches!(
            self,
            ProcessState::Starting | ProcessState::Running | ProcessState::Exiting
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ProcessState::Idle => "idle",
            ProcessState::Starting => "starting",
            ProcessState::Running => "running",
            ProcessState::Exiting => "exiting",
            ProcessState::Exited => "exited",
            ProcessState::Crashed => "crashed",
        }
    }
}

/// Package managers the launcher knows how to drive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageManager {
    Npm,
    Yarn,
    Pnpm,
    Bun,
}

impl PackageManager {
    pub fn as_str(self) -> &'static str {
        match self {
            PackageManager::Npm => "npm",
            PackageManager::Yarn => "yarn",
            PackageManager::Pnpm => "pnpm",
            PackageManager::Bun => "bun",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "npm" => Some(PackageManager::Npm),
            "yarn" => Some(PackageManager::Yarn),
            "pnpm" => Some(PackageManager::Pnpm),
            "bun" => Some(PackageManager::Bun),
            _ => None,
        }
    }
}

/// How to pick the Node runtime for an app or task.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[derive(Default)]
pub enum NodeVersionPref {
    #[default]
    Auto,
    System,
    Explicit {
        version: String,
    },
}

/// Readiness probe configuration, six kinds, unchanged from the legacy model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[derive(Default)]
pub enum ReadinessSignal {
    #[default]
    None,
    Port {
        port: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        host: Option<String>,
    },
    Log {
        regex: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flags: Option<String>,
    },
    Http {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<u16>,
    },
    Exit {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<i32>,
    },
    Delay {
        ms: u64,
    },
}

impl ReadinessSignal {
    /// The stored kind string, e.g. `"log"`. Mirrors `ReadinessSignal['kind']`.
    pub fn kind(&self) -> &'static str {
        match self {
            ReadinessSignal::None => "none",
            ReadinessSignal::Port { .. } => "port",
            ReadinessSignal::Log { .. } => "log",
            ReadinessSignal::Http { .. } => "http",
            ReadinessSignal::Exit { .. } => "exit",
            ReadinessSignal::Delay { .. } => "delay",
        }
    }
}

/// `script` runs `<pm> run <script>`; `custom` runs the user's command string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommandKind {
    Script,
    Custom,
}

impl CommandKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CommandKind::Script => "script",
            CommandKind::Custom => "custom",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "script" => Some(CommandKind::Script),
            "custom" => Some(CommandKind::Custom),
            _ => None,
        }
    }
}

/// A registered project.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct App {
    pub id: AppId,
    pub name: String,
    pub path: String,
    pub color: String,
    pub icon: Option<String>,
    pub node_version_pref: NodeVersionPref,
    pub package_manager: Option<PackageManager>,
    pub default_script: Option<String>,
    pub custom_command: Option<String>,
    pub working_dir: String,
    pub auto_restart_on_change: bool,
    pub auto_start: bool,
    pub watch_globs: Vec<String>,
    pub port_hint: Option<u16>,
    pub tags: Vec<String>,
    pub folder: Option<String>,
    pub last_started_at: Option<i64>,
    pub last_exit_code: Option<i32>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// One startable command inside an app (an app can have many).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub app_id: AppId,
    pub name: String,
    pub position: i64,
    pub command_kind: CommandKind,
    pub script: Option<String>,
    pub custom_command: Option<String>,
    pub working_dir_override: Option<String>,
    pub package_manager_override: Option<PackageManager>,
    pub node_version_pref_override: Option<NodeVersionPref>,
    pub depends_on: Vec<TaskId>,
    pub readiness: ReadinessSignal,
    pub one_shot: bool,
    pub enabled: bool,
    /// Frozen legacy JSON column. The authoritative env lives in `env_vars` rows
    /// with `task_id` set; this field is preserved so old rows round-trip.
    pub env_overrides: std::collections::BTreeMap<String, String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// One environment variable row. Scope is expressed by which ids are set:
/// `(None, None)` = global, `(Some(app), None)` = app, `(_, Some(task))` = task.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EnvVar {
    pub id: crate::ids::EnvVarId,
    pub app_id: Option<AppId>,
    pub task_id: Option<TaskId>,
    pub key: String,
    pub value: String,
    pub enabled: bool,
    pub is_secret: bool,
    pub note: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// One row of the run history table.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunHistoryRow {
    pub id: String,
    pub app_id: AppId,
    pub task_id: Option<TaskId>,
    pub task_name: Option<String>,
    pub script: Option<String>,
    pub custom_command: Option<String>,
    pub node_version: Option<String>,
    pub package_manager: Option<PackageManager>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub exit_code: Option<i32>,
    pub exit_signal: Option<String>,
    pub was_killed_by_user: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_version_pref_encodes_exactly_like_the_electron_build() {
        // These literals are copied from rows the Electron build writes; changing them
        // would silently reset every user's Node preference on first Rust read.
        assert_eq!(
            serde_json::to_string(&NodeVersionPref::Auto).unwrap(),
            r#"{"kind":"auto"}"#
        );
        assert_eq!(
            serde_json::to_string(&NodeVersionPref::System).unwrap(),
            r#"{"kind":"system"}"#
        );
        assert_eq!(
            serde_json::to_string(&NodeVersionPref::Explicit {
                version: "v22.13.0".into()
            })
            .unwrap(),
            r#"{"kind":"explicit","version":"v22.13.0"}"#
        );
        let parsed: NodeVersionPref =
            serde_json::from_str(r#"{"kind":"explicit","version":"v20"}"#)
                .expect("legacy row must parse");
        assert_eq!(
            parsed,
            NodeVersionPref::Explicit {
                version: "v20".into()
            }
        );
    }

    #[test]
    fn readiness_encodes_exactly_like_the_electron_build() {
        assert_eq!(
            serde_json::to_string(&ReadinessSignal::None).unwrap(),
            r#"{"kind":"none"}"#
        );
        assert_eq!(
            serde_json::to_string(&ReadinessSignal::Port {
                port: 3000,
                host: None
            })
            .unwrap(),
            r#"{"kind":"port","port":3000}"#
        );
        assert_eq!(
            serde_json::to_string(&ReadinessSignal::Log {
                regex: "ready".into(),
                flags: Some("i".into())
            })
            .unwrap(),
            r#"{"kind":"log","regex":"ready","flags":"i"}"#
        );
        assert_eq!(
            serde_json::to_string(&ReadinessSignal::Http {
                url: "http://127.0.0.1:8080/health".into(),
                status: Some(200)
            })
            .unwrap(),
            r#"{"kind":"http","url":"http://127.0.0.1:8080/health","status":200}"#
        );
        assert_eq!(
            serde_json::to_string(&ReadinessSignal::Exit { code: None }).unwrap(),
            r#"{"kind":"exit"}"#
        );
        assert_eq!(
            serde_json::to_string(&ReadinessSignal::Delay { ms: 1500 }).unwrap(),
            r#"{"kind":"delay","ms":1500}"#
        );
    }

    #[test]
    fn every_readiness_kind_round_trips() {
        for signal in [
            ReadinessSignal::None,
            ReadinessSignal::Port {
                port: 5173,
                host: Some("127.0.0.1".into()),
            },
            ReadinessSignal::Log {
                regex: "^ready".into(),
                flags: None,
            },
            ReadinessSignal::Http {
                url: "http://localhost:1/".into(),
                status: None,
            },
            ReadinessSignal::Exit { code: Some(0) },
            ReadinessSignal::Delay { ms: 10 },
        ] {
            let json = serde_json::to_string(&signal).unwrap();
            let back: ReadinessSignal = serde_json::from_str(&json).unwrap();
            assert_eq!(signal, back, "round trip failed for {json}");
        }
    }

    #[test]
    fn process_state_strings_match_the_legacy_union() {
        let states = [
            (ProcessState::Idle, "idle"),
            (ProcessState::Starting, "starting"),
            (ProcessState::Running, "running"),
            (ProcessState::Exiting, "exiting"),
            (ProcessState::Exited, "exited"),
            (ProcessState::Crashed, "crashed"),
        ];
        for (state, text) in states {
            assert_eq!(
                serde_json::to_string(&state).unwrap(),
                format!("\"{text}\"")
            );
            assert_eq!(state.as_str(), text);
            assert!(state.is_live() == matches!(text, "starting" | "running" | "exiting"));
        }
    }
}
