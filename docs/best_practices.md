# rsActor Best Practices

This document outlines best practices for building robust and efficient actor-based applications with rsActor.

## Actor Design Principles

### 1. Keep Actors Focused and Single-Purpose

Each actor should have a clear, single responsibility. Avoid creating "god actors" that handle too many different concerns.

```rust
// Good: Focused actor with single responsibility
#[derive(Actor)]
struct DatabaseConnection {
    pool: sqlx::Pool<sqlx::Postgres>,
}

// Bad: Actor trying to do everything
struct Application {
    db_pool: sqlx::Pool<sqlx::Postgres>,
    web_server: warp::Server,
    cache: HashMap<String, String>,
    logger: Logger,
    metrics: MetricsCollector,
}
```

### 2. Design Clear Message Protocols

Define clear, typed messages that express intent rather than implementation details.

```rust
// Good: Clear, intention-revealing messages
struct CreateUser { name: String, email: String }
struct GetUserById(UserId);
struct UpdateUserEmail { user_id: UserId, new_email: String }

// Bad: Generic, unclear messages
struct DatabaseQuery(String);
struct GenericCommand { action: String, data: serde_json::Value }
```

### 3. Use Appropriate Message Types

Choose between `tell` (fire-and-forget) and `ask` (request-reply) based on your needs:

```rust
// Use `tell` for notifications and commands that don't need responses
actor_ref.tell(LogEvent("User logged in".to_string())).await?;
actor_ref.tell(UpdateCache { key, value }).await?;

// Use `ask` when you need a response
let user = actor_ref.ask(GetUserById(user_id)).await?;
let result = actor_ref.ask(ValidateInput(input)).await?;
```

## Actor Lifecycle Management

### 1. Proper Resource Initialization in `on_start`

Use `on_start` for all resource initialization that might fail:

```rust
impl Actor for DatabaseActor {
    type Args = DatabaseConfig;
    type Error = anyhow::Error;

    async fn on_start(config: Self::Args, _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        // Initialize resources that might fail
        let pool = sqlx::PgPool::connect(&config.database_url).await?;

        // Run migrations or health checks
        sqlx::migrate!("./migrations").run(&pool).await?;

        Ok(DatabaseActor { pool })
    }
}
```

### 2. Implement Proper Cleanup in `on_stop`

Always clean up resources in `on_stop`, especially when dealing with external systems:

```rust
impl Actor for FileProcessor {
    // ... other methods ...

    async fn on_stop(&mut self, _: &ActorWeak<Self>, killed: bool) -> Result<(), Self::Error> {
        if killed {
            // Minimal cleanup for forced termination
            self.temp_files.clear();
        } else {
            // Thorough cleanup for graceful shutdown
            self.flush_pending_writes().await?;
            self.close_file_handles().await?;
            self.cleanup_temp_files().await?;
        }
        Ok(())
    }
}
```

### 3. Use `on_run` for Idle Processing

The `on_run` method is an idle handler called when the message queue is empty. Use it for background tasks:

```rust
async fn on_run(&mut self, actor_weak: &ActorWeak<Self>) -> Result<bool, Self::Error> {
    tokio::select! {
        // Handle periodic cleanup
        _ = self.cleanup_interval.tick() => {
            self.cleanup_expired_entries().await?;
        }

        // Handle health checks
        _ = self.health_check_interval.tick() => {
            if !self.is_healthy().await? {
                // Consider stopping the actor or alerting
                let actor_ref = actor_weak.upgrade()
                    .ok_or_else(|| anyhow::anyhow!("Actor reference no longer valid"))?;
                actor_ref.stop().await?;
                return Ok(false); // Stop idle processing
            }
        }
    }
    Ok(true) // Continue idle processing
}
```

## Error Handling

### 1. Use Appropriate Error Types

Define meaningful error types for your actors:

```rust
#[derive(Debug, thiserror::Error)]
enum DatabaseError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(#[from] sqlx::Error),
    #[error("User not found: {id}")]
    UserNotFound { id: UserId },
    #[error("Validation failed: {message}")]
    ValidationFailed { message: String },
}

impl Actor for DatabaseActor {
    type Error = DatabaseError;
    // ...
}
```

### 2. Handle Actor Failures Gracefully

Always check `ActorResult` when joining actor handles:

```rust
let (actor_ref, join_handle) = spawn::<MyActor>(args);

// ... use actor ...

actor_ref.stop().await?;

match join_handle.await? {
    ActorResult::Completed { actor, killed } => {
        println!("Actor completed successfully");
        // Access final actor state if needed
    }
    ActorResult::Failed { error, phase, actor, killed } => {
        match phase {
            FailurePhase::OnStart => {
                eprintln!("Actor failed to start: {}", error);
                // No actor state available
            }
            FailurePhase::OnRun => {
                eprintln!("Actor failed during runtime: {}", error);
                // Actor state available for inspection/recovery
                if let Some(actor_state) = actor {
                    // Inspect or recover state
                }
            }
            FailurePhase::OnStop => {
                eprintln!("Actor failed during cleanup: {}", error);
                // Actor state available
            }
        }
    }
}
```

## Performance Optimization

### 1. Choose Appropriate Mailbox Sizes

Consider your actor's message throughput when choosing mailbox capacity:

```rust
// For high-throughput actors
let (actor_ref, join_handle) = spawn_with_mailbox_capacity::<HighThroughputActor>(args, 10000);

// For low-throughput actors (default is usually fine)
let (actor_ref, join_handle) = spawn::<LowThroughputActor>(args);
```

### 2. Batch Operations When Possible

For actors that handle many small operations, consider batching:

```rust
#[message_handlers]
impl DatabaseActor {
    #[handler]
    async fn handle_batch_insert(&mut self, msg: BatchInsert, _: &ActorRef<Self>) -> Result<(), DatabaseError> {
        // More efficient than individual inserts
        let mut tx = self.pool.begin().await?;
        for item in msg.items {
            sqlx::query("INSERT INTO items (data) VALUES (?)")
                .bind(&item.data)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}
```

### 3. Use Timeouts Appropriately

Set reasonable timeouts for operations that might hang:

```rust
// Use timeouts for operations that might fail or hang
let result = tokio::time::timeout(
    Duration::from_secs(30),
    actor_ref.ask(SlowOperation)
).await??;

// Or use the built-in timeout methods
let result = actor_ref.ask_with_timeout(
    SlowOperation,
    Duration::from_secs(30)
).await?;
```

## Testing Strategies

### 1. Test Individual Actors in Isolation

```rust
#[tokio::test]
async fn test_counter_actor() {
    let (actor_ref, _join_handle) = spawn::<CounterActor>(CounterActor { count: 0 });

    // Test increment
    actor_ref.tell(Increment).await.unwrap();
    let count = actor_ref.ask(GetCount).await.unwrap();
    assert_eq!(count, 1);

    // Test multiple increments
    for _ in 0..5 {
        actor_ref.tell(Increment).await.unwrap();
    }
    let count = actor_ref.ask(GetCount).await.unwrap();
    assert_eq!(count, 6);

    actor_ref.stop().await.unwrap();
}
```

### 2. Test Error Scenarios

```rust
#[tokio::test]
async fn test_actor_initialization_failure() {
    let invalid_config = DatabaseConfig {
        database_url: "invalid://url".to_string(),
    };

    let (_, join_handle) = spawn::<DatabaseActor>(invalid_config);

    match join_handle.await.unwrap() {
        ActorResult::Failed { phase: FailurePhase::OnStart, .. } => {
            // Expected failure
        }
        other => panic!("Expected initialization failure, got: {:?}", other),
    }
}
```

### 3. Use Mock Actors for Integration Tests

```rust
// Create a mock actor for testing
#[derive(Actor)]
struct MockDatabaseActor {
    responses: HashMap<String, String>,
}

#[message_handlers]
impl MockDatabaseActor {
    #[handler]
    async fn handle_query(&mut self, msg: Query, _: &ActorRef<Self>) -> Option<String> {
        self.responses.get(&msg.sql).cloned()
    }
}
```

## Monitoring and Observability

### 1. Enable Tracing in Production

```toml
[dependencies]
rsactor = { version = "0.12", features = ["tracing"] }
```

### 2. Implement Health Checks

```rust
struct HealthCheck;

#[message_handlers]
impl MyActor {
    #[handler]
    async fn handle_health_check(&mut self, _: HealthCheck, _: &ActorRef<Self>) -> bool {
        // Check if actor is healthy
        self.is_connected() && self.queue_size() < 1000
    }
}
```

### 3. Monitor Actor Lifecycle

```rust
impl Actor for MonitoredActor {
    async fn on_start(args: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        tracing::info!("Actor {} starting", actor_ref.identity());
        // ... initialization ...
    }

    async fn on_stop(&mut self, actor_weak: &ActorWeak<Self>, killed: bool) -> Result<(), Self::Error> {
        tracing::warn!("Actor {} stopping (killed: {})", actor_weak.identity(), killed);
        Ok(())
    }
}
```

## Common Anti-Patterns to Avoid

### 1. Don't Share Mutable State Between Actors

```rust
// Bad: Sharing mutable state
let shared_map = Arc<Mutex<HashMap<String, String>>>::new();
let actor1 = MyActor { shared_data: shared_map.clone() };
let actor2 = MyActor { shared_data: shared_map.clone() };

// Good: Let one actor own the state, others communicate via messages
let (data_actor_ref, _) = spawn::<DataActor>(DataActor::new());
let actor1 = MyActor { data_actor: data_actor_ref.clone() };
let actor2 = MyActor { data_actor: data_actor_ref.clone() };
```

### 2. Don't Block in Message Handlers

```rust
// Bad: Blocking operations in handlers
#[handler]
async fn handle_slow_operation(&mut self, _: SlowOperation, _: &ActorRef<Self>) -> String {
    std::thread::sleep(Duration::from_secs(10)); // Blocks the entire actor!
    "done".to_string()
}

// Good: Use async operations or spawn_blocking
#[handler]
async fn handle_slow_operation(&mut self, _: SlowOperation, _: &ActorRef<Self>) -> String {
    tokio::task::spawn_blocking(|| {
        // CPU-intensive work here
        "done".to_string()
    }).await.unwrap()
}
```

### 3. Don't Ignore Actor Termination

```rust
// Bad: Fire-and-forget actor creation
let (actor_ref, _) = spawn::<MyActor>(args); // join_handle ignored

// Good: Proper lifecycle management
let (actor_ref, join_handle) = spawn::<MyActor>(args);

// ... use actor ...

// Always handle termination
actor_ref.stop().await?;
let result = join_handle.await?;
match result {
    ActorResult::Completed { .. } => println!("Actor completed successfully"),
    ActorResult::Failed { error, .. } => eprintln!("Actor failed: {}", error),
}
```

By following these best practices, you'll build more robust, maintainable, and efficient actor-based applications with rsActor.
