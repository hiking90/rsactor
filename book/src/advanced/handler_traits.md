# Handler Traits

Handler traits enable **type-erased message sending**, allowing different actor types that handle the same message to be stored in a unified collection. This is essential for building systems where you need to broadcast messages to heterogeneous actors.

## Overview

rsActor provides four handler traits:

| Trait | Reference | Message Pattern |
|-------|-----------|-----------------|
| `TellHandler<M>` | Strong | Fire-and-forget |
| `AskHandler<M, R>` | Strong | Request-response |
| `WeakTellHandler<M>` | Weak | Fire-and-forget |
| `WeakAskHandler<M, R>` | Weak | Request-response |

**Strong** handlers keep actors alive. **Weak** handlers do not prevent actors from being dropped.

## TellHandler — Fire-and-Forget

Store different actor types that handle the same message in one collection:

```rust
use rsactor::{TellHandler, ActorRef, spawn, message_handlers, Actor};

// Two different actors that both handle PingMsg
#[derive(Actor)]
struct ActorA;
#[derive(Actor)]
struct ActorB;

struct PingMsg;

#[message_handlers]
impl ActorA {
    #[handler]
    async fn handle_ping(&mut self, _: PingMsg, _: &ActorRef<Self>) -> () {}
}

#[message_handlers]
impl ActorB {
    #[handler]
    async fn handle_ping(&mut self, _: PingMsg, _: &ActorRef<Self>) -> () {}
}

// Store both in a single collection
let (ref_a, _) = spawn::<ActorA>(ActorA);
let (ref_b, _) = spawn::<ActorB>(ActorB);

let handlers: Vec<Box<dyn TellHandler<PingMsg>>> = vec![
    (&ref_a).into(),  // From<&ActorRef<T>> — clones the reference
    ref_b.into(),     // From<ActorRef<T>> — moves ownership
];

// Broadcast to all handlers
for handler in &handlers {
    handler.tell(PingMsg).await?;
}
```

## AskHandler — Request-Response

For actors that return the same reply type for a message:

```rust
use rsactor::AskHandler;

struct GetStatus;

// Both actors return String for GetStatus
let handlers: Vec<Box<dyn AskHandler<GetStatus, String>>> = vec![
    (&actor_a).into(),
    (&actor_b).into(),
];

for handler in &handlers {
    let status = handler.ask(GetStatus).await?;
    println!("Status: {}", status);
}
```

## Weak Handlers

Weak handlers do not keep actors alive. You must `upgrade()` before sending messages:

```rust
use rsactor::{WeakTellHandler, ActorRef};

let weak_handlers: Vec<Box<dyn WeakTellHandler<PingMsg>>> = vec![
    ActorRef::downgrade(&actor_a).into(),
    ActorRef::downgrade(&actor_b).into(),
];

for handler in &weak_handlers {
    if let Some(strong) = handler.upgrade() {
        strong.tell(PingMsg).await?;
    }
    // else: actor was dropped, skip it
}
```

## Available Methods

All handler traits provide these methods:

| Method | TellHandler | AskHandler |
|--------|------------|------------|
| `tell(msg)` / `ask(msg)` | Async send | Async send + reply |
| `tell_with_timeout` / `ask_with_timeout` | With timeout | With timeout |
| `blocking_tell` / `blocking_ask` | Blocking send | Blocking send + reply |
| `clone_boxed()` | Clone handler | Clone handler |
| `downgrade()` | To weak handler | To weak handler |
| `as_control()` | Access lifecycle | Access lifecycle |

## Lifecycle Access via `as_control()`

Handler traits provide access to `ActorControl` for lifecycle management:

```rust
for handler in &handlers {
    let control = handler.as_control();
    println!("Actor {} alive: {}", control.identity(), control.is_alive());
}
```

## Running the Example

```bash
cargo run --example handler_demo
```
