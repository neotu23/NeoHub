//! Builds the environment a spawned task receives.
//!
//! Layering order (later wins), unchanged from `src/main/services/EnvBuilder.ts`:
//!
//! 1. sanitized OS base (`HOME`, `USER`, `LANG`, `LC_ALL`, `TMPDIR`, `SHELL`,
//!    `SSH_AUTH_SOCK`, `TERM_PROGRAM`, `COLORTERM` only);
//! 2. computed `PATH` — the resolved Node bin dir in front of the login-shell PATH;
//! 3. project `.env` files, in precedence order `.env` → `.env.development` →
//!    `.env.local` → `.env.development.local`, with process-control keys stripped;
//! 4. global → app → task `env_vars` rows (enabled only);
//! 5. hard-coded runtime: `FORCE_COLOR=1`, `TERM=xterm-256color`.
//!
//! Project files sit **below** user-configured variables on purpose: the UI is the source
//! of truth, and a checked-in `.env` must not be able to hijack `PATH`, `NODE_OPTIONS`,
//! `NODE_PATH`, `DYLD_*` or `LD_*`.

use super::dotenv::parse_dotenv;
use neohub_domain::EnvVar;
use std::collections::BTreeMap;
use std::path::Path;

/// OS variables forwarded to tasks. Everything else is dropped so a shell's stray
/// exported state cannot leak into a project's process.
pub const FORWARDED_OS_KEYS: [&str; 10] = [
    "HOME",
    "USER",
    "LANG",
    "LC_ALL",
    "TMPDIR",
    "SHELL",
    "SSH_AUTH_SOCK",
    "TERM_PROGRAM",
    "COLORTERM",
    "LOGNAME",
];

/// `.env` files read from the project dir, later entries winning.
pub const ENV_FILE_PRECEDENCE: [&str; 4] = [
    ".env",
    ".env.development",
    ".env.local",
    ".env.development.local",
];

/// True when a key coming from a project `.env` must be dropped entirely.
pub fn is_process_control_key(key: &str) -> bool {
    matches!(key, "PATH" | "NODE_OPTIONS" | "NODE_PATH")
        || key.starts_with("DYLD_")
        || key.starts_with("LD_")
}

/// Inputs for one env build. Everything is passed in so the function stays pure and
/// testable — no globals, no `std::env::vars()` reads buried inside.
pub struct BuildEnvInput<'a> {
    /// The parent process environment (only `FORWARDED_OS_KEYS` survive).
    pub os_env: &'a BTreeMap<String, String>,
    /// Directory holding the resolved Node binary.
    pub node_bin_dir: &'a Path,
    /// PATH recovered from the user's login shell.
    pub login_path: &'a str,
    /// Project directory whose `.env*` files are layered in.
    pub project_dir: &'a Path,
    pub global: &'a [EnvVar],
    pub app: &'a [EnvVar],
    pub task: &'a [EnvVar],
}

/// Build the task environment. Later layers overwrite earlier ones.
pub fn build_env(input: &BuildEnvInput<'_>) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();

    for key in FORWARDED_OS_KEYS {
        if let Some(value) = input.os_env.get(key) {
            env.insert(key.to_string(), value.clone());
        }
    }

    let node_bin = input.node_bin_dir.to_string_lossy();
    let path = if input.login_path.is_empty() {
        node_bin.into_owned()
    } else {
        format!("{node_bin}:{}", input.login_path)
    };
    env.insert("PATH".to_string(), path);

    for (key, value) in read_env_files(input.project_dir) {
        env.insert(key, value);
    }

    layer(&mut env, input.global);
    layer(&mut env, input.app);
    layer(&mut env, input.task);

    env.insert("FORCE_COLOR".to_string(), "1".to_string());
    env.insert("TERM".to_string(), "xterm-256color".to_string());
    env
}

/// Read `.env*` files in precedence order, stripping process-control keys.
///
/// Unreadable files are skipped (a permission error on `.env.local` must not stop a
/// project from starting), matching the legacy `try { … } catch { }`.
pub fn read_env_files(project_dir: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for name in ENV_FILE_PRECEDENCE {
        let path = project_dir.join(name);
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (key, value) in parse_dotenv(&contents) {
            if is_process_control_key(&key) {
                continue;
            }
            out.insert(key, value);
        }
    }
    out
}

fn layer(env: &mut BTreeMap<String, String>, vars: &[EnvVar]) {
    for var in vars {
        if !var.enabled {
            continue;
        }
        // An empty value is meaningful ("unset this for the child"), so it is written
        // verbatim rather than treated as "remove the key".
        env.insert(var.key.clone(), var.value.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neohub_domain::{AppId, EnvVarId};

    fn var(key: &str, value: &str, enabled: bool) -> EnvVar {
        EnvVar {
            id: EnvVarId::new(),
            app_id: None,
            task_id: None,
            key: key.into(),
            value: value.into(),
            enabled,
            is_secret: false,
            note: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn os_env() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("HOME".to_string(), "/Users/tester".to_string()),
            ("LANG".to_string(), "en_US.UTF-8".to_string()),
            ("SECRET_PARENT_VAR".to_string(), "leak".to_string()),
        ])
    }

    #[test]
    fn only_whitelisted_os_variables_are_forwarded() {
        let dir = tempfile::tempdir().unwrap();
        let env = build_env(&BuildEnvInput {
            os_env: &os_env(),
            node_bin_dir: Path::new("/opt/node/bin"),
            login_path: "/usr/bin:/bin",
            project_dir: dir.path(),
            global: &[],
            app: &[],
            task: &[],
        });
        assert_eq!(env.get("HOME").unwrap(), "/Users/tester");
        assert_eq!(env.get("LANG").unwrap(), "en_US.UTF-8");
        assert!(
            !env.contains_key("SECRET_PARENT_VAR"),
            "parent vars do not leak"
        );
        assert_eq!(env.get("PATH").unwrap(), "/opt/node/bin:/usr/bin:/bin");
        assert_eq!(env.get("FORCE_COLOR").unwrap(), "1");
        assert_eq!(env.get("TERM").unwrap(), "xterm-256color");
    }

    #[test]
    fn env_files_layer_in_precedence_order_and_lose_to_user_vars() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "A=base\nB=base\nONLY_BASE=1\n").unwrap();
        std::fs::write(dir.path().join(".env.development"), "B=dev\n").unwrap();
        std::fs::write(dir.path().join(".env.local"), "B=local\n").unwrap();
        std::fs::write(dir.path().join(".env.development.local"), "B=devlocal\n").unwrap();

        let global = vec![var("A", "from-global", true)];
        let app = vec![var("C", "from-app", true)];
        let task = vec![var("C", "from-task", true)];

        let env = build_env(&BuildEnvInput {
            os_env: &os_env(),
            node_bin_dir: Path::new("/n"),
            login_path: "/bin",
            project_dir: dir.path(),
            global: &global,
            app: &app,
            task: &task,
        });

        assert_eq!(env.get("B").unwrap(), "devlocal", "last env file wins");
        assert_eq!(
            env.get("A").unwrap(),
            "from-global",
            "user vars beat .env files"
        );
        assert_eq!(env.get("ONLY_BASE").unwrap(), "1");
        assert_eq!(
            env.get("C").unwrap(),
            "from-task",
            "task scope wins over app scope"
        );
    }

    #[test]
    fn a_project_env_file_cannot_hijack_process_control_keys() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env"),
            "PATH=/evil/bin\nNODE_OPTIONS=--require ./evil.js\nNODE_PATH=/evil\n\
             DYLD_INSERT_LIBRARIES=/evil/lib.dylib\nLD_PRELOAD=/evil.so\nHARMLESS=ok\n",
        )
        .unwrap();

        let env = build_env(&BuildEnvInput {
            os_env: &os_env(),
            node_bin_dir: Path::new("/opt/node/bin"),
            login_path: "/usr/bin:/bin",
            project_dir: dir.path(),
            global: &[],
            app: &[],
            task: &[],
        });

        assert_eq!(env.get("PATH").unwrap(), "/opt/node/bin:/usr/bin:/bin");
        assert!(!env.contains_key("NODE_OPTIONS"));
        assert!(!env.contains_key("NODE_PATH"));
        assert!(!env.contains_key("DYLD_INSERT_LIBRARIES"));
        assert!(!env.contains_key("LD_PRELOAD"));
        assert_eq!(env.get("HARMLESS").unwrap(), "ok");
    }

    #[test]
    fn user_configured_control_keys_are_still_allowed() {
        // The UI is trusted: an explicit PATH in env_vars wins over the computed one.
        let dir = tempfile::tempdir().unwrap();
        let global = vec![var("PATH", "/custom/bin", true)];
        let env = build_env(&BuildEnvInput {
            os_env: &os_env(),
            node_bin_dir: Path::new("/opt/node/bin"),
            login_path: "/usr/bin:/bin",
            project_dir: dir.path(),
            global: &global,
            app: &[],
            task: &[],
        });
        assert_eq!(env.get("PATH").unwrap(), "/custom/bin");
    }

    #[test]
    fn disabled_vars_are_skipped_and_empty_values_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let app = vec![var("OFF", "x", false), var("EMPTY", "", true)];
        let env = build_env(&BuildEnvInput {
            os_env: &os_env(),
            node_bin_dir: Path::new("/n"),
            login_path: "/bin",
            project_dir: dir.path(),
            global: &[],
            app: &app,
            task: &[],
        });
        assert!(!env.contains_key("OFF"));
        assert_eq!(env.get("EMPTY").unwrap(), "");
    }

    #[test]
    fn a_missing_login_path_does_not_produce_a_broken_path() {
        let dir = tempfile::tempdir().unwrap();
        let env = build_env(&BuildEnvInput {
            os_env: &os_env(),
            node_bin_dir: Path::new("/opt/node/bin"),
            login_path: "",
            project_dir: dir.path(),
            global: &[],
            app: &[],
            task: &[],
        });
        assert_eq!(env.get("PATH").unwrap(), "/opt/node/bin");
    }

    #[test]
    fn unused_app_id_import_keeps_the_public_type_visible() {
        // Guard: EnvVar.app_id stays part of the domain surface used by callers.
        let v = var("K", "V", true);
        let _: Option<AppId> = v.app_id;
    }
}
