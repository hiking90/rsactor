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
//! for simple structs that don't require complex initialization logic.
//!
//! ### Generated Implementation
//!
//! When you use `#[derive(Actor)]`, it generates:
//! - `Args` type set to `Self` (the struct itself)
//! - `Error` type set to `std::convert::Infallible` (never fails)
//! - `on_start` method that simply returns the provided args
//!
//! ### Usage
//!
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

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Data};

/// Derive macro for automatically implementing the Actor trait.
///
/// This macro generates a default implementation of the Actor trait where:
/// - `Args` type is set to `Self`
/// - `Error` type is set to `std::convert::Infallible`
/// - `on_start` method returns the args as the actor instance
///
/// # Example
///
/// ```rust
/// use rsactor::Actor;
///
/// #[derive(Actor)]
/// struct MyActor {
///     name: String,
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
/// - Only works on structs (not enums or unions)
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

    // Check if it's a struct
    match &input.data {
        Data::Struct(_) => {
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
        _ => {
            Err(syn::Error::new_spanned(
                name,
                "Actor derive macro can only be used on structs"
            ))
        }
    }
}
