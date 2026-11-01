// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Turns `cargo metadata` output into a [`ScanInput`].
//!
//! `cargo metadata --format-version 1 --no-deps` is run directly and only the
//! fields used here are deserialised. The `cargo_metadata` crate would pull
//! about 60 crates into the workspace lockfile (`toml`, `url`, `icu_*` via
//! `cargo-util-schemas`), and every one of them is in `cargo audit`'s scope.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::{CrateSpec, ScanInput};

/// The rsactor crate itself is never in the declaration scope: its
/// items are built-in knowledge, and its own sources declare no actors
/// outside doc comments and `#[cfg(test)]`.
const RSACTOR_PACKAGE: &str = "rsactor";

/// What the user asked to scan.
#[derive(Debug, Clone, Default)]
pub struct Selection {
    pub manifest_path: Option<PathBuf>,
    pub package: Option<String>,
    pub example: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    workspace_root: PathBuf,
}

#[derive(Debug, Deserialize)]
struct Package {
    name: String,
    id: String,
    manifest_path: PathBuf,
    targets: Vec<Target>,
    dependencies: Vec<Dependency>,
}

#[derive(Debug, Deserialize)]
struct Target {
    name: String,
    kind: Vec<String>,
    src_path: PathBuf,
}

impl Target {
    fn is(&self, kind: &str) -> bool {
        self.kind.iter().any(|k| k == kind)
    }
}

#[derive(Debug, Deserialize)]
struct Dependency {
    name: String,
    /// Set for path dependencies.
    path: Option<PathBuf>,
}

pub struct Workspace {
    metadata: Metadata,
    /// Package whose manifest was passed with `--manifest-path`, if any.
    manifest_package: Option<usize>,
}

impl Workspace {
    pub fn load(selection: &Selection) -> Result<Workspace, String> {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut command = Command::new(cargo);
        command.args(["metadata", "--format-version", "1", "--no-deps"]);
        if let Some(path) = &selection.manifest_path {
            command.arg("--manifest-path").arg(path);
        }
        let output = command
            .output()
            .map_err(|error| format!("cannot run cargo metadata: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "cargo metadata failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let metadata: Metadata = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("cannot read cargo metadata output: {error}"))?;

        let manifest_package = selection
            .manifest_path
            .as_deref()
            .and_then(|path| path.canonicalize().ok())
            .and_then(|path| {
                metadata
                    .packages
                    .iter()
                    .position(|p| p.manifest_path.canonicalize().ok().as_deref() == Some(&*path))
            });
        Ok(Workspace {
            metadata,
            manifest_package,
        })
    }

    fn members(&self) -> impl Iterator<Item = &Package> {
        self.metadata
            .packages
            .iter()
            .filter(|p| self.metadata.workspace_members.contains(&p.id))
    }

    /// `--package`, else the package of `--manifest-path`, else the package
    /// at the workspace root.
    fn package(&self, selection: &Selection) -> Result<&Package, String> {
        if let Some(name) = &selection.package {
            return self
                .members()
                .find(|p| &p.name == name)
                .ok_or_else(|| format!("package `{name}` is not a workspace member"));
        }
        if let Some(index) = self.manifest_package {
            return Ok(&self.metadata.packages[index]);
        }
        let root_manifest = self.metadata.workspace_root.join("Cargo.toml");
        self.members()
            .find(|p| same_file(&p.manifest_path, &root_manifest))
            .ok_or_else(|| "virtual workspace: pass --package NAME".to_string())
    }

    /// Builds the scan input for the selection: one example, or the
    /// package's lib and bins merged into one graph.
    pub fn input(&self, selection: &Selection) -> Result<ScanInput, String> {
        let package = self.package(selection)?;
        let is_rsactor = package.name == RSACTOR_PACKAGE;

        let targets: Vec<CrateSpec> = match &selection.example {
            Some(example) => {
                let target = package
                    .targets
                    .iter()
                    .find(|t| t.is("example") && &t.name == example)
                    .ok_or_else(|| {
                        format!("package `{}` has no example `{example}`", package.name)
                    })?;
                vec![spec(target, "example")]
            }
            None => package
                .targets
                .iter()
                .filter(|t| (t.is("lib") && !is_rsactor) || t.is("bin"))
                .map(|t| spec(t, if t.is("lib") { "lib" } else { "bin" }))
                .collect(),
        };

        // The package lib (for an example) and the libs of workspace members
        // the package depends on. rsactor is excluded even when it is "the
        // package lib", which is the case for the rsactor examples.
        let mut declarations: Vec<CrateSpec> = Vec::new();
        if selection.example.is_some() && !is_rsactor {
            declarations.extend(
                package
                    .targets
                    .iter()
                    .filter(|t| t.is("lib"))
                    .map(|t| spec(t, "lib")),
            );
        }
        for dependency in &package.dependencies {
            if dependency.path.is_none() || dependency.name == RSACTOR_PACKAGE {
                continue;
            }
            let Some(member) = self.members().find(|m| m.name == dependency.name) else {
                continue;
            };
            for target in member.targets.iter().filter(|t| t.is("lib")) {
                let lib = spec(target, "lib");
                let known = targets
                    .iter()
                    .chain(declarations.iter())
                    .any(|s| s.target == lib.target);
                if !known {
                    declarations.push(lib);
                }
            }
        }

        Ok(ScanInput {
            name: selection
                .example
                .clone()
                .unwrap_or_else(|| package.name.clone()),
            targets,
            declarations,
            base_dir: self.metadata.workspace_root.clone(),
        })
    }

    /// Example names of the selected package, for the "no actors" guidance.
    pub fn examples(&self, selection: &Selection) -> Result<Vec<String>, String> {
        let package = self.package(selection)?;
        let mut names: Vec<String> = package
            .targets
            .iter()
            .filter(|t| t.is("example"))
            .map(|t| t.name.clone())
            .collect();
        names.sort();
        Ok(names)
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn spec(target: &Target, kind: &str) -> CrateSpec {
    let name = target.name.replace('-', "_");
    CrateSpec {
        target: format!("{kind}:{name}"),
        name,
        root: target.src_path.clone(),
    }
}
