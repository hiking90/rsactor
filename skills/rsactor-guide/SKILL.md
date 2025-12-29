# rsactor-guide

Quick reference guide for rsactor actor framework.

## When to use

Use this skill when the user needs information about rsactor APIs, patterns, or troubleshooting.

## Quick Start

```toml
# Cargo.toml
[dependencies]
rsactor = "0.x"  # Check crates.io for latest version
tokio = { version = "1", features = ["full"] }
anyhow = "1"
```

```rust
use rsactor::{Actor, ActorRef, message_handlers, spawn};

struct Ping;

#[derive(Actor)]
struct MyActor;

#[message_handlers]
impl MyActor {
    #[handler]
    async fn handle_ping(&mut self, _: Ping, _: &ActorRef<Self>) {
        println!("Pong!");
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (actor_ref, handle) = spawn::<MyActor>(MyActor);
    actor_ref.tell(Ping).await?;
    actor_ref.stop().await?;
    handle.await?;
    Ok(())
}
```

## Core Types

| Type | Purpose |
|------|---------|
| `Actor` | Trait defining actor lifecycle |
| `ActorRef<T>` | Strong reference for sending messages |
| `ActorWeak<T>` | Weak reference (won't keep actor alive) |
| `ActorResult` | Enum for actor completion status |
| `UntypedActorRef` | Runtime type-checked reference |

## Actor Lifecycle

```
spawn()
  → on_start()     [initialization, returns Self]
  → on_run() loop  [concurrent with message processing]
  → on_stop()      [cleanup on shutdown]
  → ActorResult    [Completed or Failed]
```

### Lifecycle Methods

```rust
impl Actor for MyActor {
    type Args = MyActorArgs;  // Arguments passed to spawn()
    type Error = anyhow::Error;

    // Required: Initialize actor state
    async fn on_start(args: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error>;

    // Optional: Background task (called repeatedly)
    async fn on_run(&mut self, actor_ref: &ActorWeak<Self>) -> Result<(), Self::Error> {
        Ok(())  // Default: immediate return
    }

    // Optional: Cleanup on shutdown
    async fn on_stop(&mut self, actor_ref: &ActorWeak<Self>) -> Result<(), Self::Error> {
        Ok(())  // Default: no-op
    }
}
```

## Message Handling

### Defining Handlers

```rust
#[message_handlers]
impl MyActor {
    #[handler]
    async fn any_name(&mut self, msg: MessageType, actor_ref: &ActorRef<Self>) -> ReturnType {
        // Process message
    }
}
```

### Communication Patterns

```rust
// Fire-and-forget (tell) - handler returns ()
actor_ref.tell(MyMessage).await?;

// Request-response (ask) - handler returns a value
let result: Response = actor_ref.ask(MyQuery).await?;

// With timeout
use std::time::Duration;
let result = actor_ref.ask_timeout(MyQuery, Duration::from_secs(5)).await?;
```

## Actor Control

```rust
// Graceful stop (process remaining messages first)
actor_ref.stop().await?;

// Immediate stop (skip remaining messages)
actor_ref.kill().await?;

// Check if alive
if actor_ref.is_alive() { /* ... */ }

// Get unique identity
let id = actor_ref.identity();
```

## ActorResult Handling

```rust
let result = join_handle.await?;

match result {
    ActorResult::Completed { actor, killed } => {
        // actor: The final actor state
        // killed: true if stopped via kill(), false if stop()
        println!("Final state: {:?}", actor);
    }
    ActorResult::Failed { actor, error, phase, killed } => {
        // actor: Option<A> - may be None if failed in on_start
        // error: The error that caused failure
        // phase: "on_start", "on_run", "on_stop", or "handler"
        eprintln!("Failed in {}: {}", phase, error);
    }
}

// Helper methods
result.stopped_normally();  // true if Completed && !killed
result.was_killed();        // true if killed flag is set
result.into_actor();        // Extract actor (panics if Failed without actor)
result.try_into_actor();    // Option<A>
```

## Handler Traits (Polymorphism)

Store different actor types handling the same message:

```rust
use rsactor::{TellHandler, AskHandler, WeakTellHandler, WeakAskHandler};

// Strong handlers
let handlers: Vec<Box<dyn TellHandler<Ping>>> = vec![
    (&actor_a).into(),
    (&actor_b).into(),
];

// Weak handlers (don't keep actors alive)
let weak: Box<dyn WeakTellHandler<Ping>> = ActorRef::downgrade(&actor_ref).into();
if let Some(strong) = weak.upgrade() {
    strong.tell(Ping).await?;
}
```

## Common Patterns

### Periodic Tasks

```rust
async fn on_run(&mut self, _: &ActorWeak<Self>) -> Result<(), Self::Error> {
    tokio::select! {
        _ = self.interval.tick() => {
            self.do_periodic_work();
        }
        _ = self.some_future => {
            self.handle_event();
        }
    }
    Ok(())
}
```

### Actor Supervision (Parent-Child)

```rust
struct ParentActor {
    children: Vec<ActorRef<ChildActor>>,
}

#[message_handlers]
impl ParentActor {
    #[handler]
    async fn spawn_child(&mut self, _: SpawnChild, _: &ActorRef<Self>) {
        let (child_ref, _handle) = spawn::<ChildActor>(ChildArgs {});
        self.children.push(child_ref);
    }
}
```

### Forwarding Messages

```rust
#[handler]
async fn handle_forward(&mut self, msg: ForwardRequest, _: &ActorRef<Self>) {
    if let Some(target) = &self.target_actor {
        let _ = target.tell(msg.inner).await;
    }
}
```

## Feature Flags

```toml
[dependencies]
rsactor = { version = "0.x", features = ["tracing"] }
```

| Feature | Description |
|---------|-------------|
| `tracing` | Enable tracing instrumentation for observability |

## Troubleshooting

### "Handler not found" at compile time
- Ensure `#[message_handlers]` is on the impl block
- Ensure `#[handler]` is on each handler method
- Check message type matches exactly

### Actor stops unexpectedly
- Check `on_run` returns `Ok(())` (errors cause shutdown)
- Check handler errors
- Use `ActorResult::Failed` to see the error and phase

### Message not delivered
- Check `actor_ref.is_alive()` before sending
- Use `ask` instead of `tell` to get errors
- Check for panics in handlers (actor will fail)

## Imports Cheatsheet

```rust
use rsactor::{
    // Core
    Actor, ActorRef, ActorWeak, ActorResult,
    // Macros
    message_handlers,
    // Functions
    spawn,
    // Handler traits (for polymorphism)
    TellHandler, AskHandler, WeakTellHandler, WeakAskHandler,
    // Untyped (advanced)
    UntypedActorRef,
};
```
