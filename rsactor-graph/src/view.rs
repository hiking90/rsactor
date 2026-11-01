// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! `--focus`: keep only the neighbourhood of one actor.
//!
//! Unlike the DOT readability options, focus narrows every output format,
//! because the user asked for a smaller scope.

use std::collections::{BTreeSet, HashMap};

use crate::model::{Node, Output};

/// Keeps the nodes within `depth` edges of the actor named `query`,
/// following edges in both directions. `query` matches an actor's full path
/// or, if unique, its trailing segments (`Router`, `net::Router`).
///
/// Only edges the search walked are kept: at least one end must be closer
/// than `depth` to the focused actor. With `depth` 1 that is the edges
/// touching it. Keeping every edge among the kept nodes would also draw how
/// the neighbours talk to each other, which for a hub actor (a database
/// actor everyone asks) is most of the graph again.
pub fn focus(output: Output, query: &str, depth: usize) -> Result<Output, String> {
    let suffix = format!("::{query}");
    let matches: Vec<&str> = output
        .actors()
        .filter(|a| a.path == query || a.path.ends_with(&suffix))
        .map(|a| a.id.as_str())
        .collect();
    let start = match matches.as_slice() {
        [only] => only.to_string(),
        [] => return Err(format!("no actor matches `{query}`")),
        several => {
            let paths: Vec<String> = several
                .iter()
                .filter_map(|id| match output.node(id) {
                    Some(Node::Actor(a)) => Some(a.path.clone()),
                    _ => None,
                })
                .collect();
            return Err(format!(
                "`{query}` matches several actors: {}",
                paths.join(", ")
            ));
        }
    };

    let mut neighbours: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in &output.graph.edges {
        neighbours
            .entry(edge.from.as_str())
            .or_default()
            .push(edge.to.as_str());
        neighbours
            .entry(edge.to.as_str())
            .or_default()
            .push(edge.from.as_str());
    }
    let mut distance: HashMap<String, usize> = HashMap::from([(start.clone(), 0)]);
    let mut frontier = vec![start];
    for step in 1..=depth {
        let mut next = Vec::new();
        for id in &frontier {
            for n in neighbours.get(id.as_str()).into_iter().flatten() {
                if !distance.contains_key(*n) {
                    distance.insert(n.to_string(), step);
                    next.push(n.to_string());
                }
            }
        }
        frontier = next;
    }
    let kept: BTreeSet<String> = distance.keys().cloned().collect();
    let walked = |from: &str, to: &str| match (distance.get(from), distance.get(to)) {
        (Some(a), Some(b)) => (*a).min(*b) < depth,
        _ => false,
    };

    let Output {
        schema_version,
        name,
        mut graph,
        report,
    } = output;
    graph.nodes.retain(|n| kept.contains(n.id()));
    graph.edges.retain(|e| walked(&e.from, &e.to));
    Ok(Output {
        schema_version,
        name,
        graph,
        report,
    })
}
