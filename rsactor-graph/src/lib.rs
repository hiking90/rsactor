// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Static analysis of the actor topology of an rsactor application.
//!
//! The tool parses source files with `syn` (no type inference) and builds a
//! type-level graph: actor types, the messages each one handles, and — in
//! later stages — references, spawns and sends between them. The design and
//! its rules live in `plan/02-topology-graph.md`; section numbers in comments
//! (`§4.5`) refer to that document.
//!
//! Pipeline: [`crate_tree`] loads module trees, [`resolve`] builds the name
//! index, [`types`] turns `syn::Type` into [`types::Ty`], [`discover`] extracts
//! the graph, and [`render`] writes JSON, DOT or SVG.

pub mod crate_tree;
pub mod discover;
pub mod model;
pub mod render;
pub mod resolve;
pub mod types;
pub mod workspace;

use std::path::PathBuf;

use crate_tree::Crate;
use model::{Output, Report};

/// One crate to parse.
#[derive(Debug, Clone)]
pub struct CrateSpec {
    /// Crate name as written in paths (`-` already replaced by `_`).
    pub name: String,
    /// Target id, unique within one scan, e.g. `lib:app`, `bin:app`,
    /// `example:basic`. Part of every node key (§6.1), because a lib and a bin
    /// of the same package share the crate name.
    pub target: String,
    /// Crate root source file.
    pub root: PathBuf,
}

/// Everything one scan needs (§3).
#[derive(Debug, Clone)]
pub struct ScanInput {
    /// Graph name shown in the output.
    pub name: String,
    /// Crates whose call sites produce edges. Their declarations are read too.
    pub targets: Vec<CrateSpec>,
    /// Crates read for declarations only (the package lib, workspace members).
    pub declarations: Vec<CrateSpec>,
    /// Locations are reported relative to this directory, so snapshots do not
    /// depend on where the repository is checked out.
    pub base_dir: PathBuf,
}

/// Runs one scan and returns the graph together with its report.
pub fn scan(input: &ScanInput) -> Output {
    let mut report = Report::default();
    let mut crates = Vec::new();
    for spec in &input.targets {
        crates.push(Crate::load(
            spec.clone(),
            true,
            &input.base_dir,
            &mut report,
        ));
    }
    for spec in &input.declarations {
        crates.push(Crate::load(
            spec.clone(),
            false,
            &input.base_dir,
            &mut report,
        ));
    }
    let index = resolve::Index::build(&crates);
    let graph = discover::discover(&index, &input.base_dir, &mut report);
    Output::new(input.name.clone(), graph, report)
}
