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
| `ActorControl` | Type-erased lifecycle control (strong) |
| `WeakActorControl` | Type-erased lifecycle control (weak) |
| `TellHandler<M>` | Type-erased fire-and-forget message sending |
| `AskHandler<M, R>` | Type-erased request-reply message sending |

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

// Access lifecycle control via as_control()
for handler in &handlers {
    println!("Actor {} alive: {}", handler.as_control().identity(), handler.as_control().is_alive());
    handler.tell(Ping).await?;
}

// Weak handlers (don't keep actors alive)
let weak: Box<dyn WeakTellHandler<Ping>> = ActorRef::downgrade(&actor_ref).into();

// Access weak control via as_weak_control()
println!("Weak identity: {}", weak.as_weak_control().identity());

if let Some(strong) = weak.upgrade() {
    strong.tell(Ping).await?;
}
```

## ActorControl (Type-Erased Lifecycle Management)

Manage different actor types without knowing their message types:

```rust
use rsactor::ActorControl;

// Store different actor types in one collection
let controls: Vec<Box<dyn ActorControl>> = vec![
    (&worker_actor).into(),
    (&logger_actor).into(),
];

// Unified lifecycle management
for control in &controls {
    println!("Actor {} alive: {}", control.identity(), control.is_alive());
}

// Stop all actors
for control in &controls {
    control.stop().await?;
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
rsactor = { version = "0.x", features = ["tracing", "metrics"] }
```

| Feature | Description |
|---------|-------------|
| `tracing` | Enable tracing instrumentation for observability |
| `metrics` | Enable actor performance metrics |
| `test-utils` | Enable dead letter counter for testing (never use in production) |

## Error Handling

### Error Methods

```rust
use rsactor::Error;

fn handle_error(err: &Error) {
    // Check if operation can be retried
    if err.is_retryable() {
        // Only Timeout errors are retryable
        println!("Retrying...");
    }

    // Get actionable debugging tips
    for tip in err.debugging_tips() {
        eprintln!("Tip: {}", tip);
    }
}
```

### Retry Pattern

```rust
async fn send_with_retry<T, M>(
    actor: &ActorRef<T>,
    msg: M,
    max_attempts: usize,
) -> Result<(), Error>
where
    T: Actor + Message<M>,
    M: Clone + Send + 'static,
{
    let mut attempts = 0;
    loop {
        match actor.tell(msg.clone()).await {
            Ok(()) => return Ok(()),
            Err(e) if e.is_retryable() && attempts < max_attempts => {
                attempts += 1;
                tokio::time::sleep(Duration::from_millis(100 * attempts as u64)).await;
            }
            Err(e) => return Err(e),
        }
    }
}
```

## Dead Letter Tracking

Dead letters are messages that could not be delivered. They are automatically logged via `tracing`:

```text
WARN rsactor::dead_letter: Dead letter: message could not be delivered
    actor.id=42
    actor.type_name="MyActor"
    message.type_name="PingMessage"
    dead_letter.reason="actor stopped"
    dead_letter.operation="tell"
```

### Dead Letter Reasons

| Reason | When it occurs |
|--------|---------------|
| `ActorStopped` | Message sent to stopped actor |
| `Timeout` | `tell_with_timeout` or `ask_with_timeout` exceeded |
| `ReplyDropped` | Handler failed before sending reply |

### Testing Dead Letters

```rust
// Enable test-utils feature
use rsactor::{dead_letter_count, reset_dead_letter_count};

#[tokio::test]
async fn test_dead_letters() {
    reset_dead_letter_count();
    let initial = dead_letter_count();

    // ... cause dead letters ...

    assert_eq!(dead_letter_count() - initial, expected_count);
}
```

## Blocking Operations with Timeout

```rust
use std::time::Duration;

// Without timeout
actor_ref.blocking_tell(msg, None)?;
actor_ref.blocking_ask(query, None)?;

// With timeout
actor_ref.blocking_tell(msg, Some(Duration::from_secs(5)))?;
let result: String = actor_ref.blocking_ask(query, Some(Duration::from_secs(10)))?;
```

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
- Enable tracing to see dead letter logs: `RUST_LOG=rsactor=warn`
- Use `err.debugging_tips()` for actionable suggestions

## Imports Cheatsheet

```rust
use rsactor::{
    // Core
    Actor, ActorRef, ActorWeak, ActorResult, Error,
    // Macros
    message_handlers,
    // Functions
    spawn,
    // Handler traits (for polymorphism)
    TellHandler, AskHandler, WeakTellHandler, WeakAskHandler,
    // Control traits (for type-erased lifecycle management)
    ActorControl, WeakActorControl,
    // Dead letters (requires test-utils feature)
    // dead_letter_count, reset_dead_letter_count,
    // Debugging
    DeadLetterReason,
};
```
