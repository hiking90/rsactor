// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Clippy regression tests for code the macros generate or require.
//!
//! The assertions here are the `deny` attributes, checked by
//! `cargo clippy --all-targets`: a macro change that brings a lint back turns
//! into a clippy error in this file. The `#[tokio::test]`s only confirm the
//! same code still runs.

use rsactor::{message_handlers, spawn, Actor, ActorRef};

/// `#[derive(Actor)]` must not generate an `async fn on_start` with nothing to
/// await: `clippy::unused_async_trait_impl` reports it at the derive, where the
/// user has no source to change.
mod derive_actor {
    // Clippy before 1.98 does not know the lint.
    #![allow(unknown_lints)]
    #![deny(clippy::unused_async_trait_impl)]

    use super::*;

    #[derive(Actor)]
    struct Plain {
        value: u32,
    }

    struct Get;

    #[message_handlers]
    impl Plain {
        #[handler]
        async fn get(&mut self, _: Get, _: &ActorRef<Self>) -> u32 {
            // An await keeps the handler itself out of the lint's reach (it
            // also fires on inherent methods: rust-clippy#17772).
            tokio::task::yield_now().await;
            self.value
        }
    }

    #[tokio::test]
    async fn derived_actor_starts_with_its_args() {
        let (actor_ref, _handle) = spawn::<Plain>(Plain { value: 7 });
        assert_eq!(actor_ref.ask(Get).await.unwrap(), 7);
    }
}

/// A handler must be an `async fn`, so `clippy::unused_async` on a handler that
/// awaits nothing cannot be acted on; `#[message_handlers]` allows it there.
mod handlers_without_await {
    #![deny(clippy::unused_async)]

    use super::*;

    #[derive(Actor)]
    struct Counter {
        count: u32,
    }

    struct Increment;
    struct Get;
    struct Checked;

    #[message_handlers]
    impl Counter {
        #[handler]
        async fn increment(&mut self, _: Increment, _: &ActorRef<Self>) {
            self.count += 1;
        }

        #[handler]
        async fn get(&mut self, _: Get, _: &ActorRef<Self>) -> u32 {
            self.count
        }

        #[handler]
        async fn checked(&mut self, _: Checked, _: &ActorRef<Self>) -> Result<u32, String> {
            Ok(self.count)
        }
    }

    #[tokio::test]
    async fn handlers_without_await_reply() {
        let (actor_ref, _handle) = spawn::<Counter>(Counter { count: 0 });
        actor_ref.tell(Increment).await.unwrap();
        assert_eq!(actor_ref.ask(Get).await.unwrap(), 1);
        assert_eq!(actor_ref.ask(Checked).await.unwrap(), Ok(1));
    }
}
