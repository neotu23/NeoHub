//! Dependency-graph cycle detection.
//!
//! Ported from `src/main/services/topo.ts` behaviour: given every task id in one app and
//! its `dependsOn` edges, return one cycle as an ordered task-id path so the error message
//! can name the tasks involved (`A → B → A`). Phase 7 reuses this for start ordering.

use std::collections::{BTreeMap, BTreeSet};

/// Return one dependency cycle, if any.
///
/// `nodes` is every task in the app; `deps` maps a task to the tasks it waits for.
/// Edges pointing outside `nodes` are ignored here — the caller validates unknown and
/// self dependencies separately so the error message stays specific.
pub fn find_cycle<N>(nodes: &[N], deps: &BTreeMap<N, Vec<N>>) -> Option<Vec<N>>
where
    N: Clone + Eq + Ord,
{
    let known: BTreeSet<&N> = nodes.iter().collect();
    let mut state: BTreeMap<&N, Visit> = BTreeMap::new();
    let mut path: Vec<N> = Vec::new();

    for node in nodes {
        if !matches!(state.get(node), Some(Visit::Done))
            && let Some(cycle) = visit(node, deps, &known, &mut state, &mut path)
        {
            return Some(cycle);
        }
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Visit {
    InProgress,
    Done,
}

fn visit<'a, N>(
    node: &'a N,
    deps: &'a BTreeMap<N, Vec<N>>,
    known: &BTreeSet<&'a N>,
    state: &mut BTreeMap<&'a N, Visit>,
    path: &mut Vec<N>,
) -> Option<Vec<N>>
where
    N: Clone + Eq + Ord + 'a,
{
    match state.get(node) {
        Some(Visit::Done) => return None,
        Some(Visit::InProgress) => {
            // Found a back edge: report the slice of the current path that closes it.
            if let Some(start) = path.iter().position(|n| n == node) {
                let mut cycle: Vec<N> = path[start..].to_vec();
                cycle.push(node.clone());
                return Some(cycle);
            }
            return Some(vec![node.clone(), node.clone()]);
        }
        None => {}
    }

    state.insert(node, Visit::InProgress);
    path.push(node.clone());

    if let Some(children) = deps.get(node) {
        for child in children {
            if !known.contains(child) {
                continue;
            }
            let child_ref = known.get(child).copied().unwrap_or(child);
            if let Some(cycle) = visit(child_ref, deps, known, state, path) {
                return Some(cycle);
            }
        }
    }

    path.pop();
    state.insert(node, Visit::Done);
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph<'a>(
        edges: &'a [(&'a str, &'a [&'a str])],
    ) -> (Vec<&'a str>, BTreeMap<&'a str, Vec<&'a str>>) {
        let mut nodes = Vec::new();
        let mut map: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for (node, deps) in edges {
            nodes.push(*node);
            map.insert(*node, deps.to_vec());
        }
        (nodes, map)
    }

    #[test]
    fn acyclic_graph_has_no_cycle() {
        let (nodes, deps) = graph(&[("a", &[]), ("b", &["a"]), ("c", &["b"])]);
        assert_eq!(find_cycle(&nodes, &deps), None);
    }

    #[test]
    fn direct_two_node_cycle_is_reported_as_a_path() {
        let (nodes, deps) = graph(&[("a", &["b"]), ("b", &["a"])]);
        let cycle = find_cycle(&nodes, &deps).expect("cycle");
        assert_eq!(cycle.first(), cycle.last(), "path must close on itself");
        assert_eq!(cycle.len(), 3);
    }

    #[test]
    fn longer_cycle_is_detected() {
        let (nodes, deps) = graph(&[("a", &["c"]), ("b", &["a"]), ("c", &["b"])]);
        let cycle = find_cycle(&nodes, &deps).expect("cycle");
        assert!(cycle.len() >= 3, "cycle should involve all three nodes");
        assert_eq!(cycle.first(), cycle.last());
    }

    #[test]
    fn unknown_edges_are_ignored_not_crashing() {
        let (nodes, deps) = graph(&[("a", &["ghost"]), ("b", &["a"])]);
        assert_eq!(find_cycle(&nodes, &deps), None);
    }

    #[test]
    fn shared_dependency_is_not_a_cycle() {
        let (nodes, deps) = graph(&[("a", &[]), ("b", &["a"]), ("c", &["a"]), ("d", &["b", "c"])]);
        assert_eq!(find_cycle(&nodes, &deps), None);
    }
}
