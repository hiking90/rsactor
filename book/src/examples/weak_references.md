# Weak Reference Demo

This example demonstrates `ActorWeak` for weak references to actors — references that don't prevent the actor from being dropped.

## Key Concepts

- **`ActorRef::downgrade()`**: Create a weak reference from a strong reference
- **`weak_ref.upgrade()`**: Try to get a strong reference back (returns `None` if actor is dead)
- **`weak_ref.is_alive()`**: Heuristic check if the actor might still be alive
- **`weak_ref.identity()`**: Always available, even after the actor is dropped

## Code Walkthrough

### Creating and using weak references

```rust
let (actor_ref, join_handle) = spawn::<PingActor>("TestActor".to_string());

// Create a weak reference
let weak_ref = ActorRef::downgrade(&actor_ref);
println!("Weak ref is_alive: {}", weak_ref.is_alive());

// Upgrade and use
if let Some(strong_ref) = weak_ref.upgrade() {
    let response: String = strong_ref.ask(Ping).await?;
    println!("Response: {response}");
}
```

### Behavior after dropping strong reference

```rust
drop(actor_ref);
tokio::time::sleep(Duration::from_millis(100)).await;

// Weak reference reflects the actor's state
println!("is_alive: {}", weak_ref.is_alive());

if let Some(strong) = weak_ref.upgrade() {
    // Actor might still be running if other strong refs exist
    let _ = strong.ask(Ping).await;
} else {
    println!("Actor is no longer available");
}
```

### Identity survives actor death

```rust
// Identity is always available, even after actor termination
println!("Actor ID: {}", weak_ref.identity());
```

## Running

```bash
cargo run --example weak_reference_demo
```
