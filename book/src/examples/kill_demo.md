# Kill Demo

This example demonstrates the difference between graceful stop and immediate kill, and how to inspect `ActorResult` after termination.

## Key Concepts

- **`stop()`**: Graceful shutdown — allows current message processing to finish, then calls `on_stop(killed: false)`
- **`kill()`**: Immediate termination — calls `on_stop(killed: true)` without waiting for pending messages
- **`ActorResult`**: Inspect how the actor terminated (completed vs failed, stopped vs killed)

## Code Walkthrough

### Actor with on_stop

```rust
use rsactor::{message_handlers, Actor, ActorRef, ActorWeak};

#[derive(Debug)]
struct DemoActor {
    name: String,
}

impl Actor for DemoActor {
    type Args = String;
    type Error = anyhow::Error;

    async fn on_start(args: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        tracing::info!("DemoActor '{}' (id: {}) started!", args, actor_ref.identity());
        Ok(DemoActor { name: args })
    }

    async fn on_stop(&mut self, actor_ref: &ActorWeak<Self>, killed: bool) -> Result<(), Self::Error> {
        let status = if killed { "killed" } else { "stopped" };
        tracing::info!("DemoActor '{}' (id: {}) {}", self.name, actor_ref.identity(), status);
        Ok(())
    }
}
```

### Inspecting ActorResult

```rust
let (actor_ref, join_handle) = rsactor::spawn::<DemoActor>("TestActor".to_string());

// Kill the actor
actor_ref.kill()?;

// Inspect the result
match join_handle.await? {
    rsactor::ActorResult::Completed { actor, killed } => {
        println!("Actor '{}' completed. Killed: {}", actor.name, killed);
    }
    rsactor::ActorResult::Failed { error, phase, killed, .. } => {
        println!("Actor failed: {}. Phase: {:?}, Killed: {}", error, phase, killed);
    }
}
```

## Running

```bash
cargo run --example kill_demo
```
