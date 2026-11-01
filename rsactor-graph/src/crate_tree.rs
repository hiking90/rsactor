// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Loads a crate's module tree from its root file (§4.5).
//!
//! Inline `mod x { .. }` bodies are moved into their own [`Module`], so every
//! module owns exactly the items written directly inside it. `#[cfg(test)]`
//! modules and items are dropped here (§3), before anything else sees them.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use quote::ToTokens;
use syn::{Attribute, Item, Meta};

use crate::model::{Category, Location, Report};
use crate::CrateSpec;

pub type ModId = usize;

pub struct Module {
    /// Path segments below the crate root; empty for the root module.
    pub path: Vec<String>,
    /// Source file the module's items come from.
    pub file: PathBuf,
    pub parent: Option<ModId>,
    pub children: BTreeMap<String, ModId>,
    pub items: Vec<Item>,
}

pub struct Crate {
    pub spec: CrateSpec,
    /// Call sites in a target crate produce edges; other crates only supply
    /// declarations (§3).
    pub is_target: bool,
    /// `modules[0]` is the crate root.
    pub modules: Vec<Module>,
}

/// Where a module's own `mod foo;` declarations look for files, and what a
/// `#[path]` attribute is relative to.
struct Dirs {
    /// Directory for `mod foo;` → `<children>/foo.rs` or `<children>/foo/mod.rs`.
    children: PathBuf,
    /// Base for `#[path = ".."]`: the directory of the source file, extended
    /// by the names of enclosing inline modules.
    path_attr_base: PathBuf,
}

impl Crate {
    pub fn load(spec: CrateSpec, is_target: bool, base: &Path, report: &mut Report) -> Crate {
        let mut krate = Crate {
            spec,
            is_target,
            modules: Vec::new(),
        };
        let root = krate.spec.root.clone();
        let items = parse_file(&root, base, report).unwrap_or_default();
        let dir = root.parent().map(Path::to_path_buf).unwrap_or_default();
        let dirs = Dirs {
            children: dir.clone(),
            path_attr_base: dir,
        };
        let mut seen = HashSet::new();
        seen.insert(root.clone());
        krate.add_module(None, Vec::new(), root, dirs, items, base, report, &mut seen);
        krate
    }

    /// Full path of a module, starting with the crate name.
    pub fn module_path(&self, id: ModId) -> Vec<String> {
        let mut path = vec![self.spec.name.clone()];
        path.extend(self.modules[id].path.iter().cloned());
        path
    }

    #[allow(clippy::too_many_arguments)]
    fn add_module(
        &mut self,
        parent: Option<ModId>,
        path: Vec<String>,
        file: PathBuf,
        dirs: Dirs,
        items: Vec<Item>,
        base: &Path,
        report: &mut Report,
        seen: &mut HashSet<PathBuf>,
    ) -> ModId {
        let id = self.modules.len();
        self.modules.push(Module {
            path: path.clone(),
            file: file.clone(),
            parent,
            children: BTreeMap::new(),
            items: Vec::new(),
        });

        let mut kept = Vec::new();
        for item in items {
            if is_cfg_test(item_attrs(&item)) {
                continue;
            }
            let Item::Mod(module) = item else {
                kept.push(item);
                continue;
            };
            let name = module.ident.to_string();
            let mut child_path = path.clone();
            child_path.push(name.clone());

            let child = match module.content {
                Some((_, inner)) => {
                    let child_dirs = Dirs {
                        children: dirs.children.join(&name),
                        path_attr_base: dirs.path_attr_base.join(&name),
                    };
                    self.add_module(
                        Some(id),
                        child_path,
                        file.clone(),
                        child_dirs,
                        inner,
                        base,
                        report,
                        seen,
                    )
                }
                None => {
                    let location = Some(location_of(&file, module.ident.span(), base));
                    let Some(child_file) = locate_module_file(&module.attrs, &name, &dirs) else {
                        report.push(
                            Category::Declaration,
                            location,
                            format!("module file for `mod {name};` not found"),
                        );
                        continue;
                    };
                    if !seen.insert(child_file.clone()) {
                        report.push(
                            Category::Declaration,
                            location,
                            format!("module file {} loaded twice", child_file.display()),
                        );
                        continue;
                    }
                    let inner = parse_file(&child_file, base, report).unwrap_or_default();
                    let child_dir = module_dir_of(&child_file);
                    let child_dirs = Dirs {
                        children: child_dir,
                        path_attr_base: child_file
                            .parent()
                            .map(Path::to_path_buf)
                            .unwrap_or_default(),
                    };
                    self.add_module(
                        Some(id),
                        child_path,
                        child_file,
                        child_dirs,
                        inner,
                        base,
                        report,
                        seen,
                    )
                }
            };
            self.modules[id].children.insert(name, child);
        }
        self.modules[id].items = kept;
        id
    }
}

fn parse_file(path: &Path, base: &Path, report: &mut Report) -> Option<Vec<Item>> {
    let location = Some(Location {
        file: relative(path, base),
        line: 1,
    });
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            report.push(
                Category::Declaration,
                location,
                format!("cannot read: {error}"),
            );
            return None;
        }
    };
    match syn::parse_file(&source) {
        Ok(file) => Some(file.items),
        Err(error) => {
            report.push(
                Category::Declaration,
                location,
                format!("cannot parse: {error}"),
            );
            None
        }
    }
}

fn locate_module_file(attrs: &[Attribute], name: &str, dirs: &Dirs) -> Option<PathBuf> {
    if let Some(path) = path_attr(attrs) {
        let file = dirs.path_attr_base.join(path);
        return file.is_file().then_some(file);
    }
    let flat = dirs.children.join(format!("{name}.rs"));
    if flat.is_file() {
        return Some(flat);
    }
    let nested = dirs.children.join(name).join("mod.rs");
    nested.is_file().then_some(nested)
}

/// Directory searched by `mod foo;` inside `file`: next to a `mod.rs`, or in a
/// directory named after a non-`mod.rs` file.
fn module_dir_of(file: &Path) -> PathBuf {
    let parent = file.parent().map(Path::to_path_buf).unwrap_or_default();
    match file.file_stem().and_then(|stem| stem.to_str()) {
        Some("mod") | None => parent,
        Some(stem) => parent.join(stem),
    }
}

fn path_attr(attrs: &[Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| {
        let Meta::NameValue(name_value) = &attr.meta else {
            return None;
        };
        if !name_value.path.is_ident("path") {
            return None;
        }
        match &name_value.value {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(lit),
                ..
            }) => Some(lit.value()),
            _ => None,
        }
    })
}

/// `#[cfg(test)]` and `#[cfg(all(test, ..))]`. Predicates are not evaluated
/// in general (§9.2); only test-only code is recognised, because §3 excludes it.
pub fn is_cfg_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("cfg") {
            return false;
        }
        let Meta::List(list) = &attr.meta else {
            return false;
        };
        let predicate = compact_tokens(&list.tokens.to_token_stream().to_string());
        if predicate == "test" {
            return true;
        }
        predicate
            .strip_prefix("all(")
            .and_then(|rest| rest.strip_suffix(')'))
            .is_some_and(|inner| inner.split(',').any(|part| part.trim() == "test"))
    })
}

pub fn item_attrs(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(i) => &i.attrs,
        Item::Enum(i) => &i.attrs,
        Item::ExternCrate(i) => &i.attrs,
        Item::Fn(i) => &i.attrs,
        Item::ForeignMod(i) => &i.attrs,
        Item::Impl(i) => &i.attrs,
        Item::Macro(i) => &i.attrs,
        Item::Mod(i) => &i.attrs,
        Item::Static(i) => &i.attrs,
        Item::Struct(i) => &i.attrs,
        Item::Trait(i) => &i.attrs,
        Item::TraitAlias(i) => &i.attrs,
        Item::Type(i) => &i.attrs,
        Item::Union(i) => &i.attrs,
        Item::Use(i) => &i.attrs,
        _ => &[],
    }
}

pub fn location_of(file: &Path, span: proc_macro2::Span, base: &Path) -> Location {
    Location {
        file: relative(file, base),
        line: span.start().line,
    }
}

pub fn relative(path: &Path, base: &Path) -> String {
    let shown = path.strip_prefix(base).unwrap_or(path);
    shown
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// `quote` prints tokens separated by spaces (`Box < dyn Fn () >`). Keep a
/// space only between two word characters, so `dyn Fn` survives and the rest
/// collapses (`Box<dyn Fn()>`).
pub fn compact_tokens(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '\'';
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == ' ' {
            let prev = out.chars().last();
            let next = chars[i + 1..].iter().copied().find(|&n| n != ' ');
            if let (Some(p), Some(n)) = (prev, next) {
                if word(p) && word(n) {
                    out.push(' ');
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}
