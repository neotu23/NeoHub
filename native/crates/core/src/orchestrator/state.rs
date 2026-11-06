//! App-level state derived from its tasks, plus the sticky outcome that keeps a stopped app
//! reading "stopped" (or "crashed") instead of flickering back to idle.
//!
//! Ported from `AppOrchestrator.deriveAppState` and the `lastOutcome` map: the Electron
//! build fixed a real bug where an app showed Idle right after Stop because the derived state
//! fell through to idle once tasks were torn down.

use neohub_domain::{AppId, ProcessState, Task};
use std::collections::HashMap;

/// Combine per-task states into one app state.
///
/// Precedence: crashed > exiting > starting/not-ready > running > exited > idle.
pub fn derive_app_state(
    tasks: &[Task],
    state_of: impl Fn(&neohub_domain::TaskId) -> ProcessState,
) -> ProcessState {
    let mut any_crashed = false;
    let mut any_exiting = false;
    let mut any_starting = false;
    let mut any_running = false;
    let mut any_exited = false;

    for task in tasks {
        if !task.enabled {
            continue;
        }
        match state_of(&task.id) {
            ProcessState::Crashed => any_crashed = true,
            ProcessState::Exiting => any_exiting = true,
            ProcessState::Starting => any_starting = true,
            ProcessState::Running => any_running = true,
            ProcessState::Exited => any_exited = true,
            ProcessState::Idle => {}
        }
    }

    if any_crashed {
        return ProcessState::Crashed;
    }
    if any_exiting {
        return ProcessState::Exiting;
    }
    if any_starting {
        return ProcessState::Starting;
    }
    if any_running {
        return ProcessState::Running;
    }
    if any_exited {
        return ProcessState::Exited;
    }
    ProcessState::Idle
}

/// Sticky per-app outcome: survives until the next successful start.
#[derive(Default)]
pub struct StickyOutcomes {
    outcomes: HashMap<AppId, ProcessState>,
}

impl StickyOutcomes {
    pub fn new() -> Self {
        Self::default()
    }

    /// Called when an app finishes starting a run (clears any previous outcome).
    pub fn clear(&mut self, app_id: &AppId) {
        self.outcomes.remove(app_id);
    }

    /// Record a terminal outcome. `Crashed` wins over `Exited` for the same run.
    pub fn record(&mut self, app_id: &AppId, state: ProcessState) {
        match state {
            ProcessState::Crashed => {
                self.outcomes.insert(app_id.clone(), ProcessState::Crashed);
            }
            ProcessState::Exited => {
                self.outcomes
                    .entry(app_id.clone())
                    .or_insert(ProcessState::Exited);
            }
            _ => {}
        }
    }

    pub fn get(&self, app_id: &AppId) -> Option<ProcessState> {
        self.outcomes.get(app_id).copied()
    }

    /// App state with the sticky outcome applied: an app whose tasks are all idle but which
    /// ran this session reads as `exited`/`crashed`.
    pub fn apply(&self, app_id: &AppId, derived: ProcessState) -> ProcessState {
        if derived == ProcessState::Idle
            && let Some(outcome) = self.get(app_id)
        {
            return outcome;
        }
        derived
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neohub_domain::{CommandKind, ReadinessSignal, TaskId};

    fn task(id: &str, enabled: bool) -> Task {
        Task {
            id: TaskId::from_existing(id),
            app_id: AppId::from_existing("app"),
            name: id.into(),
            position: 0,
            command_kind: CommandKind::Script,
            script: Some("dev".into()),
            custom_command: None,
            working_dir_override: None,
            package_manager_override: None,
            node_version_pref_override: None,
            depends_on: vec![],
            readiness: ReadinessSignal::None,
            one_shot: false,
            enabled,
            env_overrides: Default::default(),
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn crashed_wins_over_everything() {
        let tasks = vec![task("a", true), task("b", true)];
        let state = derive_app_state(&tasks, |id| match id.as_str() {
            "a" => ProcessState::Running,
            _ => ProcessState::Crashed,
        });
        assert_eq!(state, ProcessState::Crashed);
    }

    #[test]
    fn any_starting_task_makes_the_app_starting() {
        let tasks = vec![task("a", true), task("b", true)];
        let state = derive_app_state(&tasks, |id| match id.as_str() {
            "a" => ProcessState::Running,
            _ => ProcessState::Starting,
        });
        assert_eq!(state, ProcessState::Starting);
    }

    #[test]
    fn all_idle_is_idle_and_all_exited_is_exited() {
        let tasks = vec![task("a", true)];
        assert_eq!(
            derive_app_state(&tasks, |_| ProcessState::Idle),
            ProcessState::Idle
        );
        assert_eq!(
            derive_app_state(&tasks, |_| ProcessState::Exited),
            ProcessState::Exited,
            "a torn-down run must not read as idle"
        );
    }

    #[test]
    fn disabled_tasks_do_not_influence_the_app_state() {
        let tasks = vec![task("a", true), task("b", false)];
        let state = derive_app_state(&tasks, |id| match id.as_str() {
            "a" => ProcessState::Running,
            _ => ProcessState::Crashed,
        });
        assert_eq!(state, ProcessState::Running);
    }

    #[test]
    fn a_sticky_outcome_survives_teardown_and_clears_on_the_next_run() {
        let app = AppId::from_existing("app");
        let mut sticky = StickyOutcomes::new();
        assert_eq!(sticky.apply(&app, ProcessState::Idle), ProcessState::Idle);

        sticky.record(&app, ProcessState::Exited);
        assert_eq!(sticky.apply(&app, ProcessState::Idle), ProcessState::Exited);
        assert_eq!(
            sticky.apply(&app, ProcessState::Running),
            ProcessState::Running,
            "a live state always wins"
        );

        sticky.clear(&app);
        assert_eq!(sticky.apply(&app, ProcessState::Idle), ProcessState::Idle);
    }

    #[test]
    fn crashed_sticks_even_after_an_exited_is_recorded() {
        let app = AppId::from_existing("app");
        let mut sticky = StickyOutcomes::new();
        sticky.record(&app, ProcessState::Exited);
        sticky.record(&app, ProcessState::Crashed);
        assert_eq!(sticky.get(&app), Some(ProcessState::Crashed));
    }
}
