//! What to spawn, fully resolved before the PTY is created.
//!
//! Command assembly happens *here*, not inside the spawn: by the time a process is
//! created the program, its argument vector, the working directory and the environment are
//! already fixed, so a failure cannot leave a half-built command. Script tasks become
//! `<pm> run <script>` with an argument array; only an explicitly configured `custom`
//! command is handed to a shell.

use neohub_domain::{CommandKind, PackageManager, Task};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Grid a task starts with (120×30, same as the legacy PTY).
pub const DEFAULT_COLS: u16 = 120;
/// Grid a task starts with.
pub const DEFAULT_ROWS: u16 = 30;
/// Smallest grid accepted by resize (below this a TUI cannot lay itself out).
pub const MIN_COLS: u16 = 2;
/// Smallest grid accepted by resize.
pub const MIN_ROWS: u16 = 2;

/// A resolved command ready to hand to the PTY.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub initial_cols: u16,
    pub initial_rows: u16,
    /// Human-readable command line for the UI/history (`pnpm run dev`).
    pub display: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SpawnSpecError {
    #[error("task {task_id} has no script configured")]
    MissingScript { task_id: String },

    #[error("task {task_id} has no custom command configured")]
    MissingCommand { task_id: String },

    #[error("task {task_id} has no package manager and no override")]
    MissingPackageManager { task_id: String },

    #[error("invalid grid size {cols}x{rows}")]
    InvalidGrid { cols: u16, rows: u16 },

    #[error("working directory does not exist: {path}")]
    MissingWorkingDir { path: String },
}

impl SpawnSpec {
    /// Build the spec for one task.
    ///
    /// * `script` tasks run `<pm> run <script>`;
    /// * `custom` tasks run the user's string through a shell (that is the feature).
    ///
    /// `working_dir` is the app path or the task's `working_dir_override`.
    pub fn for_task(
        task: &Task,
        app_working_dir: &Path,
        env: BTreeMap<String, String>,
        package_manager: Option<PackageManager>,
        cols: u16,
        rows: u16,
    ) -> Result<Self, SpawnSpecError> {
        let (cols, rows) = clamp_grid(cols, rows)?;

        let cwd = match task.working_dir_override.as_deref() {
            Some(override_dir) if !override_dir.trim().is_empty() => {
                app_working_dir.join(override_dir)
            }
            _ => app_working_dir.to_path_buf(),
        };
        if !cwd.is_dir() {
            return Err(SpawnSpecError::MissingWorkingDir {
                path: cwd.display().to_string(),
            });
        }

        let (program, args, display) = match task.command_kind {
            CommandKind::Script => {
                let script = task
                    .script
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| SpawnSpecError::MissingScript {
                        task_id: task.id.to_string(),
                    })?;
                let pm = task
                    .package_manager_override
                    .or(package_manager)
                    .ok_or_else(|| SpawnSpecError::MissingPackageManager {
                        task_id: task.id.to_string(),
                    })?;
                let program = pm.as_str().to_string();
                let args = vec!["run".to_string(), script.to_string()];
                let display = format!("{program} run {script}");
                (program, args, display)
            }
            CommandKind::Custom => {
                let command = task
                    .custom_command
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| SpawnSpecError::MissingCommand {
                        task_id: task.id.to_string(),
                    })?;
                // Only an explicit custom command goes through a shell — that is what the
                // user asked for, and it is never used for script tasks.
                let shell = env
                    .get("SHELL")
                    .cloned()
                    .unwrap_or_else(|| "/bin/zsh".to_string());
                (
                    shell,
                    vec!["-c".to_string(), command.to_string()],
                    command.to_string(),
                )
            }
        };

        Ok(SpawnSpec {
            program,
            args,
            cwd,
            env,
            initial_cols: cols,
            initial_rows: rows,
            display,
        })
    }
}

/// Reject grids a terminal cannot use and clamp legitimately small ones.
pub fn clamp_grid(cols: u16, rows: u16) -> Result<(u16, u16), SpawnSpecError> {
    if cols == 0 || rows == 0 {
        return Err(SpawnSpecError::InvalidGrid { cols, rows });
    }
    Ok((cols.max(MIN_COLS), rows.max(MIN_ROWS)))
}

/// Grid the visible terminal asks the PTY for. Always at least 2×2.
pub fn resize_grid(cols: u16, rows: u16) -> Result<(u16, u16), SpawnSpecError> {
    clamp_grid(cols, rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use neohub_domain::{ReadinessSignal, TaskId};

    fn task(kind: CommandKind, script: Option<&str>, command: Option<&str>) -> Task {
        Task {
            id: TaskId::from_existing("task_1"),
            app_id: neohub_domain::AppId::from_existing("app_1"),
            name: "dev".into(),
            position: 0,
            command_kind: kind,
            script: script.map(str::to_owned),
            custom_command: command.map(str::to_owned),
            working_dir_override: None,
            package_manager_override: None,
            node_version_pref_override: None,
            depends_on: vec![],
            readiness: ReadinessSignal::None,
            one_shot: false,
            enabled: true,
            env_overrides: Default::default(),
            created_at: 0,
            updated_at: 0,
        }
    }

    fn env() -> BTreeMap<String, String> {
        BTreeMap::from([("SHELL".to_string(), "/bin/zsh".to_string())])
    }

    #[test]
    fn a_script_task_becomes_an_argument_vector_not_a_shell_string() {
        let dir = tempfile::tempdir().unwrap();
        let spec = SpawnSpec::for_task(
            &task(CommandKind::Script, Some("dev"), None),
            dir.path(),
            env(),
            Some(PackageManager::Pnpm),
            DEFAULT_COLS,
            DEFAULT_ROWS,
        )
        .expect("spec");
        assert_eq!(spec.program, "pnpm");
        assert_eq!(spec.args, vec!["run".to_string(), "dev".to_string()]);
        assert_eq!(spec.display, "pnpm run dev");
        assert_eq!(spec.cwd, dir.path());
        assert_eq!((spec.initial_cols, spec.initial_rows), (120, 30));
    }

    #[test]
    fn only_a_custom_command_goes_through_a_shell() {
        let dir = tempfile::tempdir().unwrap();
        let spec = SpawnSpec::for_task(
            &task(CommandKind::Custom, None, Some("node ./start.js --watch")),
            dir.path(),
            env(),
            Some(PackageManager::Npm),
            DEFAULT_COLS,
            DEFAULT_ROWS,
        )
        .expect("spec");
        assert_eq!(spec.program, "/bin/zsh");
        assert_eq!(spec.args, vec!["-c", "node ./start.js --watch"]);
    }

    #[test]
    fn a_task_package_manager_overrides_the_app_one() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = task(CommandKind::Script, Some("start"), None);
        t.package_manager_override = Some(PackageManager::Bun);
        let spec = SpawnSpec::for_task(
            &t,
            dir.path(),
            env(),
            Some(PackageManager::Npm),
            DEFAULT_COLS,
            DEFAULT_ROWS,
        )
        .expect("spec");
        assert_eq!(spec.program, "bun");
    }

    #[test]
    fn a_working_dir_override_is_relative_to_the_app_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("packages/api")).unwrap();
        let mut t = task(CommandKind::Script, Some("dev"), None);
        t.working_dir_override = Some("packages/api".into());
        let spec = SpawnSpec::for_task(
            &t,
            dir.path(),
            env(),
            Some(PackageManager::Npm),
            DEFAULT_COLS,
            DEFAULT_ROWS,
        )
        .expect("spec");
        assert_eq!(spec.cwd, dir.path().join("packages/api"));
    }

    #[test]
    fn missing_inputs_are_reported_with_the_field_that_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            SpawnSpec::for_task(
                &task(CommandKind::Script, None, None),
                dir.path(),
                env(),
                Some(PackageManager::Npm),
                DEFAULT_COLS,
                DEFAULT_ROWS
            ),
            Err(SpawnSpecError::MissingScript { .. })
        ));
        assert!(matches!(
            SpawnSpec::for_task(
                &task(CommandKind::Custom, None, None),
                dir.path(),
                env(),
                Some(PackageManager::Npm),
                DEFAULT_COLS,
                DEFAULT_ROWS
            ),
            Err(SpawnSpecError::MissingCommand { .. })
        ));
        assert!(matches!(
            SpawnSpec::for_task(
                &task(CommandKind::Script, Some("dev"), None),
                dir.path(),
                env(),
                None,
                DEFAULT_COLS,
                DEFAULT_ROWS
            ),
            Err(SpawnSpecError::MissingPackageManager { .. })
        ));
    }

    #[test]
    fn a_missing_working_directory_is_refused_before_spawning() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = task(CommandKind::Script, Some("dev"), None);
        t.working_dir_override = Some("gone".into());
        assert!(matches!(
            SpawnSpec::for_task(
                &t,
                dir.path(),
                env(),
                Some(PackageManager::Npm),
                DEFAULT_COLS,
                DEFAULT_ROWS
            ),
            Err(SpawnSpecError::MissingWorkingDir { .. })
        ));
    }

    #[test]
    fn the_grid_never_drops_below_two_by_two() {
        assert_eq!(resize_grid(1, 1).unwrap(), (2, 2));
        assert_eq!(resize_grid(80, 24).unwrap(), (80, 24));
        assert!(resize_grid(0, 24).is_err());
        assert!(resize_grid(80, 0).is_err());
    }
}
