// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Function bodies of target crates (§4.4): who the sender is, what each
//! binding's type is, and the spawn and send edges that follow.
//!
//! The walk keeps a lexical type environment: one map per scope (block,
//! closure, `for`, `if let`, `while let`, match arm), innermost first on
//! lookup. A binding whose type cannot be decided is still recorded, as
//! `None`, so it shadows an outer binding of the same name instead of
//! letting the outer type leak in.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{
    Expr, ExprCall, ExprClosure, ExprForLoop, ExprIf, ExprLet, ExprMatch, ExprMethodCall,
    ExprWhile, Fields, GenericArgument, ImplItem, Item, ItemFn, ItemImpl, ItemTrait, Local, Macro,
    Member, Pat, PathArguments, ReturnType, Signature, Stmt, Token, TraitItem, UnOp,
};

use crate::crate_tree::{location_of, ModId};
use crate::discover::{
    handler_sig, impl_role, Builder, EdgeData, Endpoint, FnDecl, HandlerSig, ImplRole,
};
use crate::model::{Category, EdgeKind, Location, Resolution};
use crate::resolve::{imports_of, type_params, DeclId, DeclKind, Import, Res, Resolved};
use crate::types::{Ty, TyCtx};

/// Macros whose arguments are comma-separated expressions; their tokens are
/// re-parsed and walked like ordinary code (G6). `format!`-style literals
/// parse as expressions too. Anything that fails to parse falls back to the
/// token scan.
const EXPR_MACROS: &[&str] = &[
    "assert",
    "assert_eq",
    "assert_ne",
    "dbg",
    "debug",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
    "eprint",
    "eprintln",
    "error",
    "format",
    "format_args",
    "info",
    "join",
    "panic",
    "print",
    "println",
    "trace",
    "try_join",
    "vec",
    "warn",
    "write",
    "writeln",
];

/// Macros that never return, so a branch ending in one does not contribute
/// a type (G3).
const DIVERGING_MACROS: &[&str] = &["panic", "todo", "unimplemented", "unreachable"];

fn macro_name(mac: &Macro) -> String {
    mac.path
        .segments
        .last()
        .map(|s| s.ident.to_string())
        .unwrap_or_default()
}

/// The first comma-separated expression of a macro's tokens, for
/// `matches!(expr, pattern)` whose second part is a pattern.
fn first_macro_expr(tokens: &TokenStream) -> Option<Expr> {
    let head: TokenStream = tokens
        .clone()
        .into_iter()
        .take_while(|t| !matches!(t, TokenTree::Punct(p) if p.as_char() == ','))
        .collect();
    syn::parse2(head).ok()
}

fn diverges(expr: &Expr) -> bool {
    match strip(expr) {
        Expr::Return(_) | Expr::Break(_) | Expr::Continue(_) => true,
        Expr::Macro(m) => DIVERGING_MACROS.contains(&macro_name(&m.mac).as_str()),
        _ => false,
    }
}

/// rsactor's spawn functions (`src/lib.rs`).
const SPAWN_FNS: &[&str] = &["spawn", "spawn_with_mailbox_capacity", "spawn_with_options"];

/// `ActorRef` send methods and whether each one is an ask
/// (`src/actor_ref.rs`). Keep in sync with the public API.
const ACTOR_REF_SENDS: &[(&str, bool)] = &[
    ("tell", false),
    ("tell_with_timeout", false),
    ("tell_priority", false),
    ("blocking_tell", false),
    ("blocking_tell_priority", false),
    ("ask", true),
    ("ask_with_timeout", true),
    ("ask_priority", true),
    ("blocking_ask", true),
    ("blocking_ask_priority", true),
    ("ask_join", true),
];

/// Send methods of `TellHandler`/`AskHandler` (`src/handler.rs`).
const HANDLER_SENDS: &[&str] = &[
    "tell",
    "tell_with_timeout",
    "blocking_tell",
    "ask",
    "ask_with_timeout",
    "blocking_ask",
];

/// Iterator adapters whose closure receives the iterator's element (§4.4 R3).
const ADAPTERS: &[&str] = &[
    "for_each",
    "map",
    "filter",
    "filter_map",
    "find",
    "any",
    "all",
];

/// rsactor types and traits a UFCS send is written through.
const UFCS_ROOTS: &[&str] = &["ActorRef", "TellHandler", "AskHandler"];

fn send_kind(method: &str) -> Option<bool> {
    ACTOR_REF_SENDS
        .iter()
        .find(|(name, _)| *name == method)
        .map(|(_, ask)| *ask)
}

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
                            // Free functions group by module in DOT output.
                            fn_endpoint(b, c, &path, Some(&module_path))
                        };
                        let generics = type_params(&f.sig.generics);
                        let mut walker = Walker::new(b, place, sender, path, generics, None);
                        walker.walk_fn(&f.sig, &f.block);
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

fn fn_endpoint(b: &Builder, krate: usize, path: &str, owner: Option<&str>) -> Endpoint {
    Endpoint::Fn {
        target: b.index.crates[krate].spec.target.clone(),
        path: path.to_string(),
        owner: owner.map(str::to_string),
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
        blocks: &[],
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
            _ => fn_endpoint(b, place.krate, &path, Some(&type_path)),
        };
        let mut generics = impl_generics.clone();
        generics.extend(type_params(&f.sig.generics));
        let mut walker = Walker::new(b, place, sender, path, generics, Some(self_ty.clone()));
        walker.walk_fn(&f.sig, &f.block);
    }
}

/// Trait default method bodies send as the trait method (§4.4).
fn scan_trait(b: &mut Builder, place: Place, module_path: &str, t: &ItemTrait) {
    for item in &t.items {
        let TraitItem::Fn(f) = item else { continue };
        let Some(block) = &f.default else { continue };
        let owner = format!("{module_path}::{}", t.ident);
        let path = format!("{owner}::{}", f.sig.ident);
        let sender = fn_endpoint(b, place.krate, &path, Some(&owner));
        let mut generics = type_params(&t.generics);
        generics.extend(type_params(&f.sig.generics));
        generics.push("Self".to_string());
        let mut walker = Walker::new(b, place, sender, path, generics, None);
        walker.walk_fn(&f.sig, block);
    }
}

/// How a send's receiver was classified (§4.4 후보 분류).
enum Receiver {
    Actor {
        to: Endpoint,
        resolution: Resolution,
        decl: Option<DeclId>,
    },
    Handlers(HandlerSig),
    /// Known not to be rsactor: drop the call.
    Unrelated,
    /// A type the rules do not follow; report it.
    Unsupported(String),
    /// Type not determined.
    Unknown,
}

type Scope = HashMap<String, Option<Ty>>;

struct Walker<'w, 'i, 'a, 'f> {
    b: &'w mut Builder<'i, 'a>,
    place: Place<'f>,
    sender: Endpoint,
    /// Path of the function being walked, the prefix for nested `fn` items.
    fn_path: String,
    generics: Vec<String>,
    self_ty: Option<Ty>,
    scopes: Vec<Scope>,
    /// Element type for the parameters of the closure passed to an iterator
    /// adapter, keyed by the closure's address.
    closure_hint: Option<(usize, Option<Ty>)>,
    /// Calls already handled from `visit_local`, keyed by position.
    handled: HashSet<(usize, usize)>,
    /// `use` declarations of the enclosing blocks, innermost last (G1).
    blocks: Vec<Vec<Import>>,
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
            scopes: Vec::new(),
            closure_hint: None,
            handled: HashSet::new(),
            blocks: Vec::new(),
        }
    }

    fn resolve_path(&self, path: &syn::Path) -> Option<Resolved> {
        self.b
            .index
            .resolve_path_scoped(self.place.krate, self.place.module, path, &self.blocks)
    }

    fn resolve_segments(&self, segments: &[String], leading_colon: bool) -> Option<Resolved> {
        self.b.index.resolve_segments_scoped(
            self.place.krate,
            self.place.module,
            segments,
            leading_colon,
            &self.blocks,
        )
    }

    /// The declared type in front of an associated function call
    /// (`Type::f(..)`, `Self::f(..)`), if the prefix names one.
    fn assoc_owner(&self, path: &syn::Path) -> Option<Ty> {
        let segments: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
        let (_, prefix) = segments.split_last()?;
        if prefix.is_empty() {
            return None;
        }
        if prefix == ["Self"] {
            return match &self.self_ty {
                Some(t @ Ty::Decl { .. }) => Some(t.clone()),
                _ => None,
            };
        }
        let resolved = self.resolve_segments(prefix, path.leading_colon.is_some())?;
        match resolved.res {
            Res::Decl(id) => Some(Ty::Decl {
                id,
                args: Vec::new(),
                inferred: resolved.inferred,
            }),
            _ => None,
        }
    }

    /// `Enum::Name` is a variant only if the enum declares `Name`; otherwise
    /// it is an associated item (`Enum::new()`).
    fn is_variant(&self, id: DeclId, name: &str) -> bool {
        match self.b.index.decl_item(id) {
            Item::Enum(e) => e.variants.iter().any(|v| v.ident == name),
            _ => false,
        }
    }

    /// Return type stated by a declared signature (G4). `receiver` is what
    /// `Self` stands for; the impl's generic parameters are replaced by the
    /// receiver's arguments. A return type that is a bare generic parameter
    /// of the function cannot be known without inference.
    fn fn_return(&self, f: &FnDecl, receiver: Option<&Ty>) -> Option<Ty> {
        let ReturnType::Type(_, ty) = &f.sig.output else {
            return Some(Ty::Tuple(Vec::new()));
        };
        let mut generics = f.impl_generics.clone();
        generics.extend(type_params(&f.sig.generics));
        let self_ty = receiver.map(|r| r.auto_deref().clone());
        let mut out = TyCtx {
            index: self.b.index,
            krate: f.krate,
            module: f.module,
            generics: &generics,
            self_ty: self_ty.as_ref(),
            blocks: &[],
        }
        .resolve(ty);
        if let Some(Ty::Decl { args, .. }) = &self_ty {
            let params: Vec<String> = f
                .impl_self_args
                .iter()
                .map(|a| match a {
                    Ty::Generic(g) => g.clone(),
                    _ => String::new(),
                })
                .collect();
            out = out.substitute(&params, args);
        }
        match out {
            Ty::Generic(_) | Ty::ImplTrait(_) => None,
            other => Some(other),
        }
    }

    /// One answer from several declarations, or none if they disagree.
    fn agree(&self, types: Vec<Ty>) -> Option<Ty> {
        let first = types.first()?.clone();
        let key = first.key(self.b.index);
        types
            .iter()
            .all(|t| t.key(self.b.index) == key)
            .then_some(first)
    }

    /// A method or associated function declared on a type in scope.
    fn declared_method(&self, owner: &Ty, name: &str) -> Option<Ty> {
        let Ty::Decl { id, .. } = owner.auto_deref() else {
            return None;
        };
        let decls = self.b.methods.get(&(*id, name.to_string()))?;
        let types = decls
            .iter()
            .map(|f| self.fn_return(f, Some(owner)))
            .collect::<Option<Vec<Ty>>>()?;
        self.agree(types)
    }

    fn walk_fn(&mut self, sig: &Signature, block: &syn::Block) {
        self.scopes.push(Scope::new());
        for input in &sig.inputs {
            if let syn::FnArg::Typed(param) = input {
                let ty = self.param_ty(&param.ty);
                self.bind_pat(&param.pat, Some(ty));
            }
        }
        self.visit_block(block);
        self.scopes.pop();
    }

    // ----- types ---------------------------------------------------------

    fn ctx(&self) -> TyCtx<'_, 'a> {
        TyCtx {
            index: self.b.index,
            krate: self.place.krate,
            module: self.place.module,
            generics: &self.generics,
            self_ty: self.self_ty.as_ref(),
            blocks: &self.blocks,
        }
    }

    fn resolve_ty(&self, ty: &syn::Type) -> Ty {
        self.ctx().resolve(ty)
    }

    /// A parameter or annotation type. `Self::Args` becomes the actor's
    /// `type Args` (§4.4 R2); everything else resolves normally.
    fn param_ty(&self, ty: &syn::Type) -> Ty {
        if let syn::Type::Path(tp) = ty {
            let segments: Vec<String> = tp
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect();
            if segments == ["Self", "Args"] {
                if let Some(Ty::Decl { id, .. }) = &self.self_ty {
                    let info = self.b.actors.get(id);
                    if let Some((args, _)) = info.and_then(|i| i.args.as_ref()) {
                        return args.clone();
                    }
                    if info.is_some_and(|i| i.derive) {
                        return self.self_ty.clone().unwrap();
                    }
                }
            }
        }
        self.resolve_ty(ty)
    }

    fn lookup(&self, name: &str) -> Option<Option<Ty>> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).cloned())
    }

    fn bind(&mut self, name: String, ty: Option<Ty>) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, ty);
        }
    }

    /// Binds every name in a pattern, taking element types from `ty` where
    /// the pattern's shape allows it; names that get no type are bound to
    /// `None` so they still shadow.
    fn bind_pat(&mut self, pat: &Pat, ty: Option<Ty>) {
        match pat {
            Pat::Ident(p) => {
                self.bind(p.ident.to_string(), ty.clone());
                if let Some((_, sub)) = &p.subpat {
                    self.bind_pat(sub, ty);
                }
            }
            Pat::Type(p) => {
                let annotated = self.param_ty(&p.ty);
                self.bind_pat(&p.pat, Some(annotated));
            }
            Pat::Tuple(p) => {
                let items = ty.and_then(|t| match t.auto_deref() {
                    Ty::Tuple(items) => Some(items.clone()),
                    _ => None,
                });
                let elems: Vec<&Pat> = p.elems.iter().collect();
                self.bind_positional(&elems, items.as_deref());
            }
            Pat::TupleStruct(p) => {
                let fields = ty.and_then(|t| self.tuple_struct_fields(&p.path, &t));
                let elems: Vec<&Pat> = p.elems.iter().collect();
                self.bind_positional(&elems, fields.as_deref());
            }
            Pat::Struct(p) => {
                let fields = ty.and_then(|t| self.named_fields(&p.path, &t));
                for field in &p.fields {
                    let field_ty = match (&field.member, &fields) {
                        (Member::Named(name), Some(fields)) => fields
                            .iter()
                            .find(|(n, _)| n.as_deref() == Some(&name.to_string()))
                            .map(|(_, t)| t.clone()),
                        _ => None,
                    };
                    self.bind_pat(&field.pat, field_ty);
                }
            }
            Pat::Reference(p) => self.bind_pat(&p.pat, ty),
            Pat::Paren(p) => self.bind_pat(&p.pat, ty),
            Pat::Guard(p) => self.bind_pat(&p.pat, ty),
            Pat::Or(p) => {
                for case in &p.cases {
                    self.bind_pat(case, ty.clone());
                }
            }
            Pat::Slice(p) => {
                let elem = ty.and_then(|t| elem_of(&t));
                for e in &p.elems {
                    self.bind_pat(e, elem.clone());
                }
            }
            _ => {}
        }
    }

    /// Tuple-like patterns, honouring a `..` rest: elements before it map
    /// from the front, elements after it from the back.
    fn bind_positional(&mut self, elems: &[&Pat], items: Option<&[Ty]>) {
        let rest = elems.iter().position(|p| matches!(p, Pat::Rest(_)));
        for (i, elem) in elems.iter().enumerate() {
            let ty = items.and_then(|items| match rest {
                Some(r) if i > r => {
                    let from_end = elems.len() - i;
                    items.len().checked_sub(from_end).and_then(|j| items.get(j))
                }
                _ => items.get(i),
            });
            self.bind_pat(elem, ty.cloned());
        }
    }

    /// Field types of `Some(..)`, `Ok(..)`, `Err(..)`, a tuple struct or a
    /// tuple variant, instantiated with the scrutinee's generic arguments.
    fn tuple_struct_fields(&self, path: &syn::Path, ty: &Ty) -> Option<Vec<Ty>> {
        let last = path.segments.last()?.ident.to_string();
        let (std_name, args) = std_parts(ty.auto_deref()).unwrap_or(("", &[]));
        match (last.as_str(), std_name) {
            ("Some", "Option") | ("Ok", "Result") => return args.first().map(|t| vec![t.clone()]),
            ("Err", "Result") => return args.get(1).map(|t| vec![t.clone()]),
            _ => {}
        }
        let fields = self.named_fields(path, ty)?;
        Some(fields.into_iter().map(|(_, t)| t).collect())
    }

    /// Fields of the struct or enum variant a pattern path names.
    fn named_fields(&self, path: &syn::Path, ty: &Ty) -> Option<Vec<(Option<String>, Ty)>> {
        let resolved = self.resolve_path(path)?;
        let args = match ty.auto_deref() {
            Ty::Decl { args, .. } => args.clone(),
            _ => Vec::new(),
        };
        match resolved.res {
            Res::Decl(id) => self.decl_fields(id, &args, None),
            Res::Variant(id, variant) => self.decl_fields(id, &args, Some(&variant)),
            _ => None,
        }
    }

    /// Field names and types of a declared struct (or one enum variant),
    /// with the declaration's generic parameters replaced by `args`.
    fn decl_fields(
        &self,
        id: DeclId,
        args: &[Ty],
        variant: Option<&str>,
    ) -> Option<Vec<(Option<String>, Ty)>> {
        let index = self.b.index;
        let decl = &index.decls[id];
        let fields: &Fields = match (index.decl_item(id), variant) {
            (Item::Struct(s), None) => &s.fields,
            (Item::Enum(e), Some(name)) => &e.variants.iter().find(|v| v.ident == name)?.fields,
            _ => return None,
        };
        let self_ty = Ty::Decl {
            id,
            args: args.to_vec(),
            inferred: false,
        };
        let cx = TyCtx {
            index,
            krate: decl.krate,
            module: decl.module,
            generics: &decl.generics,
            self_ty: Some(&self_ty),
            blocks: &[],
        };
        Some(
            fields
                .iter()
                .map(|f| {
                    let ty = cx.resolve(&f.ty).substitute(&decl.generics, args);
                    (f.ident.as_ref().map(|i| i.to_string()), ty)
                })
                .collect(),
        )
    }

    fn field_ty(&self, base: &Ty, member: &Member) -> Option<Ty> {
        let Ty::Decl { id, args, .. } = base.auto_deref() else {
            return None;
        };
        let fields = self.decl_fields(*id, args, None)?;
        match member {
            Member::Named(name) => fields
                .into_iter()
                .find(|(n, _)| n.as_deref() == Some(&name.to_string()))
                .map(|(_, t)| t),
            Member::Unnamed(i) => fields.into_iter().nth(i.index as usize).map(|(_, t)| t),
        }
    }

    /// The type of an expression, as far as the rules of §4.4 and the
    /// return type table of §4.4.1 go.
    fn type_of(&self, expr: &Expr) -> Option<Ty> {
        match strip(expr) {
            Expr::Path(p) if p.qself.is_none() => {
                if p.path.segments.len() == 1 {
                    let name = p.path.segments[0].ident.to_string();
                    if name == "self" {
                        return self.self_ty.clone();
                    }
                    if let Some(binding) = self.lookup(&name) {
                        return binding;
                    }
                }
                // A unit struct or unit variant used as a value:
                // `let job = Job;`, `let c = Command::Start;`.
                self.path_type(&p.path).map(|(t, _)| t)
            }
            Expr::Field(f) => {
                let base = self.type_of(&f.base)?;
                self.field_ty(&base, &f.member)
            }
            Expr::MethodCall(mc) => {
                let receiver = self.type_of(&mc.receiver)?;
                let method = mc.method.to_string();
                method_return(&receiver, &method)
                    .or_else(|| self.declared_method(&receiver, &method))
            }
            Expr::Call(call) => self.call_return(call),
            // The type a declared `async fn` states is what `.await` yields;
            // `fn_return` already reports that type for the call itself.
            Expr::Await(a) => self.type_of(&a.base),
            Expr::Tuple(t) => Some(Ty::Tuple(
                t.elems
                    .iter()
                    .map(|e| {
                        self.type_of(e)
                            .unwrap_or_else(|| Ty::Unknown("_".to_string()))
                    })
                    .collect(),
            )),
            Expr::Reference(r) => self.type_of(&r.expr),
            Expr::Unary(u) if matches!(u.op, UnOp::Deref(_)) => {
                self.type_of(&u.expr).map(|t| t.auto_deref().clone())
            }
            Expr::Try(t) => {
                let inner = self.type_of(&t.expr)?;
                match std_parts(inner.auto_deref()) {
                    Some(("Option" | "Result", args)) => args.first().cloned(),
                    _ => None,
                }
            }
            Expr::Index(i) => {
                let base = self.type_of(&i.expr)?;
                match std_parts(base.auto_deref()) {
                    Some(("HashMap" | "BTreeMap", args)) => args.get(1).cloned(),
                    _ => elem_of(&base),
                }
            }
            Expr::Struct(s) => self.path_type(&s.path).map(|(t, _)| t),
            Expr::Cast(c) => Some(self.resolve_ty(&c.ty)),
            _ => None,
        }
    }

    /// Calls whose result type is known: rsactor spawns,
    /// `ActorRef::downgrade(&r)`, tuple-struct and tuple-variant
    /// constructors, and functions whose signature states the type (G4).
    fn call_return(&self, call: &ExprCall) -> Option<Ty> {
        let Expr::Path(func) = &*call.func else {
            return None;
        };
        let segments: Vec<String> = func
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect();
        let resolved = self.resolve_path(&func.path).map(|r| r.res);
        let last = segments.last()?;
        if let Some(Res::Value(path)) = &resolved {
            let f = self.b.functions.get(path)?;
            return self.fn_return(f, None);
        }
        let is_assoc = match &resolved {
            None => true,
            Some(Res::Variant(id, name)) => !self.is_variant(*id, name),
            _ => false,
        };
        if is_assoc {
            let owner = self.assoc_owner(&func.path)?;
            return self.declared_method(&owner, last);
        }
        match resolved {
            Some(Res::Rsactor(name)) if SPAWN_FNS.contains(&name.as_str()) => {
                let target = self.spawn_target(call, None)?;
                Some(Ty::Tuple(vec![
                    Ty::Rsactor {
                        name: "ActorRef".to_string(),
                        args: vec![target],
                    },
                    Ty::Unknown("JoinHandle".to_string()),
                ]))
            }
            Some(Res::Rsactor(name))
                if name == "downgrade"
                    && segments.len() >= 2
                    && segments[segments.len() - 2] == "ActorRef" =>
            {
                let arg = self.type_of(call.args.first()?)?;
                method_return(&arg, "downgrade")
            }
            Some(Res::Decl(id)) if self.b.index.decls[id].kind == DeclKind::Struct => {
                Some(Ty::Decl {
                    id,
                    args: Vec::new(),
                    inferred: false,
                })
            }
            Some(Res::Variant(id, _)) => Some(Ty::Decl {
                id,
                args: Vec::new(),
                inferred: false,
            }),
            _ => None,
        }
    }

    /// The type a struct or variant path names: a struct, or the enum of a
    /// variant (§4.4 M1).
    fn path_type(&self, path: &syn::Path) -> Option<(Ty, Resolution)> {
        let resolved = self.resolve_path(path)?;
        let id = match resolved.res {
            Res::Decl(id) if self.b.index.decls[id].kind == DeclKind::Struct => id,
            Res::Variant(id, name) if self.is_variant(id, &name) => id,
            _ => return None,
        };
        let ty = Ty::Decl {
            id,
            args: Vec::new(),
            inferred: resolved.inferred,
        };
        let resolution = ty.resolution();
        Some((ty, resolution))
    }

    /// §4.4 M1/M2: the message type of a send argument.
    fn message_of(&self, expr: &Expr) -> Option<(Ty, Resolution)> {
        match strip(expr) {
            Expr::Struct(s) => self.path_type(&s.path),
            Expr::Path(p) if p.qself.is_none() => {
                if p.path.segments.len() == 1 {
                    let name = p.path.segments[0].ident.to_string();
                    if let Some(binding) = self.lookup(&name) {
                        return binding.map(|t| {
                            let r = t.resolution();
                            (t, r)
                        });
                    }
                }
                self.path_type(&p.path)
            }
            Expr::Call(call) => {
                let Expr::Path(func) = &*call.func else {
                    return None;
                };
                if let Some(found) = self.path_type(&func.path) {
                    return Some(found);
                }
                // A signature in scope states the type exactly (G4).
                if let Some(ty) = self.call_return(call) {
                    let resolution = ty.resolution();
                    return Some((ty, resolution));
                }
                // `M::new(..)` with no visible signature: assume `Self` only
                // for a type declared in the declaration scope;
                // `Box::new(..)` would otherwise give `Box`.
                let owner = self.assoc_owner(&func.path)?;
                let resolution = owner.resolution().weakest(Resolution::Assumed);
                Some((owner, resolution))
            }
            other => self.type_of(other).map(|t| {
                let r = t.resolution();
                (t, r)
            }),
        }
    }

    // ----- branch expressions (G3) ----------------------------------------

    /// The type of a `match`, `if`/`else` or block expression used as a
    /// value: every branch that does not diverge must agree. `None` means
    /// the expression diverges; `Some(None)` means the type is unknown.
    /// Branch bindings live in temporary scopes, so nothing leaks out.
    fn branch_type(&mut self, expr: &Expr) -> Option<Option<Ty>> {
        if diverges(expr) {
            return None;
        }
        match strip(expr) {
            Expr::Block(b) => self.block_type(&b.block),
            Expr::Unsafe(u) => self.block_type(&u.block),
            Expr::If(expr_if) => {
                let Some((_, else_branch)) = &expr_if.else_branch else {
                    return Some(None);
                };
                self.scopes.push(Scope::new());
                self.bind_condition(&expr_if.cond);
                let then = self.block_type(&expr_if.then_branch);
                self.scopes.pop();
                let other = self.branch_type(else_branch);
                self.merge(vec![then, other])
            }
            Expr::Match(expr_match) => {
                let scrutinee = self.type_of(&expr_match.expr);
                let mut arms = Vec::new();
                for arm in &expr_match.arms {
                    self.scopes.push(Scope::new());
                    let pat = match &arm.pat {
                        Pat::Guard(g) => &*g.pat,
                        other => other,
                    };
                    self.bind_pat(pat, scrutinee.clone());
                    arms.push(self.branch_type(&arm.body));
                    self.scopes.pop();
                }
                self.merge(arms)
            }
            other => Some(self.type_of(other)),
        }
    }

    fn block_type(&mut self, block: &syn::Block) -> Option<Option<Ty>> {
        self.scopes.push(Scope::new());
        let mut result = Some(None);
        let count = block.stmts.len();
        for (i, stmt) in block.stmts.iter().enumerate() {
            match stmt {
                Stmt::Local(local) => {
                    let ty = local
                        .init
                        .as_ref()
                        .and_then(|init| self.type_of(&init.expr));
                    self.bind_pat(&local.pat, ty);
                }
                Stmt::Expr(e, None) if i + 1 == count => result = self.branch_type(e),
                Stmt::Expr(e, Some(_)) if diverges(e) => {
                    result = None;
                    break;
                }
                Stmt::Macro(m) if DIVERGING_MACROS.contains(&macro_name(&m.mac).as_str()) => {
                    result = None;
                    break;
                }
                _ => {}
            }
        }
        self.scopes.pop();
        result
    }

    /// Binds the names of `if let` conditions, including `&&` chains.
    fn bind_condition(&mut self, cond: &Expr) {
        match strip(cond) {
            Expr::Let(expr_let) => {
                let ty = self.type_of(&expr_let.expr);
                self.bind_pat(&expr_let.pat, ty);
            }
            Expr::Binary(binary) if matches!(binary.op, syn::BinOp::And(_)) => {
                self.bind_condition(&binary.left);
                self.bind_condition(&binary.right);
            }
            _ => {}
        }
    }

    fn merge(&self, branches: Vec<Option<Option<Ty>>>) -> Option<Option<Ty>> {
        let live: Vec<Option<Ty>> = branches.into_iter().flatten().collect();
        if live.is_empty() {
            return None;
        }
        let types: Option<Vec<Ty>> = live.into_iter().collect();
        Some(types.and_then(|t| self.agree(t)))
    }

    // ----- receivers -----------------------------------------------------

    fn classify(&self, ty: Option<&Ty>, method: &str) -> Receiver {
        let Some(ty) = ty else {
            return Receiver::Unknown;
        };
        if let Some(args) = ty.rsactor("ActorRef") {
            return match args.first() {
                Some(target @ Ty::Decl { id, .. }) => Receiver::Actor {
                    to: Endpoint::Actor(*id),
                    resolution: target.resolution(),
                    decl: Some(*id),
                },
                Some(Ty::External { path, .. }) => Receiver::Actor {
                    to: Endpoint::ExternalActor(path.clone()),
                    resolution: Resolution::Exact,
                    decl: None,
                },
                Some(other) => {
                    Receiver::Unsupported(format!("ActorRef<{}>", other.label(self.b.index)))
                }
                None => Receiver::Unknown,
            };
        }
        if let Some((name, args)) = ty.handler_trait() {
            if name.starts_with("Weak") || !HANDLER_SENDS.contains(&method) {
                return Receiver::Unrelated;
            }
            return match handler_sig(name, args) {
                Some(sig) => Receiver::Handlers(sig),
                None => Receiver::Unknown,
            };
        }
        match ty.auto_deref() {
            Ty::Std { .. } | Ty::Tuple(_) | Ty::Slice(_) | Ty::Iter(_) | Ty::Rsactor { .. } => {
                Receiver::Unrelated
            }
            Ty::Decl { id, .. } => {
                let decl = &self.b.index.decls[*id];
                if decl.kind == DeclKind::TypeAlias {
                    Receiver::Unsupported(format!("type alias `{}`", decl.name))
                } else if self.may_wrap_actor(*id, &mut HashSet::new()) {
                    Receiver::Unsupported(format!("wrapper type `{}`", decl.name))
                } else {
                    Receiver::Unrelated
                }
            }
            Ty::External { path, .. } => {
                Receiver::Unsupported(format!("`{path}`, outside the declaration scope"))
            }
            Ty::Generic(name) => Receiver::Unsupported(format!("generic parameter `{name}`")),
            Ty::Dyn(_) | Ty::ImplTrait(_) | Ty::Unknown(_) | Ty::Ref(_) => Receiver::Unknown,
        }
    }

    /// A declared type that implements `Deref` or (transitively) holds an
    /// `ActorRef` or handler may forward a send, so it is not "unrelated".
    fn may_wrap_actor(&self, id: DeclId, seen: &mut HashSet<DeclId>) -> bool {
        if self.b.deref_impls.contains(&id) {
            return true;
        }
        if !seen.insert(id) {
            return false;
        }
        let Some(fields) = self.decl_fields(id, &[], None) else {
            return false;
        };
        fields
            .iter()
            .any(|(_, ty)| self.type_mentions_actor(ty, seen))
    }

    fn type_mentions_actor(&self, ty: &Ty, seen: &mut HashSet<DeclId>) -> bool {
        match ty {
            Ty::Rsactor { name, args } => {
                matches!(
                    name.as_str(),
                    "ActorRef"
                        | "ActorWeak"
                        | "TellHandler"
                        | "AskHandler"
                        | "WeakTellHandler"
                        | "WeakAskHandler"
                ) || args.iter().any(|a| self.type_mentions_actor(a, seen))
            }
            Ty::Decl { id, args, .. } => {
                self.may_wrap_actor(*id, seen)
                    || args.iter().any(|a| self.type_mentions_actor(a, seen))
            }
            Ty::Std { args, .. } | Ty::External { args, .. } => {
                args.iter().any(|a| self.type_mentions_actor(a, seen))
            }
            Ty::Ref(t) | Ty::Slice(t) | Ty::Iter(t) => self.type_mentions_actor(t, seen),
            Ty::Tuple(items) | Ty::Dyn(items) | Ty::ImplTrait(items) => {
                items.iter().any(|a| self.type_mentions_actor(a, seen))
            }
            Ty::Generic(_) | Ty::Unknown(_) => false,
        }
    }

    // ----- reporting and edges --------------------------------------------

    fn location(&self, span: proc_macro2::Span) -> Location {
        location_of(self.place.file, span, self.b.base)
    }

    fn report(&mut self, category: Category, location: Location, detail: String) {
        let sender = self.b.endpoint_display(&self.sender);
        self.b.report.push_from(category, location, &sender, detail);
    }

    fn handled_by_some_actor(&self, key: &str) -> bool {
        self.b
            .actors
            .values()
            .any(|a| a.messages.iter().any(|m| m.key == key))
    }

    /// A method call named like a send (§4.4 후보 분류).
    fn send(&mut self, mc: &ExprMethodCall, method: &str, ask: bool) {
        let index = self.b.index;
        let location = self.location(mc.method.span());
        let receiver_ty = self.type_of(&mc.receiver);
        let message = mc.args.first().and_then(|arg| self.message_of(arg));
        let (message_key, message_label, message_resolution) = match &message {
            Some((ty, resolution)) if *resolution != Resolution::Unresolved => {
                (Some(ty.key(index)), Some(ty.label(index)), *resolution)
            }
            _ => (None, None, Resolution::Unresolved),
        };
        let variant = (method != "tell" && method != "ask").then(|| method.to_string());
        let kind = if ask { EdgeKind::Ask } else { EdgeKind::Tell };
        let shown_message = message_label.clone().unwrap_or_else(|| "?".to_string());

        match self.classify(receiver_ty.as_ref(), method) {
            Receiver::Actor {
                to,
                resolution,
                decl,
            } => {
                let handled = decl.and_then(|id| self.b.actors.get(&id));
                let reply = handled.zip(message_key.as_ref()).and_then(|(info, key)| {
                    info.messages
                        .iter()
                        .find(|m| &m.key == key)
                        .and_then(|m| m.reply.clone())
                });
                if let (Some(info), Some(key), Some(id)) = (handled, &message_key, decl) {
                    if !info.open && !info.messages.iter().any(|m| &m.key == key) {
                        let actor = index.decl_path(id);
                        self.report(
                            Category::CrossCheck,
                            location.clone(),
                            format!(
                                "`{actor}` does not handle `{key}`; the receiver or the \
                                 message was misjudged"
                            ),
                        );
                    }
                }
                if message_key.is_none() {
                    self.report(
                        Category::UnresolvedEdge,
                        location.clone(),
                        format!(".{method}(..): message type not determined"),
                    );
                }
                self.add_send(
                    to,
                    kind,
                    variant,
                    message_key,
                    message_label,
                    reply,
                    resolution,
                    message_resolution,
                    location,
                );
            }
            Receiver::Handlers(sig) => {
                if let Some(key) = &message_key {
                    let expected = sig.message.key(index);
                    if *key != expected {
                        self.report(
                            Category::CrossCheck,
                            location.clone(),
                            format!(
                                "handler for `{expected}` is sent `{key}`; the receiver or \
                                 the message was misjudged"
                            ),
                        );
                    }
                }
                let reply = sig.reply.as_ref().map(|r| r.label(index));
                let resolution = sig.message.resolution();
                self.add_send(
                    Endpoint::Handlers(sig),
                    kind,
                    variant,
                    message_key,
                    message_label,
                    reply,
                    resolution,
                    message_resolution,
                    location,
                );
            }
            Receiver::Unrelated => {}
            Receiver::Unsupported(what) => self.report(
                Category::UnsupportedReceiver,
                location,
                format!(".{method}({shown_message}) on {what}"),
            ),
            Receiver::Unknown => match &message_key {
                Some(key) if self.handled_by_some_actor(key) => {
                    let sender_id = self.b.endpoint_id(&self.sender);
                    self.report(
                        Category::UnresolvedEdge,
                        location.clone(),
                        format!(".{method}({shown_message}): receiver not determined"),
                    );
                    self.add_send(
                        Endpoint::Unknown(sender_id),
                        kind,
                        variant,
                        message_key,
                        message_label,
                        None,
                        Resolution::Unresolved,
                        message_resolution,
                        location,
                    );
                }
                _ => self.report(
                    Category::Unclassified,
                    location,
                    format!(
                        ".{method}({shown_message}): neither the receiver nor a handled \
                         message could be determined"
                    ),
                ),
            },
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn add_send(
        &mut self,
        to: Endpoint,
        kind: EdgeKind,
        variant: Option<String>,
        message: Option<String>,
        message_label: Option<String>,
        reply: Option<String>,
        receiver_resolution: Resolution,
        message_resolution: Resolution,
        location: Location,
    ) {
        let from = self.sender.clone();
        self.b.add_edge(
            &from,
            &to,
            EdgeData {
                kind,
                variant,
                message,
                message_label,
                reply,
                receiver_resolution,
                message_resolution,
                location,
            },
        );
    }

    // ----- spawn -----------------------------------------------------------

    /// `Some((name, resolved))` when the call is an rsactor spawn, or a call
    /// named like one whose path does not resolve (`resolved == false`).
    /// `tokio::spawn`, `task::spawn` and local `fn spawn` resolve elsewhere
    /// and are not candidates. Neither is an associated function such as
    /// `Writer::spawn(..)` or `Self::spawn(..)`: rsactor's spawns are free
    /// functions (G7).
    fn spawn_fn(&self, call: &ExprCall) -> Option<(String, bool)> {
        let Expr::Path(path) = &*call.func else {
            return None;
        };
        let name = path.path.segments.last()?.ident.to_string();
        if !SPAWN_FNS.contains(&name.as_str()) {
            return None;
        }
        if self.assoc_owner(&path.path).is_some()
            || path
                .path
                .segments
                .first()
                .is_some_and(|s| s.ident == "Self")
        {
            return None;
        }
        match self.resolve_path(&path.path) {
            Some(r) if !r.inferred && r.res == Res::Rsactor(name.clone()) => Some((name, true)),
            Some(_) => None,
            None => Some((name, false)),
        }
    }

    /// The spawned type by turbofish, annotation, or a literal of a
    /// `#[derive(Actor)]` type that is no other actor's `type Args`.
    fn spawn_target(&self, call: &ExprCall, annotation: Option<&syn::Type>) -> Option<Ty> {
        let Expr::Path(path) = &*call.func else {
            return None;
        };
        let last = path.path.segments.last()?;
        if let PathArguments::AngleBracketed(angle) = &last.arguments {
            if let Some(GenericArgument::Type(ty)) = angle.args.first() {
                return Some(self.resolve_ty(ty));
            }
        }
        if let Some(target) = annotation.and_then(|a| actor_ref_target(self.param_ty(a))) {
            return Some(target);
        }
        let id = self.literal_struct(call.args.first()?)?;
        if !self.b.actors.get(&id).is_some_and(|a| a.derive) || !self.args_of(id).is_empty() {
            return None;
        }
        Some(Ty::Decl {
            id,
            args: Vec::new(),
            inferred: false,
        })
    }

    /// Actors other than `id` whose `type Args` is `id`.
    fn args_of(&self, id: DeclId) -> Vec<String> {
        let key = self.b.index.decl_path(id);
        self.b
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
            .collect()
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
        if let Some(target) = self.spawn_target(call, annotation) {
            self.spawn_edge(target, variant, location);
            return;
        }
        let ambiguous = call
            .args
            .first()
            .and_then(|arg| self.literal_struct(arg))
            .filter(|id| self.b.actors.get(id).is_some_and(|a| a.derive))
            .map(|id| (self.b.index.decl_path(id), self.args_of(id)));
        let detail = match ambiguous {
            Some((key, others)) if !others.is_empty() => format!(
                "`{name}({key} {{ .. }})`: `{key}` derives `Actor`, but is also `type Args` \
                 of {}",
                others.join(", ")
            ),
            _ => format!(
                "`{name}(..)` without a type argument; the spawned actor cannot be determined"
            ),
        };
        self.report(Category::UnresolvedSpawn, location, detail);
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
            EdgeData::structural(EdgeKind::Spawn, variant, resolution, location),
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
        let resolved = self.resolve_path(path)?;
        match resolved.res {
            Res::Decl(id)
                if !resolved.inferred && self.b.index.decls[id].kind == DeclKind::Struct =>
            {
                Some(id)
            }
            _ => None,
        }
    }

    // ----- token fallbacks -------------------------------------------------

    /// `ActorRef::tell(&r, m)` and friends (§4.4 UFCS): reported, not judged.
    fn ufcs(&mut self, call: &ExprCall) {
        let Expr::Path(func) = &*call.func else {
            return;
        };
        let segments: Vec<String> = func
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect();
        let [.., root, method] = segments.as_slice() else {
            return;
        };
        let is_send = send_kind(method).is_some() || HANDLER_SENDS.contains(&method.as_str());
        if !is_send || !UFCS_ROOTS.contains(&root.as_str()) {
            return;
        }
        let span = func.path.segments.last().map(|s| s.ident.span());
        let location = self.location(span.unwrap_or_else(proc_macro2::Span::call_site));
        self.report(
            Category::Ufcs,
            location,
            format!("`{root}::{method}(..)` is not judged in v1"),
        );
    }

    /// Macro arguments are opaque to the AST visitor (§4.4): find
    /// `.method(` sequences in the tokens, including nested groups, and
    /// report them with their location.
    fn macro_candidates(&mut self, mac: &Macro) {
        let name = mac
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default();
        let mut found = Vec::new();
        scan_tokens(mac.tokens.clone(), &mut found);
        for (method, span) in found {
            let location = self.location(span);
            self.report(
                Category::MacroCandidate,
                location,
                format!("`.{method}(..)` inside `{name}!`"),
            );
        }
    }
}

fn scan_tokens(tokens: TokenStream, found: &mut Vec<(String, proc_macro2::Span)>) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    for (i, tree) in trees.iter().enumerate() {
        match tree {
            TokenTree::Group(group) => scan_tokens(group.stream(), found),
            TokenTree::Punct(p) if p.as_char() == '.' => {
                let (Some(TokenTree::Ident(ident)), Some(TokenTree::Group(args))) =
                    (trees.get(i + 1), trees.get(i + 2))
                else {
                    continue;
                };
                let method = ident.to_string();
                let is_send =
                    send_kind(&method).is_some() || HANDLER_SENDS.contains(&method.as_str());
                if is_send && args.delimiter() == Delimiter::Parenthesis {
                    found.push((method, ident.span()));
                }
            }
            _ => {}
        }
    }
}

/// `(name, args)` of a std type, by its last path segment.
fn std_parts(ty: &Ty) -> Option<(&str, &[Ty])> {
    match ty {
        Ty::Std { path, args } => Some((path.rsplit("::").next().unwrap_or(path), args)),
        _ => None,
    }
}

/// Element type when iterating a value with `for` or `into_iter()`.
fn elem_of(ty: &Ty) -> Option<Ty> {
    let ty = ty.auto_deref();
    if let Ty::Iter(inner) = ty {
        return Some((**inner).clone());
    }
    if let Ty::Slice(inner) = ty {
        return Some((**inner).clone());
    }
    match std_parts(ty)? {
        ("Vec" | "VecDeque" | "HashSet" | "BTreeSet" | "Option", args) => args.first().cloned(),
        ("HashMap" | "BTreeMap", args) if args.len() == 2 => Some(Ty::Tuple(args.to_vec())),
        _ => None,
    }
}

fn option(inner: Ty) -> Ty {
    Ty::Std {
        path: "std::option::Option".to_string(),
        args: vec![inner],
    }
}

fn boxed_dyn(name: &str, args: &[Ty]) -> Ty {
    Ty::Std {
        path: "std::boxed::Box".to_string(),
        args: vec![Ty::Dyn(vec![Ty::Rsactor {
            name: name.to_string(),
            args: args.to_vec(),
        }])],
    }
}

/// The built-in return type table (§4.4.1).
fn method_return(receiver: &Ty, method: &str) -> Option<Ty> {
    match method {
        "clone" | "clone_boxed" | "to_owned" => return Some(receiver.clone()),
        "downgrade" => {
            if let Some(args) = receiver.rsactor("ActorRef") {
                return Some(Ty::Rsactor {
                    name: "ActorWeak".to_string(),
                    args: args.to_vec(),
                });
            }
            return match receiver.handler_trait() {
                Some(("TellHandler", args)) => Some(boxed_dyn("WeakTellHandler", args)),
                Some(("AskHandler", args)) => Some(boxed_dyn("WeakAskHandler", args)),
                _ => None,
            };
        }
        "upgrade" => {
            if let Some(args) = receiver.rsactor("ActorWeak") {
                return Some(option(Ty::Rsactor {
                    name: "ActorRef".to_string(),
                    args: args.to_vec(),
                }));
            }
            return match receiver.handler_trait() {
                Some(("WeakTellHandler", args)) => Some(option(boxed_dyn("TellHandler", args))),
                Some(("WeakAskHandler", args)) => Some(option(boxed_dyn("AskHandler", args))),
                _ => None,
            };
        }
        _ => {}
    }

    let target = receiver.auto_deref();
    if let Ty::Iter(elem) = target {
        let elem = (**elem).clone();
        return match method {
            "enumerate" => Some(Ty::Iter(Box::new(Ty::Tuple(vec![
                Ty::Std {
                    path: "usize".to_string(),
                    args: Vec::new(),
                },
                elem,
            ])))),
            "filter" | "skip" | "take" | "rev" | "cloned" | "copied" | "peekable"
            | "skip_while" | "take_while" | "step_by" | "into_iter" | "iter" => {
                Some(Ty::Iter(Box::new(elem)))
            }
            "next" | "last" | "nth" | "find" | "min" | "max" => Some(option(elem)),
            _ => None,
        };
    }
    let (name, args) = std_parts(target).or_else(|| match target {
        Ty::Slice(_) => Some(("slice", &[][..])),
        _ => None,
    })?;
    let first = || match target {
        Ty::Slice(inner) => Some((**inner).clone()),
        _ => args.first().cloned(),
    };
    match (name, method) {
        ("Option" | "Result", "unwrap" | "expect" | "unwrap_or_default") => first(),
        ("Option", "as_ref" | "as_mut" | "as_deref" | "cloned" | "copied") => Some(target.clone()),
        ("Option", "iter" | "into_iter") => first().map(|t| Ty::Iter(Box::new(t))),
        (
            "Vec" | "VecDeque" | "slice",
            "get" | "get_mut" | "first" | "last" | "first_mut" | "last_mut",
        ) => first().map(option),
        (
            "Vec" | "VecDeque" | "slice" | "HashSet" | "BTreeSet",
            "iter" | "iter_mut" | "into_iter" | "drain",
        ) => first().map(|t| Ty::Iter(Box::new(t))),
        ("HashMap" | "BTreeMap", "get" | "get_mut") => args.get(1).cloned().map(option),
        ("HashMap" | "BTreeMap", "values" | "values_mut" | "into_values") => {
            args.get(1).cloned().map(|t| Ty::Iter(Box::new(t)))
        }
        ("HashMap" | "BTreeMap", "keys" | "into_keys") => {
            args.first().cloned().map(|t| Ty::Iter(Box::new(t)))
        }
        ("HashMap" | "BTreeMap", "iter" | "iter_mut" | "into_iter" | "drain")
            if args.len() == 2 =>
        {
            Some(Ty::Iter(Box::new(Ty::Tuple(args.to_vec()))))
        }
        _ => None,
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
    /// A block-level `use` is visible in the whole block (G1).
    fn visit_block(&mut self, block: &'ast syn::Block) {
        let imports: Vec<Import> = block
            .stmts
            .iter()
            .filter_map(|stmt| match stmt {
                Stmt::Item(Item::Use(item_use)) => Some(imports_of(item_use)),
                _ => None,
            })
            .flatten()
            .collect();
        self.blocks.push(imports);
        self.scopes.push(Scope::new());
        visit::visit_block(self, block);
        self.scopes.pop();
        self.blocks.pop();
    }

    /// The initializer is walked and typed before the pattern binds, so
    /// `let w = w.clone();` sees the outer `w`.
    fn visit_local(&mut self, local: &'ast Local) {
        let annotation = match &local.pat {
            Pat::Type(typed) => Some(&*typed.ty),
            _ => None,
        };
        match &local.init {
            Some(init) => {
                if let (Some(annotation), Expr::Call(call)) = (annotation, strip(&init.expr)) {
                    if let Some((name, resolved)) = self.spawn_fn(call) {
                        self.spawn(call, &name, resolved, Some(annotation));
                    }
                }
                self.visit_expr(&init.expr);
                let ty = self
                    .type_of(&init.expr)
                    .or_else(|| self.branch_type(&init.expr).flatten());
                if let Some((_, diverge)) = &init.diverge {
                    self.visit_expr(diverge);
                }
                self.bind_pat(&local.pat, ty);
            }
            None => self.bind_pat(&local.pat, None),
        }
    }

    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if let Some((name, resolved)) = self.spawn_fn(call) {
            self.spawn(call, &name, resolved, None);
        }
        self.ufcs(call);
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, mc: &'ast ExprMethodCall) {
        let method = mc.method.to_string();
        if ADAPTERS.contains(&method.as_str()) {
            if let Some(Expr::Closure(closure)) = mc.args.first() {
                let elem = self.type_of(&mc.receiver).and_then(|t| elem_of(&t));
                self.closure_hint = Some((closure as *const ExprClosure as usize, elem));
            }
        }
        let ask = send_kind(&method);
        if let Some(ask) = ask {
            self.send(mc, &method, ask);
        }
        visit::visit_expr_method_call(self, mc);
    }

    fn visit_expr_closure(&mut self, closure: &'ast ExprClosure) {
        let address = closure as *const ExprClosure as usize;
        let hint = match self.closure_hint.take() {
            Some((at, elem)) if at == address => elem,
            other => {
                self.closure_hint = other;
                None
            }
        };
        self.scopes.push(Scope::new());
        for (i, input) in closure.inputs.iter().enumerate() {
            let ty = if i == 0 { hint.clone() } else { None };
            self.bind_pat(input, ty);
        }
        self.visit_expr(&closure.body);
        self.scopes.pop();
    }

    fn visit_expr_for_loop(&mut self, for_loop: &'ast ExprForLoop) {
        self.visit_expr(&for_loop.expr);
        let elem = self.type_of(&for_loop.expr).and_then(|t| elem_of(&t));
        self.scopes.push(Scope::new());
        self.bind_pat(&for_loop.pat, elem);
        self.visit_block(&for_loop.body);
        self.scopes.pop();
    }

    /// `if let` bindings live in the then-branch only.
    fn visit_expr_if(&mut self, expr_if: &'ast ExprIf) {
        self.scopes.push(Scope::new());
        self.visit_expr(&expr_if.cond);
        self.visit_block(&expr_if.then_branch);
        self.scopes.pop();
        if let Some((_, else_branch)) = &expr_if.else_branch {
            self.visit_expr(else_branch);
        }
    }

    fn visit_expr_while(&mut self, expr_while: &'ast ExprWhile) {
        self.scopes.push(Scope::new());
        self.visit_expr(&expr_while.cond);
        self.visit_block(&expr_while.body);
        self.scopes.pop();
    }

    fn visit_expr_let(&mut self, expr_let: &'ast ExprLet) {
        self.visit_expr(&expr_let.expr);
        let ty = self.type_of(&expr_let.expr);
        self.bind_pat(&expr_let.pat, ty);
    }

    fn visit_expr_match(&mut self, expr_match: &'ast ExprMatch) {
        self.visit_expr(&expr_match.expr);
        let ty = self.type_of(&expr_match.expr);
        for arm in &expr_match.arms {
            self.scopes.push(Scope::new());
            // syn 3 carries a match guard inside the pattern (`Pat::Guard`).
            let (pat, guard) = match &arm.pat {
                Pat::Guard(g) => (&*g.pat, Some(&*g.guard)),
                other => (other, None),
            };
            self.bind_pat(pat, ty.clone());
            if let Some(guard) = guard {
                self.visit_expr(guard);
            }
            self.visit_expr(&arm.body);
            self.scopes.pop();
        }
    }

    /// Expression macros are re-parsed and walked as code (G6); any other
    /// macro, or one whose tokens do not parse, gets the token scan.
    fn visit_macro(&mut self, mac: &'ast Macro) {
        let name = macro_name(mac);
        if name == "matches" {
            if let Some(expr) = first_macro_expr(&mac.tokens) {
                self.visit_expr(&expr);
                return;
            }
        } else if EXPR_MACROS.contains(&name.as_str()) {
            let parsed = Punctuated::<Expr, Token![,]>::parse_terminated.parse2(mac.tokens.clone());
            if let Ok(exprs) = parsed {
                for expr in &exprs {
                    self.visit_expr(expr);
                }
                return;
            }
        }
        self.macro_candidates(mac);
    }

    /// A nested `fn` is its own sender, not part of the enclosing method,
    /// and cannot see the enclosing bindings. It does see block-level `use`
    /// declarations, which are items.
    fn visit_item_fn(&mut self, f: &'ast ItemFn) {
        let path = format!("{}::{}", self.fn_path, f.sig.ident);
        let sender = fn_endpoint(self.b, self.place.krate, &path, None);
        let generics = type_params(&f.sig.generics);
        let blocks = self.blocks.clone();
        let mut nested = Walker::new(self.b, self.place, sender, path, generics, None);
        nested.blocks = blocks;
        nested.walk_fn(&f.sig, &f.block);
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
