// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Runtime deadlock detection for `ask` cycles.
//!
//! The whole module is gated behind the `deadlock-detection` feature (it is only
//! declared when the feature is on), so individual items need no per-item
//! `#[cfg(...)]` and nothing here costs anything in normal builds.
//!
//! Every in-flight `ask`/`ask_priority` registers a `caller -> callee` edge in a
//! process-global wait-for graph via [`register_ask_edge`]. If a new edge would
//! close a cycle, registration panics with the cycle path — the documented
//! behavior of this feature.
//!
//! # Edge removal — two cooperating sides
//!
//! An edge `A -> B` means "A's handler cannot make progress until B replies".
//! That statement stops being true the moment B *sends* the reply, not when A's
//! task is next polled. Removing the edge only from the caller side (when the
//! ask future resumes and drops its guard) leaves a stale-edge window between
//! reply-send and caller-resume; if B's next handler asks A inside that window,
//! the stale edge closes a phantom cycle and the detector panics on a perfectly
//! healthy system. To close the window, every edge is removed by whichever of
//! two owners fires first (idempotent via an atomic flag on the shared
//! [`AskEdge`]):
//!
//! - [`AskEdgeToken`], carried inside the ask envelope: dropped by the callee's
//!   runtime right after the reply was sent (or when the envelope itself is
//!   dropped unprocessed, which also unblocks the caller).
//! - [`WaitForGuard`], held by the caller's ask future: backstop for the paths
//!   the token cannot see — timeout, caller-side cancellation, or a send that
//!   never reached the mailbox.

use crate::Identity;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

tokio::task_local! {
    /// Identity of the actor whose handler is currently running, used as the
    /// `caller` when it issues an `ask`. Unset outside an actor context.
    pub(crate) static CURRENT_ACTOR: Identity;
}

/// Global wait-for graph.
///
/// Key: a waiting actor's ID. Value: the list of callee identities that actor
/// is currently blocked on — one entry per outstanding `ask`/`ask_priority`.
///
/// The value is a `Vec` (multiset), not a single edge, so a handler can hold
/// several concurrent asks (e.g. `tokio::join!(a.ask(..), b.ask(..))`) without
/// the edges clobbering each other. Each [`WaitForGuard`] removes exactly the
/// one edge it inserted, and two concurrent asks to the *same* callee are
/// reference-counted by the two separate entries.
static WAIT_FOR: OnceLock<Mutex<HashMap<u64, Vec<Identity>>>> = OnceLock::new();

fn wait_for_graph() -> &'static Mutex<HashMap<u64, Vec<Identity>>> {
    WAIT_FOR.get_or_init(|| Mutex::new(HashMap::new()))
}

/// One registered wait-for edge, shared between the caller-side
/// [`WaitForGuard`] and the callee-side [`AskEdgeToken`]. The `removed` flag
/// makes removal idempotent so the two owners cannot double-remove an edge
/// that a concurrent identical ask re-inserted (the graph is a multiset).
pub(crate) struct AskEdge {
    caller: u64,
    callee_id: u64,
    removed: AtomicBool,
}

impl AskEdge {
    /// Removes the edge from the global graph exactly once, no matter how many
    /// owners call this. Recovers a poisoned lock the same way insertion does —
    /// skipping removal on poison would leak the edge permanently and turn
    /// every later ask through these actors into a false-positive panic.
    fn remove_once(&self) {
        if self.removed.swap(true, Ordering::AcqRel) {
            return;
        }
        let mut graph = wait_for_graph().lock().unwrap_or_else(|e| e.into_inner());
        remove_edge(&mut graph, self.caller, self.callee_id);
    }
}

/// Caller-side RAII guard for a wait-for edge. Removes the edge when the `ask`
/// future completes or is dropped (reply received, timed out, or cancelled),
/// unless the callee-side [`AskEdgeToken`] already removed it at reply time.
pub(crate) struct WaitForGuard {
    edge: Arc<AskEdge>,
}

impl WaitForGuard {
    /// Returns the callee-side token to embed in the ask envelope. The actor
    /// runtime drops it right after the reply is sent, removing the edge at
    /// the earliest moment the wait is actually over.
    pub(crate) fn token(&self) -> AskEdgeToken {
        AskEdgeToken {
            edge: self.edge.clone(),
        }
    }
}

impl Drop for WaitForGuard {
    fn drop(&mut self) {
        self.edge.remove_once();
    }
}

/// Callee-side handle to a wait-for edge, carried inside the ask envelope.
/// Dropping it removes the edge — the runtime lets it fall out of scope as
/// soon as the reply has been sent. If the envelope is dropped unprocessed
/// (actor killed before reaching it), the drop is equally correct: the reply
/// channel closes at the same time, so the caller is no longer waiting.
pub(crate) struct AskEdgeToken {
    edge: Arc<AskEdge>,
}

impl Drop for AskEdgeToken {
    fn drop(&mut self) {
        self.edge.remove_once();
    }
}

/// Removes a single `caller -> callee` edge from the graph (multiset semantics).
///
/// Two concurrent asks to the same callee push two entries; this removes only
/// one of them, and clears the key once the last edge is gone. Factored out of
/// [`WaitForGuard::drop`] so it can be unit-tested without the process-global
/// graph.
fn remove_edge(graph: &mut HashMap<u64, Vec<Identity>>, caller: u64, callee_id: u64) {
    if let Some(edges) = graph.get_mut(&caller) {
        if let Some(pos) = edges.iter().position(|c| c.id == callee_id) {
            edges.swap_remove(pos);
        }
        if edges.is_empty() {
            graph.remove(&caller);
        }
    }
}

/// Registers a `caller -> callee` wait-for edge for an outstanding ask and
/// returns a guard that removes it on drop.
///
/// Panics (the documented deadlock-detection behavior) if adding the edge would
/// close a cycle. Returns `None` when called outside an actor context — there is
/// no current actor to attribute the wait to, so no cycle can form through it.
#[must_use = "dropping the guard immediately removes the wait-for edge, disabling detection for this ask"]
pub(crate) fn register_ask_edge(callee: Identity, operation: &str) -> Option<WaitForGuard> {
    let caller = CURRENT_ACTOR.try_with(|id| *id).ok()?;
    let mut graph = wait_for_graph().lock().unwrap_or_else(|e| e.into_inner());
    // The `contains_key` pre-check skips the DFS (and its two allocations) in
    // the common case where the callee is not itself waiting on anyone: with
    // no outgoing edge from the callee and `caller != callee`, no path back to
    // the caller can exist.
    if caller.id == callee.id
        || (graph.contains_key(&callee.id) && has_path(&graph, callee.id, caller.id))
    {
        let cycle = format_cycle_path(&graph, caller, callee);
        drop(graph);
        panic!(
            "Deadlock detected: {operation} cycle {cycle}\n\
             This is a design error. Use `tell` to break the cycle, \
             or restructure actor dependencies."
        );
    }
    graph.entry(caller.id).or_default().push(callee);
    Some(WaitForGuard {
        edge: Arc::new(AskEdge {
            caller: caller.id,
            callee_id: callee.id,
            removed: AtomicBool::new(false),
        }),
    })
}

/// Returns `true` if a directed path exists from `from` to `to` in the wait-for
/// graph. Iterative DFS with a visited set, so branching (concurrent asks) and
/// pre-existing cycles cannot cause unbounded traversal.
///
/// Self-ask (`caller == callee`) is checked separately by [`register_ask_edge`].
fn has_path(graph: &HashMap<u64, Vec<Identity>>, from: u64, to: u64) -> bool {
    let mut stack = vec![from];
    let mut visited = HashSet::new();
    while let Some(current) = stack.pop() {
        if current == to {
            return true;
        }
        if !visited.insert(current) {
            continue;
        }
        if let Some(edges) = graph.get(&current) {
            for callee in edges {
                stack.push(callee.id);
            }
        }
    }
    false
}

/// Format the cycle path for panic messages: `caller -> callee -> ... -> caller`.
///
/// The `caller -> callee` edge is the one about to be added; the remainder is an
/// existing path from `callee` back to `caller` (which [`has_path`] just
/// confirmed exists). Only ever called on the cold panic path.
fn format_cycle_path(
    graph: &HashMap<u64, Vec<Identity>>,
    caller: Identity,
    callee: Identity,
) -> String {
    if caller.id == callee.id {
        return format!("{caller} -> {caller}");
    }
    let mut path = vec![caller.to_string()];
    match find_path(graph, callee, caller.id) {
        Some(nodes) => path.extend(nodes.iter().map(|id| id.to_string())),
        // Defensive: has_path returned true, so a path should exist.
        None => path.push(callee.to_string()),
    }
    path.join(" -> ")
}

/// DFS that returns an actual `start -> ... -> target` path (inclusive of both
/// endpoints) if one exists, reconstructing the identities along the way. Cold
/// path only (panic formatting), so cloning the partial path per branch is fine.
fn find_path(
    graph: &HashMap<u64, Vec<Identity>>,
    start: Identity,
    target: u64,
) -> Option<Vec<Identity>> {
    let mut stack: Vec<Vec<Identity>> = vec![vec![start]];
    let mut visited = HashSet::new();
    while let Some(path) = stack.pop() {
        let current = *path.last().expect("path is never empty");
        if current.id == target {
            return Some(path);
        }
        if !visited.insert(current.id) {
            continue;
        }
        if let Some(edges) = graph.get(&current.id) {
            for callee in edges {
                let mut next = path.clone();
                next.push(*callee);
                stack.push(next);
            }
        }
    }
    None
}

#[cfg(test)]
mod deadlock_graph_tests {
    use super::{find_path, format_cycle_path, has_path, remove_edge, Identity};
    use std::collections::HashMap;

    fn id(n: u64) -> Identity {
        // Map ids to stable static type-name literals.
        let name: &'static str = match n {
            1 => "A",
            2 => "B",
            3 => "C",
            4 => "D",
            _ => "X",
        };
        Identity::new(n, name)
    }

    /// Builds a graph from `(caller, [callees])` pairs.
    fn graph_of(edges: &[(u64, &[u64])]) -> HashMap<u64, Vec<Identity>> {
        let mut g = HashMap::new();
        for (caller, callees) in edges {
            g.insert(*caller, callees.iter().map(|c| id(*c)).collect());
        }
        g
    }

    #[test]
    fn has_path_linear_chain() {
        let g = graph_of(&[(1, &[2]), (2, &[3])]);
        assert!(has_path(&g, 1, 3));
        assert!(has_path(&g, 1, 2));
        assert!(!has_path(&g, 3, 1));
    }

    #[test]
    fn has_path_follows_every_branch() {
        // Concurrent asks from actor 1 to both 2 and 3; 3 then waits on 4.
        let g = graph_of(&[(1, &[2, 3]), (3, &[4])]);
        assert!(has_path(&g, 1, 4), "must reach 4 through the second branch");
        assert!(has_path(&g, 1, 2));
        assert!(!has_path(&g, 2, 4), "no edge out of 2");
    }

    #[test]
    fn has_path_terminates_on_preexisting_cycle() {
        // 1 <-> 2 cycle must not loop forever when searching for an absent node.
        let g = graph_of(&[(1, &[2]), (2, &[1])]);
        assert!(!has_path(&g, 1, 99));
        assert!(has_path(&g, 1, 2));
    }

    #[test]
    fn find_path_reconstructs_identities() {
        // Existing path 2 -> 3 -> 1 (the back-edge that closes a 1->2 ask cycle).
        let g = graph_of(&[(2, &[3]), (3, &[1])]);
        let path = find_path(&g, id(2), 1).expect("path 2->3->1 exists");
        let ids: Vec<u64> = path.iter().map(|i| i.id).collect();
        assert_eq!(ids, vec![2, 3, 1]);
    }

    #[test]
    fn format_cycle_path_renders_full_cycle() {
        // Adding edge A(1) -> B(2) closes the cycle because 2 -> 1 already exists.
        let g = graph_of(&[(2, &[1])]);
        let rendered = format_cycle_path(&g, id(1), id(2));
        assert_eq!(rendered, "A(#1) -> B(#2) -> A(#1)");
    }

    #[test]
    fn format_cycle_path_self_ask() {
        let g = HashMap::new();
        assert_eq!(format_cycle_path(&g, id(1), id(1)), "A(#1) -> A(#1)");
    }

    #[test]
    fn remove_edge_drops_only_one_distinct_edge() {
        // Regression for the concurrent-ask bug: edges 1->2 and 1->3 coexist,
        // and dropping the guard for 1->2 must leave 1->3 intact.
        let mut g = graph_of(&[(1, &[2, 3])]);
        remove_edge(&mut g, 1, 2);
        let remaining: Vec<u64> = g.get(&1).unwrap().iter().map(|i| i.id).collect();
        assert_eq!(remaining, vec![3]);
    }

    #[test]
    fn remove_edge_is_multiset_for_duplicate_targets() {
        // Two concurrent asks to the same callee push two entries; one drop
        // leaves exactly one, not zero.
        let mut g = graph_of(&[(1, &[2, 2])]);
        remove_edge(&mut g, 1, 2);
        let remaining: Vec<u64> = g.get(&1).unwrap().iter().map(|i| i.id).collect();
        assert_eq!(remaining, vec![2]);
    }

    #[test]
    fn remove_edge_clears_empty_key() {
        let mut g = graph_of(&[(1, &[2])]);
        remove_edge(&mut g, 1, 2);
        assert!(
            !g.contains_key(&1),
            "key removed once its last edge is gone"
        );
    }

    #[test]
    fn remove_edge_missing_is_noop() {
        let mut g = graph_of(&[(1, &[2])]);
        remove_edge(&mut g, 1, 99); // no such callee
        remove_edge(&mut g, 42, 2); // no such caller
        assert_eq!(g.get(&1).unwrap().len(), 1);
    }
}
