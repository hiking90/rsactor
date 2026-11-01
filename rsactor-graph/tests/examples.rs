// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Golden snapshots of every rsactor example (plan §7.1), and the CLI guidance
//! for the repository itself.
//!
//! Regenerate after an intended change with
//! `RSACTOR_GRAPH_BLESS=1 cargo test -p rsactor-graph --test examples`
//! and review the diff.

use std::path::{Path, PathBuf};
use std::process::Command;

use rsactor_graph::render::{to_dot, to_json};
use rsactor_graph::scan;
use rsactor_graph::workspace::{Selection, Workspace};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rsactor-graph lives inside the rsactor workspace")
        .to_path_buf()
}

fn snapshot_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/examples")
}

fn selection() -> Selection {
    Selection {
        manifest_path: Some(repo_root().join("Cargo.toml")),
        ..Selection::default()
    }
}

/// Compares `actual` with the snapshot file, or rewrites it when blessing.
/// Returns a failure description instead of panicking so one run reports
/// every stale snapshot.
fn check(path: &Path, actual: &str, bless: bool) -> Option<String> {
    if bless {
        std::fs::write(path, actual).expect("write snapshot");
        return None;
    }
    match std::fs::read_to_string(path) {
        Ok(expected) if expected == actual => None,
        Ok(_) => Some(format!("{} differs", path.display())),
        Err(_) => Some(format!("{} is missing", path.display())),
    }
}

#[test]
fn examples_match_snapshots() {
    let bless = std::env::var_os("RSACTOR_GRAPH_BLESS").is_some();
    let selection = selection();
    let workspace = Workspace::load(&selection).expect("cargo metadata");
    let examples = workspace.examples(&selection).expect("examples");
    assert!(
        examples.len() >= 17,
        "expected the rsactor examples, got {examples:?}"
    );

    let dir = snapshot_dir();
    std::fs::create_dir_all(&dir).expect("snapshot dir");
    let mut failures = Vec::new();
    for example in &examples {
        let input = workspace
            .input(&Selection {
                example: Some(example.clone()),
                ..selection.clone()
            })
            .expect("scan input");
        let output = scan(&input);
        assert!(
            output.actors().next().is_some(),
            "example `{example}` has no actor"
        );
        failures.extend(check(
            &dir.join(format!("{example}.json")),
            &to_json(&output),
            bless,
        ));
        failures.extend(check(
            &dir.join(format!("{example}.dot")),
            &to_dot(&output),
            bless,
        ));
    }

    // A snapshot whose example no longer exists is stale too.
    for entry in std::fs::read_dir(&dir).expect("read snapshot dir") {
        let path = entry.expect("dir entry").path();
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if !examples.iter().any(|e| e == stem) {
            failures.push(format!("{} has no example", path.display()));
        }
    }

    assert!(
        failures.is_empty(),
        "stale snapshots (re-run with RSACTOR_GRAPH_BLESS=1 and review):\n{}",
        failures.join("\n")
    );
}

#[test]
fn cli_without_selection_lists_examples_with_actors() {
    let output = Command::new(env!("CARGO_BIN_EXE_rsactor-graph"))
        .arg("--manifest-path")
        .arg(repo_root().join("Cargo.toml"))
        .output()
        .expect("run rsactor-graph");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no actor types found in the lib and bins of `rsactor`"),
        "{stderr}"
    );
    assert!(stderr.contains("  dining_philosophers\n"), "{stderr}");
    assert!(stderr.contains("--example NAME"), "{stderr}");
    assert!(output.stdout.is_empty());
}
