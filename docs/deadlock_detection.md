# Deadlock Detection

rsActor provides runtime deadlock detection for `ask` cycles. When enabled, the framework detects circular `ask` dependencies **before** they cause an actual deadlock, preventing your application from hanging.

## Table of Contents

- [The Problem](#the-problem)
- [How It Works](#how-it-works)
- [Enabling Deadlock Detection](#enabling-deadlock-detection)
- [Detected Scenarios](#detected-scenarios)
- [Panic Behavior](#panic-behavior)
- [How to Fix Deadlocks](#how-to-fix-deadlocks)
- [Limitations](#limitations)
- [Performance Impact](#performance-impact)

---

## The Problem

rsActor actors process messages sequentially in a single message loop. When an actor uses `ask` inside a handler, it awaits the target actor's reply while its own message loop is paused. If two actors `ask` each other simultaneously, both message loops stall — neither can process the incoming request because they're waiting for a reply from the other.

```
Actor A handler: actor_ref_b.ask(msg).await  ← waiting for B's reply, A's loop paused
Actor B handler: actor_ref_a.ask(msg).await  ← waiting for A's reply, B's loop paused
→ Both actors wait forever = deadlock
```

The simplest form is a self-ask:

```
Actor A handler: actor_ref_a.ask(msg).await  ← waiting for own reply
                                                but own loop is paused = deadlock
```

This problem applies to all lifecycle hooks (`on_start`, message handlers, `on_run`, `on_stop`) — any `ask` call from within an actor context can participate in a cycle.

## How It Works

rsActor uses a **wait-for graph**, a classic technique from database deadlock detection:

1. **Before** each `ask` call, the framework registers an edge in a global graph: `caller → callee`
2. It checks if adding this edge creates a cycle
3. If a cycle is found, it **panics immediately** instead of entering the deadlock
4. When `ask` completes (reply received), the edge is automatically removed via a drop guard

The graph is protected by a single `Mutex` to ensure the check-and-insert operation is atomic, preventing race conditions where two actors could simultaneously pass the check and then both enter a deadlock.

### Caller Identification

The framework uses `tokio::task_local!` to track which actor is currently executing. This is set automatically for all lifecycle hooks. Non-actor callers (e.g., `main` function, regular async tasks) are excluded from detection since they cannot participate in cycles — no other actor can `ask` them back.

## Enabling Deadlock Detection

Add the `deadlock-detection` feature flag:

```toml
[dependencies]
rsactor = { version = "0.14", features = ["deadlock-detection"] }
```

When the feature is disabled, all detection code is removed at compile time via `#[cfg(feature = "deadlock-detection")]` — there is **zero overhead** in production builds.

### Auto-Enabling in Debug Builds

Cargo does not natively support enabling features based on build profile. Here are practical approaches to automatically activate deadlock detection during development while keeping it disabled in release builds.

#### Approach 1: Cargo Aliases (Recommended)

Define aliases in `.cargo/config.toml` so development commands automatically include the feature:

```toml
# .cargo/config.toml
[alias]
dev-run = "run --features deadlock-detection"
dev-test = "test --features deadlock-detection"
```

Usage:

```bash
cargo dev-run                    # debug + deadlock detection
cargo dev-test                   # test + deadlock detection
cargo run --release              # release, no detection overhead
```

This is the simplest approach and requires no build script changes.

#### Approach 2: Makefile / justfile

Centralize all development commands in a Makefile or [justfile](https://github.com/casey/just):

```makefile
# Makefile
DEADLOCK_FLAGS = --features deadlock-detection

run:
	cargo run $(DEADLOCK_FLAGS)

test:
	cargo test $(DEADLOCK_FLAGS)

release:
	cargo build --release
```

```just
# justfile
run:
    cargo run --features deadlock-detection

test:
    cargo test --features deadlock-detection

release:
    cargo build --release
```

#### Approach 3: build.rs Auto-Detection

For fully automatic activation, add a `build.rs` to your application crate that emits a custom cfg based on the build profile, combined with a crate-level feature that activates rsactor's deadlock detection:

```toml
# Cargo.toml
[features]
deadlock-detection = ["rsactor/deadlock-detection"]

[dependencies]
rsactor = "0.14"
```

```rust
// build.rs
fn main() {
    // Cargo sets PROFILE to "debug" or "release"
    let profile = std::env::var("PROFILE").unwrap();

    println!("cargo::rustc-check-cfg=cfg(debug_build)");

    if profile == "debug" {
        println!("cargo:rustc-cfg=debug_build");
    }
}
```

Then in your `main.rs`:

```rust
// Emit a compile_error if debug build without the feature.
// This serves as a reminder to enable the feature during development.
#[cfg(all(debug_build, not(feature = "deadlock-detection")))]
compile_error!(
    "Enable deadlock detection in debug builds: \
     cargo run --features deadlock-detection"
);
```

> **Note:** Cargo features cannot be activated from `build.rs` — `build.rs` can only emit `rustc-cfg` flags. The `compile_error!` approach acts as a guardrail that reminds developers to pass the feature flag during debug builds.

## Detected Scenarios

| Scenario | Example | Detected? |
|----------|---------|-----------|
| Self-ask | A → A | Yes |
| 2-actor cycle | A → B → A | Yes |
| N-actor chain | A → B → C → ... → A | Yes |
| Sequential asks (no cycle) | A → B, then A → C | No false positive |
| Non-actor caller | main → A | Skipped (no cycle possible) |

## Panic Behavior

Deadlocks are **design errors**, not recoverable runtime conditions. Following Rust's convention (like index-out-of-bounds), the framework panics with a descriptive message:

```text
thread 'tokio-runtime-worker' panicked at 'Deadlock detected: ask cycle MyActorA(#1) -> MyActorB(#2) -> MyActorA(#1)
This is a design error. Use `tell` to break the cycle,
or restructure actor dependencies.'
```

The panic message includes:
- The full cycle path with actor type names and IDs
- Guidance on how to fix the issue

### What Happens After the Panic

1. The actor that attempted the cycle-forming `ask` panics
2. Its reply channel is dropped, causing the upstream `ask` to receive an error
3. The wait-for graph entry is cleaned up by a drop guard (even during panics)
4. The Mutex is released before panicking to prevent poisoning

## How to Fix Deadlocks

### 1. Replace `ask` with `tell` to Break the Cycle

The most common fix — if one direction doesn't need a synchronous reply, use `tell`:

```rust
// Before (deadlock-prone):
// Actor A handler
async fn handle(&mut self, msg: Request, _: &ActorRef<Self>) -> Response {
    let result = self.actor_b.ask(QueryB).await?;  // A waits for B
    Response(result)
}

// After (deadlock-free):
// Actor A handler
async fn handle(&mut self, msg: Request, _: &ActorRef<Self>) {
    self.actor_b.tell(NotifyB { data: msg.data }).await?;  // fire-and-forget
}
```

### 2. Restructure Actor Dependencies

Introduce a mediator actor or restructure the dependency graph to eliminate cycles:

```
// Before: A ↔ B (bidirectional ask)
// After:  A → Mediator ← B (no cycles)
```

### 3. Use Callbacks or Channels

Pass a `oneshot::Sender` in a `tell` message instead of using `ask`:

```rust
struct RequestWithCallback {
    data: String,
    reply_tx: tokio::sync::oneshot::Sender<Response>,
}

// Actor A: send request via tell with a callback channel
let (tx, rx) = tokio::sync::oneshot::channel();
actor_b.tell(RequestWithCallback { data, reply_tx: tx }).await?;
let response = rx.await?;
```

## Limitations

- **`blocking_ask`**: Runs on a separate thread for non-actor contexts. Not covered by detection since these callers can't form cycles.
- **Concurrent asks in a handler**: Using `tokio::join!` or `tokio::select!` to send multiple `ask` calls simultaneously may miss some cycles, as the wait-for graph stores one edge per actor.
- **`tokio::spawn` inside handlers**: New tokio tasks don't inherit the `task_local` actor identity, so `ask` calls from spawned tasks are not tracked.

## Performance Impact

| Condition | Overhead |
|-----------|----------|
| Feature disabled (production) | **Zero** — all code removed at compile time |
| Feature enabled, non-actor caller | ~1 ns (`try_with` returns `None`, skip) |
| Feature enabled, actor `ask` call | ~100-500 ns (mutex lock + graph traversal) |

The wait-for graph only contains entries for currently in-flight `ask` calls, so it remains small. The graph traversal is O(n) where n is the number of concurrent `ask` calls.

---

## See Also

- [Debugging Guide](debugging_guide.md) - Error handling and dead letter tracking
- [Best Practices](best_practices.md) - Recommended patterns for actor design
- [FAQ](FAQ.md) - Common questions and answers
