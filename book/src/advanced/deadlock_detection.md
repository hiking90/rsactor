# Deadlock Detection

rsActor provides runtime deadlock detection that catches circular `ask` dependencies **before** they cause your application to hang.

## Why Deadlocks Happen

Each actor processes messages sequentially in a single loop. When an actor calls `ask`, it pauses its loop to wait for the reply. If Actor A asks Actor B, and B simultaneously asks A, both loops are paused — neither can process the other's request.

```text
Actor A handler: actor_ref_b.ask(msg).await  ← A's loop paused, waiting for B
Actor B handler: actor_ref_a.ask(msg).await  ← B's loop paused, waiting for A
→ Both wait forever = deadlock
```

This can also happen with:
- **Self-ask**: An actor asking itself
- **Indirect chains**: A → B → C → A

## Enabling Detection

Add the `deadlock-detection` feature:

```toml
[dependencies]
rsactor = { version = "0.14", features = ["deadlock-detection"] }
```

When disabled, all detection code is removed at compile time — **zero overhead** in production.

### Auto-Enabling in Debug Builds

Cargo does not support enabling features based on build profile (debug vs release). Here are practical ways to keep detection active during development while excluding it from release builds.

#### Cargo Aliases (Recommended)

Define aliases in `.cargo/config.toml`:

```toml
# .cargo/config.toml
[alias]
dev-run = "run --features deadlock-detection"
dev-test = "test --features deadlock-detection"
```

```bash
cargo dev-run                    # debug + deadlock detection
cargo dev-test                   # test + deadlock detection
cargo run --release              # release, no overhead
```

#### Makefile / justfile

Centralize dev commands with the feature flag included:

```makefile
# Makefile
run:
	cargo run --features deadlock-detection

test:
	cargo test --features deadlock-detection

release:
	cargo build --release
```

#### build.rs Guardrail

For a compile-time reminder, add a `build.rs` that detects debug profile and a `compile_error!` that fires when the feature is missing:

```toml
# Cargo.toml
[features]
deadlock-detection = ["rsactor/deadlock-detection"]
```

```rust
// build.rs
fn main() {
    let profile = std::env::var("PROFILE").unwrap();
    println!("cargo::rustc-check-cfg=cfg(debug_build)");
    if profile == "debug" {
        println!("cargo:rustc-cfg=debug_build");
    }
}
```

```rust
// main.rs — reminds you to enable the feature in debug
#[cfg(all(debug_build, not(feature = "deadlock-detection")))]
compile_error!(
    "Enable deadlock detection in debug builds: \
     cargo run --features deadlock-detection"
);
```

> **Note:** `build.rs` cannot activate Cargo features — it can only set `cfg` flags. The `compile_error!` approach acts as a guardrail that reminds developers to pass the feature flag.

For full details, see the [Deadlock Detection Guide](../../../docs/deadlock_detection.md).

## How It Works

The framework maintains a global **wait-for graph**:

1. Before each `ask`, an edge `caller → callee` is registered
2. The graph is checked for cycles
3. If a cycle exists, the framework **panics immediately** with a descriptive message
4. When `ask` completes, the edge is automatically removed

```text
1. A asks B → graph: {A → B} → no cycle → proceed
2. B asks A → graph: {A → B, B → A} → cycle detected! → panic!
```

### Panic Message

Deadlocks are design errors, so the framework panics (like an index out of bounds) rather than returning an error:

```text
Deadlock detected: ask cycle MyActorA(#1) -> MyActorB(#2) -> MyActorA(#1)
This is a design error. Use `tell` to break the cycle,
or restructure actor dependencies.
```

The message includes actor type names and IDs, making it easy to identify the problematic interaction.

## Fixing Deadlocks

### Option 1: Replace `ask` with `tell`

If one direction doesn't need a synchronous reply, use `tell` (fire-and-forget):

```rust
// Before: A and B ask each other (deadlock risk)
#[handler]
async fn handle_request(&mut self, msg: Request, _: &ActorRef<Self>) -> Response {
    let result = self.other_actor.ask(Query).await?;
    Response(result)
}

// After: A tells B, no waiting (deadlock-free)
#[handler]
async fn handle_request(&mut self, msg: Request, _: &ActorRef<Self>) {
    self.other_actor.tell(Notify { data: msg.data }).await?;
}
```

### Option 2: Restructure Dependencies

Eliminate cycles by introducing a mediator:

```text
Before: A ↔ B (bidirectional ask = cycle risk)
After:  A → Mediator ← B (no cycles possible)
```

### Option 3: Use Callbacks

Pass a `oneshot::Sender` via `tell` instead of using `ask`:

```rust
use tokio::sync::oneshot;

struct RequestWithCallback {
    data: String,
    reply_tx: oneshot::Sender<Response>,
}

// Send via tell with a reply channel
let (tx, rx) = oneshot::channel();
actor_b.tell(RequestWithCallback { data, reply_tx: tx }).await?;
let response = rx.await?;
```

## What Gets Detected

| Scenario | Detected? |
|----------|-----------|
| Self-ask (A → A) | Yes |
| 2-actor cycle (A → B → A) | Yes |
| N-actor chain (A → B → C → A) | Yes |
| Sequential asks (A → B, then A → C) | No false positive |
| Non-actor caller (main → A) | Skipped (safe) |

## Limitations

- **`blocking_ask`**: Not tracked, as it's designed for non-actor contexts that can't form cycles.
- **Concurrent asks**: Using `tokio::join!` to send multiple `ask` calls from one handler may miss some cycles, since the graph stores one edge per caller.
- **`tokio::spawn` in handlers**: Spawned tasks don't inherit the actor identity, so their `ask` calls aren't tracked.

## Performance

| Condition | Overhead |
|-----------|----------|
| Feature disabled | Zero (compile-time removal) |
| Feature enabled, normal `ask` | ~100-500 ns per call |

The wait-for graph only contains in-flight `ask` calls, so it stays small and traversal is fast.
