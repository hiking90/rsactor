# Timeouts

Timeouts are essential for building resilient actor systems. rsActor provides timeout functionality for both message sending (`tell`) and request-response patterns (`ask`). This prevents your application from hanging indefinitely when actors become unresponsive or slow.

## Overview

rsActor provides two timeout-enabled communication methods:

- **`ask_with_timeout`**: Sends a message and waits for a reply within a specified timeout
- **`tell_with_timeout`**: Sends a fire-and-forget message with a timeout for the send operation

Both methods use `tokio::time::Duration` to specify timeout values and return a `Result` that will contain a `Timeout` error if the operation exceeds the specified duration.

## `ask_with_timeout`

The `ask_with_timeout` method is similar to the regular `ask` method, but allows you to specify a timeout for the entire request-response cycle.

### Syntax

```rust
pub async fn ask_with_timeout<M>(&self, msg: M, timeout: Duration) -> Result<T::Reply>
where
    T: Message<M>,
    M: Send + 'static,
    T::Reply: Send + 'static,
```

### Usage

```rust
use rsactor::{spawn, Actor, ActorRef, Message, impl_message_handler};
use std::time::Duration;
use anyhow::Result;

#[derive(Debug)]
struct MathActor;

impl Actor for MathActor {
    type Args = ();
    type Error = anyhow::Error;

    async fn on_start(_args: Self::Args, _ar: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(MathActor)
    }
}

struct MultiplyMsg(i32, i32);

impl Message<MultiplyMsg> for MathActor {
    type Reply = i32;

    async fn handle(&mut self, msg: MultiplyMsg, _ar: &ActorRef<Self>) -> Self::Reply {
        msg.0 * msg.1
    }
}

impl_message_handler!(MathActor, [MultiplyMsg]);

#[tokio::main]
async fn main() -> Result<()> {
    let (math_ref, _join_handle) = spawn::<MathActor>(());

    // Ask with timeout - will succeed if the actor responds within 1 second
    match math_ref.ask_with_timeout(MultiplyMsg(6, 7), Duration::from_secs(1)).await {
        Ok(product) => println!("Product: {}", product),
        Err(e) => println!("Failed to get product: {:?}", e),
    }

    Ok(())
}
```

### When `ask_with_timeout` Times Out

If the actor doesn't respond within the specified timeout, you'll receive a `Timeout` error:

```rust
// This will likely timeout if the actor takes longer than 10ms to respond
match math_ref.ask_with_timeout(MultiplyMsg(1, 2), Duration::from_millis(10)).await {
    Ok(result) => println!("Result: {}", result),
    Err(e) => {
        if e.to_string().contains("timed out") {
            println!("Request timed out!");
        } else {
            println!("Other error: {}", e);
        }
    }
}
```

## `tell_with_timeout`

The `tell_with_timeout` method is similar to the regular `tell` method, but allows you to specify a timeout for the send operation itself.

### Syntax

```rust
pub async fn tell_with_timeout<M>(&self, msg: M, timeout: Duration) -> Result<()>
where
    T: Message<M>,
    M: Send + 'static,
```

### Usage

```rust
use rsactor::{spawn, Actor, ActorRef, Message, impl_message_handler};
use std::time::Duration;
use anyhow::Result;
use log::info;

#[derive(Debug)]
struct EventActor;

impl Actor for EventActor {
    type Args = ();
    type Error = anyhow::Error;

    async fn on_start(_args: Self::Args, _ar: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(EventActor)
    }
}

struct EventMessage(String);

impl Message<EventMessage> for EventActor {
    type Reply = ();

    async fn handle(&mut self, msg: EventMessage, _ar: &ActorRef<Self>) -> Self::Reply {
        println!("EVENT: {}", msg.0);
    }
}

impl_message_handler!(EventActor, [EventMessage]);

#[tokio::main]
async fn main() -> Result<()> {
    let (event_ref, _join_handle) = spawn::<EventActor>(());

    // Tell with timeout - will fail if the message can't be enqueued within 100ms
    match event_ref.tell_with_timeout(
        EventMessage("System started".to_string()),
        Duration::from_millis(100)
    ).await {
        Ok(_) => info!("Event message sent successfully"),
        Err(e) => info!("Failed to send event message: {:?}", e),
    }

    Ok(())
}
```

### When `tell_with_timeout` Times Out

`tell_with_timeout` typically only times out if:
- The actor's mailbox is full and cannot accept new messages
- The actor has terminated and can no longer receive messages
- There are system-level issues preventing message delivery

In most cases, `tell_with_timeout` completes quickly since sending a message is usually a fast operation.

## Practical Example: Slow Actor with Timeouts

Here's a comprehensive example demonstrating timeout behavior with actors that simulate different response times:

```rust
use rsactor::{message_handlers, spawn, Actor, ActorRef};
use std::time::Duration;
use anyhow::Result;
use tracing::info;

// Define an actor that can process requests with varying response times
struct TimeoutDemoActor {
    name: String,
}

impl Actor for TimeoutDemoActor {
    type Args = String;
    type Error = anyhow::Error;

    async fn on_start(name: Self::Args, _ar: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(Self { name })
    }
}

struct FastQuery(String);
struct SlowQuery(String);

#[message_handlers]
impl TimeoutDemoActor {
    #[handler]
    async fn handle_fast_query(&mut self, msg: FastQuery, _: &ActorRef<Self>) -> String {
        // Fast response - completes immediately
        format!("{}: Fast response to: {}", self.name, msg.0)
    }

    #[handler]
    async fn handle_slow_query(&mut self, msg: SlowQuery, _: &ActorRef<Self>) -> String {
        // Slow response - takes 500ms to complete
        tokio::time::sleep(Duration::from_millis(500)).await;
        format!("{}: Slow response to: {}", self.name, msg.0)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let (actor_ref, _join_handle) = spawn::<TimeoutDemoActor>("Demo".to_string());

    // Fast query with sufficient timeout - should succeed
    info!("=== Fast query with long timeout ===");
    match actor_ref.ask_with_timeout(
        FastQuery("What is your name?".to_string()),
        Duration::from_millis(100)
    ).await {
        Ok(response) => info!("Success: {}", response),
        Err(e) => info!("Failed: {}", e),
    }

    // Slow query with insufficient timeout - should fail
    info!("=== Slow query with short timeout ===");
    match actor_ref.ask_with_timeout(
        SlowQuery("Complex calculation".to_string()),
        Duration::from_millis(100)  // Less than 500ms needed
    ).await {
        Ok(response) => info!("Success: {}", response),
        Err(e) => info!("Failed: {}", e),
    }

    // Slow query with sufficient timeout - should succeed
    info!("=== Slow query with sufficient timeout ===");
    match actor_ref.ask_with_timeout(
        SlowQuery("Another calculation".to_string()),
        Duration::from_millis(1000)  // More than 500ms needed
    ).await {
        Ok(response) => info!("Success: {}", response),
        Err(e) => info!("Failed: {}", e),
    }

    Ok(())
}
```

## Best Practices

### 1. Choose Appropriate Timeout Values

- **Too short**: May cause unnecessary failures due to normal processing delays
- **Too long**: May not protect against truly unresponsive actors
- **Consider your use case**: Interactive UI vs batch processing will have different timeout requirements

```rust
// For interactive applications
let timeout = Duration::from_millis(100);

// For backend processing
let timeout = Duration::from_secs(30);

// For critical real-time systems
let timeout = Duration::from_millis(10);
```

### 2. Handle Timeout Errors Gracefully

Always handle timeout errors appropriately for your application:

```rust
match actor_ref.ask_with_timeout(msg, timeout).await {
    Ok(response) => {
        // Process successful response
        handle_response(response);
    }
    Err(e) if e.to_string().contains("timed out") => {
        // Handle timeout specifically
        log::warn!("Actor response timed out, using fallback");
        use_fallback_value();
    }
    Err(e) => {
        // Handle other errors
        log::error!("Actor communication failed: {}", e);
        return Err(e);
    }
}
```

### 3. Consider Retries with Backoff

For important operations, consider implementing retry logic:

```rust
use tokio::time::{sleep, Duration};

async fn ask_with_retries<T, M>(
    actor_ref: &ActorRef<T>,
    msg: M,
    timeout: Duration,
    max_retries: u32
) -> Result<T::Reply>
where
    T: Actor + Message<M>,
    M: Send + Clone + 'static,
    T::Reply: Send + 'static,
{
    for attempt in 1..=max_retries {
        match actor_ref.ask_with_timeout(msg.clone(), timeout).await {
            Ok(response) => return Ok(response),
            Err(e) if attempt == max_retries => return Err(e),
            Err(e) => {
                log::warn!("Attempt {} failed: {}", attempt, e);
                sleep(Duration::from_millis(100 * attempt as u64)).await;
            }
        }
    }
    unreachable!()
}
```

### 4. Use Timeouts in Supervision Strategies

Timeouts are particularly useful in supervision scenarios where you need to detect and handle unresponsive child actors:

```rust
// Supervisor checking child actor health
match child_ref.ask_with_timeout(HealthCheck, Duration::from_secs(5)).await {
    Ok(_) => {
        // Child is responsive
    }
    Err(_) => {
        // Child is unresponsive, consider restarting
        log::warn!("Child actor unresponsive, restarting...");
        restart_child_actor().await?;
    }
}
```

## Error Types

When a timeout occurs, you'll receive an error that contains information about:
- The actor's identity
- The timeout duration that was exceeded
- The operation that timed out ("ask" or "tell")

This information helps with debugging and monitoring actor system health.

## Performance Considerations

- Timeouts add minimal overhead to message passing
- Very short timeouts (< 1ms) may be unreliable due to system scheduling
- Consider your system's typical response times when setting timeouts
- Monitor timeout rates to detect system performance issues

Timeouts are a crucial tool for building resilient actor systems that can handle failures gracefully and maintain responsive behavior even when some actors become slow or unresponsive.
