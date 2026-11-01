// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Rule tests over the source trees in `tests/fixtures/` (plan §7.2). The
//! fixtures are scanner input only; they are not compiled.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rsactor_graph::model::{ActorNode, Category, Output, Resolution};
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
