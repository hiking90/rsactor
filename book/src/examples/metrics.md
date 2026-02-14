# Metrics Demo

This example demonstrates the per-actor metrics system for monitoring performance. Requires the `metrics` feature flag.

## Key Concepts

- **Lock-free counters**: Metrics use `AtomicU64` for zero-contention updates
- **Zero overhead when disabled**: No metrics code compiles without the feature flag
- **Automatic collection**: Metrics are gathered transparently during message processing
- **Snapshot API**: Get all metrics at once or query individual values

## Code Walkthrough

### Getting a metrics snapshot

```rust
let (actor_ref, handle) = spawn::<WorkerActor>(actor);

// Send some messages
for _ in 0..10 {
    actor_ref.tell(FastTask).await?;
}

// Get all metrics at once
let metrics = actor_ref.metrics();
println!("Message count: {}", metrics.message_count);
println!("Avg processing time: {:?}", metrics.avg_processing_time);
println!("Max processing time: {:?}", metrics.max_processing_time);
println!("Error count: {}", metrics.error_count);
println!("Uptime: {:?}", metrics.uptime);
println!("Last activity: {:?}", metrics.last_activity);
```

### Individual accessors

```rust
let count = actor_ref.message_count();
let avg = actor_ref.avg_processing_time();
let max = actor_ref.max_processing_time();
let errors = actor_ref.error_count();
let uptime = actor_ref.uptime();
let last = actor_ref.last_activity();
```

## Running

```bash
cargo run --example metrics_demo --features metrics
```

Without the `metrics` feature, the example still runs but skips metrics output.
