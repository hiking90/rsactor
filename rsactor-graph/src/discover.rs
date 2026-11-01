// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Graph assembly: actors and handled messages (§4.1, §4.2), holds edges and
//! type-erased handler nodes (§4.3). Call sites are scanned by [`crate::calls`].

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use quote::ToTokens;
use syn::punctuated::Punctuated;
use syn::{Attribute, FnArg, ImplItem, Item, ItemImpl, Meta, ReturnType, Token};

use crate::crate_tree::{compact_tokens, location_of, ModId};
use crate::model::{
    ActorNode, Category, Edge, EdgeKind, ExternalActorNode, ExternalNode, FnNode, Graph,
    HandledMessage, HandlersNode, Location, Node, Report, Resolution, UnknownNode,
};
use crate::resolve::{type_params, DeclId, Index, Res};
use crate::types::{Ty, TyCtx};

#[derive(Default)]
pub struct ActorInfo {
    pub open: bool,
    /// `#[derive(Actor)]`, which sets `type Args = Self`.
    pub derive: bool,
    /// `type Args` of `impl Actor for X`, with its location.
    pub args: Option<(Ty, Location)>,
    pub messages: Vec<HandledMessage>,
}

/// A node an edge can point at.
#[derive(Debug, Clone)]
pub enum Endpoint {
    Actor(DeclId),
    ExternalActor(String),
    Fn {
        target: String,
        path: String,
    },
    External,
    Handlers(HandlerSig),
    /// The `?` receiver of one sender, identified by the sender's node id.
    Unknown(String),
}

/// `TellHandler<M>` or `AskHandler<M, R>`.
#[derive(Debug, Clone)]
pub struct HandlerSig {
    pub ask: bool,
    pub message: Ty,
    pub reply: Option<Ty>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct EdgeKey {
    kind: EdgeKind,
    variant: Option<String>,
    from: String,
    to: String,
    message: Option<String>,
    message_label: Option<String>,
    reply: Option<String>,
    receiver_resolution: Resolution,
    message_resolution: Resolution,
}

/// A function or method declared in the declaration scope. Its signature
/// states the return type, which needs no inference to read (G4).
pub struct FnDecl<'a> {
    pub krate: usize,
    pub module: ModId,
    /// Generic parameters of the enclosing impl.
    pub impl_generics: Vec<String>,
    /// Arguments of the impl's self type, e.g. `[T]` for `impl<T> W<T>`.
    pub impl_self_args: Vec<Ty>,
    pub sig: &'a syn::Signature,
}

/// Collects nodes and edges while the scan runs.
pub struct Builder<'i, 'a> {
    pub index: &'i Index<'a>,
    pub base: &'i Path,
    pub report: &'i mut Report,
    pub actors: BTreeMap<DeclId, ActorInfo>,
    /// Types with an `impl Deref`/`DerefMut`: a method call on them may reach
    /// an `ActorRef` inside, so a send on them is never dropped (§4.4).
    pub deref_impls: HashSet<DeclId>,
    /// Methods and associated functions by (type, name). Several entries
    /// mean several impls (inherent and trait) declare the name.
    pub methods: HashMap<(DeclId, String), Vec<FnDecl<'a>>>,
    /// Free functions by full path.
    pub functions: HashMap<String, FnDecl<'a>>,
    extra: BTreeMap<String, Node>,
    handlers: BTreeMap<String, (HandlerSig, String)>,
    edges: BTreeMap<EdgeKey, BTreeSet<Location>>,
}

/// Per-edge data beyond its endpoints.
pub struct EdgeData {
    pub kind: EdgeKind,
    pub variant: Option<String>,
    pub message: Option<String>,
    pub message_label: Option<String>,
    pub reply: Option<String>,
    pub receiver_resolution: Resolution,
    pub message_resolution: Resolution,
    pub location: Location,
}

impl EdgeData {
    /// Holds and spawn edges carry no message.
    pub fn structural(
        kind: EdgeKind,
        variant: Option<String>,
        receiver: Resolution,
        location: Location,
    ) -> Self {
        EdgeData {
            kind,
            variant,
            message: None,
            message_label: None,
            reply: None,
            receiver_resolution: receiver,
            message_resolution: Resolution::Exact,
            location,
        }
    }
}

impl<'i, 'a> Builder<'i, 'a> {
    /// Node id for an endpoint, registering the node on first use. Sender
    /// nodes (`fn`, `external`) exist only when an edge starts at them.
    pub fn endpoint_id(&mut self, endpoint: &Endpoint) -> String {
        match endpoint {
            Endpoint::Actor(id) => {
                // `ActorRef<Y>`/`spawn::<Y>` proves `Y: Actor` even when the
                // scanner did not see its `impl Actor`.
                self.actors.entry(*id).or_default();
                actor_id(self.index, *id)
            }
            Endpoint::ExternalActor(path) => {
                let id = format!("external_actor:{path}");
                self.extra.entry(id.clone()).or_insert_with(|| {
                    Node::ExternalActor(ExternalActorNode {
                        id: id.clone(),
                        path: path.clone(),
                    })
                });
                id
            }
            Endpoint::Fn { target, path } => {
                let id = format!("fn:{target}:{path}");
                self.extra.entry(id.clone()).or_insert_with(|| {
                    Node::Fn(FnNode {
                        id: id.clone(),
                        target: target.clone(),
                        path: path.clone(),
                    })
                });
                id
            }
            Endpoint::External => {
                let id = "external".to_string();
                self.extra
                    .entry(id.clone())
                    .or_insert_with(|| Node::External(ExternalNode { id: id.clone() }));
                id
            }
            Endpoint::Handlers(sig) => {
                let message = sig.message.key(self.index);
                let reply = sig.reply.as_ref().map(|r| r.key(self.index));
                let handler = if sig.ask { "AskHandler" } else { "TellHandler" };
                let id = match &reply {
                    Some(reply) => format!("handlers:{handler}:{message}:{reply}"),
                    None => format!("handlers:{handler}:{message}"),
                };
                self.handlers
                    .entry(id.clone())
                    .or_insert_with(|| (sig.clone(), handler.to_string()));
                id
            }
            Endpoint::Unknown(sender) => {
                let id = format!("unknown:{sender}");
                self.extra.entry(id.clone()).or_insert_with(|| {
                    Node::Unknown(UnknownNode {
                        id: id.clone(),
                        sender: sender.clone(),
                    })
                });
                id
            }
        }
    }

    /// A short name for report entries.
    pub fn endpoint_display(&self, endpoint: &Endpoint) -> String {
        match endpoint {
            Endpoint::Actor(id) => self.index.decl_path(*id),
            Endpoint::ExternalActor(path) => path.clone(),
            Endpoint::Fn { path, .. } => format!("fn {path}"),
            Endpoint::External => "external".to_string(),
            Endpoint::Handlers(sig) => sig.message.label(self.index),
            Endpoint::Unknown(_) => "?".to_string(),
        }
    }

    pub fn add_edge(&mut self, from: &Endpoint, to: &Endpoint, data: EdgeData) {
        let key = EdgeKey {
            kind: data.kind,
            variant: data.variant,
            from: self.endpoint_id(from),
            to: self.endpoint_id(to),
            message: data.message,
            message_label: data.message_label,
            reply: data.reply,
            receiver_resolution: data.receiver_resolution,
            message_resolution: data.message_resolution,
        };
        self.edges.entry(key).or_default().insert(data.location);
    }
}

pub fn actor_id(index: &Index, id: DeclId) -> String {
    let decl = &index.decls[id];
    format!(
        "actor:{}:{}",
        index.crates[decl.krate].spec.target,
        index.decl_path(id)
    )
}

/// Builds the graph for the loaded crates.
pub fn discover(index: &Index, base: &Path, report: &mut Report) -> Graph {
    let mut builder = Builder {
        index,
        base,
        report,
        actors: BTreeMap::new(),
        deref_impls: HashSet::new(),
        methods: HashMap::new(),
        functions: HashMap::new(),
        extra: BTreeMap::new(),
        handlers: BTreeMap::new(),
        edges: BTreeMap::new(),
    };
    collect_actors(&mut builder);
    holds_edges(&mut builder);
    crate::calls::scan_bodies(&mut builder);
    assemble(builder)
}

fn collect_actors(b: &mut Builder<'_, '_>) {
    let index = b.index;
    for (c, krate) in index.crates.iter().enumerate() {
        for (m, module) in krate.modules.iter().enumerate() {
            for item in &module.items {
                let derived = match item {
                    Item::Struct(s) if derives_actor(index, c, m, &s.attrs) => Some(&s.ident),
                    Item::Enum(e) if derives_actor(index, c, m, &e.attrs) => Some(&e.ident),
                    _ => None,
                };
                if let Some(ident) = derived {
                    if let Some(id) = index.local_decl(c, m, &ident.to_string()) {
                        b.actors.entry(id).or_default().derive = true;
                    }
                }
                if let Item::Impl(imp) = item {
                    record_deref(b, c, m, imp);
                    record_methods(b, c, m, imp);
                    scan_impl(b, c, m, &module.file, imp);
                }
                if let Item::Fn(f) = item {
                    let mut path = krate.module_path(m);
                    path.push(f.sig.ident.to_string());
                    b.functions.insert(
                        path.join("::"),
                        FnDecl {
                            krate: c,
                            module: m,
                            impl_generics: Vec::new(),
                            impl_self_args: Vec::new(),
                            sig: &f.sig,
                        },
                    );
                }
            }
        }
    }
}

fn record_methods<'a>(b: &mut Builder<'_, 'a>, krate: usize, module: ModId, imp: &'a ItemImpl) {
    let generics = type_params(&imp.generics);
    let ty = TyCtx {
        index: b.index,
        krate,
        module,
        generics: &generics,
        self_ty: None,
        blocks: &[],
    }
    .resolve(&imp.self_ty);
    let Ty::Decl { id, args, .. } = ty else {
        return;
    };
    for item in &imp.items {
        let ImplItem::Fn(f) = item else { continue };
        b.methods
            .entry((id, f.sig.ident.to_string()))
            .or_default()
            .push(FnDecl {
                krate,
                module,
                impl_generics: generics.clone(),
                impl_self_args: args.clone(),
                sig: &f.sig,
            });
    }
}

fn record_deref(b: &mut Builder, krate: usize, module: ModId, imp: &ItemImpl) {
    let is_deref = imp
        .trait_
        .as_ref()
        .and_then(|(path, _)| path.segments.last())
        .is_some_and(|s| s.ident == "Deref" || s.ident == "DerefMut");
    if !is_deref {
        return;
    }
    let generics = type_params(&imp.generics);
    let ty = TyCtx {
        index: b.index,
        krate,
        module,
        generics: &generics,
        self_ty: None,
        blocks: &[],
    }
    .resolve(&imp.self_ty);
    if let Ty::Decl { id, .. } = ty {
        b.deref_impls.insert(id);
    }
}

fn scan_impl(b: &mut Builder, krate: usize, module: ModId, file: &Path, imp: &ItemImpl) {
    let index = b.index;
    let base = b.base;
    let Some(role) = impl_role(index, krate, module, imp) else {
        return;
    };

    let impl_generics = type_params(&imp.generics);
    let plain = TyCtx {
        index,
        krate,
        module,
        generics: &impl_generics,
        self_ty: None,
        blocks: &[],
    };
    let self_ty = plain.resolve(&imp.self_ty);
    let Ty::Decl { id, .. } = self_ty else {
        b.report.push(
            Category::Declaration,
            Some(location_of(file, imp.impl_token.span, base)),
            format!(
                "cannot resolve the type of this impl: {}",
                compact_tokens(&imp.self_ty.to_token_stream().to_string())
            ),
        );
        return;
    };
    let cx = TyCtx {
        self_ty: Some(&self_ty),
        ..plain
    };
    let info = b.actors.entry(id).or_default();

    match role {
        ImplRole::Actor => {
            info.args = imp.items.iter().find_map(|item| match item {
                ImplItem::Type(t) if t.ident == "Args" => {
                    Some((cx.resolve(&t.ty), location_of(file, t.ident.span(), base)))
                }
                _ => None,
            });
        }
        ImplRole::Message => {
            let Some(message) = imp
                .trait_
                .as_ref()
                .and_then(|(path, _)| path.segments.last())
                .and_then(|segment| single_type_arg(&segment.arguments))
            else {
                return;
            };
            let message_ty = cx.resolve(message);
            if is_impl_generic(&message_ty, &impl_generics) {
                info.open = true;
                return;
            }
            let reply = imp.items.iter().find_map(|item| match item {
                ImplItem::Type(t) if t.ident == "Reply" => Some(cx.resolve(&t.ty)),
                _ => None,
            });
            info.messages.push(handled(
                index,
                &message_ty,
                reply.as_ref(),
                cfg_predicates(&imp.attrs),
                location_of(file, imp.impl_token.span, base),
            ));
        }
        ImplRole::MessageHandlers => {
            for item in &imp.items {
                let ImplItem::Fn(f) = item else { continue };
                // The derive macro matches `is_ident("handler")` exactly
                // (rsactor-derive/src/lib.rs:456): `#[rsactor::handler]` is not
                // a handler there, so it is not one here either.
                if !f.attrs.iter().any(|a| a.path().is_ident("handler")) {
                    continue;
                }
                let location = location_of(file, f.sig.ident.span(), base);
                if f.sig.inputs.len() != 3 {
                    b.report.push(
                        Category::Declaration,
                        Some(location),
                        format!(
                            "handler `{}` has {} parameters; the derive macro requires 3",
                            f.sig.ident,
                            f.sig.inputs.len()
                        ),
                    );
                    continue;
                }
                let FnArg::Typed(message) = &f.sig.inputs[1] else {
                    continue;
                };
                let message_ty = cx.resolve(&message.ty);
                let info = b.actors.entry(id).or_default();
                if is_impl_generic(&message_ty, &impl_generics) {
                    info.open = true;
                    continue;
                }
                let reply = match &f.sig.output {
                    ReturnType::Default => None,
                    ReturnType::Type(_, ty) => Some(cx.resolve(ty)),
                };
                info.messages.push(handled(
                    index,
                    &message_ty,
                    reply.as_ref(),
                    cfg_predicates(&f.attrs),
                    location,
                ));
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImplRole {
    Actor,
    Message,
    MessageHandlers,
}

/// Which rsactor item an impl is about. A trait or attribute merely named
/// `Actor`/`Message` from another crate must not turn a type into an actor.
pub fn impl_role(index: &Index, krate: usize, module: ModId, imp: &ItemImpl) -> Option<ImplRole> {
    match &imp.trait_ {
        Some((path, _)) => {
            let last = path.segments.last()?.ident.to_string();
            let role = match last.as_str() {
                "Actor" => ImplRole::Actor,
                "Message" => ImplRole::Message,
                _ => return None,
            };
            is_rsactor(index, krate, module, path).then_some(role)
        }
        None => imp
            .attrs
            .iter()
            .find(|attr| {
                attr.path()
                    .segments
                    .last()
                    .is_some_and(|s| s.ident == "message_handlers")
            })
            .filter(|attr| is_rsactor(index, krate, module, attr.path()))
            .map(|_| ImplRole::MessageHandlers),
    }
}

/// Holds edges from actor fields and `type Args` (§4.3).
fn holds_edges(b: &mut Builder) {
    let index = b.index;
    let ids: Vec<DeclId> = b.actors.keys().copied().collect();
    for id in ids {
        let decl = &index.decls[id];
        let file = index.crates[decl.krate].modules[decl.module].file.clone();
        let self_ty = Ty::Decl {
            id,
            args: decl.generics.iter().cloned().map(Ty::Generic).collect(),
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
        let mut types: Vec<(Ty, Location)> = Vec::new();
        let mut push_fields = |fields: &syn::Fields| {
            for field in fields {
                let span = field
                    .ident
                    .as_ref()
                    .map(|i| i.span())
                    .unwrap_or_else(|| syn::spanned::Spanned::span(&field.ty));
                types.push((cx.resolve(&field.ty), location_of(&file, span, b.base)));
            }
        };
        match index.decl_item(id) {
            Item::Struct(s) => push_fields(&s.fields),
            Item::Enum(e) => e.variants.iter().for_each(|v| push_fields(&v.fields)),
            _ => {}
        }
        if let Some((args, location)) = b.actors.get(&id).and_then(|i| i.args.clone()) {
            types.push((args, location));
        }

        let from = Endpoint::Actor(id);
        for (ty, location) in types {
            let mut found = Vec::new();
            held_refs(index, &ty, &mut found);
            for held in found {
                match held {
                    Held::Endpoint(to, resolution) => b.add_edge(
                        &from,
                        &to,
                        EdgeData::structural(EdgeKind::Holds, None, resolution, location.clone()),
                    ),
                    Held::Unsupported(what) => b.report.push(
                        Category::Declaration,
                        Some(location.clone()),
                        format!("holds {what}; the referenced actor cannot be determined"),
                    ),
                }
            }
        }
    }
}

enum Held {
    Endpoint(Endpoint, Resolution),
    /// `ActorRef<T>` with a generic or unresolved `T`.
    Unsupported(String),
}

/// Actor references and type-erased handlers anywhere inside a type.
fn held_refs(index: &Index, ty: &Ty, out: &mut Vec<Held>) {
    let recurse = |items: &[Ty], out: &mut Vec<Held>| {
        items.iter().for_each(|a| held_refs(index, a, out));
    };
    match ty {
        Ty::Rsactor { name, args } => match name.as_str() {
            "ActorRef" | "ActorWeak" => match args.first() {
                Some(target @ Ty::Decl { id, .. }) => {
                    out.push(Held::Endpoint(Endpoint::Actor(*id), target.resolution()))
                }
                Some(Ty::External { path, .. }) => out.push(Held::Endpoint(
                    Endpoint::ExternalActor(path.clone()),
                    Resolution::Exact,
                )),
                Some(other) => {
                    out.push(Held::Unsupported(format!("{name}<{}>", other.label(index))))
                }
                None => {}
            },
            "TellHandler" | "WeakTellHandler" | "AskHandler" | "WeakAskHandler" => {
                if let Some(sig) = handler_sig(name, args) {
                    let resolution = sig.message.resolution();
                    out.push(Held::Endpoint(Endpoint::Handlers(sig), resolution));
                }
            }
            _ => recurse(args, out),
        },
        Ty::Decl { args, .. } | Ty::Std { args, .. } | Ty::External { args, .. } => {
            recurse(args, out)
        }
        Ty::Ref(inner) | Ty::Slice(inner) | Ty::Iter(inner) => held_refs(index, inner, out),
        Ty::Tuple(items) | Ty::Dyn(items) | Ty::ImplTrait(items) => recurse(items, out),
        Ty::Generic(_) | Ty::Unknown(_) => {}
    }
}

pub fn handler_sig(name: &str, args: &[Ty]) -> Option<HandlerSig> {
    let ask = name.contains("Ask");
    let message = args.first()?.clone();
    let reply = if ask { args.get(1).cloned() } else { None };
    Some(HandlerSig {
        ask,
        message,
        reply,
    })
}

fn assemble(b: Builder) -> Graph {
    let index = b.index;
    let mut graph = Graph::default();
    let may_be_outside = b.actors.values().any(|a| a.open)
        || b.extra
            .values()
            .any(|n| matches!(n, Node::ExternalActor(_)));

    let mut actor_nodes = Vec::new();
    for (id, info) in &b.actors {
        let decl = &index.decls[*id];
        let krate = &index.crates[decl.krate];
        let file = &krate.modules[decl.module].file;
        let path = index.decl_path(*id);
        let display = if decl.generics.is_empty() {
            path.clone()
        } else {
            format!("{path}<{}>", decl.generics.join(", "))
        };
        let mut messages = info.messages.clone();
        messages.sort();
        messages.dedup();
        actor_nodes.push(ActorNode {
            id: actor_id(index, *id),
            target: krate.spec.target.clone(),
            path,
            display,
            open: info.open,
            messages,
            location: location_of(file, decl_ident_span(index, *id), b.base),
        });
    }

    for (id, (sig, handler)) in &b.handlers {
        let message = sig.message.key(index);
        // `AskHandler<M, ()>` matches a handler whose reply is `()`, which
        // `HandledMessage` records as `None`.
        let reply = sig
            .reply
            .as_ref()
            .map(|r| r.key(index))
            .filter(|r| r != "()");
        let candidates = actor_nodes
            .iter()
            .filter(|a| {
                a.messages
                    .iter()
                    .any(|m| m.key == message && (!sig.ask || m.reply_key == reply))
            })
            .map(|a| a.id.clone())
            .collect();
        graph.nodes.push(Node::Handlers(HandlersNode {
            id: id.clone(),
            handler: handler.clone(),
            message,
            message_label: sig.message.label(index),
            reply: sig.reply.as_ref().map(|r| r.key(index)),
            reply_label: sig.reply.as_ref().map(|r| r.label(index)),
            candidates,
            may_be_outside,
        }));
    }

    graph.nodes.extend(actor_nodes.into_iter().map(Node::Actor));
    graph.nodes.extend(b.extra.into_values());
    for (key, locations) in b.edges {
        graph.edges.push(Edge {
            kind: key.kind,
            variant: key.variant,
            from: key.from,
            to: key.to,
            message: key.message,
            message_label: key.message_label,
            reply: key.reply,
            receiver_resolution: key.receiver_resolution,
            message_resolution: key.message_resolution,
            locations: locations.into_iter().collect(),
        });
    }
    graph
}

fn handled(
    index: &Index,
    message: &Ty,
    reply: Option<&Ty>,
    cfg: Vec<String>,
    location: Location,
) -> HandledMessage {
    let reply = reply.filter(|r| !matches!(r, Ty::Tuple(items) if items.is_empty()));
    HandledMessage {
        key: message.key(index),
        label: message.label(index),
        reply: reply.map(|r| r.label(index)),
        reply_key: reply.map(|r| r.key(index)),
        resolution: message.resolution(),
        cfg,
        location,
    }
}

/// The message type is the impl's own type parameter, so the handled set has
/// no bound (§4.2 `open`).
fn is_impl_generic(ty: &Ty, impl_generics: &[String]) -> bool {
    matches!(ty, Ty::Generic(name) if impl_generics.contains(name))
}

fn single_type_arg(arguments: &syn::PathArguments) -> Option<&syn::Type> {
    let syn::PathArguments::AngleBracketed(angle) = arguments else {
        return None;
    };
    let mut types = angle.args.iter().filter_map(|arg| match arg {
        syn::GenericArgument::Type(ty) => Some(ty),
        _ => None,
    });
    let first = types.next()?;
    types.next().is_none().then_some(first)
}

fn derives_actor(index: &Index, krate: usize, module: ModId, attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("derive")
            && attr
                .parse_args_with(Punctuated::<syn::Path, Token![,]>::parse_terminated)
                .is_ok_and(|paths| {
                    paths.iter().any(|p| {
                        p.segments.last().is_some_and(|s| s.ident == "Actor")
                            && is_rsactor(index, krate, module, p)
                    })
                })
    })
}

/// The path names an rsactor item, or cannot be resolved at all. An
/// unresolvable name is accepted because rsactor names usually arrive through
/// `use rsactor::..`, which resolves; refusing unresolved ones would drop
/// actors behind an import form the resolver does not follow. A name that
/// resolves to anything else (a local trait, another crate) is refused.
pub fn is_rsactor(index: &Index, krate: usize, module: ModId, path: &syn::Path) -> bool {
    match index.resolve_path(krate, module, path) {
        Some(resolved) => matches!(resolved.res, Res::Rsactor(_)) && !resolved.inferred,
        None => true,
    }
}

fn cfg_predicates(attrs: &[Attribute]) -> Vec<String> {
    attrs
        .iter()
        .filter(|attr| attr.path().is_ident("cfg"))
        .filter_map(|attr| match &attr.meta {
            Meta::List(list) => Some(format!("cfg({})", compact_tokens(&list.tokens.to_string()))),
            _ => None,
        })
        .collect()
}

fn decl_ident_span(index: &Index, id: DeclId) -> proc_macro2::Span {
    match index.decl_item(id) {
        Item::Struct(i) => i.ident.span(),
        Item::Enum(i) => i.ident.span(),
        Item::Union(i) => i.ident.span(),
        Item::Trait(i) => i.ident.span(),
        Item::Type(i) => i.ident.span(),
        _ => proc_macro2::Span::call_site(),
    }
}
