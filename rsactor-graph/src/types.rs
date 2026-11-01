// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Resolved type representation and the message key rules.

use quote::ToTokens;
use syn::{GenericArgument, Item, PathArguments, Type, TypeParamBound};

use crate::crate_tree::{compact_tokens, ModId};
use crate::model::Resolution;
use crate::resolve::{DeclId, DeclKind, Import, Index, Res};

/// A `syn::Type` with every path resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    Decl {
        id: DeclId,
        args: Vec<Ty>,
        inferred: bool,
    },
    Rsactor {
        name: String,
        args: Vec<Ty>,
    },
    Std {
        path: String,
        args: Vec<Ty>,
    },
    External {
        path: String,
        args: Vec<Ty>,
    },
    /// A generic parameter in scope (impl or fn generics).
    Generic(String),
    Ref(Box<Ty>),
    Tuple(Vec<Ty>),
    Slice(Box<Ty>),
    Dyn(Vec<Ty>),
    ImplTrait(Vec<Ty>),
    /// An iterator yielding the inner type. Never written in source; the
    /// type environment produces it for `iter()`, `values()` and friends.
    Iter(Box<Ty>),
    /// Could not be resolved; holds the type as written (compacted).
    Unknown(String),
}

/// Where a type is written.
pub struct TyCtx<'i, 'a> {
    pub index: &'i Index<'a>,
    pub krate: usize,
    pub module: ModId,
    /// Generic type parameters in scope.
    pub generics: &'i [String],
    /// What `Self` stands for, if anything.
    pub self_ty: Option<&'i Ty>,
    /// `use` declarations of the enclosing function body's blocks,
    /// innermost last. Empty outside function bodies.
    pub blocks: &'i [Vec<Import>],
}

impl TyCtx<'_, '_> {
    pub fn resolve(&self, ty: &Type) -> Ty {
        match ty {
            Type::Path(tp) if tp.qself.is_none() => self.resolve_type_path(&tp.path, ty),
            Type::Reference(r) => Ty::Ref(Box::new(self.resolve(&r.elem))),
            Type::Tuple(t) => Ty::Tuple(t.elems.iter().map(|e| self.resolve(e)).collect()),
            Type::Slice(s) => Ty::Slice(Box::new(self.resolve(&s.elem))),
            Type::Array(a) => Ty::Slice(Box::new(self.resolve(&a.elem))),
            Type::Paren(p) => self.resolve(&p.elem),
            Type::Group(g) => self.resolve(&g.elem),
            Type::TraitObject(t) => Ty::Dyn(self.bounds(t.bounds.iter())),
            Type::ImplTrait(t) => Ty::ImplTrait(self.bounds(t.bounds.iter())),
            other => unknown(other),
        }
    }

    fn resolve_type_path(&self, path: &syn::Path, whole: &Type) -> Ty {
        if path.leading_colon.is_none() && path.segments.len() == 1 {
            let ident = path.segments[0].ident.to_string();
            if ident == "Self" {
                if let Some(self_ty) = self.self_ty {
                    return self_ty.clone();
                }
            }
            if self.generics.contains(&ident) {
                return Ty::Generic(ident);
            }
        }
        let args = path
            .segments
            .last()
            .map(|last| self.generic_args(&last.arguments))
            .unwrap_or_default();
        match self
            .index
            .resolve_path_scoped(self.krate, self.module, path, self.blocks)
        {
            Some(resolved) => match resolved.res {
                Res::Decl(id) if self.index.decls[id].kind == DeclKind::TypeAlias => {
                    self.expand_alias(id, &args).unwrap_or(Ty::Decl {
                        id,
                        args,
                        inferred: resolved.inferred,
                    })
                }
                Res::Decl(id) => Ty::Decl {
                    id,
                    args,
                    inferred: resolved.inferred,
                },
                Res::Rsactor(name) => Ty::Rsactor { name, args },
                Res::Std(path) => Ty::Std { path, args },
                Res::External(path) => Ty::External { path, args },
                Res::Module(..) | Res::Variant(..) | Res::Value(..) => unknown(whole),
            },
            None => unknown(whole),
        }
    }

    /// `type Result<T> = std::result::Result<T, Error>` used as `Result<X>`:
    /// the declared target with the alias parameters replaced. The target is
    /// written in the declaration, so this needs no inference. Depth is
    /// capped because a last-segment fallback could otherwise loop.
    fn expand_alias(&self, id: DeclId, args: &[Ty]) -> Option<Ty> {
        thread_local! {
            static DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
        }
        let Item::Type(alias) = self.index.decl_item(id) else {
            return None;
        };
        let depth = DEPTH.with(|d| d.get());
        if depth >= 8 {
            return None;
        }
        DEPTH.with(|d| d.set(depth + 1));
        let decl = &self.index.decls[id];
        let target = TyCtx {
            index: self.index,
            krate: decl.krate,
            module: decl.module,
            generics: &decl.generics,
            self_ty: None,
            blocks: &[],
        }
        .resolve(&alias.ty)
        .substitute(&decl.generics, args);
        DEPTH.with(|d| d.set(depth));
        Some(target)
    }

    fn generic_args(&self, arguments: &PathArguments) -> Vec<Ty> {
        match arguments {
            PathArguments::AngleBracketed(angle) => angle
                .args
                .iter()
                .filter_map(|arg| match arg {
                    GenericArgument::Type(ty) => Some(self.resolve(ty)),
                    _ => None,
                })
                .collect(),
            // `FnOnce(u32) -> String`: keep the written form so different
            // signatures keep different keys.
            PathArguments::Parenthesized(p) => {
                vec![Ty::Unknown(compact_tokens(
                    &p.to_token_stream().to_string(),
                ))]
            }
            PathArguments::None => Vec::new(),
        }
    }

    fn bounds<'b>(&self, bounds: impl Iterator<Item = &'b TypeParamBound>) -> Vec<Ty> {
        bounds
            .filter_map(|bound| match bound {
                TypeParamBound::Trait(t) => {
                    let ty = Type::Path(syn::TypePath {
                        attrs: Vec::new(),
                        qself: None,
                        path: t.path.clone(),
                    });
                    Some(self.resolve(&ty))
                }
                _ => None,
            })
            .collect()
    }
}

fn unknown(ty: &Type) -> Ty {
    Ty::Unknown(compact_tokens(&ty.to_token_stream().to_string()))
}

impl Ty {
    /// Comparison key: a declared type is its full path with generic
    /// arguments dropped; anything else is the whole type with every inner
    /// path normalised, so `Box<dyn Calculator>` and `Box<dyn FnOnce()>` stay
    /// apart while `Box<dyn crate::calc::Calculator>` matches the first.
    pub fn key(&self, index: &Index) -> String {
        match self {
            Ty::Decl { id, .. } => index.decl_path(*id),
            _ => self.render(index, true),
        }
    }

    /// Short form for labels: last path segments, arguments kept.
    pub fn label(&self, index: &Index) -> String {
        self.render(index, false)
    }

    /// Strips references and the smart pointers method calls see through
    /// (auto-deref): `&T`, `Box<T>`, `Arc<T>`, `Rc<T>`.
    pub fn auto_deref(&self) -> &Ty {
        match self {
            Ty::Ref(inner) => inner.auto_deref(),
            Ty::Std { path, args }
                if args.len() == 1
                    && matches!(
                        path.as_str(),
                        "std::boxed::Box" | "std::sync::Arc" | "std::rc::Rc"
                    ) =>
            {
                args[0].auto_deref()
            }
            other => other,
        }
    }

    /// `Ty::Rsactor` with the given name, after auto-deref.
    pub fn rsactor(&self, wanted: &str) -> Option<&[Ty]> {
        match self.auto_deref() {
            Ty::Rsactor { name, args } if name == wanted => Some(args),
            _ => None,
        }
    }

    /// `dyn TellHandler<M>`-style trait object, after auto-deref: the
    /// rsactor trait name and its arguments.
    pub fn handler_trait(&self) -> Option<(&str, &[Ty])> {
        let Ty::Dyn(bounds) = self.auto_deref() else {
            return None;
        };
        bounds.iter().find_map(|bound| match bound {
            Ty::Rsactor { name, args }
                if matches!(
                    name.as_str(),
                    "TellHandler" | "AskHandler" | "WeakTellHandler" | "WeakAskHandler"
                ) =>
            {
                Some((name.as_str(), args.as_slice()))
            }
            _ => None,
        })
    }

    /// Replaces generic parameters by name.
    pub fn substitute(&self, params: &[String], args: &[Ty]) -> Ty {
        let sub = |items: &[Ty]| -> Vec<Ty> {
            items.iter().map(|t| t.substitute(params, args)).collect()
        };
        match self {
            Ty::Generic(name) => match params.iter().position(|p| p == name) {
                Some(i) if i < args.len() => args[i].clone(),
                _ => self.clone(),
            },
            Ty::Decl {
                id,
                args: a,
                inferred,
            } => Ty::Decl {
                id: *id,
                args: sub(a),
                inferred: *inferred,
            },
            Ty::Rsactor { name, args: a } => Ty::Rsactor {
                name: name.clone(),
                args: sub(a),
            },
            Ty::Std { path, args: a } => Ty::Std {
                path: path.clone(),
                args: sub(a),
            },
            Ty::External { path, args: a } => Ty::External {
                path: path.clone(),
                args: sub(a),
            },
            Ty::Ref(inner) => Ty::Ref(Box::new(inner.substitute(params, args))),
            Ty::Slice(inner) => Ty::Slice(Box::new(inner.substitute(params, args))),
            Ty::Iter(inner) => Ty::Iter(Box::new(inner.substitute(params, args))),
            Ty::Tuple(items) => Ty::Tuple(sub(items)),
            Ty::Dyn(items) => Ty::Dyn(sub(items)),
            Ty::ImplTrait(items) => Ty::ImplTrait(sub(items)),
            Ty::Unknown(_) => self.clone(),
        }
    }

    /// Weakest judgement that went into this type.
    pub fn resolution(&self) -> Resolution {
        match self {
            Ty::Decl { args, inferred, .. } => {
                let own = if *inferred {
                    Resolution::Inferred
                } else {
                    Resolution::Exact
                };
                args.iter().fold(own, |acc, a| acc.weakest(a.resolution()))
            }
            Ty::Rsactor { args, .. } | Ty::Std { args, .. } | Ty::External { args, .. } => args
                .iter()
                .fold(Resolution::Exact, |acc, a| acc.weakest(a.resolution())),
            Ty::Generic(_) => Resolution::Exact,
            Ty::Ref(inner) | Ty::Slice(inner) | Ty::Iter(inner) => inner.resolution(),
            Ty::Tuple(items) | Ty::Dyn(items) | Ty::ImplTrait(items) => items
                .iter()
                .fold(Resolution::Exact, |acc, a| acc.weakest(a.resolution())),
            Ty::Unknown(_) => Resolution::Unresolved,
        }
    }

    fn render(&self, index: &Index, full: bool) -> String {
        let args = |args: &[Ty]| -> String {
            if args.is_empty() {
                String::new()
            } else {
                let inner: Vec<String> = args.iter().map(|a| a.render(index, full)).collect();
                format!("<{}>", inner.join(", "))
            }
        };
        let name = |path: &str| -> String {
            if full {
                path.to_string()
            } else {
                path.rsplit("::").next().unwrap_or(path).to_string()
            }
        };
        match self {
            Ty::Decl { id, args: a, .. } => {
                let base = if full {
                    index.decl_path(*id)
                } else {
                    index.decls[*id].name.clone()
                };
                format!("{base}{}", args(a))
            }
            Ty::Rsactor { name: n, args: a } => {
                let base = if full {
                    format!("rsactor::{n}")
                } else {
                    n.clone()
                };
                format!("{base}{}", args(a))
            }
            Ty::Std { path, args: a } | Ty::External { path, args: a } => {
                // `FnOnce(u32)` keeps its parenthesised arguments verbatim.
                match a.as_slice() {
                    [Ty::Unknown(paren)] if paren.starts_with('(') => {
                        format!("{}{paren}", name(path))
                    }
                    _ => format!("{}{}", name(path), args(a)),
                }
            }
            Ty::Generic(g) => g.clone(),
            Ty::Ref(inner) => format!("&{}", inner.render(index, full)),
            Ty::Tuple(items) => {
                let inner: Vec<String> = items.iter().map(|i| i.render(index, full)).collect();
                format!("({})", inner.join(", "))
            }
            Ty::Slice(inner) => format!("[{}]", inner.render(index, full)),
            Ty::Iter(inner) => format!("Iterator<{}>", inner.render(index, full)),
            Ty::Dyn(bounds) => {
                let inner: Vec<String> = bounds.iter().map(|b| b.render(index, full)).collect();
                format!("dyn {}", inner.join(" + "))
            }
            Ty::ImplTrait(bounds) => {
                let inner: Vec<String> = bounds.iter().map(|b| b.render(index, full)).collect();
                format!("impl {}", inner.join(" + "))
            }
            Ty::Unknown(text) => text.clone(),
        }
    }
}
