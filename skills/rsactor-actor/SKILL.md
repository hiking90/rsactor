# rsactor-actor

Create a new rsactor actor with proper structure and patterns.

## When to use

Use this skill when the user wants to create a new actor using the rsactor framework.

## Actor Creation Patterns

### Pattern 1: Simple Actor (Recommended for most cases)

Use `#[derive(Actor)]` for actors without complex initialization:

```rust
use rsactor::{Actor, ActorRef, message_handlers};

// Define message types
struct MyMessage {
    data: String,
}

// Simple actor using derive macro
#[derive(Actor)]
struct MyActor {
    state: String,
    count: u32,
}

#[message_handlers]
impl MyActor {
    #[handler]
    async fn handle_my_message(&mut self, msg: MyMessage, _: &ActorRef<Self>) -> String {
        self.count += 1;
        format!("Processed: {}", msg.data)
    }
}
```

**Spawning:**
```rust
let (actor_ref, join_handle) = rsactor::spawn::<MyActor>(MyActor {
    state: "initial".to_string(),
    count: 0,
});
```

### Pattern 2: Actor with Custom Initialization

Use manual `Actor` trait implementation when you need custom `on_start` logic:

```rust
use rsactor::{Actor, ActorRef, ActorWeak, message_handlers};
use anyhow::Result;

struct MyActor {
    connection: DatabaseConnection,  // Resource that needs initialization
    count: u32,
}

// Arguments passed to spawn()
struct MyActorArgs {
    db_url: String,
}

impl Actor for MyActor {
    type Args = MyActorArgs;
    type Error = anyhow::Error;

    async fn on_start(args: Self::Args, _actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        // Complex initialization logic
        let connection = DatabaseConnection::connect(&args.db_url).await?;
        Ok(Self {
            connection,
            count: 0,
        })
    }

    // Optional: background task that runs concurrently with message processing
    async fn on_run(&mut self, _actor_ref: &ActorWeak<Self>) -> Result<(), Self::Error> {
        // Called repeatedly while actor is alive
        // Use tokio::select! for multiple async operations
        Ok(())
    }

    // Optional: cleanup when actor stops
    async fn on_stop(&mut self, _actor_ref: &ActorWeak<Self>) -> Result<(), Self::Error> {
        self.connection.close().await?;
        Ok(())
    }
}

#[message_handlers]
impl MyActor {
    #[handler]
    async fn handle_query(&mut self, msg: Query, _: &ActorRef<Self>) -> QueryResult {
        // Handle message
    }
}
```

### Pattern 3: Actor with Periodic Tasks

Use `on_run` with `tokio::select!` for timers and async operations:

```rust
use rsactor::{Actor, ActorRef, ActorWeak, message_handlers};
use tokio::time::{interval, Duration};

struct PeriodicActor {
    tick_interval: tokio::time::Interval,
    tick_count: u32,
}

impl Actor for PeriodicActor {
    type Args = Self;
    type Error = anyhow::Error;

    async fn on_start(args: Self::Args, _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(args)
    }

    async fn on_run(&mut self, _: &ActorWeak<Self>) -> Result<(), Self::Error> {
        tokio::select! {
            _ = self.tick_interval.tick() => {
                self.tick_count += 1;
                println!("Tick #{}", self.tick_count);
            }
        }
        Ok(())
    }
}
```

**Spawning:**
```rust
let (actor_ref, handle) = rsactor::spawn::<PeriodicActor>(PeriodicActor {
    tick_interval: interval(Duration::from_secs(1)),
    tick_count: 0,
});
```

## Key Rules

1. **Message types**: Define as simple structs, no trait implementation needed
2. **Handler methods**: Use `#[handler]` attribute, method name doesn't matter
3. **Handler signature**: `async fn name(&mut self, msg: MsgType, actor_ref: &ActorRef<Self>) -> ReturnType`
4. **Return type**: `()` for fire-and-forget (tell), any type for request-response (ask)
5. **Error handling**: Use `Self::Error` type in Actor trait, typically `anyhow::Error`

## Spawning and Communication

```rust
// Spawn actor
let (actor_ref, join_handle) = rsactor::spawn::<MyActor>(args);

// Fire-and-forget (tell)
actor_ref.tell(MyMessage { data: "hello".into() }).await?;

// Request-response (ask)
let response: String = actor_ref.ask(MyMessage { data: "hello".into() }).await?;

// Stop actor gracefully
actor_ref.stop().await?;

// Wait for actor completion
let result = join_handle.await?;
match result {
    rsactor::ActorResult::Completed { actor, killed } => { /* success */ }
    rsactor::ActorResult::Failed { actor, error, phase, killed } => { /* failure */ }
}
```

## Common Imports

```rust
use rsactor::{
    Actor,           // Core trait
    ActorRef,        // Strong reference for sending messages
    ActorWeak,       // Weak reference (used in on_run, on_stop)
    ActorResult,     // Result of actor lifecycle
    message_handlers, // Macro for handler impl block
    spawn,           // Function to create actors
    // For polymorphic handler collections
    TellHandler, AskHandler, WeakTellHandler, WeakAskHandler,
    // For type-erased lifecycle management
    ActorControl, WeakActorControl,
};
```
