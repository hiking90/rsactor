// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Actor and handled-message discovery (§4.1, §4.2).

use std::collections::BTreeMap;
use std::path::Path;

use quote::ToTokens;
use syn::punctuated::Punctuated;
use syn::{Attribute, FnArg, ImplItem, Item, ItemImpl, Meta, ReturnType, Token};

use crate::crate_tree::{compact_tokens, location_of, ModId};
use crate::model::{ActorNode, Category, Graph, HandledMessage, Location, Node, Report};
use crate::resolve::{type_params, DeclId, Index, Res};
use crate::types::{Ty, TyCtx};

#[derive(Default)]
struct ActorInfo {
    open: bool,
    messages: Vec<HandledMessage>,
}

/// Builds the stage-1 graph: one node per actor type in the declaration
/// scope, with the messages it handles.
pub fn discover(index: &Index, base: &Path, report: &mut Report) -> Graph {
    let mut actors: BTreeMap<DeclId, ActorInfo> = BTreeMap::new();
    for (c, krate) in index.crates.iter().enumerate() {
        for (m, module) in krate.modules.iter().enumerate() {
            for item in &module.items {
                match item {
                    Item::Struct(s) if derives_actor(index, c, m, &s.attrs) => {
                        if let Some(id) = index.local_decl(c, m, &s.ident.to_string()) {
                            actors.entry(id).or_default();
                        }
                    }
                    Item::Enum(e) if derives_actor(index, c, m, &e.attrs) => {
                        if let Some(id) = index.local_decl(c, m, &e.ident.to_string()) {
                            actors.entry(id).or_default();
                        }
                    }
                    Item::Impl(imp) => {
                        let file = &module.file;
                        scan_impl(index, c, m, file, base, imp, &mut actors, report);
                    }
                    _ => {}
                }
            }
        }
    }

    let mut graph = Graph::default();
    for (id, info) in actors {
        let decl = &index.decls[id];
        let krate = &index.crates[decl.krate];
        let file = &krate.modules[decl.module].file;
        let path = index.decl_path(id);
        let display = if decl.generics.is_empty() {
            path.clone()
        } else {
            format!("{path}<{}>", decl.generics.join(", "))
        };
        let mut messages = info.messages;
        messages.sort();
        messages.dedup();
        graph.nodes.push(Node::Actor(ActorNode {
            target: krate.spec.target.clone(),
            path,
            display,
            open: info.open,
            messages,
            location: location_of(file, decl_ident_span(index, id), base),
        }));
    }
    graph
}

#[allow(clippy::too_many_arguments)]
fn scan_impl(
    index: &Index,
    krate: usize,
    module: ModId,
    file: &Path,
    base: &Path,
    imp: &ItemImpl,
    actors: &mut BTreeMap<DeclId, ActorInfo>,
    report: &mut Report,
) {
    // Which rsactor item this impl is about. A trait or attribute merely named
    // `Actor`/`Message` from another crate must not turn a type into an actor.
    let role = match &imp.trait_ {
        Some((path, _)) => match path.segments.last().map(|s| s.ident.to_string()).as_deref() {
            Some(name @ ("Actor" | "Message")) if is_rsactor(index, krate, module, path) => {
                Some(name.to_string())
            }
            _ => None,
        },
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
            .map(|_| "message_handlers".to_string()),
    };
    let Some(role) = role else {
        return;
    };

    let impl_generics = type_params(&imp.generics);
    let plain = TyCtx {
        index,
        krate,
        module,
        generics: &impl_generics,
        self_ty: None,
    };
    let self_ty = plain.resolve(&imp.self_ty);
    let Ty::Decl { id, .. } = self_ty else {
        report.push(
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
    let info = actors.entry(id).or_default();

    match role.as_str() {
        "Actor" => {}
        "Message" => {
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
        _ => {
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
                    report.push(
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
fn is_rsactor(index: &Index, krate: usize, module: ModId, path: &syn::Path) -> bool {
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
