// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Name resolution in the type namespace.
//!
//! Resolution follows `use` declarations (renames, groups, globs, re-exports),
//! child modules and local items across every crate in the declaration scope.
//! Privacy is ignored: the input compiled, so any path written in it is
//! visible from where it is written, and `use super::*` legitimately imports
//! private items.
//!
//! When a path cannot be resolved, a single fallback applies: the only
//! declared type with the same last segment, reported as
//! [`Resolution::Inferred`](crate::model::Resolution::Inferred). It is refused
//! when a glob in scope reaches a crate outside the declaration scope, because
//! a same-named type may have come in through that glob.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use syn::{Item, UseTree};

use crate::crate_tree::{Crate, ModId};

pub type DeclId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclKind {
    Struct,
    Enum,
    Union,
    Trait,
    TypeAlias,
}

/// A type declared somewhere in the declaration scope.
#[derive(Debug)]
pub struct Decl {
    pub krate: usize,
    pub module: ModId,
    /// Index into `Module::items`.
    pub item: usize,
    pub name: String,
    pub kind: DeclKind,
    /// Declared generic type parameters, in order.
    pub generics: Vec<String>,
}

/// What a path resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Res {
    Module(usize, ModId),
    Decl(DeclId),
    /// `Enum::Variant`.
    Variant(DeclId, String),
    /// An item of the rsactor crate, by name. rsactor is never parsed;
    /// its public names are listed in [`RSACTOR_NAMES`].
    Rsactor(String),
    /// A `std`/`core`/`alloc` path (or a primitive), normalised to `std::`.
    Std(String),
    /// A path into a crate outside the declaration scope.
    External(String),
    /// A local `fn`, `const` or `static`, by full path. Tracked so that a
    /// local `fn spawn()` is not mistaken for `rsactor::spawn`.
    Value(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub res: Res,
    /// Came from the last-segment fallback.
    pub inferred: bool,
}

/// Public names of the rsactor crate (`src/lib.rs` re-exports and items).
/// A glob from rsactor supplies exactly these names, which is why such a glob
/// does not block the fallback. Keep in sync with `src/lib.rs`.
pub const RSACTOR_NAMES: &[&str] = &[
    "Actor",
    "ActorControl",
    "ActorFailure",
    "ActorRef",
    "ActorResult",
    "ActorWeak",
    "AskHandler",
    "BoxFuture",
    "Channel",
    "DeadLetterReason",
    "Error",
    "FailurePhase",
    "Identity",
    "IdleSubscribeError",
    "Message",
    "MetricsSnapshot",
    "Operation",
    "Result",
    "SpawnOptions",
    "TellHandler",
    "WeakActorControl",
    "WeakAskHandler",
    "WeakTellHandler",
    "dead_letter_count",
    "message_handlers",
    "reset_dead_letter_count",
    "set_default_mailbox_capacity",
    "spawn",
    "spawn_with_mailbox_capacity",
    "spawn_with_options",
];

/// Names the std prelude puts in scope, with a canonical path so that `String`
/// and `std::string::String` produce the same key.
const STD_PRELUDE: &[(&str, &str)] = &[
    ("AsMut", "std::convert::AsMut"),
    ("AsRef", "std::convert::AsRef"),
    ("Box", "std::boxed::Box"),
    ("Clone", "std::clone::Clone"),
    ("Copy", "std::marker::Copy"),
    ("Default", "std::default::Default"),
    ("Drop", "std::ops::Drop"),
    ("Eq", "std::cmp::Eq"),
    ("Extend", "std::iter::Extend"),
    ("Fn", "std::ops::Fn"),
    ("FnMut", "std::ops::FnMut"),
    ("FnOnce", "std::ops::FnOnce"),
    ("From", "std::convert::From"),
    ("Into", "std::convert::Into"),
    ("IntoIterator", "std::iter::IntoIterator"),
    ("Iterator", "std::iter::Iterator"),
    ("Option", "std::option::Option"),
    ("Ord", "std::cmp::Ord"),
    ("PartialEq", "std::cmp::PartialEq"),
    ("PartialOrd", "std::cmp::PartialOrd"),
    ("Result", "std::result::Result"),
    ("Send", "std::marker::Send"),
    ("Sized", "std::marker::Sized"),
    ("String", "std::string::String"),
    ("Sync", "std::marker::Sync"),
    ("ToOwned", "std::borrow::ToOwned"),
    ("ToString", "std::string::ToString"),
    ("TryFrom", "std::convert::TryFrom"),
    ("TryInto", "std::convert::TryInto"),
    ("Unpin", "std::marker::Unpin"),
    ("Vec", "std::vec::Vec"),
];

const PRIMITIVES: &[&str] = &[
    "bool", "char", "str", "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64",
    "u128", "usize", "f32", "f64",
];

/// One name brought in by a `use` declaration.
#[derive(Debug, Clone)]
pub enum Import {
    Named {
        alias: String,
        path: Vec<String>,
        leading_colon: bool,
    },
    Glob {
        path: Vec<String>,
        leading_colon: bool,
    },
}

#[derive(Debug, Default)]
struct Scope {
    types: HashMap<String, DeclId>,
    values: HashSet<String>,
    imports: Vec<Import>,
}

/// The name index over all loaded crates.
pub struct Index<'a> {
    pub crates: &'a [Crate],
    pub decls: Vec<Decl>,
    scopes: Vec<Vec<Scope>>,
    /// Declaration-scope crates by name. A lib wins over a bin of the same
    /// name, because `app::X` written in the bin refers to the lib.
    crate_by_name: HashMap<String, usize>,
    by_last_segment: HashMap<String, Vec<DeclId>>,
    /// Cycle guard for name lookups (`use a::b; ` in `b`, glob cycles).
    active: RefCell<HashSet<(usize, ModId, String)>>,
}

impl<'a> Index<'a> {
    pub fn build(crates: &'a [Crate]) -> Index<'a> {
        let mut decls = Vec::new();
        let mut scopes = Vec::new();
        let mut crate_by_name: HashMap<String, usize> = HashMap::new();
        let mut by_last_segment: HashMap<String, Vec<DeclId>> = HashMap::new();

        for (c, krate) in crates.iter().enumerate() {
            let prefer = krate.spec.target.starts_with("lib:");
            match crate_by_name.get(&krate.spec.name) {
                Some(&existing) if !prefer || crates[existing].spec.target.starts_with("lib:") => {}
                _ => {
                    crate_by_name.insert(krate.spec.name.clone(), c);
                }
            }

            let mut crate_scopes = Vec::new();
            for (m, module) in krate.modules.iter().enumerate() {
                let mut scope = Scope::default();
                for (i, item) in module.items.iter().enumerate() {
                    if let Some((name, kind, generics)) = declared_type(item) {
                        let id = decls.len();
                        decls.push(Decl {
                            krate: c,
                            module: m,
                            item: i,
                            name: name.clone(),
                            kind,
                            generics,
                        });
                        by_last_segment.entry(name.clone()).or_default().push(id);
                        scope.types.insert(name, id);
                    }
                    match item {
                        Item::Fn(i) => {
                            scope.values.insert(i.sig.ident.to_string());
                        }
                        Item::Const(i) => {
                            scope.values.insert(i.ident.to_string());
                        }
                        Item::Static(i) => {
                            scope.values.insert(i.ident.to_string());
                        }
                        _ => {}
                    }
                    if let Item::Use(item_use) = item {
                        flatten_use(
                            &item_use.tree,
                            Vec::new(),
                            item_use.leading_colon.is_some(),
                            &mut scope.imports,
                        );
                    }
                }
                crate_scopes.push(scope);
            }
            scopes.push(crate_scopes);
        }

        Index {
            crates,
            decls,
            scopes,
            crate_by_name,
            by_last_segment,
            active: RefCell::new(HashSet::new()),
        }
    }

    /// Full path of a declared type, e.g. `app::router::Router`.
    pub fn decl_path(&self, id: DeclId) -> String {
        let decl = &self.decls[id];
        let mut path = self.crates[decl.krate].module_path(decl.module);
        path.push(decl.name.clone());
        path.join("::")
    }

    pub fn decl_item(&self, id: DeclId) -> &Item {
        let decl = &self.decls[id];
        &self.crates[decl.krate].modules[decl.module].items[decl.item]
    }

    /// The declared type named `name` directly in module `(krate, module)`.
    pub fn local_decl(&self, krate: usize, module: ModId, name: &str) -> Option<DeclId> {
        self.scopes[krate][module].types.get(name).copied()
    }

    /// Resolves a path written in module `(krate, module)`, with the
    /// last-segment fallback.
    pub fn resolve_path(&self, krate: usize, module: ModId, path: &syn::Path) -> Option<Resolved> {
        let segments: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
        self.resolve_segments(krate, module, &segments, path.leading_colon.is_some())
    }

    pub fn resolve_segments(
        &self,
        krate: usize,
        module: ModId,
        segments: &[String],
        leading_colon: bool,
    ) -> Option<Resolved> {
        self.resolve_segments_inner(krate, module, segments, leading_colon, true)
    }

    /// Like [`Index::resolve_path`], but `use` declarations written inside
    /// the enclosing function body (`blocks`, innermost last) are consulted
    /// first. Rust makes a block-level `use` visible in the whole block.
    pub fn resolve_path_scoped(
        &self,
        krate: usize,
        module: ModId,
        path: &syn::Path,
        blocks: &[Vec<Import>],
    ) -> Option<Resolved> {
        let segments: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
        self.resolve_segments_scoped(
            krate,
            module,
            &segments,
            path.leading_colon.is_some(),
            blocks,
        )
    }

    pub fn resolve_segments_scoped(
        &self,
        krate: usize,
        module: ModId,
        segments: &[String],
        leading_colon: bool,
        blocks: &[Vec<Import>],
    ) -> Option<Resolved> {
        if blocks.is_empty() || leading_colon {
            return self.resolve_segments(krate, module, segments, leading_colon);
        }
        let (first, rest) = segments.split_first()?;
        if !matches!(first.as_str(), "crate" | "self" | "super") {
            for imports in blocks.iter().rev() {
                for import in imports {
                    if let Import::Named {
                        alias,
                        path,
                        leading_colon,
                    } = import
                    {
                        if alias == first {
                            let mut full = path.clone();
                            full.extend(rest.iter().cloned());
                            return self.resolve_segments(krate, module, &full, *leading_colon);
                        }
                    }
                }
                if rest.is_empty() {
                    for import in imports {
                        if let Import::Glob {
                            path,
                            leading_colon,
                        } = import
                        {
                            let mut full = path.clone();
                            full.push(first.clone());
                            if let Some(res) =
                                self.resolve_exact(krate, module, &full, *leading_colon)
                            {
                                return Some(Resolved {
                                    res,
                                    inferred: false,
                                });
                            }
                        }
                    }
                }
            }
        }
        // A block-level glob reaching outside the declaration scope blocks
        // the fallback just as a module-level one does.
        let outside = blocks.iter().flatten().any(|import| match import {
            Import::Glob {
                path,
                leading_colon,
            } => match self.resolve_use(krate, module, path, *leading_colon) {
                Some(Res::Module(c, m)) => self.glob_reaches_outside(c, m, &mut HashSet::new()),
                Some(Res::Rsactor(_)) | Some(Res::Decl(_)) => false,
                _ => true,
            },
            Import::Named { .. } => false,
        });
        self.resolve_segments_inner(krate, module, segments, leading_colon, !outside)
    }

    fn resolve_segments_inner(
        &self,
        krate: usize,
        module: ModId,
        segments: &[String],
        leading_colon: bool,
        allow_fallback: bool,
    ) -> Option<Resolved> {
        if let Some(res) = self.resolve_exact(krate, module, segments, leading_colon) {
            return Some(Resolved {
                res,
                inferred: false,
            });
        }
        let (first, rest) = segments.split_first()?;
        // `Self::f` is an associated item of the impl's type, never a crate.
        let relative_root = matches!(first.as_str(), "crate" | "self" | "super" | "Self");
        if !rest.is_empty() && !relative_root && self.lookup(krate, module, first).is_none() {
            // `tokio::task::JoinHandle` with no `use`: a path rooted at a name
            // that is neither in scope nor a known crate is an external crate.
            return Some(Resolved {
                res: Res::External(segments.join("::")),
                inferred: false,
            });
        }
        let last = segments.last()?;
        if !allow_fallback || self.glob_reaches_outside(krate, module, &mut HashSet::new()) {
            return None;
        }
        match self.by_last_segment.get(last).map(Vec::as_slice) {
            Some([only]) => Some(Resolved {
                res: Res::Decl(*only),
                inferred: true,
            }),
            _ => None,
        }
    }

    fn resolve_exact(
        &self,
        krate: usize,
        module: ModId,
        segments: &[String],
        leading_colon: bool,
    ) -> Option<Res> {
        let (first, rest) = segments.split_first()?;
        let mut res = if leading_colon {
            self.extern_root(first)?
        } else {
            match first.as_str() {
                "crate" => Res::Module(krate, 0),
                "self" => Res::Module(krate, module),
                "super" => Res::Module(krate, self.crates[krate].modules[module].parent?),
                _ => self
                    .lookup(krate, module, first)
                    .or_else(|| self.extern_root(first))
                    .or_else(|| prelude(first))?,
            }
        };
        for segment in rest {
            res = self.step(&res, segment)?;
        }
        Some(res)
    }

    /// Path roots that are crate names: declaration-scope crates, rsactor,
    /// std. Any other name is taken to be an external crate only when more
    /// segments follow; a lone unknown name stays unresolved.
    fn extern_root(&self, name: &str) -> Option<Res> {
        if let Some(&c) = self.crate_by_name.get(name) {
            return Some(Res::Module(c, 0));
        }
        match name {
            "rsactor" => Some(Res::Rsactor(String::new())),
            "std" | "core" | "alloc" => Some(Res::Std("std".to_string())),
            _ => None,
        }
    }

    fn step(&self, res: &Res, segment: &str) -> Option<Res> {
        match res {
            Res::Module(c, m) => match segment {
                "super" => Some(Res::Module(*c, self.crates[*c].modules[*m].parent?)),
                "self" => Some(res.clone()),
                _ => self.lookup(*c, *m, segment),
            },
            Res::Decl(id) if self.decls[*id].kind == DeclKind::Enum => {
                Some(Res::Variant(*id, segment.to_string()))
            }
            Res::Rsactor(_) => Some(Res::Rsactor(segment.to_string())),
            Res::Std(prefix) => Some(Res::Std(format!("{prefix}::{segment}"))),
            Res::External(prefix) => Some(Res::External(format!("{prefix}::{segment}"))),
            _ => None,
        }
    }

    /// Looks `name` up inside one module: child modules, local items, named
    /// imports, then globs (an explicit name shadows a glob, as in Rust).
    fn lookup(&self, krate: usize, module: ModId, name: &str) -> Option<Res> {
        let key = (krate, module, name.to_string());
        if !self.active.borrow_mut().insert(key.clone()) {
            return None;
        }
        let found = self.lookup_inner(krate, module, name);
        self.active.borrow_mut().remove(&key);
        found
    }

    fn lookup_inner(&self, krate: usize, module: ModId, name: &str) -> Option<Res> {
        if let Some(&child) = self.crates[krate].modules[module].children.get(name) {
            return Some(Res::Module(krate, child));
        }
        let scope = &self.scopes[krate][module];
        if let Some(&id) = scope.types.get(name) {
            return Some(Res::Decl(id));
        }
        if scope.values.contains(name) {
            let mut path = self.crates[krate].module_path(module);
            path.push(name.to_string());
            return Some(Res::Value(path.join("::")));
        }
        for import in &scope.imports {
            if let Import::Named {
                alias,
                path,
                leading_colon,
            } = import
            {
                if alias == name {
                    if let Some(res) = self.resolve_use(krate, module, path, *leading_colon) {
                        return Some(res);
                    }
                }
            }
        }
        for import in &scope.imports {
            if let Import::Glob {
                path,
                leading_colon,
            } = import
            {
                let found = match self.resolve_use(krate, module, path, *leading_colon) {
                    Some(Res::Module(c, m)) => self.lookup(c, m, name),
                    Some(Res::Decl(id)) if self.decls[id].kind == DeclKind::Enum => {
                        Some(Res::Variant(id, name.to_string()))
                    }
                    Some(Res::Rsactor(_)) if RSACTOR_NAMES.contains(&name) => {
                        Some(Res::Rsactor(name.to_string()))
                    }
                    _ => None,
                };
                if found.is_some() {
                    return found;
                }
            }
        }
        None
    }

    /// Resolves the target of a `use` path. Unknown roots become external
    /// crates here, because a `use` path always starts at a crate or module.
    fn resolve_use(
        &self,
        krate: usize,
        module: ModId,
        path: &[String],
        leading_colon: bool,
    ) -> Option<Res> {
        if let Some(res) = self.resolve_exact(krate, module, path, leading_colon) {
            return Some(res);
        }
        let (first, rest) = path.split_first()?;
        if leading_colon || self.lookup(krate, module, first).is_none() {
            let mut res = Res::External(first.clone());
            for segment in rest {
                res = self.step(&res, segment)?;
            }
            return Some(res);
        }
        None
    }

    /// Whether any glob reachable from this module's scope (directly or
    /// through globs of other in-scope modules) leads outside the declaration
    /// scope. An unresolvable glob target counts as outside.
    fn glob_reaches_outside(
        &self,
        krate: usize,
        module: ModId,
        visited: &mut HashSet<(usize, ModId)>,
    ) -> bool {
        if !visited.insert((krate, module)) {
            return false;
        }
        for import in &self.scopes[krate][module].imports {
            let Import::Glob {
                path,
                leading_colon,
            } = import
            else {
                continue;
            };
            match self.resolve_use(krate, module, path, *leading_colon) {
                Some(Res::Module(c, m)) => {
                    if self.glob_reaches_outside(c, m, visited) {
                        return true;
                    }
                }
                Some(Res::Rsactor(_)) | Some(Res::Decl(_)) => {}
                _ => return true,
            }
        }
        false
    }
}

fn prelude(name: &str) -> Option<Res> {
    if let Some((_, path)) = STD_PRELUDE.iter().find(|(n, _)| *n == name) {
        return Some(Res::Std((*path).to_string()));
    }
    PRIMITIVES
        .contains(&name)
        .then(|| Res::Std(name.to_string()))
}

fn declared_type(item: &Item) -> Option<(String, DeclKind, Vec<String>)> {
    let (ident, kind, generics) = match item {
        Item::Struct(i) => (&i.ident, DeclKind::Struct, &i.generics),
        Item::Enum(i) => (&i.ident, DeclKind::Enum, &i.generics),
        Item::Union(i) => (&i.ident, DeclKind::Union, &i.generics),
        Item::Trait(i) => (&i.ident, DeclKind::Trait, &i.generics),
        Item::Type(i) => (&i.ident, DeclKind::TypeAlias, &i.generics),
        _ => return None,
    };
    Some((ident.to_string(), kind, type_params(generics)))
}

pub fn type_params(generics: &syn::Generics) -> Vec<String> {
    generics
        .params
        .iter()
        .filter_map(|param| match param {
            syn::GenericParam::Type(t) => Some(t.ident.to_string()),
            _ => None,
        })
        .collect()
}

/// The imports of one `use` item, for block-level `use`.
pub fn imports_of(item_use: &syn::ItemUse) -> Vec<Import> {
    let mut out = Vec::new();
    flatten_use(
        &item_use.tree,
        Vec::new(),
        item_use.leading_colon.is_some(),
        &mut out,
    );
    out
}

fn flatten_use(tree: &UseTree, prefix: Vec<String>, leading_colon: bool, out: &mut Vec<Import>) {
    match tree {
        UseTree::Path(p) => {
            let mut next = prefix;
            next.push(p.ident.to_string());
            flatten_use(&p.tree, next, leading_colon, out);
        }
        UseTree::Name(n) => {
            let ident = n.ident.to_string();
            let (alias, path) = if ident == "self" {
                match prefix.last() {
                    Some(last) => (last.clone(), prefix),
                    None => return,
                }
            } else {
                let mut path = prefix;
                path.push(ident.clone());
                (ident, path)
            };
            out.push(Import::Named {
                alias,
                path,
                leading_colon,
            });
        }
        UseTree::Rename(r) => {
            let alias = r.rename.to_string();
            if alias == "_" {
                return;
            }
            let ident = r.ident.to_string();
            let path = if ident == "self" {
                prefix
            } else {
                let mut path = prefix;
                path.push(ident);
                path
            };
            out.push(Import::Named {
                alias,
                path,
                leading_colon,
            });
        }
        UseTree::Glob(_) => out.push(Import::Glob {
            path: prefix,
            leading_colon,
        }),
        UseTree::Group(g) => {
            for item in &g.items {
                flatten_use(item, prefix.clone(), leading_colon, out);
            }
        }
    }
}
