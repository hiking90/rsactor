// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! DOT readability options and `--focus` over `tests/fixtures/view`.

use std::collections::BTreeSet;
use std::path::Path;

use rsactor_graph::model::{Node, Output};
use rsactor_graph::render::{to_dot_with, DotOptions, HoldsMode};
use rsactor_graph::view::focus;
use rsactor_graph::{scan, CrateSpec, ScanInput};

fn scan_view() -> Output {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/view");
    scan(&ScanInput {
        name: "view".to_string(),
        targets: vec![CrateSpec {
            name: "view".to_string(),
            target: "lib:view".to_string(),
            root: dir.join("src/lib.rs"),
        }],
        declarations: Vec::new(),
        base_dir: dir,
    })
}

fn actor(name: &str) -> String {
    format!("actor:lib:view:view::{name}")
}

/// `(from, to, attributes)` of every arrow in a DOT text.
fn arrows(dot: &str) -> Vec<(String, String, String)> {
    dot.lines()
        .filter_map(|line| {
            let line = line.trim();
            let (from, rest) = line.strip_prefix('"')?.split_once("\" -> \"")?;
            let (to, attrs) = rest.split_once("\" [")?;
            Some((from.to_string(), to.to_string(), attrs.to_string()))
        })
        .collect()
}

fn between<'a>(arrows: &'a [(String, String, String)], from: &str, to: &str) -> Vec<&'a str> {
    arrows
        .iter()
        .filter(|(f, t, _)| f == from && t == to)
        .map(|(_, _, attrs)| attrs.as_str())
        .collect()
}

fn options(group_fns: bool, holds: HoldsMode) -> DotOptions {
    DotOptions { group_fns, holds }
}

#[test]
fn functions_group_by_owner() {
    let output = scan_view();
    let dot = to_dot_with(&output, &DotOptions::default());
    assert!(
        dot.contains("\"fngroup:lib:view:view::Service\" [shape=box, style=rounded, label=\"view::Service\\n3 fns: a, b, c\"]"),
        "{dot}"
    );
    assert!(
        dot.contains("label=\"view::tools\\n2 fns: one, two\""),
        "{dot}"
    );
    // A function with no sibling stays a plain function node.
    assert!(dot.contains("\"fn:lib:view:view::alone\""), "{dot}");
    // Grouped members no longer appear on their own.
    assert!(!dot.contains("\"fn:lib:view:view::Service::a\""), "{dot}");

    let ungrouped = to_dot_with(&output, &options(false, HoldsMode::Uncovered));
    assert!(!ungrouped.contains("fngroup:"), "{ungrouped}");
    assert!(ungrouped.contains("\"fn:lib:view:view::Service::a\""));
}

#[test]
fn json_model_is_not_grouped() {
    let output = scan_view();
    let fns: BTreeSet<&str> = output
        .graph
        .nodes
        .iter()
        .filter_map(|n| match n {
            Node::Fn(f) => Some(f.path.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(fns.len(), 6, "{fns:?}");
    assert!(fns.contains("view::Service::a"));
}

#[test]
fn tell_and_ask_share_one_arrow() {
    let dot = to_dot_with(&scan_view(), &DotOptions::default());
    let all = arrows(&dot);
    let caller_hub = between(&all, &actor("Caller"), &actor("Hub"));
    assert_eq!(caller_hub.len(), 1, "{caller_hub:?}");
    assert!(caller_hub[0].contains("ask Query -> u32"), "{caller_hub:?}");
    assert!(caller_hub[0].contains("tell Ping"), "{caller_hub:?}");
}

#[test]
fn holds_modes() {
    let output = scan_view();
    let dotted = |dot: &str, from: &str, to: &str| {
        between(&arrows(dot), from, to)
            .iter()
            .any(|a| a.contains("dotted"))
    };

    let uncovered = to_dot_with(&output, &DotOptions::default());
    // Caller sends to Hub, so its holds edge is hidden; Keeper only holds.
    assert!(!dotted(&uncovered, &actor("Caller"), &actor("Hub")));
    assert!(dotted(&uncovered, &actor("Keeper"), &actor("Hub")));

    let all = to_dot_with(&output, &options(true, HoldsMode::All));
    assert!(dotted(&all, &actor("Caller"), &actor("Hub")));

    let none = to_dot_with(&output, &options(true, HoldsMode::None));
    assert!(!none.contains("dotted"), "{none}");
}

#[test]
fn focus_keeps_the_walked_neighbourhood() {
    let hub = actor("Hub");
    let one = focus(scan_view(), "Hub", 1).expect("focus");
    let nodes: BTreeSet<&str> = one.graph.nodes.iter().map(|n| n.id()).collect();
    assert!(nodes.contains(hub.as_str()));
    assert!(nodes.contains(actor("Keeper").as_str()));
    assert!(nodes.contains(actor("Caller").as_str()));
    assert!(!nodes.contains(actor("Leaf").as_str()), "{nodes:?}");
    // Depth 1 keeps only edges touching Hub: not Keeper -> Caller.
    assert!(
        one.graph.edges.iter().all(|e| e.from == hub || e.to == hub),
        "{:#?}",
        one.graph.edges
    );

    let two = focus(scan_view(), "view::Hub", 2).expect("focus");
    assert!(two
        .graph
        .edges
        .iter()
        .any(|e| e.from == actor("Keeper") && e.to == actor("Caller")));
}

#[test]
fn focus_reports_unknown_and_ambiguous_names() {
    let missing = focus(scan_view(), "Nope", 1).unwrap_err();
    assert!(missing.contains("no actor matches"), "{missing}");
    let ambiguous = focus(scan_view(), "Twin", 1).unwrap_err();
    assert!(ambiguous.contains("view::a::Twin"), "{ambiguous}");
    assert!(ambiguous.contains("view::b::Twin"), "{ambiguous}");
    // A longer suffix picks one.
    assert!(focus(scan_view(), "a::Twin", 1).is_ok());
}
