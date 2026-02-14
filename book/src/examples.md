# Examples

This section provides practical examples demonstrating various features and patterns of the rsActor framework.

## Available Examples

### Core Patterns
- **[Basic Usage](examples/basic.md)** — Simple counter actor with tell and ask
- **[Async Worker](examples/async_worker.md)** — Actor performing periodic async work
- **[Blocking Task](examples/blocking_task.md)** — CPU-intensive work in blocking contexts
- **[Actor with Timeout](examples/actor_with_timeout.md)** — Timeout handling in actor communication

### Advanced Patterns
- **[Kill Demo](examples/kill_demo.md)** — Graceful stop vs immediate kill, ActorResult inspection
- **[Ask Join](examples/ask_join.md)** — ask_join pattern with JoinHandle
- **[Handler Demo](examples/handler_demo.md)** — Handler traits for type-erased heterogeneous collections
- **[Weak References](examples/weak_references.md)** — ActorWeak for breaking circular references
- **[Metrics](examples/metrics.md)** — Per-actor performance metrics
- **[Tracing](examples/tracing.md)** — Structured tracing and observability

### Classic Problems
- **[Dining Philosophers](examples/dining_philosophers.md)** — Classic concurrency problem solved with actors

## Running Examples

All examples are in the `examples/` directory:

```bash
# Core examples
cargo run --example basic
cargo run --example actor_async_worker
cargo run --example actor_blocking_task

# Advanced examples
cargo run --example kill_demo
cargo run --example ask_join_demo
cargo run --example handler_demo
cargo run --example weak_reference_demo

# Feature-gated examples
cargo run --example metrics_demo --features metrics
cargo run --example tracing_demo --features tracing
```

## Example Structure

Each example follows this pattern:

1. **Actor Definition**: Define the actor struct and implement the `Actor` trait (or use `#[derive(Actor)]`)
2. **Message Types**: Define messages that the actor can handle
3. **Message Handlers**: Use `#[message_handlers]` with `#[handler]` attributes
4. **Usage**: Demonstrate spawning, messaging, and lifecycle management
