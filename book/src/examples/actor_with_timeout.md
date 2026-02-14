# Actor with Timeout

This example demonstrates how to handle timeouts in actor communication using `ask_with_timeout` and `tell_with_timeout`. Proper timeout handling is crucial for building resilient actor systems that can gracefully handle slow or unresponsive actors.

## Overview

Timeout handling addresses several important scenarios:
- **Slow operations** that might take longer than expected
- **Unresponsive actors** due to blocking or infinite loops
- **Network delays** in distributed systems
- **Resource contention** causing processing delays
- **Graceful degradation** when services are overloaded

## Key Methods

rsActor provides timeout-enabled communication methods:

```rust
// Request with timeout - fails if no response within duration
let result = actor_ref.ask_with_timeout(message, Duration::from_secs(5)).await;

// Fire-and-forget with timeout - fails if message can't be sent within duration
actor_ref.tell_with_timeout(message, Duration::from_millis(100)).await;
```

## Implementation

### Actor Definition

```rust
struct TimeoutDemoActor {
    name: String,
}

impl Actor for TimeoutDemoActor {
    type Args = String;
    type Error = anyhow::Error;

    async fn on_start(args: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        info!("{} actor (id: {}) started", args, actor_ref.identity());
        Ok(Self { name: args })
    }
}
```

### Message Types with Different Response Times

```rust
// Fast response message
struct FastQuery(String);

// Slow response message (simulates long processing)
struct SlowQuery(String);

// Configurable delay message
struct ConfigurableQuery {
    question: String,
    delay_ms: u64,
}
```

### Message Handlers

```rust
impl Message<FastQuery> for TimeoutDemoActor {
    type Reply = String;

    async fn handle(&mut self, msg: FastQuery, _: &ActorRef<Self>) -> Self::Reply {
        debug!("{} handling a FastQuery: {}", self.name, msg.0);
        // Immediate response
        format!("Fast response to: {}", msg.0)
    }
}

impl Message<SlowQuery> for TimeoutDemoActor {
    type Reply = String;

    async fn handle(&mut self, msg: SlowQuery, _: &ActorRef<Self>) -> Self::Reply {
        debug!("{} handling a SlowQuery: {}", self.name, msg.0);

        // Simulate slow processing
        tokio::time::sleep(Duration::from_secs(3)).await;

        format!("Slow response to: {}", msg.0)
    }
}

impl Message<ConfigurableQuery> for TimeoutDemoActor {
    type Reply = String;

    async fn handle(&mut self, msg: ConfigurableQuery, _: &ActorRef<Self>) -> Self::Reply {
        debug!("{} handling ConfigurableQuery with {}ms delay",
               self.name, msg.delay_ms);

        // Configurable delay
        tokio::time::sleep(Duration::from_millis(msg.delay_ms)).await;

        format!("Response after {}ms to: {}", msg.delay_ms, msg.question)
    }
}

// Wire up message handlers
impl_message_handler!(TimeoutDemoActor, [FastQuery, SlowQuery, ConfigurableQuery]);
```

## Timeout Scenarios

### 1. **Successful Fast Operation**

```rust
// This should succeed quickly
let fast_result = actor_ref
    .ask_with_timeout(FastQuery("Quick question".to_string()), Duration::from_secs(1))
    .await;

match fast_result {
    Ok(response) => println!("Fast query succeeded: {}", response),
    Err(e) => println!("Fast query failed: {}", e),
}
```

### 2. **Timeout on Slow Operation**

```rust
// This will timeout because SlowQuery takes 3 seconds but we only wait 1 second
let slow_result = actor_ref
    .ask_with_timeout(SlowQuery("Slow question".to_string()), Duration::from_secs(1))
    .await;

match slow_result {
    Ok(response) => println!("Slow query succeeded: {}", response),
    Err(e) => println!("Slow query timed out: {}", e), // This will be printed
}
```

### 3. **Configurable Timeout Testing**

```rust
async fn test_configurable_timeouts(actor_ref: &ActorRef<TimeoutDemoActor>) -> Result<()> {
    let test_cases = vec![
        (100, 500),  // 100ms delay, 500ms timeout - should succeed
        (800, 500),  // 800ms delay, 500ms timeout - should timeout
        (200, 1000), // 200ms delay, 1000ms timeout - should succeed
    ];

    for (delay_ms, timeout_ms) in test_cases {
        let query = ConfigurableQuery {
            question: format!("Test with {}ms delay", delay_ms),
            delay_ms,
        };

        let result = actor_ref
            .ask_with_timeout(query, Duration::from_millis(timeout_ms))
            .await;

        match result {
            Ok(response) => {
                println!("✅ Success ({}ms delay, {}ms timeout): {}",
                        delay_ms, timeout_ms, response);
            }
            Err(e) => {
                println!("❌ Timeout ({}ms delay, {}ms timeout): {}",
                        delay_ms, timeout_ms, e);
            }
        }
    }

    Ok(())
}
```

## Error Handling Patterns

### 1. **Timeout vs Other Errors**

```rust
match actor_ref.ask_with_timeout(message, timeout).await {
    Ok(response) => {
        // Handle successful response
        println!("Received: {}", response);
    }
    Err(rsactor::Error::Timeout { identity, duration }) => {
        // Handle timeout specifically
        println!("Actor {} timed out after {:?}", identity, duration);
        // Could implement retry logic, fallback, or circuit breaker
    }
    Err(other_error) => {
        // Handle other errors (send failures, actor crashes, etc.)
        println!("Communication error: {}", other_error);
    }
}
```

### 2. **Retry with Exponential Backoff**

```rust
async fn retry_with_backoff<T>(
    actor_ref: &ActorRef<TimeoutDemoActor>,
    message: T,
    max_retries: usize,
) -> Result<String>
where
    T: Clone + Send + 'static,
    TimeoutDemoActor: Message<T, Reply = String>,
{
    let mut delay = Duration::from_millis(100);

    for attempt in 0..max_retries {
        match actor_ref.ask_with_timeout(message.clone(), delay * 2).await {
            Ok(response) => return Ok(response),
            Err(rsactor::Error::Timeout { .. }) if attempt < max_retries - 1 => {
                println!("Attempt {} timed out, retrying...", attempt + 1);
                tokio::time::sleep(delay).await;
                delay *= 2; // Exponential backoff
            }
            Err(e) => return Err(e.into()),
        }
    }

    anyhow::bail!("All retry attempts failed")
}
```

### 3. **Circuit Breaker Pattern**

```rust
struct CircuitBreaker {
    failure_count: usize,
    failure_threshold: usize,
    last_failure: Option<std::time::Instant>,
    reset_timeout: Duration,
}

impl CircuitBreaker {
    fn new(failure_threshold: usize, reset_timeout: Duration) -> Self {
        Self {
            failure_count: 0,
            failure_threshold,
            last_failure: None,
            reset_timeout,
        }
    }

    fn should_attempt(&self) -> bool {
        if self.failure_count < self.failure_threshold {
            return true;
        }

        if let Some(last_failure) = self.last_failure {
            last_failure.elapsed() > self.reset_timeout
        } else {
            true
        }
    }

    fn on_success(&mut self) {
        self.failure_count = 0;
        self.last_failure = None;
    }

    fn on_failure(&mut self) {
        self.failure_count += 1;
        self.last_failure = Some(std::time::Instant::now());
    }
}
```

## Best Practices

### 1. **Choose Appropriate Timeouts**
- **Fast operations**: 100ms - 1s
- **Network operations**: 5s - 30s
- **Database queries**: 1s - 10s
- **Heavy computations**: 30s - 5min

### 2. **Graceful Degradation**
```rust
async fn get_user_data_with_fallback(
    actor_ref: &ActorRef<UserActor>,
    user_id: u64,
) -> UserData {
    match actor_ref
        .ask_with_timeout(GetUser(user_id), Duration::from_secs(2))
        .await
    {
        Ok(user_data) => user_data,
        Err(_) => {
            // Return cached or default data
            UserData::default_for_user(user_id)
        }
    }
}
```

### 3. **Monitoring and Metrics**
```rust
async fn monitored_ask<T, R>(
    actor_ref: &ActorRef<T>,
    message: impl Send + 'static,
    timeout: Duration,
    operation_name: &str,
) -> Result<R>
where
    T: Actor + Message<impl Send + 'static, Reply = R>,
{
    let start = std::time::Instant::now();

    let result = actor_ref.ask_with_timeout(message, timeout).await;

    let duration = start.elapsed();

    match &result {
        Ok(_) => {
            metrics::histogram!("actor_request_duration", duration, "operation" => operation_name, "status" => "success");
        }
        Err(rsactor::Error::Timeout { .. }) => {
            metrics::histogram!("actor_request_duration", duration, "operation" => operation_name, "status" => "timeout");
        }
        Err(_) => {
            metrics::histogram!("actor_request_duration", duration, "operation" => operation_name, "status" => "error");
        }
    }

    result.map_err(Into::into)
}
```

## Running the Example

```bash
cargo run --example actor_with_timeout
```

You'll see output demonstrating:
- Fast queries completing successfully
- Slow queries timing out
- Different timeout scenarios
- Error handling patterns

## When to Use Timeouts

- **User-facing operations** that need responsive feedback
- **Dependent services** that might become unavailable
- **Resource-intensive operations** that could hang
- **Network communications** subject to delays
- **Any operation** where infinite waiting is unacceptable

Proper timeout handling is essential for building robust, responsive actor systems that gracefully handle real-world operational challenges.
