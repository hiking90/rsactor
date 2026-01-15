# rsactor-handler

Add message handlers to an existing rsactor actor.

## When to use

Use this skill when the user wants to add new message handling capabilities to an existing actor.

## Adding Handlers

### Step 1: Define the Message Type

```rust
// Message for fire-and-forget (tell)
struct Ping;

// Message for request-response (ask)
struct GetStatus;

// Message with data
struct UpdateConfig {
    key: String,
    value: String,
}
```

### Step 2: Add Handler Method

Add the handler inside the `#[message_handlers]` impl block:

```rust
#[message_handlers]
impl MyActor {
    // Existing handlers...

    // NEW: Fire-and-forget handler (returns nothing)
    #[handler]
    async fn handle_ping(&mut self, _msg: Ping, _: &ActorRef<Self>) {
        println!("Received ping");
    }

    // NEW: Request-response handler (returns a value)
    #[handler]
    async fn handle_get_status(&mut self, _msg: GetStatus, _: &ActorRef<Self>) -> Status {
        Status {
            name: self.name.clone(),
            count: self.count,
        }
    }

    // NEW: Handler with message data
    #[handler]
    async fn handle_update_config(&mut self, msg: UpdateConfig, _: &ActorRef<Self>) -> bool {
        self.config.insert(msg.key, msg.value);
        true
    }
}
```

## Handler Patterns

### Pattern 1: Fire-and-Forget (tell)

No return value, caller doesn't wait for completion:

```rust
// Definition
#[handler]
async fn handle_log(&mut self, msg: LogMessage, _: &ActorRef<Self>) {
    println!("[{}] {}", msg.level, msg.text);
}

// Usage
actor_ref.tell(LogMessage { level: "INFO", text: "Hello" }).await?;
```

### Pattern 2: Request-Response (ask)

Returns a value, caller awaits the response:

```rust
// Definition
#[handler]
async fn handle_query(&mut self, msg: Query, _: &ActorRef<Self>) -> QueryResult {
    self.database.query(&msg.sql).await
}

// Usage
let result: QueryResult = actor_ref.ask(Query { sql: "SELECT *".into() }).await?;
```

### Pattern 3: Self-Messaging

Actor sends message to itself using the ActorRef parameter:

```rust
#[handler]
async fn handle_start_batch(&mut self, msg: StartBatch, actor_ref: &ActorRef<Self>) {
    for item in msg.items {
        // Send message to self
        let _ = actor_ref.tell(ProcessItem { item }).await;
    }
}
```

### Pattern 4: Handler with Error

Return `Result` for fallible operations:

```rust
#[handler]
async fn handle_save(&mut self, msg: SaveData, _: &ActorRef<Self>) -> Result<(), SaveError> {
    self.storage.save(&msg.data).map_err(SaveError::StorageError)
}
```

## Handler Traits for Polymorphism

When you need to store different actor types that handle the same message:

```rust
use rsactor::{TellHandler, AskHandler};

// Store handlers for the same message type
let handlers: Vec<Box<dyn TellHandler<Ping>>> = vec![
    (&actor_a).into(),
    (&actor_b).into(),
];

// Send to all (access lifecycle via as_control())
for handler in &handlers {
    println!("Sending to actor: {}", handler.as_control().identity());
    handler.tell(Ping).await?;
}

// For ask pattern
let ask_handlers: Vec<Box<dyn AskHandler<GetStatus, Status>>> = vec![
    (&actor_a).into(),
    (&actor_b).into(),
];

// Access lifecycle control
for handler in &ask_handlers {
    if handler.as_control().is_alive() {
        let status = handler.ask(GetStatus).await?;
    }
}
```

## Weak Handlers

For avoiding reference cycles or optional actor references:

```rust
use rsactor::{WeakTellHandler, WeakAskHandler, ActorRef};

let weak_handler: Box<dyn WeakTellHandler<Ping>> = ActorRef::downgrade(&actor_ref).into();

// Access identity via as_weak_control()
println!("Weak handler identity: {}", weak_handler.as_weak_control().identity());

// Must upgrade before use
if let Some(strong) = weak_handler.upgrade() {
    strong.tell(Ping).await?;
}
```

## ActorControl for Lifecycle Management

When you only need lifecycle control without message sending:

```rust
use rsactor::ActorControl;

// Store different actor types for lifecycle management only
let controls: Vec<Box<dyn ActorControl>> = vec![
    (&actor_a).into(),
    (&actor_b).into(),
];

// Check status and stop all
for control in &controls {
    println!("Actor {} alive: {}", control.identity(), control.is_alive());
    control.stop().await?;
}
```

## Key Rules

1. **One message type per handler**: Each `#[handler]` method handles exactly one message type
2. **Method name is arbitrary**: `handle_foo`, `on_foo`, `process_foo` - all work
3. **Signature must match**: `async fn(&mut self, msg: T, &ActorRef<Self>) -> R`
4. **Return type determines usage**: `()` = tell only, other types = ask supported
5. **Thread safety**: Handlers run sequentially, no concurrent access to `&mut self`
