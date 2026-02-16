# Metrics

rsActor provides optional per-actor performance metrics. Enable with the `metrics` feature flag:

```toml
[dependencies]
rsactor = { version = "0.14", features = ["metrics"] }
```

## Tracked Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `message_count` | `u64` | Total messages processed |
| `avg_processing_time` | `Duration` | Average message processing time |
| `max_processing_time` | `Duration` | Maximum processing time observed |
| `error_count` | `u64` | Total errors during message handling |
| `uptime` | `Duration` | Time since actor was started |
| `last_activity` | `Option<SystemTime>` | Timestamp of last message processing |

## Usage

### Snapshot

Get all metrics at once:

```rust
let metrics = actor_ref.metrics();
println!("Messages: {}", metrics.message_count);
println!("Avg time: {:?}", metrics.avg_processing_time);
println!("Max time: {:?}", metrics.max_processing_time);
println!("Errors: {}", metrics.error_count);
println!("Uptime: {:?}", metrics.uptime);
println!("Last activity: {:?}", metrics.last_activity);
```

### Individual Accessors

```rust
let count = actor_ref.message_count();
let avg = actor_ref.avg_processing_time();
let max = actor_ref.max_processing_time();
let errors = actor_ref.error_count();
let uptime = actor_ref.uptime();
let last = actor_ref.last_activity();
```

## Design

- **Lock-free**: Uses `AtomicU64` counters for zero-contention updates
- **Zero overhead when disabled**: When the `metrics` feature is not enabled, no metrics code is compiled
- **Automatic collection**: Metrics are collected transparently during message processing
- **Post-mortem analysis**: Metrics survive actor drop via `ActorWeak` (strong reference to metrics collector)

## Philosophy

rsActor exposes raw metrics data. How you monitor, alert, or visualize this data is your responsibility. You can integrate with any monitoring system (Prometheus, Grafana, custom dashboards, etc.).

## Running the Example

```bash
cargo run --example metrics_demo --features metrics
```
