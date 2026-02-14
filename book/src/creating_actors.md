# Creating Actors

`rsActor` provides flexible ways to define and spawn actors. The core of actor creation revolves around implementing the `Actor` trait and then using spawn functions to bring actors to life.

## Actor Creation Methods

There are several approaches to creating actors in rsActor:

### 1. Manual Implementation
Implement the `Actor` trait manually for full control over initialization and lifecycle:

```rust
struct DatabaseActor {
    connection: DbConnection,
}

impl Actor for DatabaseActor {
    type Args = String; // connection string
    type Error = anyhow::Error;

    async fn on_start(conn_str: Self::Args, _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        let connection = DbConnection::connect(&conn_str).await?;
        Ok(Self { connection })
    }
}
```

### 2. Derive Macro
Use `#[derive(Actor)]` for simple actors that don't need complex initialization:

```rust
#[derive(Actor)]
struct CacheActor {
    data: HashMap<String, String>,
}
```

## Spawning Actors

Once you've defined an actor, spawn it using one of these functions:

```rust
// Basic spawning
let (actor_ref, join_handle) = spawn(my_actor_args);

// Spawning with custom mailbox capacity
let (actor_ref, join_handle) = spawn_with_mailbox_capacity(my_actor_args, 1000);
```

## Actor Categories

### Basic Actors
Simple actors that process messages and maintain state without complex background tasks.

### Async Actors
Actors that perform continuous asynchronous work using the `on_run` lifecycle method, such as:
- Periodic tasks with timers
- Network I/O operations
- Database queries
- File system operations

### Blocking Task Actors
Actors designed to handle CPU-intensive or blocking operations that might block the async runtime:
- Heavy computations
- Synchronous I/O
- Legacy blocking APIs
- CPU-bound algorithms

## Key Concepts

### Actor Arguments (`Args`)
The `Args` associated type defines what data is needed to initialize your actor:
- `Self` - Pass the entire actor struct as initialization data
- Custom types - Define specific initialization parameters
- `()` - No initialization data needed

### Error Handling
The `Error` associated type defines how initialization failures are handled:
- `anyhow::Error` - General error handling
- `std::convert::Infallible` - For actors that never fail to initialize
- Custom error types - Domain-specific error handling

### Lifecycle Integration
Different actor types leverage lifecycle methods differently:
- `on_start`: Always required for initialization
- `on_run`: Optional, used for background tasks and periodic work
- `on_stop`: Optional, used for cleanup when the actor shuts down

This section covers these different actor patterns and when to use each approach.
