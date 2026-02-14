# Weak References

`ActorWeak<T>` is a weak reference to an actor that does not prevent the actor from being dropped. It is used to avoid circular references and memory leaks in complex actor graphs.

## Creating Weak References

Weak references are created by calling `ActorRef::downgrade()`:

```rust
use rsactor::{ActorRef, ActorWeak};

let weak_ref: ActorWeak<MyActor> = ActorRef::downgrade(&actor_ref);
```

## Upgrading to Strong References

A weak reference can be upgraded back to a strong `ActorRef` if the actor is still alive:

```rust
if let Some(strong_ref) = weak_ref.upgrade() {
    // Actor is still alive, send a message
    strong_ref.tell(MyMessage).await?;
} else {
    // Actor has been dropped
    println!("Actor is no longer alive");
}
```

## Checking Liveness

```rust
// Heuristic check — may return true even if upgrade would fail (race condition)
if weak_ref.is_alive() {
    // Actor might be alive, but always use upgrade() for certainty
}

// The identity is always available, even after the actor is dropped
println!("Actor ID: {}", weak_ref.identity());
```

## Use in Lifecycle Methods

`on_run` and `on_stop` receive `ActorWeak` instead of `ActorRef`. This prevents the actor from holding a strong reference to itself, which would prevent graceful shutdown:

```rust
impl Actor for MyActor {
    // ...

    async fn on_run(&mut self, actor_weak: &ActorWeak<Self>) -> Result<bool, Self::Error> {
        // Use weak reference for identity/logging
        println!("Actor {} processing", actor_weak.identity());

        // Upgrade if you need to send messages to self
        if let Some(strong) = actor_weak.upgrade() {
            // Use strong_ref for operations that need ActorRef
        }
        Ok(false)
    }

    async fn on_stop(&mut self, actor_weak: &ActorWeak<Self>, killed: bool) -> Result<(), Self::Error> {
        println!("Actor {} stopping (killed: {})", actor_weak.identity(), killed);
        Ok(())
    }
}
```

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

### Observer Pattern

Use weak references to observers so they don't prevent the subject from being dropped:

```rust
struct EventBus {
    listeners: Vec<ActorWeak<ListenerActor>>,
}

impl EventBus {
    async fn notify_all(&mut self, event: Event) {
        // Retain only alive listeners
        self.listeners.retain(|weak| weak.is_alive());

        for weak in &self.listeners {
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
