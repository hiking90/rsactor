# Handler Traits

Handler traits enable unified management of different Actor types that handle the same message in a single collection.

## Overview

Handler traits are divided into three categories:

| Category | Traits | Description |
|----------|--------|-------------|
| **Strong Handlers** | `TellHandler<M>`, `AskHandler<M, R>` | Maintain strong references, keeping actors alive |
| **Weak Handlers** | `WeakTellHandler<M>`, `WeakAskHandler<M, R>` | Weak references that don't affect actor lifetime |
| **Control Traits** | `ActorControl`, `WeakActorControl` | Type-erased lifecycle management without message type knowledge |

## Quick Start

```rust
use rsactor::{TellHandler, AskHandler, WeakTellHandler, WeakAskHandler};

// Manage different Actor types in a single collection
let handlers: Vec<Box<dyn TellHandler<PingMsg>>> = vec![
    (&actor_a).into(),  // From<&ActorRef<T>>
    actor_b.into(),     // From<ActorRef<T>>
];

// Send messages to all handlers
for handler in &handlers {
    handler.tell(PingMsg { timestamp: 12345 }).await?;
}
```

---

## Strong Handler Traits

### TellHandler\<M\>

Trait for fire-and-forget message sending.

```rust
pub trait TellHandler<M: Send + 'static>: Send + Sync {
    fn tell(&self, msg: M) -> BoxFuture<'_, Result<()>>;
    fn tell_with_timeout(&self, msg: M, timeout: Duration) -> BoxFuture<'_, Result<()>>;
    fn blocking_tell(&self, msg: M) -> Result<()>;

    fn clone_boxed(&self) -> Box<dyn TellHandler<M>>;
    fn downgrade(&self) -> Box<dyn WeakTellHandler<M>>;

    fn as_control(&self) -> &dyn ActorControl;  // Access lifecycle control
}
```

**Usage Example:**

```rust
// Multiple Actor types in a single collection
let handlers: Vec<Box<dyn TellHandler<Ping>>> = vec![
    (&counter_actor).into(),
    (&logger_actor).into(),
];

for handler in &handlers {
    handler.tell(Ping { timestamp: 1000 }).await?;
}

// Clone handlers
let cloned = handlers[0].clone_boxed();
let cloned_vec = handlers.clone();  // Clone entire Vec

// Downgrade to weak
let weak: Box<dyn WeakTellHandler<Ping>> = handlers[0].downgrade();
```

### AskHandler\<M, R\>

Trait for request-response message sending.

```rust
pub trait AskHandler<M: Send + 'static, R: Send + 'static>: Send + Sync {
    fn ask(&self, msg: M) -> BoxFuture<'_, Result<R>>;
    fn ask_with_timeout(&self, msg: M, timeout: Duration) -> BoxFuture<'_, Result<R>>;
    fn blocking_ask(&self, msg: M) -> Result<R>;

    fn clone_boxed(&self) -> Box<dyn AskHandler<M, R>>;
    fn downgrade(&self) -> Box<dyn WeakAskHandler<M, R>>;

    fn as_control(&self) -> &dyn ActorControl;  // Access lifecycle control
}
```

**Usage Example:**

```rust
// Actors that return the same Reply type
let handlers: Vec<Box<dyn AskHandler<GetStatus, Status>>> = vec![
    (&counter_actor).into(),
    (&logger_actor).into(),
];

// Collect status from all actors
for handler in &handlers {
    let status = handler.ask(GetStatus).await?;
    println!("{}: {} messages", status.name, status.message_count);
}
```

---

## Weak Handler Traits

Weak Handlers follow the **upgrade-only** pattern. They do not provide `tell`/`ask` methods directly. You must call `upgrade()` to obtain a Strong Handler before use.

### WeakTellHandler\<M\>

```rust
pub trait WeakTellHandler<M: Send + 'static>: Send + Sync {
    fn upgrade(&self) -> Option<Box<dyn TellHandler<M>>>;
    fn clone_boxed(&self) -> Box<dyn WeakTellHandler<M>>;
    fn as_weak_control(&self) -> &dyn WeakActorControl;  // Access lifecycle control
}
```

### WeakAskHandler\<M, R\>

```rust
pub trait WeakAskHandler<M: Send + 'static, R: Send + 'static>: Send + Sync {
    fn upgrade(&self) -> Option<Box<dyn AskHandler<M, R>>>;
    fn clone_boxed(&self) -> Box<dyn WeakAskHandler<M, R>>;
    fn as_weak_control(&self) -> &dyn WeakActorControl;  // Access lifecycle control
}
```

**Usage Example:**

```rust
// Create weak handlers
let weak_handlers: Vec<Box<dyn WeakTellHandler<Ping>>> = vec![
    ActorRef::downgrade(&actor_a).into(),
    ActorRef::downgrade(&actor_b).into(),
];

// Individual upgrade pattern
for handler in &weak_handlers {
    if let Some(strong) = handler.upgrade() {
        strong.tell(Ping { timestamp: 12345 }).await?;
    }
}

// Batch operations: single upgrade for multiple messages
if let Some(strong) = weak_handlers[0].upgrade() {
    strong.tell(Ping { timestamp: 1 }).await?;
    strong.tell(Ping { timestamp: 2 }).await?;
    strong.tell(Ping { timestamp: 3 }).await?;
}

// After actor stops, upgrade returns None
actor_a.stop().await?;
assert!(weak_handlers[0].upgrade().is_none());
```

---

## Control Traits

Control traits provide type-erased lifecycle management without requiring knowledge of the actor's message types.

### ActorControl

```rust
pub trait ActorControl: Send + Sync {
    fn identity(&self) -> Identity;
    fn is_alive(&self) -> bool;
    fn stop(&self) -> BoxFuture<'_, Result<()>>;
    fn kill(&self) -> Result<()>;
    fn downgrade(&self) -> Box<dyn WeakActorControl>;
    fn clone_boxed(&self) -> Box<dyn ActorControl>;
}
```

### WeakActorControl

```rust
pub trait WeakActorControl: Send + Sync {
    fn identity(&self) -> Identity;
    fn is_alive(&self) -> bool;
    fn upgrade(&self) -> Option<Box<dyn ActorControl>>;
    fn clone_boxed(&self) -> Box<dyn WeakActorControl>;
}
```

**Usage Example:**

```rust
use rsactor::ActorControl;

// Store different actor types for unified lifecycle management
let controls: Vec<Box<dyn ActorControl>> = vec![
    (&worker_actor).into(),
    (&logger_actor).into(),
];

// Check status and stop all
for control in &controls {
    println!("Actor {} alive: {}", control.identity(), control.is_alive());
    control.stop().await?;
}
```

---

## Conversions

### From Trait Support

Handler traits, Control traits support conversions via the `From` trait:

| From | To | Description |
|------|----|-------------|
| `ActorRef<T>` | `Box<dyn TellHandler<M>>` | Ownership transfer |
| `&ActorRef<T>` | `Box<dyn TellHandler<M>>` | Clone reference |
| `ActorRef<T>` | `Box<dyn AskHandler<M, R>>` | Ownership transfer |
| `&ActorRef<T>` | `Box<dyn AskHandler<M, R>>` | Clone reference |
| `ActorRef<T>` | `Box<dyn ActorControl>` | Ownership transfer |
| `&ActorRef<T>` | `Box<dyn ActorControl>` | Clone reference |
| `ActorWeak<T>` | `Box<dyn WeakTellHandler<M>>` | Ownership transfer |
| `&ActorWeak<T>` | `Box<dyn WeakTellHandler<M>>` | Clone reference |
| `ActorWeak<T>` | `Box<dyn WeakAskHandler<M, R>>` | Ownership transfer |
| `&ActorWeak<T>` | `Box<dyn WeakAskHandler<M, R>>` | Clone reference |
| `ActorWeak<T>` | `Box<dyn WeakActorControl>` | Ownership transfer |
| `&ActorWeak<T>` | `Box<dyn WeakActorControl>` | Clone reference |

```rust
// Ownership transfer
let handler: Box<dyn TellHandler<Msg>> = actor_ref.into();

// Clone via reference (keeps original ActorRef)
let handler: Box<dyn TellHandler<Msg>> = (&actor_ref).into();
```

### Strong ↔ Weak Conversion

```rust
// Strong → Weak (downgrade)
let strong: Box<dyn TellHandler<Msg>> = (&actor).into();
let weak: Box<dyn WeakTellHandler<Msg>> = strong.downgrade();

// Weak → Strong (upgrade)
if let Some(strong) = weak.upgrade() {
    strong.tell(msg).await?;
}
```

---

## Clone and Debug Support

All `Box<dyn Handler>` types implement `Clone` and `Debug` traits:

```rust
// Clone
let handlers: Vec<Box<dyn TellHandler<Msg>>> = vec![...];
let cloned = handlers.clone();

// Debug
println!("{:?}", handlers[0]);
// Output: TellHandler { identity: actor-123, alive: true }
```

---

## Design Decisions

### 1. Separate Strong and Weak Traits

Strong Handlers and Weak Handlers are separate traits. This makes ownership semantics explicit and prevents confusion in actor lifetime management.

### 2. Weak Handlers are Upgrade-Only

Weak Handlers do not provide `tell`/`ask` methods directly:

- **Explicit**: Upgrade overhead is clearly visible in code
- **Efficient**: Single upgrade enables multiple operations
- **Consistent**: Matches `std::sync::Weak::upgrade()` pattern

### 3. Lifecycle Control via Control Traits

Lifecycle methods (`stop()`, `kill()`) are accessed via `as_control()` on Strong Handlers. Weak Handlers provide `as_weak_control()` for read-only access (`identity()`, `is_alive()`). Actor lifecycle control should only be performed through strong references.

### 4. clone_boxed Method

The `clone_boxed()` method enables cloning of trait objects, which in turn enables the `Clone` trait implementation for `Box<dyn Handler>`.

---

## Constraints

- **Same Reply Type**: All actors in an `AskHandler<M, R>` / `WeakAskHandler<M, R>` collection must have the same Reply type `R`.
- **BoxFuture Overhead**: Minor runtime overhead due to trait object usage.
- **Backward Compatible**: Fully compatible with existing `ActorRef` API; opt-in usage.

---

## Complete Example

See [examples/handler_demo.rs](../examples/handler_demo.rs) for a complete example:

```bash
cargo run --example handler_demo
```

```rust
use rsactor::{
    message_handlers, spawn, Actor, ActorRef,
    AskHandler, TellHandler, WeakTellHandler,
    ActorControl,
};

struct Ping { timestamp: u64 }
struct GetStatus;

#[derive(Debug, Clone)]
struct Status {
    name: String,
    message_count: u32,
}

#[derive(Actor)]
struct CounterActor {
    name: String,
    count: u32,
}

#[message_handlers]
impl CounterActor {
    #[handler]
    async fn handle_ping(&mut self, msg: Ping, _: &ActorRef<Self>) -> () {
        self.count += 1;
        println!("[{}] Ping: {}", self.name, msg.timestamp);
    }

    #[handler]
    async fn handle_get_status(&mut self, _: GetStatus, _: &ActorRef<Self>) -> Status {
        Status {
            name: self.name.clone(),
            message_count: self.count,
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (actor_a, _) = spawn::<CounterActor>(CounterActor {
        name: "A".into(),
        count: 0,
    });

    let (actor_b, _) = spawn::<CounterActor>(CounterActor {
        name: "B".into(),
        count: 0,
    });

    // Unified management with TellHandler
    let handlers: Vec<Box<dyn TellHandler<Ping>>> = vec![
        (&actor_a).into(),
        (&actor_b).into(),
    ];

    for handler in &handlers {
        handler.tell(Ping { timestamp: 1000 }).await?;
    }

    // Collect responses with AskHandler
    let ask_handlers: Vec<Box<dyn AskHandler<GetStatus, Status>>> = vec![
        (&actor_a).into(),
        (&actor_b).into(),
    ];

    for handler in &ask_handlers {
        let status = handler.ask(GetStatus).await?;
        println!("{}: {} messages", status.name, status.message_count);
    }

    Ok(())
}
```
