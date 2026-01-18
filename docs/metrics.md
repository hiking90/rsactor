# rsactor Metrics Guide

The rsactor Metrics system provides functionality for monitoring actor performance and health.

## Enabling the Feature

To use the metrics functionality, enable the `metrics` feature flag.

```toml
# Cargo.toml
[dependencies]
rsactor = { version = "0.11", features = ["metrics"] }
```

## Basic Usage

### Querying Metrics

Access actor metrics through `ActorRef`.

```rust
use rsactor::{message_handlers, spawn, Actor, ActorRef, MetricsSnapshot};
use std::time::Duration;

#[derive(Actor)]
struct MyActor;

struct Work;

#[message_handlers]
impl MyActor {
    #[handler]
    async fn handle_work(&mut self, _msg: Work, _ctx: &ActorRef<Self>) {
        // Message processing logic
    }
}

#[tokio::main]
async fn main() {
    let (actor_ref, _handle) = spawn::<MyActor>(());

    // Send messages
    for _ in 0..100 {
        actor_ref.tell(Work).await.unwrap();
    }

    // Wait for processing
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Query metrics
    let metrics = actor_ref.metrics();
    println!("Messages processed: {}", metrics.message_count);
    println!("Avg processing time: {:?}", metrics.avg_processing_time);
    println!("Max processing time: {:?}", metrics.max_processing_time);
    println!("Error count: {}", metrics.error_count);
    println!("Uptime: {:?}", metrics.uptime);
    println!("Last activity: {:?}", metrics.last_activity);
}
```

### Individual Accessor Methods

You can also query individual metrics instead of the full snapshot.

```rust
// Query individual metrics
let message_count = actor_ref.message_count();
let avg_time = actor_ref.avg_processing_time();
let max_time = actor_ref.max_processing_time();
let error_count = actor_ref.error_count();
let uptime = actor_ref.uptime();
let last_activity = actor_ref.last_activity();
```

## API Reference

### MetricsSnapshot

`MetricsSnapshot` is an immutable struct containing actor metrics at a point in time.

| Field | Type | Description |
|-------|------|-------------|
| `message_count` | `u64` | Total number of messages processed |
| `avg_processing_time` | `Duration` | Average processing time per message |
| `max_processing_time` | `Duration` | Maximum processing time observed |
| `error_count` | `u64` | Number of errors during message handling |
| `uptime` | `Duration` | Time elapsed since actor started |
| `last_activity` | `Option<SystemTime>` | Timestamp of last message processing completion |

### ActorRef Metrics Methods

| Method | Return Type | Description |
|--------|-------------|-------------|
| `metrics()` | `MetricsSnapshot` | Full metrics snapshot |
| `message_count()` | `u64` | Number of messages processed |
| `avg_processing_time()` | `Duration` | Average processing time |
| `max_processing_time()` | `Duration` | Maximum processing time |
| `error_count()` | `u64` | Error count |
| `uptime()` | `Duration` | Actor uptime |
| `last_activity()` | `Option<SystemTime>` | Last activity timestamp |

## Usage Examples

### Calculating Error Rate

```rust
let metrics = actor_ref.metrics();
let error_rate = if metrics.message_count > 0 {
    (metrics.error_count as f64 / metrics.message_count as f64) * 100.0
} else {
    0.0
};
println!("Error rate: {:.2}%", error_rate);
```

### Performance Monitoring

```rust
let metrics = actor_ref.metrics();

// Detect slow message handlers
if metrics.max_processing_time > Duration::from_secs(1) {
    eprintln!(
        "Warning: Max processing time exceeds 1 second: {:?}",
        metrics.max_processing_time
    );
}

// Calculate throughput
let throughput = if metrics.uptime.as_secs() > 0 {
    metrics.message_count as f64 / metrics.uptime.as_secs_f64()
} else {
    0.0
};
println!("Throughput: {:.2} msg/sec", throughput);
```

### Periodic Monitoring

```rust
use std::time::Duration;

async fn monitor_actor(actor_ref: ActorRef<MyActor>) {
    while actor_ref.is_alive() {
        tokio::time::sleep(Duration::from_secs(10)).await;

        let m = actor_ref.metrics();
        println!(
            "[Monitor] messages={} avg={:?} max={:?} errors={} uptime={:?}",
            m.message_count,
            m.avg_processing_time,
            m.max_processing_time,
            m.error_count,
            m.uptime
        );
    }
}
```

### Querying Metrics After Actor Termination

Metrics remain accessible through `ActorRef` even after the actor has stopped.

```rust
let (actor_ref, handle) = spawn::<MyActor>(());

// Process messages
actor_ref.tell(Work).await?;

// Stop the actor
actor_ref.stop().await?;
let _ = handle.await;

// Metrics are still accessible after stop
let final_metrics = actor_ref.metrics();
println!("Final message count: {}", final_metrics.message_count);
```

## Design Principles

### Lock-free Implementation

All metrics use `AtomicU64` for lock-free operations, minimizing performance overhead.

### Zero-cost Abstraction

When the `metrics` feature is disabled, all related code is completely excluded from compilation. If metrics are not needed in production, disable the feature to eliminate any overhead.

### Eventual Consistency

During concurrent reads/writes, individual fields are consistent, but the snapshot as a whole may not reflect exactly the same point in time. This is sufficient for monitoring purposes and is an intentional design choice to maintain lock-free performance.

### Overflow Protection

- `total_processing_nanos` uses saturating addition to prevent overflow
- Individual durations are capped at `u64::MAX` nanoseconds (~584 years)

## Running the Example

```bash
# Basic execution
cargo run --example metrics_demo --features metrics

# With tracing enabled
RUST_LOG=debug cargo run --example metrics_demo --features "metrics tracing"
```

## Building Without the Feature Flag

When building without the `metrics` feature, metrics-related APIs do not exist.

```rust
// Compilation error without metrics feature
// actor_ref.metrics(); // Method does not exist

// Handle with conditional compilation
#[cfg(feature = "metrics")]
{
    let metrics = actor_ref.metrics();
    println!("Messages: {}", metrics.message_count);
}

#[cfg(not(feature = "metrics"))]
println!("Metrics feature is not enabled");
```

## Monitoring System Integration

rsactor only exposes metrics. Integration with monitoring systems like Prometheus, Grafana, or DataDog is the user's responsibility.

```rust
// Example: Converting to Prometheus format
fn to_prometheus_format(actor_name: &str, m: &MetricsSnapshot) -> String {
    format!(
        r#"# HELP actor_message_count Total messages processed
# TYPE actor_message_count counter
actor_message_count{{actor="{}"}} {}
# HELP actor_processing_time_avg Average processing time in seconds
# TYPE actor_processing_time_avg gauge
actor_processing_time_avg{{actor="{}"}} {}
# HELP actor_processing_time_max Max processing time in seconds
# TYPE actor_processing_time_max gauge
actor_processing_time_max{{actor="{}"}} {}
# HELP actor_error_count Total errors
# TYPE actor_error_count counter
actor_error_count{{actor="{}"}} {}
"#,
        actor_name, m.message_count,
        actor_name, m.avg_processing_time.as_secs_f64(),
        actor_name, m.max_processing_time.as_secs_f64(),
        actor_name, m.error_count
    )
}
```
