//! Dependency topology: which tasks may start in parallel, and in what order to stop them.
//!
//! Ported from `src/main/services/topo.ts`. Levels are computed with Kahn's algorithm: every
//! task in level *N* may run in parallel because it depends only on levels `0..N-1`. A cycle
//! or an unknown dependency is a hard error naming the tasks involved — the UI shows it
//! instead of starting half a stack.

use neohub_domain::{Task, TaskId};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TopologyError {
    #[error("task {task} depends on unknown task {missing}")]
    UnknownDependency { task: TaskId, missing: TaskId },

    #[error("dependency cycle detected among tasks: {tasks:?}")]
    Cycle { tasks: Vec<TaskId> },
}

/// Group task ids into parallelisable levels.
pub fn topo_levels(
    nodes: &[TaskId],
    deps: &BTreeMap<TaskId, Vec<TaskId>>,
) -> Result<Vec<Vec<TaskId>>, TopologyError> {
    let known: BTreeSet<&TaskId> = nodes.iter().collect();

    // Validate dependencies first so the error names the offending task, not a cycle.
    for node in nodes {
        for dep in deps.get(node).into_iter().flatten() {
            if !known.contains(dep) {
                return Err(TopologyError::UnknownDependency {
                    task: node.clone(),
                    missing: dep.clone(),
                });
            }
            if dep == node {
                return Err(TopologyError::Cycle {
                    tasks: vec![node.clone()],
                });
            }
        }
    }

    let mut in_degree: BTreeMap<TaskId, usize> = nodes.iter().map(|id| (id.clone(), 0)).collect();
    let mut dependents: BTreeMap<TaskId, Vec<TaskId>> =
        nodes.iter().map(|id| (id.clone(), Vec::new())).collect();
    for node in nodes {
        for dep in deps.get(node).into_iter().flatten() {
            dependents
                .get_mut(dep)
                .expect("validated above")
                .push(node.clone());
            *in_degree.get_mut(node).expect("present") += 1;
        }
    }

    let mut levels: Vec<Vec<TaskId>> = Vec::new();
    let mut remaining: BTreeSet<TaskId> = nodes.iter().cloned().collect();
    while !remaining.is_empty() {
        let level: Vec<TaskId> = remaining
            .iter()
            .filter(|id| in_degree.get(*id).copied().unwrap_or(0) == 0)
            .cloned()
            .collect();
        if level.is_empty() {
            return Err(TopologyError::Cycle {
                tasks: remaining.into_iter().collect(),
            });
        }
        for id in &level {
            remaining.remove(id);
            for dependent in dependents.get(id).into_iter().flatten() {
                if let Some(degree) = in_degree.get_mut(dependent) {
                    *degree = degree.saturating_sub(1);
                }
            }
        }
        levels.push(level);
    }
    Ok(levels)
}

/// Start plan: enabled tasks grouped into levels, each level ordered by `position` (stable,
/// so two tasks that can run in parallel still start in the user's order).
///
/// Disabled tasks are never started, and their dependencies are not "inherited": a disabled
/// task simply does not appear in the graph.
pub fn plan_start(tasks: &[Task]) -> Result<Vec<Vec<Task>>, TopologyError> {
    let enabled: Vec<&Task> = tasks.iter().filter(|task| task.enabled).collect();
    let ids: Vec<TaskId> = enabled.iter().map(|task| task.id.clone()).collect();
    let id_set: BTreeSet<&TaskId> = ids.iter().collect();

    let mut deps: BTreeMap<TaskId, Vec<TaskId>> = BTreeMap::new();
    for task in &enabled {
        let filtered: Vec<TaskId> = task
            .depends_on
            .iter()
            .filter(|dep| id_set.contains(dep))
            .cloned()
            .collect();
        deps.insert(task.id.clone(), filtered);
    }

    let levels = topo_levels(&ids, &deps)?;
    let by_id: BTreeMap<&TaskId, &Task> = enabled.iter().map(|task| (&task.id, *task)).collect();
    Ok(levels
        .into_iter()
        .map(|level| {
            let mut row: Vec<Task> = level
                .into_iter()
                .filter_map(|id| by_id.get(&id).map(|task| (**task).clone()))
                .collect();
            row.sort_by_key(|task| (task.position, task.created_at));
            row
        })
        .collect())
}

/// Stop plan: the same levels, reversed — dependents stop before their dependencies.
pub fn plan_stop(levels: &[Vec<Task>]) -> Vec<Vec<TaskId>> {
    levels
        .iter()
        .rev()
        .map(|level| level.iter().map(|task| task.id.clone()).collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use neohub_domain::{AppId, CommandKind, ReadinessSignal};

    fn task(id: &str, position: i64, deps: &[&str], enabled: bool) -> Task {
        Task {
            id: TaskId::from_existing(id),
            app_id: AppId::from_existing("app"),
            name: id.to_string(),
            position,
            command_kind: CommandKind::Script,
            script: Some("dev".into()),
            custom_command: None,
            working_dir_override: None,
            package_manager_override: None,
            node_version_pref_override: None,
            depends_on: deps.iter().map(|d| TaskId::from_existing(*d)).collect(),
            readiness: ReadinessSignal::None,
            one_shot: false,
            enabled,
            env_overrides: Default::default(),
            created_at: position,
            updated_at: position,
        }
    }

    fn ids(level: &[Task]) -> Vec<&str> {
        level.iter().map(|task| task.id.as_str()).collect()
    }

    #[test]
    fn independent_tasks_share_one_level() {
        let tasks = vec![task("db", 0, &[], true), task("api", 1, &[], true)];
        let levels = plan_start(&tasks).expect("plan");
        assert_eq!(levels.len(), 1);
        assert_eq!(ids(&levels[0]), vec!["db", "api"], "ordered by position");
    }

    #[test]
    fn a_chain_becomes_one_task_per_level() {
        let tasks = vec![
            task("db", 0, &[], true),
            task("api", 1, &["db"], true),
            task("web", 2, &["api"], true),
        ];
        let levels = plan_start(&tasks).expect("plan");
        assert_eq!(levels.len(), 3);
        assert_eq!(ids(&levels[0]), vec!["db"]);
        assert_eq!(ids(&levels[1]), vec!["api"]);
        assert_eq!(ids(&levels[2]), vec!["web"]);
    }

    #[test]
    fn a_diamond_runs_the_middle_tasks_in_parallel() {
        let tasks = vec![
            task("db", 0, &[], true),
            task("api", 1, &["db"], true),
            task("worker", 2, &["db"], true),
            task("web", 3, &["api", "worker"], true),
        ];
        let levels = plan_start(&tasks).expect("plan");
        assert_eq!(levels.len(), 3);
        assert_eq!(ids(&levels[1]), vec!["api", "worker"]);
        assert_eq!(ids(&levels[2]), vec!["web"]);
    }

    #[test]
    fn disabled_tasks_are_skipped_without_breaking_the_graph() {
        let tasks = vec![task("db", 0, &[], false), task("api", 1, &["db"], true)];
        let levels = plan_start(&tasks).expect("a dependency on a disabled task is dropped");
        assert_eq!(levels.len(), 1);
        assert_eq!(ids(&levels[0]), vec!["api"]);
    }

    #[test]
    fn a_cycle_is_reported_with_the_tasks_in_it() {
        let tasks = vec![
            task("a", 0, &["c"], true),
            task("b", 1, &["a"], true),
            task("c", 2, &["b"], true),
        ];
        match plan_start(&tasks) {
            Err(TopologyError::Cycle { tasks }) => {
                let names: Vec<&str> = tasks.iter().map(|t| t.as_str()).collect();
                assert_eq!(names.len(), 3);
            }
            other => panic!("expected a cycle, got {other:?}"),
        }
    }

    #[test]
    fn a_self_dependency_is_a_cycle() {
        let tasks = vec![task("a", 0, &["a"], true)];
        assert!(matches!(
            plan_start(&tasks),
            Err(TopologyError::Cycle { .. })
        ));
    }

    #[test]
    fn an_unknown_dependency_names_both_tasks() {
        // `topo_levels` is the strict layer: it validates every edge it is given.
        let nodes = vec![TaskId::from_existing("api")];
        let mut deps = BTreeMap::new();
        deps.insert(
            TaskId::from_existing("api"),
            vec![TaskId::from_existing("ghost")],
        );
        match topo_levels(&nodes, &deps) {
            Err(TopologyError::UnknownDependency { task, missing }) => {
                assert_eq!(task.as_str(), "api");
                assert_eq!(missing.as_str(), "ghost");
            }
            other => panic!("expected unknown dependency, got {other:?}"),
        }
    }

    #[test]
    fn plan_start_ignores_edges_to_tasks_that_are_not_enabled() {
        // The catalog refuses to store a dependency on a task that does not exist, so the
        // only edge `plan_start` can see here is one pointing at a disabled task — the same
        // filtering the Electron orchestrator applied (`dependsOn.filter(ids.includes)`).
        let tasks = vec![
            task("db", 0, &[], false),
            task("api", 1, &["db"], true),
            task("web", 2, &["api", "ghost"], true),
        ];
        let levels = plan_start(&tasks).expect("unknown-but-filtered edges do not fail the plan");
        let flattened: Vec<&str> = levels.iter().flatten().map(|t| t.id.as_str()).collect();
        assert_eq!(flattened, vec!["api", "web"]);
    }

    #[test]
    fn stop_order_is_the_reverse_of_start_order() {
        let tasks = vec![
            task("db", 0, &[], true),
            task("api", 1, &["db"], true),
            task("web", 2, &["api"], true),
        ];
        let levels = plan_start(&tasks).expect("plan");
        let stop = plan_stop(&levels);
        let flattened: Vec<&str> = stop.iter().flatten().map(|id| id.as_str()).collect();
        assert_eq!(flattened, vec!["web", "api", "db"]);
    }

    #[test]
    fn an_app_with_no_enabled_tasks_plans_nothing() {
        let tasks = vec![task("db", 0, &[], false)];
        assert!(plan_start(&tasks).expect("plan").is_empty());
    }
}
