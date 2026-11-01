// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Function bodies of target crates: who the sender is (§4.4) and which
//! actors it spawns. Stage 3 adds sends to the same walk.

use std::collections::HashSet;
use std::path::Path;

use syn::visit::{self, Visit};
use syn::{
    Expr, ExprCall, GenericArgument, ImplItem, Item, ItemFn, ItemImpl, ItemTrait, Local, Pat,
    PathArguments, TraitItem,
};

use crate::crate_tree::{location_of, ModId};
use crate::discover::{impl_role, Builder, EdgeData, Endpoint, ImplRole};
use crate::model::{Category, EdgeKind, Location, Resolution};
use crate::resolve::{type_params, DeclId, DeclKind, Res};
use crate::types::{Ty, TyCtx};

/// rsactor's spawn functions (`src/lib.rs`).
const SPAWN_FNS: &[&str] = &["spawn", "spawn_with_mailbox_capacity", "spawn_with_options"];

pub fn scan_bodies(b: &mut Builder) {
    let index = b.index;
    for (c, krate) in index.crates.iter().enumerate() {
        if !krate.is_target {
            continue;
        }
        let has_main = !krate.spec.target.starts_with("lib:");
        for (m, module) in krate.modules.iter().enumerate() {
            let module_path = krate.module_path(m).join("::");
            for item in &module.items {
                let place = Place {
                    krate: c,
                    module: m,
                    file: &module.file,
                };
                match item {
                    Item::Fn(f) => {
                        let path = format!("{module_path}::{}", f.sig.ident);
                        let sender = if has_main && m == 0 && f.sig.ident == "main" {
                            Endpoint::External
                        } else {
                            fn_endpoint(b, c, &path)
                        };
                        let generics = type_params(&f.sig.generics);
                        let mut walker = Walker::new(b, place, sender, path, generics, None);
                        walker.visit_block(&f.block);
                    }
                    Item::Impl(imp) => scan_impl(b, place, imp),
                    Item::Trait(t) => scan_trait(b, place, &module_path, t),
                    _ => {}
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Place<'f> {
    krate: usize,
    module: ModId,
    file: &'f Path,
}

fn fn_endpoint(b: &Builder, krate: usize, path: &str) -> Endpoint {
    Endpoint::Fn {
        target: b.index.crates[krate].spec.target.clone(),
        path: path.to_string(),
    }
}

/// Sender rules for methods (§4.4): an actor's `self` methods and its
/// `Actor`/`Message` trait methods send as the actor; any other function in
/// the impl is a function node. Treating `self`-less associated functions as
/// the actor would turn `Counter::incr(r)` called from `main` into a false
/// `Counter -> Counter` self-loop.
fn scan_impl(b: &mut Builder, place: Place, imp: &ItemImpl) {
    let index = b.index;
    let impl_generics = type_params(&imp.generics);
    let self_ty = TyCtx {
        index,
        krate: place.krate,
        module: place.module,
        generics: &impl_generics,
        self_ty: None,
    }
    .resolve(&imp.self_ty);
    let actor: Option<DeclId> = match &self_ty {
        Ty::Decl { id, .. } if b.actors.contains_key(id) => Some(*id),
        _ => None,
    };
    let trait_methods_send_as_actor = matches!(
        impl_role(index, place.krate, place.module, imp),
        Some(ImplRole::Actor | ImplRole::Message)
    );
    let type_path = match &self_ty {
        Ty::Decl { id, .. } => index.decl_path(*id),
        other => other.key(index),
    };
    for item in &imp.items {
        let ImplItem::Fn(f) = item else { continue };
        let path = format!("{type_path}::{}", f.sig.ident);
        let sender = match actor {
            Some(id) if f.sig.receiver().is_some() || trait_methods_send_as_actor => {
                Endpoint::Actor(id)
            }
            _ => fn_endpoint(b, place.krate, &path),
        };
        let mut generics = impl_generics.clone();
        generics.extend(type_params(&f.sig.generics));
        let mut walker = Walker::new(b, place, sender, path, generics, Some(self_ty.clone()));
        walker.visit_block(&f.block);
    }
}

/// Trait default method bodies send as the trait method (§4.4).
fn scan_trait(b: &mut Builder, place: Place, module_path: &str, t: &ItemTrait) {
    for item in &t.items {
        let TraitItem::Fn(f) = item else { continue };
        let Some(block) = &f.default else { continue };
        let path = format!("{module_path}::{}::{}", t.ident, f.sig.ident);
        let sender = fn_endpoint(b, place.krate, &path);
        let mut generics = type_params(&t.generics);
        generics.extend(type_params(&f.sig.generics));
        generics.push("Self".to_string());
        let mut walker = Walker::new(b, place, sender, path, generics, None);
        walker.visit_block(block);
    }
}

struct Walker<'w, 'i, 'a, 'f> {
    b: &'w mut Builder<'i, 'a>,
    place: Place<'f>,
    sender: Endpoint,
    /// Path of the function being walked, the prefix for nested `fn` items.
    fn_path: String,
    generics: Vec<String>,
    self_ty: Option<Ty>,
    /// Calls already handled from `visit_local`, keyed by position.
    handled: HashSet<(usize, usize)>,
}

impl<'w, 'i, 'a, 'f> Walker<'w, 'i, 'a, 'f> {
    fn new(
        b: &'w mut Builder<'i, 'a>,
        place: Place<'f>,
        sender: Endpoint,
        fn_path: String,
        generics: Vec<String>,
        self_ty: Option<Ty>,
    ) -> Self {
        Walker {
            b,
            place,
            sender,
            fn_path,
            generics,
            self_ty,
            handled: HashSet::new(),
        }
    }

    fn resolve_ty(&self, ty: &syn::Type) -> Ty {
        TyCtx {
            index: self.b.index,
            krate: self.place.krate,
            module: self.place.module,
            generics: &self.generics,
            self_ty: self.self_ty.as_ref(),
        }
        .resolve(ty)
    }

    fn location(&self, span: proc_macro2::Span) -> Location {
        location_of(self.place.file, span, self.b.base)
    }

    fn report(&mut self, category: Category, location: Location, detail: String) {
        let sender = self.b.endpoint_display(&self.sender);
        self.b.report.push_from(category, location, &sender, detail);
    }

    /// `Some((name, resolved))` when the call is an rsactor spawn, or a call
    /// named like one whose path does not resolve (`resolved == false`).
    /// `tokio::spawn`, `task::spawn` and local `fn spawn` resolve elsewhere
    /// and are not candidates.
    fn spawn_fn(&self, call: &ExprCall) -> Option<(String, bool)> {
        let Expr::Path(path) = &*call.func else {
            return None;
        };
        let name = path.path.segments.last()?.ident.to_string();
        if !SPAWN_FNS.contains(&name.as_str()) {
            return None;
        }
        match self
            .b
            .index
            .resolve_path(self.place.krate, self.place.module, &path.path)
        {
            Some(r) if !r.inferred && r.res == Res::Rsactor(name.clone()) => Some((name, true)),
            Some(_) => None,
            None => Some((name, false)),
        }
    }

    /// §4.4 spawn: turbofish, then the binding annotation, then a struct
    /// literal of a `#[derive(Actor)]` type; otherwise report.
    fn spawn(
        &mut self,
        call: &ExprCall,
        name: &str,
        resolved: bool,
        annotation: Option<&syn::Type>,
    ) {
        let Expr::Path(path) = &*call.func else {
            return;
        };
        let Some(last) = path.path.segments.last() else {
            return;
        };
        let start = last.ident.span().start();
        if !self.handled.insert((start.line, start.column)) {
            return;
        }
        let location = self.location(last.ident.span());
        if !resolved {
            self.report(
                Category::UnresolvedSpawn,
                location,
                format!("`{name}(..)` does not resolve, so it is not known to be rsactor's"),
            );
            return;
        }
        let variant = (name != "spawn").then(|| name.to_string());

        if let PathArguments::AngleBracketed(angle) = &last.arguments {
            if let Some(GenericArgument::Type(ty)) = angle.args.first() {
                let target = self.resolve_ty(ty);
                self.spawn_edge(target, variant, location);
                return;
            }
        }
        if let Some(annotation) = annotation {
            if let Some(target) = actor_ref_target(self.resolve_ty(annotation)) {
                self.spawn_edge(target, variant, location);
                return;
            }
        }
        if let Some(id) = call.args.first().and_then(|arg| self.literal_struct(arg)) {
            let derived = self.b.actors.get(&id).is_some_and(|a| a.derive);
            if derived {
                let key = self.b.index.decl_path(id);
                let also_args_of: Vec<String> = self
                    .b
                    .actors
                    .iter()
                    .filter(|(other, info)| {
                        **other != id
                            && info
                                .args
                                .as_ref()
                                .is_some_and(|(ty, _)| ty.key(self.b.index) == key)
                    })
                    .map(|(other, _)| self.b.index.decl_path(*other))
                    .collect();
                if also_args_of.is_empty() {
                    let target = Ty::Decl {
                        id,
                        args: Vec::new(),
                        inferred: false,
                    };
                    self.spawn_edge(target, variant, location);
                } else {
                    self.report(
                        Category::UnresolvedSpawn,
                        location,
                        format!(
                            "`{name}({key} {{ .. }})`: `{key}` derives `Actor`, but is also \
                             `type Args` of {}",
                            also_args_of.join(", ")
                        ),
                    );
                }
                return;
            }
        }
        self.report(
            Category::UnresolvedSpawn,
            location,
            format!("`{name}(..)` without a type argument; the spawned actor cannot be determined"),
        );
    }

    fn spawn_edge(&mut self, target: Ty, variant: Option<String>, location: Location) {
        let resolution = target.resolution();
        let to = match target {
            Ty::Decl { id, .. } => Endpoint::Actor(id),
            Ty::External { path, .. } => Endpoint::ExternalActor(path),
            other => {
                let label = other.label(self.b.index);
                self.report(
                    Category::UnresolvedSpawn,
                    location,
                    format!("spawns `{label}`, which is not a concrete actor type"),
                );
                return;
            }
        };
        let from = self.sender.clone();
        self.b.add_edge(
            &from,
            &to,
            EdgeData {
                kind: EdgeKind::Spawn,
                variant,
                message: None,
                reply: None,
                receiver_resolution: resolution,
                message_resolution: Resolution::Exact,
                location,
            },
        );
    }

    /// `L { .. }`, `L` or `L(..)` where `L` resolves exactly to a struct.
    fn literal_struct(&self, expr: &Expr) -> Option<DeclId> {
        let path = match expr {
            Expr::Struct(s) => &s.path,
            Expr::Path(p) => &p.path,
            Expr::Call(c) => match &*c.func {
                Expr::Path(p) => &p.path,
                _ => return None,
            },
            _ => return None,
        };
        let resolved = self
            .b
            .index
            .resolve_path(self.place.krate, self.place.module, path)?;
        match resolved.res {
            Res::Decl(id)
                if !resolved.inferred && self.b.index.decls[id].kind == DeclKind::Struct =>
            {
                Some(id)
            }
            _ => None,
        }
    }
}

/// `(ActorRef<Y>, _)` or `ActorRef<Y>` → `Y`.
fn actor_ref_target(ty: Ty) -> Option<Ty> {
    let first = match ty {
        Ty::Tuple(mut items) if !items.is_empty() => items.swap_remove(0),
        other => other,
    };
    match first {
        Ty::Rsactor { name, mut args } if name == "ActorRef" && !args.is_empty() => {
            Some(args.swap_remove(0))
        }
        _ => None,
    }
}

fn strip(expr: &Expr) -> &Expr {
    match expr {
        Expr::Paren(p) => strip(&p.expr),
        Expr::Group(g) => strip(&g.expr),
        other => other,
    }
}

impl<'ast> Visit<'ast> for Walker<'_, '_, '_, '_> {
    fn visit_local(&mut self, local: &'ast Local) {
        if let (Pat::Type(typed), Some(init)) = (&local.pat, &local.init) {
            if let Expr::Call(call) = strip(&init.expr) {
                if let Some((name, resolved)) = self.spawn_fn(call) {
                    self.spawn(call, &name, resolved, Some(&typed.ty));
                }
            }
        }
        visit::visit_local(self, local);
    }

    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if let Some((name, resolved)) = self.spawn_fn(call) {
            self.spawn(call, &name, resolved, None);
        }
        visit::visit_expr_call(self, call);
    }

    /// A nested `fn` is its own sender, not part of the enclosing method.
    fn visit_item_fn(&mut self, f: &'ast ItemFn) {
        let path = format!("{}::{}", self.fn_path, f.sig.ident);
        let sender = fn_endpoint(self.b, self.place.krate, &path);
        let generics = type_params(&f.sig.generics);
        let mut nested = Walker::new(self.b, self.place, sender, path, generics, None);
        nested.visit_block(&f.block);
    }

    /// An `impl` inside a function body follows the same rules as a
    /// top-level one.
    fn visit_item_impl(&mut self, imp: &'ast ItemImpl) {
        scan_impl(self.b, self.place, imp);
    }

    fn visit_item_trait(&mut self, t: &'ast ItemTrait) {
        let prefix = self.fn_path.clone();
        scan_trait(self.b, self.place, &prefix, t);
    }
}
