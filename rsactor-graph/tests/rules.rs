// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Rule tests over the source trees in `tests/fixtures/` (plan §7.2). The
//! fixtures are scanner input only; they are not compiled.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rsactor_graph::model::{ActorNode, Category, Edge, EdgeKind, Node, Output, Resolution};
use rsactor_graph::render::{escape_record, to_dot};
use rsactor_graph::{scan, CrateSpec, ScanInput};

fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn spec(kind: &str, name: &str, root: PathBuf) -> CrateSpec {
    CrateSpec {
        name: name.to_string(),
        target: format!("{kind}:{name}"),
        root,
    }
}

/// Scans `tests/fixtures/<name>/src/lib.rs` as a lone lib target.
fn scan_lib(name: &str) -> Output {
    let dir = fixture_dir(name);
    scan(&ScanInput {
        name: name.to_string(),
        targets: vec![spec("lib", name, dir.join("src/lib.rs"))],
        declarations: Vec::new(),
        base_dir: dir,
    })
}

fn actor<'a>(output: &'a Output, path: &str) -> &'a ActorNode {
    output
        .actors()
        .find(|a| a.path == path)
        .unwrap_or_else(|| panic!("no actor `{path}`; found {:?}", actor_paths(output)))
}

fn actor_paths(output: &Output) -> BTreeSet<String> {
    output.actors().map(|a| a.path.clone()).collect()
}

fn keys(actor: &ActorNode) -> BTreeSet<&str> {
    actor.messages.iter().map(|m| m.key.as_str()).collect()
}

fn set<'a>(items: &[&'a str]) -> BTreeSet<&'a str> {
    items.iter().copied().collect()
}

#[test]
fn modules_resolve_across_files() {
    let output = scan_lib("modules");
    assert!(
        output.report.entries.is_empty(),
        "{:?}",
        output.report.entries
    );
    assert_eq!(
        actor_paths(&output),
        [
            "modules::actors::worker::Worker",
            "modules::extra::Manual",
            "modules::inline::InlineActor",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    );

    // Glob, full path and renamed re-export all land on the declaring path.
    let worker = actor(&output, "modules::actors::worker::Worker");
    assert_eq!(
        keys(worker),
        set(&[
            "modules::msgs::Ping",
            "modules::msgs::Pong",
            "modules::msgs::nested::Deep",
        ])
    );
    let pong = worker
        .messages
        .iter()
        .find(|m| m.key == "modules::msgs::Pong")
        .unwrap();
    assert_eq!(pong.reply.as_deref(), Some("u32"));

    let inline = actor(&output, "modules::inline::InlineActor");
    assert_eq!(keys(inline), set(&["modules::msgs::Ping"]));

    // `#[path]` module and a manual `impl Message<M>`.
    let manual = actor(&output, "modules::extra::Manual");
    assert_eq!(keys(manual), set(&["modules::msgs::nested::Deep"]));
    assert_eq!(manual.messages[0].reply.as_deref(), Some("String"));
    assert_eq!(manual.location.file, "src/custom/extra_impl.rs");

    for actor in output.actors() {
        for message in &actor.messages {
            assert_eq!(message.resolution, Resolution::Exact, "{message:?}");
        }
    }
}

#[test]
fn cfg_test_code_is_excluded() {
    let paths = actor_paths(&scan_lib("modules"));
    assert!(!paths.iter().any(|p| p.contains("TestOnly")), "{paths:?}");
}

#[test]
fn fallback_is_refused_behind_external_globs() {
    let output = scan_lib("fallback");

    for path in [
        "fallback::with_external_glob::A",
        "fallback::with_chained_glob::B",
    ] {
        let message = &actor(&output, path).messages[0];
        assert_eq!(message.resolution, Resolution::Unresolved, "{path}");
        assert_eq!(message.key, "Shutdown", "{path}");
    }

    let message = &actor(&output, "fallback::without_glob::C").messages[0];
    assert_eq!(message.resolution, Resolution::Inferred);
    assert_eq!(message.key, "fallback::msgs::Lonely");
}

#[test]
fn handler_recognition_matches_the_derive_macro() {
    let output = scan_lib("handlers");

    let basic = actor(&output, "handlers::Basic");
    assert_eq!(
        keys(basic),
        set(&[
            "handlers::Job",
            "handlers::Ping",
            "std::boxed::Box<dyn handlers::Calculator>",
        ]),
        "`#[rsactor::handler]` must not count as a handler"
    );
    let job = basic
        .messages
        .iter()
        .find(|m| m.key == "handlers::Job")
        .unwrap();
    assert_eq!(job.cfg, vec!["cfg(feature=\"extra\")".to_string()]);
    assert_eq!(job.reply.as_deref(), Some("JoinHandle<String>"));

    let arity: Vec<_> = output
        .report
        .entries
        .iter()
        .filter(|e| e.category == Category::Declaration)
        .collect();
    assert_eq!(arity.len(), 1, "{arity:?}");
    assert!(arity[0].detail.contains("wrong_arity"), "{arity:?}");
}

#[test]
fn message_keys_normalise_inner_paths() {
    let output = scan_lib("handlers");
    let other = actor(&output, "handlers::Other");
    assert_eq!(
        keys(other),
        set(&[
            "std::boxed::Box<dyn handlers::Calculator>",
            "std::boxed::Box<dyn std::ops::FnOnce() + std::marker::Send>",
        ])
    );
}

#[test]
fn open_actors_are_marked() {
    let output = scan_lib("handlers");

    let logger = actor(&output, "handlers::Logger");
    assert!(logger.open);
    assert!(logger.messages.is_empty());

    let worker = actor(&output, "handlers::Worker");
    assert!(worker.open);
    assert_eq!(worker.display, "handlers::Worker<T>");

    assert!(!actor(&output, "handlers::Basic").open);
}

#[test]
fn a_local_trait_named_message_does_not_make_an_actor() {
    let paths = actor_paths(&scan_lib("handlers"));
    assert!(
        !paths.contains("handlers::foreign::NotAnActor"),
        "{paths:?}"
    );
}

#[test]
fn lib_and_bin_with_the_same_name_stay_apart() {
    let dir = fixture_dir("libbin");
    let output = scan(&ScanInput {
        name: "libbin".to_string(),
        targets: vec![
            spec("lib", "libbin", dir.join("src/lib.rs")),
            spec("bin", "libbin", dir.join("src/main.rs")),
        ],
        declarations: Vec::new(),
        base_dir: dir,
    });

    let shared: Vec<&ActorNode> = output
        .actors()
        .filter(|a| a.path == "libbin::Shared")
        .collect();
    let targets: BTreeSet<&str> = shared.iter().map(|a| a.target.as_str()).collect();
    assert_eq!(targets, set(&["bin:libbin", "lib:libbin"]));

    // `libbin::Note` written in the bin resolves into the lib.
    let in_bin = shared.iter().find(|a| a.target == "bin:libbin").unwrap();
    assert_eq!(keys(in_bin), set(&["libbin::Note"]));
    assert_eq!(in_bin.messages[0].resolution, Resolution::Exact);
}

#[test]
fn workspace_member_libs_supply_declarations() {
    let dir = fixture_dir("member");
    let output = scan(&ScanInput {
        name: "app".to_string(),
        targets: vec![spec("lib", "app", dir.join("app/src/lib.rs"))],
        declarations: vec![spec(
            "lib",
            "core_actors",
            dir.join("core_actors/src/lib.rs"),
        )],
        base_dir: dir,
    });

    let engine = actor(&output, "core_actors::Engine");
    assert_eq!(engine.target, "lib:core_actors");
    assert_eq!(engine.location.file, "core_actors/src/lib.rs");

    let frontend = actor(&output, "app::Frontend");
    assert_eq!(keys(frontend), set(&["core_actors::Start"]));
    assert_eq!(frontend.messages[0].resolution, Resolution::Exact);
}

#[test]
fn dot_escapes_record_syntax() {
    let dot = to_dot(&scan_lib("handlers"));
    assert!(dot.contains("JoinHandle\\<String\\>"), "{dot}");
    assert!(dot.contains("Box\\<dyn Calculator\\>"), "{dot}");
    assert!(dot.contains("handlers::Worker\\<T\\>"), "{dot}");
    assert!(dot.contains("Job -\\> JoinHandle"), "{dot}");
}

#[test]
fn escape_record_covers_every_structural_character() {
    assert_eq!(escape_record("a{b}|<c>\"\\"), "a\\{b\\}\\|\\<c\\>\\\"\\\\");
    assert_eq!(escape_record("x\ny"), "x\\ny");
}

/// `(from, to, variant)` of every edge of one kind.
fn edges(output: &Output, kind: EdgeKind) -> BTreeSet<(String, String, Option<String>)> {
    output
        .graph
        .edges
        .iter()
        .filter(|e| e.kind == kind)
        .map(|e| (e.from.clone(), e.to.clone(), e.variant.clone()))
        .collect()
}

fn edge(from: &str, to: &str, variant: Option<&str>) -> (String, String, Option<String>) {
    (
        from.to_string(),
        to.to_string(),
        variant.map(str::to_string),
    )
}

#[test]
fn senders_follow_the_method_rules() {
    let dir = fixture_dir("senders");
    let output = scan(&ScanInput {
        name: "senders".to_string(),
        targets: vec![
            spec("lib", "senders", dir.join("src/lib.rs")),
            spec("bin", "senders", dir.join("src/main.rs")),
        ],
        declarations: Vec::new(),
        base_dir: dir,
    });

    let parent = "actor:lib:senders:senders::Parent";
    let child = "actor:lib:senders:senders::Child";
    let derived = "actor:lib:senders:senders::Derived";
    assert_eq!(
        edges(&output, EdgeKind::Spawn),
        [
            // `on_start` (trait method without `self`) and a `self` method.
            edge(parent, child, None),
            edge(parent, child, Some("spawn_with_options")),
            // Closure/async block inside a `self` method; unit-struct literal
            // of a `#[derive(Actor)]` type without a turbofish.
            edge(parent, derived, None),
            // `self`-less associated function: not Parent.
            edge("fn:lib:senders:senders::Parent::assoc", child, None),
            edge("fn:lib:senders:senders::Parent::outer::inner", child, None),
            edge("fn:lib:senders:senders::Boot::boot", child, None),
            edge("fn:lib:senders:senders::Helper::go", child, None),
            // Binding annotation `(ActorRef<Child>, _)`.
            edge("fn:lib:senders:senders::free", child, None),
            // `main` of the bin.
            edge("external", child, None),
        ]
        .into_iter()
        .collect()
    );

    let unresolved: Vec<_> = output
        .report
        .entries
        .iter()
        .filter(|e| e.category == Category::UnresolvedSpawn)
        .map(|e| (e.sender.clone().unwrap_or_default(), e.detail.clone()))
        .collect();
    assert_eq!(unresolved.len(), 3, "{unresolved:#?}");
    assert!(unresolved
        .iter()
        .any(|(s, d)| s == "fn senders::free" && d.contains("without a type argument")));
    assert!(unresolved
        .iter()
        .any(|(s, d)| s == "fn senders::ambiguous" && d.contains("senders::Wrapper")));
    assert!(unresolved
        .iter()
        .any(|(s, d)| s == "fn senders::generic" && d.contains("not a concrete actor")));
    // `tokio::spawn` and the local `fn spawn` produce nothing.
    assert_eq!(
        output.report.entries.len(),
        3,
        "{:#?}",
        output.report.entries
    );
}

#[test]
fn holds_edges_and_handler_candidates() {
    let output = scan_lib("holds");
    let a = "actor:lib:holds:holds::A";
    let b = "actor:lib:holds:holds::B";
    let c = "actor:lib:holds:holds::C";
    let tell_ping = "handlers:TellHandler:holds::Ping";
    let ask_query = "handlers:AskHandler:holds::Query:u32";
    assert_eq!(
        edges(&output, EdgeKind::Holds),
        [
            edge(a, b, None),
            edge(a, c, None),
            edge(a, tell_ping, None),
            edge(a, ask_query, None),
            edge("actor:lib:holds:holds::E", b, None),
        ]
        .into_iter()
        .collect()
    );

    // `b` and `weak_b` merge into one edge with both locations.
    let a_to_b = output
        .graph
        .edges
        .iter()
        .find(|e| e.from == a && e.to == b)
        .unwrap();
    assert_eq!(a_to_b.locations.len(), 2, "{a_to_b:?}");

    let Some(Node::Handlers(tell)) = output.node(tell_ping) else {
        panic!("no {tell_ping}");
    };
    assert_eq!(tell.candidates, vec![b.to_string(), c.to_string()]);
    assert!(!tell.may_be_outside);

    // `D` handles `Query` but replies `String`.
    let Some(Node::Handlers(ask)) = output.node(ask_query) else {
        panic!("no {ask_query}");
    };
    assert_eq!(ask.candidates, vec![c.to_string()]);

    // `ActorRef<T>` in the generic actor: field and `type Args`.
    let generic: Vec<_> = output
        .report
        .entries
        .iter()
        .filter(|e| e.category == Category::Declaration)
        .collect();
    assert_eq!(generic.len(), 2, "{generic:#?}");
    assert!(generic.iter().all(|e| e.detail.contains("ActorRef<T>")));
}

#[test]
fn an_actor_outside_the_workspace_widens_handler_candidates() {
    let output = scan_lib("holds_outside");
    let local = "actor:lib:holds_outside:holds_outside::Local";
    assert!(edges(&output, EdgeKind::Holds).contains(&edge(
        local,
        "external_actor:remote_crate::Remote",
        None
    )));
    let Some(Node::Handlers(tell)) = output.node("handlers:TellHandler:holds_outside::Ping") else {
        panic!("no handler node");
    };
    assert_eq!(tell.candidates, vec![local.to_string()]);
    assert!(tell.may_be_outside);
}

fn find_edge<'a>(
    output: &'a Output,
    from: &str,
    to: &str,
    kind: EdgeKind,
    variant: Option<&str>,
    message: Option<&str>,
) -> &'a Edge {
    output
        .graph
        .edges
        .iter()
        .find(|e| {
            e.from == from
                && e.to == to
                && e.kind == kind
                && e.variant.as_deref() == variant
                && e.message.as_deref() == message
        })
        .unwrap_or_else(|| {
            panic!(
                "no {kind:?} {from} -> {to} {variant:?} {message:?}; edges: {:#?}",
                output.graph.edges
            )
        })
}

fn reports(output: &Output, category: Category) -> Vec<String> {
    output
        .report
        .entries
        .iter()
        .filter(|e| e.category == category)
        .map(|e| e.detail.clone())
        .collect()
}

const WORKER: &str = "actor:lib:sends:sends::Worker";
const ROUTER: &str = "actor:lib:sends:sends::Router";

#[test]
fn receivers_resolve_through_the_type_environment() {
    let output = scan_lib("sends");
    // Every `Job` tell that must reach Router: async-block capture (1),
    // `let else`, `match`, UFCS downgrade + upgrade (3), containers (9), and
    // field access through a message, a wrapper struct, a generic struct and
    // a renamed `ActorRef` (4). The shadowed closure parameter and the
    // re-`let` to `Batcher` must not add locations here.
    let job = find_edge(
        &output,
        WORKER,
        ROUTER,
        EdgeKind::Tell,
        None,
        Some("sends::Job"),
    );
    assert_eq!(job.locations.len(), 17, "{:#?}", job.locations);
    assert_eq!(job.receiver_resolution, Resolution::Exact);
    assert_eq!(job.message_resolution, Resolution::Exact);

    // `Self::Args` destructuring + `if let Some(r) = weak.upgrade()`, and
    // enum variants of every shape: one `Command` edge, three locations.
    let command = find_edge(
        &output,
        WORKER,
        ROUTER,
        EdgeKind::Tell,
        None,
        Some("sends::Command"),
    );
    assert_eq!(command.locations.len(), 3, "{:#?}", command.locations);
}

#[test]
fn messages_follow_m1_and_m2() {
    let output = scan_lib("sends");

    let made = find_edge(
        &output,
        WORKER,
        ROUTER,
        EdgeKind::Tell,
        None,
        Some("sends::Made"),
    );
    assert_eq!(made.message_resolution, Resolution::Assumed);

    // `Box::new(Job)`: the receiver is known and kept; only the message is `?`.
    let unknown_message = find_edge(&output, WORKER, ROUTER, EdgeKind::Tell, None, None);
    assert_eq!(unknown_message.receiver_resolution, Resolution::Exact);
    assert_eq!(unknown_message.message_resolution, Resolution::Unresolved);

    find_edge(
        &output,
        WORKER,
        ROUTER,
        EdgeKind::Ask,
        None,
        Some("sends::Job"),
    );
    find_edge(
        &output,
        WORKER,
        ROUTER,
        EdgeKind::Tell,
        Some("tell_priority"),
        Some("sends::Job"),
    );
}

#[test]
fn unresolved_receivers_go_to_a_per_sender_unknown_node() {
    let output = scan_lib("sends");
    // The shadowing closure parameter.
    let shadowed = find_edge(
        &output,
        WORKER,
        &format!("unknown:{WORKER}"),
        EdgeKind::Tell,
        None,
        Some("sends::Job"),
    );
    assert_eq!(shadowed.receiver_resolution, Resolution::Unresolved);
    // A call result with no known type.
    let from = "fn:lib:sends:sends::unknown_receiver";
    find_edge(
        &output,
        from,
        &format!("unknown:{from}"),
        EdgeKind::Tell,
        None,
        Some("sends::Job"),
    );
}

#[test]
fn type_erased_receivers_reach_handler_nodes() {
    let output = scan_lib("sends");
    find_edge(
        &output,
        WORKER,
        "handlers:AskHandler:sends::Query:u32",
        EdgeKind::Ask,
        None,
        Some("sends::Query"),
    );
    find_edge(
        &output,
        "fn:lib:sends:sends::handlers",
        "handlers:TellHandler:sends::Job",
        EdgeKind::Tell,
        None,
        Some("sends::Job"),
    );
}

#[test]
fn candidates_that_cannot_be_drawn_are_reported() {
    let output = scan_lib("sends");

    let unsupported = reports(&output, Category::UnsupportedReceiver);
    assert_eq!(unsupported.len(), 3, "{unsupported:#?}");
    assert!(unsupported
        .iter()
        .any(|d| d.contains("wrapper type `RouterHandle`")));
    assert!(unsupported
        .iter()
        .any(|d| d.contains("type alias `RouterRef`")));
    assert!(unsupported
        .iter()
        .any(|d| d.contains("generic parameter `R`")));

    // Unknown receiver, message no actor handles: reported, not a `?` edge.
    let unclassified = reports(&output, Category::Unclassified);
    assert_eq!(unclassified.len(), 1, "{unclassified:#?}");
    assert!(unclassified[0].contains("LogLine"));

    assert_eq!(reports(&output, Category::Ufcs).len(), 1);

    let in_macros = reports(&output, Category::MacroCandidate);
    assert_eq!(in_macros.len(), 2, "{in_macros:#?}");
    assert!(in_macros.iter().any(|d| d.contains("select!")));
    assert!(in_macros.iter().any(|d| d.contains("println!")));

    // `Box::new(..)` message, the shadowed closure, the unknown call result.
    assert_eq!(reports(&output, Category::UnresolvedEdge).len(), 3);

    let cross = reports(&output, Category::CrossCheck);
    assert_eq!(cross.len(), 1, "{cross:#?}");
    assert!(cross[0].contains("sends::Unhandled"));

    // `Batcher::tell` (field and re-`let`) is dropped without a trace, and
    // nothing else is reported: 3 + 1 + 1 + 2 + 3 + 1.
    assert_eq!(
        output.report.entries.len(),
        11,
        "{:#?}",
        output.report.entries
    );
}
