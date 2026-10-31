// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Derive macros for rsActor framework
//!
//! This crate provides derive macros for the rsActor framework, allowing users
//! to automatically implement common traits with sensible defaults.
//!
//! ## Actor Derive Macro
//!
//! The `#[derive(Actor)]` macro provides a convenient way to implement the Actor trait
//! for simple structs and enums that don't require complex initialization logic.
//!
//! ### Generated Implementation
//!
//! When you use `#[derive(Actor)]`, it generates:
//! - `Args` type set to `Self` (the struct or enum itself)
//! - `Error` type set to `std::convert::Infallible` (never fails)
//! - `on_start` method that simply returns the provided args
//!
//! ### Usage
//!
//! #### With Structs
//! ```rust
//! use rsactor::Actor;
//!
//! #[derive(Actor)]
//! struct MyActor {
//!     name: String,
//!     count: u32,
//! }
//! ```
//!
//! #### With Enums
//! ```rust
//! use rsactor::Actor;
//!
//! #[derive(Actor)]
//! enum StateActor {
//!     Idle,
//!     Processing(String),
//!     Completed(i32),
//! }
//! ```
//!
//! This is equivalent to manually writing:
//!
//! ```rust
//! # struct MyActor { name: String, count: u32 }
//! use rsactor::{Actor, ActorRef};
//! use std::convert::Infallible;
//!
//! impl Actor for MyActor {
//!     type Args = Self;
//!     type Error = Infallible;
//!     type IdleEvent = ();
//!
//!     async fn on_start(
//!         args: Self::Args,
//!         _actor_ref: &ActorRef<Self>,
//!     ) -> std::result::Result<Self, Self::Error> {
//!         Ok(args)
//!     }
//! }
//! ```
//!
//! ### When to Use
//!
//! Use the derive macro when:
//! - Your actor doesn't need complex initialization
//! - You want to pass a fully constructed instance to `spawn()`
//! - You don't need custom error handling during initialization
//!
//! For complex initialization (async resource setup, validation, etc.),
//! implement the Actor trait manually.
//!
//! ## Message Handlers Attribute Macro
//!
//! The `#[message_handlers]` attribute macro combined with `#[handler]` method attributes
//! provides an automated way to generate Message trait implementations and register message handlers.
//!
//! ### Usage
//!
//! ```rust
//! use rsactor::{Actor, ActorRef, message_handlers};
//!
//! #[derive(Actor)]
//! struct MyActor {
//!     count: u32,
//! }
//!
//! struct Increment;
//! struct GetCount;
//!
//! #[message_handlers]
//! impl MyActor {
//!     #[handler]
//!     async fn handle_increment(&mut self, _msg: Increment, _: &ActorRef<Self>) -> u32 {
//!         self.count += 1;
//!         self.count
//!     }
//!
//!     #[handler]
//!     async fn handle_get_count(&mut self, _msg: GetCount, _: &ActorRef<Self>) -> u32 {
//!         self.count
//!     }
//!
//!     // Regular methods without #[handler] are left unchanged
//!     fn internal_method(&self) -> u32 {
//!         self.count * 2
//!     }
//! }
//! ```
//!
//! ### Benefits
//!
//! - Automatic generation of `Message<T>` trait implementations
//! - Selective processing: only methods with `#[handler]` attribute are processed
//! - Reduced boilerplate and potential for errors
//! - Type-safe message handling with compile-time checks
//!
//! ## Crate Path
//!
//! Both macros emit absolute `::rsactor::…` paths, which resolve only through
//! the extern prelude. The crate that uses the macros must therefore see
//! `rsactor` under that exact name:
//!
//! - A crate that reaches rsactor only through a facade crate's re-export
//!   (`use facade::rsactor;`) or a `use … as rsactor` alias fails to compile
//!   with "could not find `rsactor`". Add `rsactor` to its own `[dependencies]`.
//! - A renamed dependency (`actors = { package = "rsactor", … }`) can instead
//!   declare `extern crate actors as rsactor;` at the crate root.
//!
//! The macros take no option to override this path.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    parse_macro_input, Data, DeriveInput, FnArg, ImplItem, ImplItemFn, ItemImpl, PatType,
    ReceiverKind, ReturnType, Type,
};

/// Derive macro for automatically implementing the Actor trait.
///
/// This macro generates a default implementation of the Actor trait where:
/// - `Args` type is set to `Self`
/// - `Error` type is set to `std::convert::Infallible`
/// - `on_start` method returns the args as the actor instance
///
/// # Examples
///
/// ## Struct Actor
/// ```rust
/// use rsactor::Actor;
///
/// #[derive(Actor)]
/// struct MyActor {
///     name: String,
/// }
/// ```
///
/// ## Enum Actor
/// ```rust
/// use rsactor::Actor;
///
/// #[derive(Actor)]
/// enum StateActor {
///     Idle,
///     Processing(String),
///     Completed(i32),
/// }
/// ```
///
/// This generates an implementation equivalent to:
///
/// ```rust
/// # struct MyActor { name: String }
/// impl rsactor::Actor for MyActor {
///     type Args = Self;
///     type Error = std::convert::Infallible;
///     type IdleEvent = ();
///
///     async fn on_start(
///         args: Self::Args,
///         _actor_ref: &rsactor::ActorRef<Self>,
///     ) -> std::result::Result<Self, Self::Error> {
///         Ok(args)
///     }
/// }
/// ```
///
/// # Limitations
///
/// - Only works on structs and enums (not unions)
/// - Generates a very basic implementation - for complex initialization logic,
///   implement the Actor trait manually
/// - Leaves `MAILBOX_CAPACITY` at its `None` default, so a derived actor takes
///   the process-wide default capacity. To declare a capacity on the type,
///   implement `Actor` manually and set the constant there
/// - Emits `::rsactor::…` paths, so the using crate must have `rsactor` in its
///   extern prelude under that name. A crate that reaches rsactor only through
///   a facade crate's re-export or a `use … as rsactor` alias fails with "could
///   not find `rsactor`"; add `rsactor` to its own `[dependencies]`. A renamed
///   dependency (`actors = { package = "rsactor", … }`) can instead declare
///   `extern crate actors as rsactor;` at the crate root. There is no option to
///   override the path
#[proc_macro_derive(Actor)]
pub fn derive_actor(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let expanded = match derive_actor_impl(input) {
        Ok(tokens) => tokens,
        Err(err) => err.to_compile_error(),
    };

    TokenStream::from(expanded)
}

fn derive_actor_impl(input: DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    // Check if it's a struct or enum
    match &input.data {
        Data::Struct(_) | Data::Enum(_) => {
            // Generate the Actor implementation with proper generic support
            let expanded = quote! {
                impl #impl_generics ::rsactor::Actor for #name #ty_generics #where_clause {
                    type Args = Self;
                    type Error = ::core::convert::Infallible;
                    type IdleEvent = ();

                    async fn on_start(
                        args: Self::Args,
                        _actor_ref: &::rsactor::ActorRef<Self>,
                    ) -> ::core::result::Result<Self, Self::Error> {
                        ::core::result::Result::Ok(args)
                    }
                }
            };

            Ok(expanded)
        }
        _ => Err(syn::Error::new_spanned(
            name,
            "Actor derive macro can only be used on structs and enums",
        )),
    }
}

/// Attribute macro for automatically generating Message trait implementations
/// from method definitions.
///
/// This macro analyzes method signatures in an impl block and generates the corresponding
/// Message trait implementations for methods marked with `#[handler]` attribute, reducing
/// boilerplate code.
///
/// # Usage
///
/// ```rust
/// use rsactor::{Actor, ActorRef, message_handlers};
///
/// #[derive(Actor)]
/// struct MyActor {
///     count: u32,
/// }
///
/// struct Increment;
/// struct Decrement;
///
/// #[message_handlers]
/// impl MyActor {
///     #[handler]
///     async fn handle_increment(&mut self, _msg: Increment, _: &ActorRef<Self>) -> u32 {
///         self.count += 1;
///         self.count
///     }
///
///     #[handler]
///     async fn handle_decrement(&mut self, _msg: Decrement, _: &ActorRef<Self>) -> u32 {
///         self.count -= 1;
///         self.count
///     }
///
///     // Regular methods without #[handler] are left unchanged
///     fn get_internal_state(&self) -> u32 {
///         self.count
///     }
/// }
/// ```
///
/// This will automatically generate:
/// - The `Message<MessageType>` trait implementations for each handler method
///
/// # Requirements
///
/// Each method marked with `#[handler]` must follow this signature pattern:
/// - Must be an `async fn`
/// - First parameter: `&mut self`
/// - Second parameter: `msg: MessageType` (where MessageType is the message struct)
/// - Third parameter: `&ActorRef<Self>` or `&rsactor::ActorRef<Self>`
/// - Return type: the reply type for the message
/// - No type or const generic parameters on the method itself
/// - No `impl Trait` in the message or return type
///
/// A `#[cfg(...)]` on a handler method is copied onto its generated
/// `Message` impl, so a feature-gated handler is removed together with its impl.
/// Predicates in the method's `where` clause are copied onto the impl as well,
/// except those that name one of the method's own lifetime parameters.
///
/// `#[message_handlers]` itself takes no arguments; options belong on
/// `#[handler(...)]`.
///
/// The generated impls use `::rsactor::…` paths, so the using crate must have
/// `rsactor` in its extern prelude under that name. A crate that reaches
/// rsactor only through a facade crate's re-export or a `use … as rsactor`
/// alias fails with "could not find `rsactor`"; add `rsactor` to its own
/// `[dependencies]`. A renamed dependency
/// (`actors = { package = "rsactor", … }`) can instead declare
/// `extern crate actors as rsactor;` at the crate root. There is no option to
/// override the path.
///
/// # Error Messages
///
/// The macro provides detailed error messages for common mistakes:
/// - Wrong parameter count
/// - Missing `async` keyword
/// - Incorrect parameter types
/// - Invalid `#[handler]` attribute usage
///
/// # Benefits
///
/// - No need to manually implement `Message<T>` trait for each message type
/// - Reduces boilerplate code and potential for errors
/// - Only processes methods marked with `#[handler]`, leaving other methods unchanged
#[proc_macro_attribute]
pub fn message_handlers(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return syn::Error::new_spanned(
            TokenStream2::from(attr),
            "`#[message_handlers]` takes no arguments; put handler options on `#[handler(...)]`",
        )
        .to_compile_error()
        .into();
    }

    let input = parse_macro_input!(item as ItemImpl);

    let expanded = match message_impl(input) {
        Ok(tokens) => tokens,
        Err(err) => err.to_compile_error(),
    };

    TokenStream::from(expanded)
}

fn message_impl(mut input: ItemImpl) -> syn::Result<TokenStream2> {
    let actor_type = &input.self_ty;
    let generics = &input.generics;

    let message_impls = process_handler_methods(&input.items, actor_type, generics)?;

    // Remove `#[handler]` attributes from the impl block for clean output
    clean_handler_attributes(&mut input.items);

    let result = quote! {
        #input
        #(#message_impls)*
    };

    Ok(result)
}

/// Options parsed from the `#[handler]` attribute.
#[derive(Default)]
struct HandlerOptions {
    /// `#[handler(result)]` — treat return type as Result and generate on_tell_result override.
    force_result: bool,
    /// `#[handler(no_log)]` — suppress automatic on_tell_result generation.
    no_log: bool,
}

fn parse_handler_options(attr: &syn::Attribute) -> syn::Result<HandlerOptions> {
    let mut options = HandlerOptions::default();

    match &attr.meta {
        // #[handler] — no arguments
        syn::Meta::Path(_) => {}

        // #[handler(...)] — parse arguments
        syn::Meta::List(_) => {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("result") {
                    options.force_result = true;
                    Ok(())
                } else if meta.path.is_ident("no_log") {
                    options.no_log = true;
                    Ok(())
                } else {
                    Err(meta.error("unknown handler option; expected `result` or `no_log`"))
                }
            })?;
        }

        _ => {
            return Err(syn::Error::new_spanned(
                attr,
                "expected `#[handler]`, `#[handler(result)]`, or `#[handler(no_log)]`",
            ));
        }
    }

    if options.force_result && options.no_log {
        return Err(syn::Error::new_spanned(
            attr,
            "`result` and `no_log` are mutually exclusive",
        ));
    }

    Ok(options)
}

/// Returns true if the type is syntactically `Result<...>`.
///
/// Detectable: `Result<T, E>`, `std::result::Result<T, E>`, `anyhow::Result<T>`
/// Not detectable: type aliases (e.g., `MyResult`) — use `#[handler(result)]` instead.
///
/// Note: only compares the last path segment's ident, so a user-defined type named
/// `Result` will produce a false positive. Use `#[handler(no_log)]` to suppress.
fn is_result_type(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Path(type_path) => type_path
            .path
            .segments
            .last()
            .map(|seg| seg.ident == "Result")
            .unwrap_or(false),
        _ => false,
    }
}

fn process_handler_methods(
    items: &[ImplItem],
    actor_type: &Type,
    generics: &syn::Generics,
) -> syn::Result<Vec<TokenStream2>> {
    let mut message_impls = Vec::new();

    for item in items {
        if let ImplItem::Fn(method) = item {
            let handler_attr = method
                .attrs
                .iter()
                .find(|attr| attr.path().is_ident("handler"));

            if let Some(attr) = handler_attr {
                let options = parse_handler_options(attr)?;
                let impl_tokens = generate_message_impl(method, actor_type, generics, &options)?;
                message_impls.push(impl_tokens);
            }
        }
    }

    Ok(message_impls)
}

fn clean_handler_attributes(items: &mut [ImplItem]) {
    for item in items {
        if let ImplItem::Fn(method) = item {
            method.attrs.retain(|attr| !attr.path().is_ident("handler"));
        }
    }
}

fn generate_message_impl(
    method: &ImplItemFn,
    actor_type: &Type,
    generics: &syn::Generics,
    options: &HandlerOptions,
) -> syn::Result<TokenStream2> {
    // Parse method signature
    let inputs = &method.sig.inputs;

    // Validate that the method is async
    if method.sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            &method.sig,
            format!("Handler method '{}' must be async", method.sig.ident),
        ));
    }

    // The generated `impl Message<M>` only declares the impl block's generics,
    // so a method-level type or const parameter would be unbound there.
    // Lifetime parameters are left alone: one used only by the `&ActorRef`
    // parameter is inferred at the call and compiles.
    if let Some(param) = method
        .sig
        .generics
        .params
        .iter()
        .find(|param| !matches!(param, syn::GenericParam::Lifetime(_)))
    {
        return Err(syn::Error::new_spanned(
            param,
            format!(
                "Handler method '{}' cannot have type or const parameters: the generated `impl Message<_>` has no place to declare them",
                method.sig.ident
            ),
        ));
    }

    if inputs.len() != 3 {
        return Err(syn::Error::new_spanned(
            &method.sig,
            format!(
                "Message handler method '{}' must have exactly 3 parameters: &mut self, message, &ActorRef<Self>. Found {} parameters.",
                method.sig.ident,
                inputs.len()
            )
        ));
    }

    // Validate first parameter (&mut self)
    // Judge by the receiver's kind, not `receiver.mutability`: that flag is the
    // binding's `mut` (by-value `mut self`), not the reference's. Accept the
    // shorthand `&mut self` and the explicit `self: &mut Self`. `ReceiverKind`
    // is `#[non_exhaustive]`; any form added later (e.g. `&pin mut self`) is
    // rejected until it is handled here. Tested by `HandlerReceiverDoctests`
    // in rsactor's `src/lib.rs`.
    if !matches!(
        &inputs[0],
        FnArg::Receiver(receiver) if match &receiver.kind {
            ReceiverKind::Reference(_, _, mutability) => mutability.is_some(),
            ReceiverKind::Typed(_, ty) => {
                matches!(ty.as_ref(), Type::Reference(r) if r.mutability.is_some())
            }
            _ => false,
        }
    ) {
        return Err(syn::Error::new_spanned(
            &inputs[0],
            "First parameter must be '&mut self'",
        ));
    }

    // Extract message type from second parameter
    let message_type = match &inputs[1] {
        FnArg::Typed(PatType { ty, .. }) => ty,
        _ => {
            return Err(syn::Error::new_spanned(
                &inputs[1],
                "Second parameter must be a typed message parameter (e.g., 'msg: MessageType')",
            ))
        }
    };

    // Argument-position `impl Trait` is an anonymous type parameter, which the
    // method-generics check above cannot see. It would become
    // `impl Message<impl Trait>`.
    if tokens_contain_ident(quote::ToTokens::to_token_stream(message_type), "impl") {
        return Err(syn::Error::new_spanned(
            message_type,
            format!(
                "Handler method '{}' cannot take `impl Trait` as its message type: the generated `impl Message<_>` needs a concrete type",
                method.sig.ident
            ),
        ));
    }

    // Validate third parameter (&ActorRef<Self>)
    let third_param_valid = match &inputs[2] {
        FnArg::Typed(PatType { ty, .. }) => {
            match ty.as_ref() {
                // A shared reference whose path ends in `ActorRef`; the
                // generated call passes `&ActorRef<Self>`, so `&mut` cannot
                // match.
                Type::Reference(type_ref) if type_ref.mutability.is_none() => {
                    if let Type::Path(type_path) = type_ref.elem.as_ref() {
                        type_path
                            .path
                            .segments
                            .last()
                            .is_some_and(|seg| seg.ident == "ActorRef")
                    } else {
                        false
                    }
                }
                _ => false,
            }
        }
        _ => false,
    };

    if !third_param_valid {
        return Err(syn::Error::new_spanned(
            &inputs[2],
            "Third parameter must be '&ActorRef<Self>' or '&rsactor::ActorRef<Self>'",
        ));
    }

    // Extract return type
    let return_type_ty = match &method.sig.output {
        ReturnType::Type(_, ty) => Some(ty.as_ref()),
        ReturnType::Default => None,
    };

    // `-> impl Trait` would become `type Reply = impl Trait;`.
    if let Some(ty) = return_type_ty {
        if tokens_contain_ident(quote::ToTokens::to_token_stream(ty), "impl") {
            return Err(syn::Error::new_spanned(
                ty,
                format!(
                    "Handler method '{}' cannot return `impl Trait`: the generated `type Reply` needs a concrete type",
                    method.sig.ident
                ),
            ));
        }
    }

    let return_type = match return_type_ty {
        Some(ty) => quote! { #ty },
        None => quote! { () },
    };

    // Determine whether to generate on_tell_result override
    let is_result = return_type_ty.map(is_result_type).unwrap_or(false);

    let should_generate_on_tell_result = if options.no_log {
        false
    } else if options.force_result {
        if return_type_ty.is_none() {
            return Err(syn::Error::new_spanned(
                &method.sig,
                "`#[handler(result)]` requires a return type, but this method returns `()`",
            ));
        }
        true
    } else {
        is_result
    };

    // Get method name
    let method_name = &method.sig.ident;

    // The generated impl calls the method, so the method's `where` predicates
    // must hold there too. A predicate naming a method lifetime parameter
    // cannot be written on the impl (the lifetime is undeclared there) and
    // stays on the method only.
    let method_lifetimes: Vec<String> = method
        .sig
        .generics
        .lifetimes()
        .map(|param| param.lifetime.ident.to_string())
        .collect();
    let carried_predicates: Vec<syn::WherePredicate> = method
        .sig
        .generics
        .where_clause
        .iter()
        .flat_map(|clause| clause.predicates.iter())
        .filter(|pred| {
            !tokens_contain_lifetime(quote::ToTokens::to_token_stream(*pred), &method_lifetimes)
        })
        .cloned()
        .collect();
    let mut generics = generics.clone();
    if !carried_predicates.is_empty() {
        generics
            .make_where_clause()
            .predicates
            .extend(carried_predicates);
    }

    let (impl_generics, _ty_generics, where_clause) = generics.split_for_impl();

    let on_tell_result_impl = if should_generate_on_tell_result {
        quote! {
            fn on_tell_result(result: &Self::Reply, actor_ref: &::rsactor::ActorRef<Self>) {
                if let ::core::result::Result::Err(e) = result {
                    ::rsactor::__log_handler_error(
                        &actor_ref.identity(),
                        ::core::any::type_name::<#message_type>(),
                        e
                    );
                }
            }
        }
    } else {
        quote! {}
    };

    // An attribute macro sees the method's `#[cfg]` unevaluated, and the
    // compiler later strips only the method itself. Copy the cfg onto the
    // generated impl so it is removed together with the method it calls.
    let cfg_attrs = method
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("cfg"));

    // Generate the Message trait implementation
    let impl_tokens = quote! {
        #(#cfg_attrs)*
        impl #impl_generics ::rsactor::Message<#message_type> for #actor_type #where_clause {
            type Reply = #return_type;

            async fn handle(
                &mut self,
                msg: #message_type,
                actor_ref: &::rsactor::ActorRef<Self>,
            ) -> Self::Reply {
                self.#method_name(msg, actor_ref).await
            }

            #on_tell_result_impl
        }
    };

    Ok(impl_tokens)
}

/// Whether `tokens` contains the identifier (or keyword) `ident` at any group depth.
fn tokens_contain_ident(tokens: TokenStream2, ident: &str) -> bool {
    tokens.into_iter().any(|tt| match tt {
        proc_macro2::TokenTree::Ident(i) => i == ident,
        proc_macro2::TokenTree::Group(g) => tokens_contain_ident(g.stream(), ident),
        _ => false,
    })
}

/// Whether `tokens` contains a lifetime `'name` for any `name` in `names`, at
/// any group depth. A lifetime is a `'` punct followed by an identifier.
fn tokens_contain_lifetime(tokens: TokenStream2, names: &[String]) -> bool {
    if names.is_empty() {
        return false;
    }
    let mut after_apostrophe = false;
    for tt in tokens {
        match tt {
            proc_macro2::TokenTree::Punct(p) => {
                after_apostrophe = p.as_char() == '\'';
                continue;
            }
            proc_macro2::TokenTree::Ident(i) => {
                if after_apostrophe && names.iter().any(|name| i == name) {
                    return true;
                }
            }
            proc_macro2::TokenTree::Group(g) => {
                if tokens_contain_lifetime(g.stream(), names) {
                    return true;
                }
            }
            proc_macro2::TokenTree::Literal(_) => {}
        }
        after_apostrophe = false;
    }
    false
}

// TODO: Future enhancements that could be added:
//
// 1. Support for custom error types in derive macro:
//    #[derive(Actor)]
//    #[actor(error = "MyCustomError")]
//    struct MyActor { ... }
//
// 2. Support for custom Args types:
//    #[derive(Actor)]
//    #[actor(args = "MyArgsType")]
//    struct MyActor { ... }
//
// 3. Handler attribute with options:
//    #[handler(timeout = "5s")]
//    #[handler(priority = "high")]
//    async fn handle_message(&mut self, msg: Msg, _: &ActorRef<Self>) -> Reply
//
// 4. Automatic message struct generation:
//    #[message_handlers]
//    impl MyActor {
//        #[handler]
//        #[message(name = "Increment")]  // Generates struct Increment;
//        async fn handle_increment(&mut self, _: (), _: &ActorRef<Self>) -> u32
//    }
//
// 5. Validation attributes:
//    #[handler]
//    #[validate(non_empty, range(1..100))]
//    async fn handle_set_value(&mut self, msg: SetValue, _: &ActorRef<Self>) -> Result<(), Error>
