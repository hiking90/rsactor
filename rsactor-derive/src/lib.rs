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
//! - Automatic generation of Message<T> trait implementations
//! - Automatic implementation of MessageHandler trait for the actor
//! - Selective processing: only methods with `#[handler]` attribute are processed
//! - Reduced boilerplate and potential for errors
//! - Type-safe message handling with compile-time checks

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    parse_macro_input, Data, DeriveInput, FnArg, ImplItem, ImplItemFn, ItemImpl, PatType,
    ReturnType, Type,
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

    // Check if it's a struct or enum
    match &input.data {
        Data::Struct(_) | Data::Enum(_) => {
            // Generate the Actor implementation
            let expanded = quote! {
                impl rsactor::Actor for #name {
                    type Args = Self;
                    type Error = std::convert::Infallible;

                    async fn on_start(
                        args: Self::Args,
                        _actor_ref: &rsactor::ActorRef<Self>,
                    ) -> std::result::Result<Self, Self::Error> {
                        Ok(args)
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
/// boilerplate code. It also automatically generates the `impl_message_handler!` macro call
/// to register the message handlers with the actor.
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
/// - The Message<MessageType> trait implementations for each handler method
/// - The MessageHandler trait implementation for the actor
///
/// # Requirements
///
/// Each method marked with `#[handler]` must follow this signature pattern:
/// - First parameter: `&mut self`
/// - Second parameter: `_msg: MessageType` (where MessageType is the message struct)
/// - Third parameter: `_: &ActorRef<Self>` or `_actor_ref: &ActorRef<Self>`
/// - Return type: the reply type for the message
///
/// # Benefits
///
/// - No need to manually implement Message<T> trait for each message type
/// - No need to manually implement MessageHandler trait
/// - Reduces boilerplate code and potential for errors
/// - Automatically keeps message handler registration in sync with method definitions
/// - Only processes methods marked with `#[handler]`, leaving other methods unchanged
#[proc_macro_attribute]
pub fn message_handlers(_attr: TokenStream, item: TokenStream) -> TokenStream {
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

    let (message_impls, message_types) =
        process_handler_methods(&input.items, actor_type, generics)?;

    // Remove `#[handler]` attributes from the impl block for clean output
    clean_handler_attributes(&mut input.items);

    // Generate MessageHandler implementation directly instead of using deprecated macro
    let message_handler_impl = generate_message_handler_impl(&message_types, actor_type, generics);

    let result = quote! {
        #input
        #(#message_impls)*
        #message_handler_impl
    };

    Ok(result)
}

fn process_handler_methods(
    items: &[ImplItem],
    actor_type: &Type,
    generics: &syn::Generics,
) -> syn::Result<(Vec<TokenStream2>, Vec<Type>)> {
    let mut message_impls = Vec::new();
    let mut message_types = Vec::new();

    for item in items {
        if let ImplItem::Fn(method) = item {
            // Check for `#[handler]` attribute
            if method
                .attrs
                .iter()
                .any(|attr| attr.path().is_ident("handler"))
            {
                // Generate message implementation for this method
                let impl_tokens = generate_message_impl(method, actor_type, generics)?;
                message_impls.push(impl_tokens);

                // Extract message type
                if let Some(message_type) = extract_message_type(method)? {
                    message_types.push(message_type);
                }
            }
        }
    }

    Ok((message_impls, message_types))
}

fn clean_handler_attributes(items: &mut [ImplItem]) {
    for item in items {
        if let ImplItem::Fn(method) = item {
            method.attrs.retain(|attr| !attr.path().is_ident("handler"));
        }
    }
}

fn generate_message_handler_impl(
    message_types: &[Type],
    actor_type: &Type,
    generics: &syn::Generics,
) -> TokenStream2 {
    if message_types.is_empty() {
        return quote! {};
    }

    let message_handler_body = generate_message_handler_body(message_types);

    if generics.params.is_empty() {
        quote! {
            impl rsactor::MessageHandler for #actor_type {
                #message_handler_body
            }
        }
    } else {
        let params = &generics.params;
        let where_clause = &generics.where_clause;
        quote! {
            impl<#params> rsactor::MessageHandler for #actor_type #where_clause {
                #message_handler_body
            }
        }
    }
}

fn generate_message_impl(
    method: &ImplItemFn,
    actor_type: &Type,
    generics: &syn::Generics,
) -> syn::Result<TokenStream2> {
    // Parse method signature
    let inputs = &method.sig.inputs;

    if inputs.len() != 3 {
        return Err(syn::Error::new_spanned(
            &method.sig,
            "Message handler method must have exactly 3 parameters: &mut self, message, &ActorRef<Self>"
        ));
    }

    // Extract message type from second parameter
    let message_type = match &inputs[1] {
        FnArg::Typed(PatType { ty, .. }) => ty,
        _ => {
            return Err(syn::Error::new_spanned(
                &inputs[1],
                "Expected typed parameter for message",
            ))
        }
    };

    // Extract return type - handle both explicit return types and unit return (no return type)
    let return_type = match &method.sig.output {
        ReturnType::Type(_, ty) => quote! { #ty },
        ReturnType::Default => quote! { () }, // Default to unit type for functions without explicit return type
    };

    // Get method name
    let method_name = &method.sig.ident;

    let (impl_generics, _ty_generics, where_clause) = generics.split_for_impl();

    // Generate the Message trait implementation
    let impl_tokens = quote! {
        impl #impl_generics rsactor::Message<#message_type> for #actor_type #where_clause {
            type Reply = #return_type;

            async fn handle(
                &mut self,
                msg: #message_type,
                actor_ref: &rsactor::ActorRef<Self>,
            ) -> Self::Reply {
                self.#method_name(msg, actor_ref).await
            }
        }
    };

    Ok(impl_tokens)
}

fn extract_message_type(method: &ImplItemFn) -> syn::Result<Option<Type>> {
    // Parse method signature
    let inputs = &method.sig.inputs;

    if inputs.len() != 3 {
        return Ok(None); // Skip methods that don't match the pattern
    }

    // Extract message type from second parameter
    match &inputs[1] {
        FnArg::Typed(PatType { ty, .. }) => Ok(Some(ty.as_ref().clone())),
        _ => Ok(None),
    }
}

fn generate_message_handler_body(message_types: &[Type]) -> TokenStream2 {
    quote! {
        async fn handle(
            &mut self,
            _msg_any: Box<dyn std::any::Any + Send>,
            actor_ref: &rsactor::ActorRef<Self>,
        ) -> rsactor::Result<Box<dyn std::any::Any + Send>> {
            let mut _msg_any = _msg_any;
            #(
                match _msg_any.downcast::<#message_types>() {
                    Ok(concrete_msg_box) => {
                        let reply = <Self as rsactor::Message<#message_types>>::handle(self, *concrete_msg_box, &actor_ref).await;
                        return Ok(Box::new(reply) as Box<dyn std::any::Any + Send>);
                    }
                    Err(original_box_back) => {
                        _msg_any = original_box_back;
                    }
                }
            )*
            let expected_msg_types: Vec<&'static str> = vec![#(stringify!(#message_types)),*];
            return Err(rsactor::Error::UnhandledMessageType {
                identity: actor_ref.identity(),
                expected_types: expected_msg_types,
                actual_type_id: _msg_any.type_id()
            });
        }
    }
}
