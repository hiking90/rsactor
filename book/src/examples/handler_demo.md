# Handler Demo

This example demonstrates how to use handler traits (`TellHandler`, `AskHandler`, `WeakTellHandler`, `WeakAskHandler`) to manage different actor types that handle the same message through a unified interface.

## Key Concepts

- **`TellHandler<M>`**: Type-erased fire-and-forget for any actor handling message `M`
- **`AskHandler<M, R>`**: Type-erased request-response for any actor returning `R` for message `M`
- **`WeakTellHandler<M>` / `WeakAskHandler<M, R>`**: Weak reference variants that don't keep actors alive
- **Heterogeneous collections**: Store different actor types in a single `Vec`

## Code Walkthrough

### Different actors, same messages

```rust
#[derive(Actor)]
struct CounterActor { name: String, count: u32 }

#[derive(Actor)]
struct LoggerActor { prefix: String, log_count: u32 }

// Both actors handle the same Ping and GetStatus messages
#[message_handlers]
impl CounterActor {
    #[handler]
    async fn handle_ping(&mut self, msg: Ping, _: &ActorRef<Self>) { ... }
    #[handler]
    async fn handle_get_status(&mut self, _: GetStatus, _: &ActorRef<Self>) -> Status { ... }
}

#[message_handlers]
impl LoggerActor {
    #[handler]
    async fn handle_ping(&mut self, msg: Ping, _: &ActorRef<Self>) { ... }
    #[handler]
    async fn handle_get_status(&mut self, _: GetStatus, _: &ActorRef<Self>) -> Status { ... }
}
```

### TellHandler — fire-and-forget

```rust
let tell_handlers: Vec<Box<dyn TellHandler<Ping>>> = vec![
    (&counter_actor).into(),
    (&logger_actor).into(),
];

for handler in &tell_handlers {
    handler.tell(Ping { timestamp: 1000 }).await?;
}
```

### AskHandler — request-response

```rust
let ask_handlers: Vec<Box<dyn AskHandler<GetStatus, Status>>> = vec![
    (&counter_actor).into(),
    (&logger_actor).into(),
];

for handler in &ask_handlers {
    let status = handler.ask(GetStatus).await?;
    println!("{}: {} messages", status.name, status.message_count);
}
```

### WeakTellHandler — upgrade pattern

```rust
let weak_handlers: Vec<Box<dyn WeakTellHandler<Ping>>> = vec![
    ActorRef::downgrade(&counter_actor).into(),
    ActorRef::downgrade(&logger_actor).into(),
];

for weak in &weak_handlers {
    if let Some(strong) = weak.upgrade() {
        strong.tell(Ping { timestamp: 3000 }).await?;
    }
}
```

### Downgrade strong to weak

```rust
let strong: Box<dyn TellHandler<Ping>> = (&counter_actor).into();
let weak: Box<dyn WeakTellHandler<Ping>> = strong.downgrade();
```

## Running

```bash
cargo run --example handler_demo
```
