# Weak References

`ActorWeak<T>` is a weak reference to an actor. It does not count toward the strong references that keep an actor running: the actor stops once the last `ActorRef<T>` is dropped, even while `ActorWeak`s remain. Use it wherever holding a strong reference would form a keep-alive cycle — two actors referencing each other, or an actor referencing itself.

## Creating Weak References

Weak references are created by calling `downgrade()` on an `ActorRef`, either as a method or as an associated function:

```rust
use rsactor::{ActorRef, ActorWeak};

let weak_ref: ActorWeak<MyActor> = actor_ref.downgrade();
let weak_ref: ActorWeak<MyActor> = ActorRef::downgrade(&actor_ref);
```

## Upgrading to Strong References

`upgrade()` returns `Some(ActorRef<T>)` while at least one strong `ActorRef` exists and the actor's runtime loop has not yet exited, and `None` otherwise:

```rust
if let Some(strong_ref) = weak_ref.upgrade() {
    // A strong reference existed and the runtime loop had not exited.
    // This does not guarantee the message is processed (see below).
    strong_ref.tell(MyMessage).await?;
} else {
    // No strong reference is left, or the runtime loop has exited;
    // `on_stop` may still be running.
    println!("Actor is stopping or has stopped");
}
```

Neither result is a statement about the actor value itself:

- `None` does not mean the actor has been dropped. Once the last strong reference is gone, `on_stop` can still be running.
- `Some` does not mean a message will be processed. After `stop()` or `kill()` has been observed, a `tell` through the returned reference can still return `Ok` and the message is then discarded at shutdown (recorded as a dead letter).

`upgrade()` also returns `None` once the runtime loop has exited — after a graceful stop, a kill, or a lifecycle failure — even if strong `ActorRef` clones still linger elsewhere, for example in a registry.

The returned `ActorRef` keeps the actor from terminating by ref-drop for as long as you hold it, but not from `stop()`, `kill()`, or a lifecycle failure.

## Checking Liveness

`is_alive()` performs the same check as `upgrade()` without creating a strong reference:

```rust
// `false` means the actor is definitely stopped or stopping: every strong reference
// has been dropped, or its runtime loop has exited.
// `true` is a point-in-time answer: the actor can stop before your next call.
if weak_ref.is_alive() {
    // To act on the actor, upgrade — and still handle the `None` case.
}

// The identity is always available, even after the actor has stopped
println!("Actor ID: {}", weak_ref.identity());
```

Because the actor can stop between two calls, `is_alive()` followed by `upgrade()` can disagree; always handle `upgrade()` returning `None`. Neither call guarantees that a later message is processed. If you need proof that the actor handled something, use `ask` and check the reply.

With the `metrics` feature enabled, `ActorWeak::metrics()` keeps returning the actor's metrics snapshot after the actor has fully stopped, so a retained weak reference can be used for post-mortem analysis.

## Use in Lifecycle Methods

`on_idle` and `on_stop` receive `&ActorWeak<Self>`; `on_start` and message handlers receive `&ActorRef<Self>`. After `on_start` returns, the runtime drops its own strong reference and keeps only an `ActorWeak`, so the actor stops (with `on_stop(killed = false)`) once every `ActorRef` outside the actor has been dropped.

Your code must follow the same rule. Do not store a strong clone of the actor's own `ActorRef` in its state, and do not capture one in a stream passed to `subscribe_idle`. The runtime task owns both the actor value and its subscribed streams, so a strong self-reference there is never dropped: ref-drop termination can never happen. The actor then ends only through another path, such as `stop()` or `kill()` called from the actor's own code or through a reference upgraded from an `ActorWeak` kept elsewhere; if nothing takes such a path, the actor task runs for the life of the runtime. Store `actor_ref.downgrade()` instead and upgrade at each use.

The following actor uses the `ActorWeak` it receives in `on_idle` for logging, and upgrades it only when it needs an `ActorRef` to send itself a message:

```rust
use futures::stream::StreamExt;
use rsactor::{message_handlers, spawn_with_options, Actor, ActorRef, ActorWeak, SpawnOptions};
use std::time::Duration;
use tokio_stream::wrappers::IntervalStream;

struct Beat;

struct Heartbeat {
    beats: u32,
}

impl Actor for Heartbeat {
    type Args = ();
    type Error = anyhow::Error;
    type IdleEvent = ();

    async fn on_start(_: (), actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        // The stream captures no reference to this actor, so it creates no cycle.
        actor_ref.subscribe_idle(
            IntervalStream::new(tokio::time::interval(Duration::from_secs(1))).map(|_| ()),
        )?;
        Ok(Heartbeat { beats: 0 })
    }

    async fn on_idle(&mut self, _: (), actor_weak: &ActorWeak<Self>) -> Result<(), Self::Error> {
        // Use the weak reference for identity/logging
        println!("Actor {} tick", actor_weak.identity());

        // Upgrade only when an ActorRef is needed. `None` here means every
        // external ActorRef has been dropped and the actor is about to stop.
        if let Some(actor_ref) = actor_weak.upgrade() {
            // A plain `tell` to the actor's own full mailbox from inside the actor
            // deadlocks; bound the wait instead.
            if let Err(e) = actor_ref.tell_with_timeout(Beat, Duration::from_millis(100)).await {
                println!("self-send failed: {e}");
            }
        }
        Ok(())
    }

    async fn on_stop(&mut self, actor_weak: &ActorWeak<Self>, killed: bool) -> Result<(), Self::Error> {
        println!(
            "Actor {} stopping (killed: {}, beats: {})",
            actor_weak.identity(),
            killed,
            self.beats
        );
        Ok(())
    }
}

#[message_handlers]
impl Heartbeat {
    #[handler]
    async fn handle_beat(&mut self, _: Beat, _: &ActorRef<Self>) {
        self.beats += 1;
    }
}

// `on_idle` is driven only when the idle channel is enabled at spawn time.
let (actor_ref, join_handle) = spawn_with_options::<Heartbeat>((), SpawnOptions::new().with_idle());
```

Inside `on_stop`, do not rely on `upgrade()` to reach the actor itself. When the stop was caused by the last `ActorRef` being dropped, it returns `None`. After `stop()` or `kill()`, it can return `Some` while external references exist, but messages sent through it are discarded at shutdown rather than handled.

## Common Patterns

### Avoiding Circular References

When actors reference each other, use weak references to break cycles:

```rust
struct ParentActor {
    children: Vec<ActorRef<ChildActor>>,  // Strong refs — parent owns children
}

struct ChildActor {
    parent: ActorWeak<ParentActor>,  // Weak ref — child doesn't keep parent alive
}
```

If the child held an `ActorRef<ParentActor>` as well, each actor would hold a strong reference to the other, so neither would ever reach zero strong references and neither would stop by ref-drop; only an explicit `stop()` or `kill()` would end them. The child upgrades `parent` at each use and treats `None` as "the parent is stopping or has stopped".

### Observer Pattern

Hold observers by weak reference so that registering with the bus does not keep a listener running. When the last `ActorRef` to a listener is dropped elsewhere, the listener stops, and the bus drops its entry on the next notification:

```rust
struct EventBus {
    listeners: Vec<ActorWeak<ListenerActor>>,
}

impl EventBus {
    async fn notify_all(&mut self, event: Event) {
        // Drop entries for listeners that have stopped or are stopping
        self.listeners.retain(|weak| weak.is_alive());

        for weak in &self.listeners {
            // A listener can stop between `retain` and here, so handle `None`
            if let Some(strong) = weak.upgrade() {
                let _ = strong.tell(event.clone()).await;
            }
        }
    }
}
```

## Running the Example

```bash
cargo run --example weak_reference_demo
```
