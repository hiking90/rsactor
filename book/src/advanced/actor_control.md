# Actor Control

The `ActorControl` trait provides **type-erased lifecycle management** for actors. It allows you to manage different actor types through a unified interface without knowing their message types.

## Use Case

When you need to manage a collection of actors of different types (e.g., stopping all actors during shutdown), `ActorControl` lets you store them in a single `Vec` and control their lifecycle uniformly.

## ActorControl — Strong Reference

```rust
use rsactor::{ActorControl, ActorRef, spawn, Actor};

// Store different actor types in a single collection
let controls: Vec<Box<dyn ActorControl>> = vec![
    (&worker_ref).into(),   // ActorRef<WorkerActor>
    (&logger_ref).into(),   // ActorRef<LoggerActor>
    (&cache_ref).into(),    // ActorRef<CacheActor>
];

// Check status of all actors
for control in &controls {
    println!("Actor {} alive: {}", control.identity(), control.is_alive());
}

// Stop all actors gracefully
for control in &controls {
    control.stop().await?;
}
```

## Available Methods

| Method | Description |
|--------|-------------|
| `identity()` | Returns the actor's unique `Identity` |
| `is_alive()` | Checks if the actor is still running |
| `stop()` | Gracefully stops the actor (async) |
| `kill()` | Immediately terminates the actor |
| `downgrade()` | Creates a `Box<dyn WeakActorControl>` |
| `clone_boxed()` | Clones the control reference |

## WeakActorControl — Weak Reference

`WeakActorControl` does not keep actors alive and requires upgrading before performing operations:

```rust
use rsactor::{WeakActorControl, ActorRef};

let weak_controls: Vec<Box<dyn WeakActorControl>> = vec![
    ActorRef::downgrade(&worker_ref).into(),
    ActorRef::downgrade(&logger_ref).into(),
];

for control in &weak_controls {
    println!("Actor {} might be alive: {}", control.identity(), control.is_alive());

    if let Some(strong) = control.upgrade() {
        strong.stop().await?;
    }
}
```

## Conversion

`ActorControl` is automatically implemented for all `ActorRef<T>` types:

```rust
// From ownership transfer
let control: Box<dyn ActorControl> = actor_ref.into();

// From reference (clones)
let control: Box<dyn ActorControl> = (&actor_ref).into();

// Similarly for weak references
let weak: Box<dyn WeakActorControl> = ActorRef::downgrade(&actor_ref).into();
```

## Combining with Handler Traits

Handler traits (`TellHandler`, `AskHandler`) also provide access to `ActorControl` via `as_control()`:

```rust
let handler: Box<dyn TellHandler<PingMsg>> = (&actor_ref).into();

// Access lifecycle through handler
let control = handler.as_control();
if control.is_alive() {
    handler.tell(PingMsg).await?;
}
```
