# Message Handler Traits Implementation Plan

## Goal
Implement `TellHandler<M>` and `AskHandler<M, R>` traits to manage different Actor types that handle the same message in a unified collection. Also provide `WeakTellHandler<M>` and `WeakAskHandler<M, R>` for weak reference collections.

## Usage Example (Target)
```rust
// === Strong Reference Handlers (keeps actors alive) ===

// Fire-and-forget
let handlers: Vec<Box<dyn TellHandler<PingMsg>>> = vec![
    (&actor_a).into(),  // From<&ActorRef<T>> - keeps original
    actor_b.into(),     // From<ActorRef<T>> - moves ownership
];

for handler in &handlers {
    handler.tell(PingMsg { timestamp: 12345 }).await?;
}

// Clone support via clone_boxed
let handlers_clone = handlers.iter().map(|h| h.clone_boxed()).collect::<Vec<_>>();
// Or using Clone impl on Box<dyn TellHandler<M>>
let handlers_clone = handlers.clone();

// Request-response (same Reply type)
let handlers: Vec<Box<dyn AskHandler<GetStatus, Status>>> = vec![
    (&actor_a).into(),
    (&actor_b).into(),
];

for handler in &handlers {
    let status = handler.ask(GetStatus).await?;
}

// === Weak Reference Handlers (does NOT keep actors alive) ===

let weak_handlers: Vec<Box<dyn WeakTellHandler<PingMsg>>> = vec![
    ActorRef::downgrade(&actor_a).into(),  // From<ActorWeak<T>>
    ActorRef::downgrade(&actor_b).into(),
];

// Must upgrade before use - explicit and efficient
for handler in &weak_handlers {
    if let Some(strong) = handler.upgrade() {
        strong.tell(PingMsg { timestamp: 12345 }).await?;
    }
}

// Batch operations with single upgrade
if let Some(strong) = weak_handlers[0].upgrade() {
    strong.tell(PingMsg { timestamp: 1 }).await?;
    strong.tell(PingMsg { timestamp: 2 }).await?;
    strong.tell(PingMsg { timestamp: 3 }).await?;
}

// Downgrade strong to weak handler
let weak: Box<dyn WeakTellHandler<PingMsg>> = handlers[0].downgrade();
```

---

## Phase 1: Strong Handler Traits

### File: `src/handler.rs` (new)

**Reuse existing BoxFuture** (already defined at `lib.rs:328`):
```rust
// Change to pub in lib.rs for reuse
pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn Future<Output = T> + Send + 'a>>;
```

```rust
use crate::{Actor, ActorRef, ActorWeak, BoxFuture, Identity, Message, Result};
use futures::FutureExt;
use std::fmt;
use std::time::Duration;

/// Fire-and-forget message handler for strong references (object-safe)
pub trait TellHandler<M: Send + 'static>: Send + Sync {
    /// Sends a message without waiting for a reply
    fn tell(&self, msg: M) -> BoxFuture<'_, Result<()>>;

    /// Sends a message with timeout
    fn tell_with_timeout(&self, msg: M, timeout: Duration) -> BoxFuture<'_, Result<()>>;

    /// Blocking version of tell
    fn blocking_tell(&self, msg: M) -> Result<()>;

    /// Clone this handler into a new boxed instance
    fn clone_boxed(&self) -> Box<dyn TellHandler<M>>;

    /// Downgrade to a weak handler
    fn downgrade(&self) -> Box<dyn WeakTellHandler<M>>;

    /// Returns the unique identity of the actor
    fn identity(&self) -> Identity;

    /// Checks if the actor is still alive
    fn is_alive(&self) -> bool;

    /// Gracefully stops the actor
    fn stop(&self) -> BoxFuture<'_, Result<()>>;

    /// Immediately terminates the actor
    fn kill(&self) -> Result<()>;

    /// Debug formatting support for trait objects
    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
}

/// Request-response message handler for strong references (object-safe)
pub trait AskHandler<M: Send + 'static, R: Send + 'static>: Send + Sync {
    /// Sends a message and awaits a reply
    fn ask(&self, msg: M) -> BoxFuture<'_, Result<R>>;

    /// Sends a message and awaits a reply with timeout
    fn ask_with_timeout(&self, msg: M, timeout: Duration) -> BoxFuture<'_, Result<R>>;

    /// Blocking version of ask
    fn blocking_ask(&self, msg: M) -> Result<R>;

    /// Clone this handler into a new boxed instance
    fn clone_boxed(&self) -> Box<dyn AskHandler<M, R>>;

    /// Downgrade to a weak handler
    fn downgrade(&self) -> Box<dyn WeakAskHandler<M, R>>;

    /// Returns the unique identity of the actor
    fn identity(&self) -> Identity;

    /// Checks if the actor is still alive
    fn is_alive(&self) -> bool;

    /// Gracefully stops the actor
    fn stop(&self) -> BoxFuture<'_, Result<()>>;

    /// Immediately terminates the actor
    fn kill(&self) -> Result<()>;

    /// Debug formatting support for trait objects
    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
}
```

---

## Phase 2: Weak Handler Traits

### File: `src/handler.rs` (continued)

Weak handlers are **upgrade-only**. They do not provide `tell`/`ask` methods directly.
Users must explicitly call `upgrade()` to obtain a strong handler before sending messages.

```rust
/// Weak handler for fire-and-forget messages (object-safe)
///
/// Unlike `TellHandler`, this does not keep the actor alive.
/// Must call `upgrade()` to obtain a strong handler before sending messages.
///
/// # Example
/// ```rust
/// if let Some(strong) = weak_handler.upgrade() {
///     strong.tell(MyMessage).await?;
/// }
/// ```
pub trait WeakTellHandler<M: Send + 'static>: Send + Sync {
    /// Attempts to upgrade to a strong handler.
    /// Returns `None` if the actor has been dropped.
    fn upgrade(&self) -> Option<Box<dyn TellHandler<M>>>;

    /// Clone this handler into a new boxed instance
    fn clone_boxed(&self) -> Box<dyn WeakTellHandler<M>>;

    /// Returns the unique identity of the actor
    fn identity(&self) -> Identity;

    /// Checks if the actor might still be alive (heuristic, not guaranteed)
    fn is_alive(&self) -> bool;

    /// Debug formatting support for trait objects
    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
}

/// Weak handler for request-response messages (object-safe)
///
/// Unlike `AskHandler`, this does not keep the actor alive.
/// Must call `upgrade()` to obtain a strong handler before sending messages.
///
/// # Example
/// ```rust
/// if let Some(strong) = weak_handler.upgrade() {
///     let reply = strong.ask(MyQuery).await?;
/// }
/// ```
pub trait WeakAskHandler<M: Send + 'static, R: Send + 'static>: Send + Sync {
    /// Attempts to upgrade to a strong handler.
    /// Returns `None` if the actor has been dropped.
    fn upgrade(&self) -> Option<Box<dyn AskHandler<M, R>>>;

    /// Clone this handler into a new boxed instance
    fn clone_boxed(&self) -> Box<dyn WeakAskHandler<M, R>>;

    /// Returns the unique identity of the actor
    fn identity(&self) -> Identity;

    /// Checks if the actor might still be alive (heuristic, not guaranteed)
    fn is_alive(&self) -> bool;

    /// Debug formatting support for trait objects
    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
}
```

**Design Rationale**: Weak handlers are upgrade-only because:
1. **Explicit is better than implicit** - Users see exactly when upgrade happens
2. **Efficiency** - Single upgrade enables multiple operations without repeated overhead
3. **Consistency** - Matches `std::sync::Weak::upgrade()` pattern
4. **Simplicity** - Fewer methods, cleaner trait

---

## Phase 3: Clone and Debug Implementation for Box<dyn Handler>

### File: `src/handler.rs` (continued)

```rust
// Strong handlers
impl<M: Send + 'static> Clone for Box<dyn TellHandler<M>> {
    fn clone(&self) -> Self {
        self.clone_boxed()
    }
}

impl<M: Send + 'static, R: Send + 'static> Clone for Box<dyn AskHandler<M, R>> {
    fn clone(&self) -> Self {
        self.clone_boxed()
    }
}

impl<M: Send + 'static> fmt::Debug for Box<dyn TellHandler<M>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.debug_fmt(f)
    }
}

impl<M: Send + 'static, R: Send + 'static> fmt::Debug for Box<dyn AskHandler<M, R>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.debug_fmt(f)
    }
}

// Weak handlers
impl<M: Send + 'static> Clone for Box<dyn WeakTellHandler<M>> {
    fn clone(&self) -> Self {
        self.clone_boxed()
    }
}

impl<M: Send + 'static, R: Send + 'static> Clone for Box<dyn WeakAskHandler<M, R>> {
    fn clone(&self) -> Self {
        self.clone_boxed()
    }
}

impl<M: Send + 'static> fmt::Debug for Box<dyn WeakTellHandler<M>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.debug_fmt(f)
    }
}

impl<M: Send + 'static, R: Send + 'static> fmt::Debug for Box<dyn WeakAskHandler<M, R>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.debug_fmt(f)
    }
}
```

---

## Phase 4: Blanket Implementations for ActorRef

### File: `src/handler.rs` (continued)

```rust
// TellHandler implementation for ActorRef<T>
impl<T, M> TellHandler<M> for ActorRef<T>
where
    T: Actor + Message<M> + 'static,
    M: Send + 'static,
{
    fn tell(&self, msg: M) -> BoxFuture<'_, Result<()>> {
        ActorRef::tell(self, msg).boxed()
    }

    fn tell_with_timeout(&self, msg: M, timeout: Duration) -> BoxFuture<'_, Result<()>> {
        ActorRef::tell_with_timeout(self, msg, timeout).boxed()
    }

    fn blocking_tell(&self, msg: M) -> Result<()> {
        ActorRef::blocking_tell(self, msg)
    }

    fn clone_boxed(&self) -> Box<dyn TellHandler<M>> {
        Box::new(self.clone())
    }

    fn downgrade(&self) -> Box<dyn WeakTellHandler<M>> {
        Box::new(ActorRef::downgrade(self))
    }

    fn identity(&self) -> Identity {
        ActorRef::identity(self)
    }

    fn is_alive(&self) -> bool {
        ActorRef::is_alive(self)
    }

    fn stop(&self) -> BoxFuture<'_, Result<()>> {
        ActorRef::stop(self).boxed()
    }

    fn kill(&self) -> Result<()> {
        ActorRef::kill(self)
    }

    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TellHandler")
            .field("identity", &self.identity())
            .field("alive", &self.is_alive())
            .finish()
    }
}

// AskHandler implementation for ActorRef<T>
impl<T, M> AskHandler<M, <T as Message<M>>::Reply> for ActorRef<T>
where
    T: Actor + Message<M> + 'static,
    M: Send + 'static,
    <T as Message<M>>::Reply: Send + 'static,
{
    fn ask(&self, msg: M) -> BoxFuture<'_, Result<<T as Message<M>>::Reply>> {
        ActorRef::ask(self, msg).boxed()
    }

    fn ask_with_timeout(&self, msg: M, timeout: Duration) -> BoxFuture<'_, Result<<T as Message<M>>::Reply>> {
        ActorRef::ask_with_timeout(self, msg, timeout).boxed()
    }

    fn blocking_ask(&self, msg: M) -> Result<<T as Message<M>>::Reply> {
        ActorRef::blocking_ask(self, msg)
    }

    fn clone_boxed(&self) -> Box<dyn AskHandler<M, <T as Message<M>>::Reply>> {
        Box::new(self.clone())
    }

    fn downgrade(&self) -> Box<dyn WeakAskHandler<M, <T as Message<M>>::Reply>> {
        Box::new(ActorRef::downgrade(self))
    }

    fn identity(&self) -> Identity {
        ActorRef::identity(self)
    }

    fn is_alive(&self) -> bool {
        ActorRef::is_alive(self)
    }

    fn stop(&self) -> BoxFuture<'_, Result<()>> {
        ActorRef::stop(self).boxed()
    }

    fn kill(&self) -> Result<()> {
        ActorRef::kill(self)
    }

    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AskHandler")
            .field("identity", &self.identity())
            .field("alive", &self.is_alive())
            .finish()
    }
}
```

---

## Phase 5: Blanket Implementations for ActorWeak

### File: `src/handler.rs` (continued)

```rust
// WeakTellHandler implementation for ActorWeak<T>
impl<T, M> WeakTellHandler<M> for ActorWeak<T>
where
    T: Actor + Message<M> + 'static,
    M: Send + 'static,
{
    fn upgrade(&self) -> Option<Box<dyn TellHandler<M>>> {
        ActorWeak::upgrade(self).map(|r| Box::new(r) as Box<dyn TellHandler<M>>)
    }

    fn clone_boxed(&self) -> Box<dyn WeakTellHandler<M>> {
        Box::new(self.clone())
    }

    fn identity(&self) -> Identity {
        ActorWeak::identity(self)
    }

    fn is_alive(&self) -> bool {
        ActorWeak::is_alive(self)
    }

    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WeakTellHandler")
            .field("identity", &self.identity())
            .field("alive", &self.is_alive())
            .finish()
    }
}

// WeakAskHandler implementation for ActorWeak<T>
impl<T, M> WeakAskHandler<M, <T as Message<M>>::Reply> for ActorWeak<T>
where
    T: Actor + Message<M> + 'static,
    M: Send + 'static,
    <T as Message<M>>::Reply: Send + 'static,
{
    fn upgrade(&self) -> Option<Box<dyn AskHandler<M, <T as Message<M>>::Reply>>> {
        ActorWeak::upgrade(self).map(|r| Box::new(r) as Box<dyn AskHandler<M, <T as Message<M>>::Reply>>)
    }

    fn clone_boxed(&self) -> Box<dyn WeakAskHandler<M, <T as Message<M>>::Reply>> {
        Box::new(self.clone())
    }

    fn identity(&self) -> Identity {
        ActorWeak::identity(self)
    }

    fn is_alive(&self) -> bool {
        ActorWeak::is_alive(self)
    }

    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WeakAskHandler")
            .field("identity", &self.identity())
            .field("alive", &self.is_alive())
            .finish()
    }
}
```

---

## Phase 6: From Trait Implementations

### File: `src/handler.rs` (continued)

```rust
// === Strong Handler From implementations ===

// From ActorRef (ownership transfer) to Box<dyn TellHandler<M>>
impl<T, M> From<ActorRef<T>> for Box<dyn TellHandler<M>>
where
    T: Actor + Message<M> + 'static,
    M: Send + 'static,
{
    fn from(actor_ref: ActorRef<T>) -> Self {
        Box::new(actor_ref)
    }
}

// From &ActorRef (clone) to Box<dyn TellHandler<M>>
impl<T, M> From<&ActorRef<T>> for Box<dyn TellHandler<M>>
where
    T: Actor + Message<M> + 'static,
    M: Send + 'static,
{
    fn from(actor_ref: &ActorRef<T>) -> Self {
        Box::new(actor_ref.clone())
    }
}

// From ActorRef (ownership transfer) to Box<dyn AskHandler<M, R>>
impl<T, M> From<ActorRef<T>> for Box<dyn AskHandler<M, <T as Message<M>>::Reply>>
where
    T: Actor + Message<M> + 'static,
    M: Send + 'static,
    <T as Message<M>>::Reply: Send + 'static,
{
    fn from(actor_ref: ActorRef<T>) -> Self {
        Box::new(actor_ref)
    }
}

// From &ActorRef (clone) to Box<dyn AskHandler<M, R>>
impl<T, M> From<&ActorRef<T>> for Box<dyn AskHandler<M, <T as Message<M>>::Reply>>
where
    T: Actor + Message<M> + 'static,
    M: Send + 'static,
    <T as Message<M>>::Reply: Send + 'static,
{
    fn from(actor_ref: &ActorRef<T>) -> Self {
        Box::new(actor_ref.clone())
    }
}

// === Weak Handler From implementations ===

// From ActorWeak (ownership transfer) to Box<dyn WeakTellHandler<M>>
impl<T, M> From<ActorWeak<T>> for Box<dyn WeakTellHandler<M>>
where
    T: Actor + Message<M> + 'static,
    M: Send + 'static,
{
    fn from(actor_weak: ActorWeak<T>) -> Self {
        Box::new(actor_weak)
    }
}

// From &ActorWeak (clone) to Box<dyn WeakTellHandler<M>>
impl<T, M> From<&ActorWeak<T>> for Box<dyn WeakTellHandler<M>>
where
    T: Actor + Message<M> + 'static,
    M: Send + 'static,
{
    fn from(actor_weak: &ActorWeak<T>) -> Self {
        Box::new(actor_weak.clone())
    }
}

// From ActorWeak (ownership transfer) to Box<dyn WeakAskHandler<M, R>>
impl<T, M> From<ActorWeak<T>> for Box<dyn WeakAskHandler<M, <T as Message<M>>::Reply>>
where
    T: Actor + Message<M> + 'static,
    M: Send + 'static,
    <T as Message<M>>::Reply: Send + 'static,
{
    fn from(actor_weak: ActorWeak<T>) -> Self {
        Box::new(actor_weak)
    }
}

// From &ActorWeak (clone) to Box<dyn WeakAskHandler<M, R>>
impl<T, M> From<&ActorWeak<T>> for Box<dyn WeakAskHandler<M, <T as Message<M>>::Reply>>
where
    T: Actor + Message<M> + 'static,
    M: Send + 'static,
    <T as Message<M>>::Reply: Send + 'static,
{
    fn from(actor_weak: &ActorWeak<T>) -> Self {
        Box::new(actor_weak.clone())
    }
}
```

---

## Phase 7: lib.rs Update

### File: `src/lib.rs` (modify)

```rust
mod handler;

pub use handler::{TellHandler, AskHandler, WeakTellHandler, WeakAskHandler};
```

---

## Phase 8: Tests and Examples

### File: `tests/handler_tests.rs` (new)

**Strong handler tests:**
- Test TellHandler conversion for multiple Actor types
- Test AskHandler conversion with same Reply type
- Test clone_boxed and Clone for Box<dyn Handler>
- Test From<ActorRef> and From<&ActorRef> conversions
- Test control methods (stop, kill)
- Test downgrade() returns valid weak handler
- Test Debug formatting

**Weak handler tests:**
- Test WeakTellHandler conversion for multiple Actor types
- Test WeakAskHandler conversion with same Reply type
- Test clone_boxed and Clone for Box<dyn WeakHandler>
- Test From<ActorWeak> and From<&ActorWeak> conversions
- Test upgrade() returns Some when actor alive
- Test upgrade() returns None when actor stopped
- Test is_alive() heuristic check
- Test Debug formatting

**Interop tests:**
- Test downgrade() from strong to weak handler
- Test upgrade() from weak to strong handler
- Test round-trip: strong -> downgrade -> upgrade -> use
- Test batch operations with single upgrade

### File: `examples/handler_demo.rs` (new)
- TellHandler multi-actor example
- AskHandler collect responses example
- Clone handlers example
- WeakTellHandler with upgrade pattern
- Batch operations with single upgrade
- Downgrade/upgrade round-trip example

---

## Files to Modify

| File | Action |
|------|--------|
| `src/lib.rs` | Modify - Change `BoxFuture` to `pub`, add module declaration and re-exports |
| `src/handler.rs` | New - All handler traits + impls |
| `tests/handler_tests.rs` | New - Tests |
| `examples/handler_demo.rs` | New - Example |

---

## Design Decisions

### 1. Separate Strong and Weak Handler Traits
`TellHandler`/`AskHandler` are for strong references, `WeakTellHandler`/`WeakAskHandler` are for weak references. They should NOT be mixed in the same collection to maintain clear ownership semantics.

### 2. Weak Handlers are Upgrade-Only
Weak handlers do not provide `tell`/`ask` methods. Users must explicitly call `upgrade()` to obtain a strong handler before sending messages. This design:
- Makes upgrade overhead explicit and visible
- Enables efficient batch operations with single upgrade
- Follows Rust's philosophy of explicit over implicit
- Matches `std::sync::Weak::upgrade()` pattern

### 3. No Lifecycle Control for Weak Handlers
Weak handlers do not have `stop()` and `kill()` methods. Lifecycle control should only be performed through strong references.

### 4. Clone Support via `clone_boxed`
Both strong and weak handlers support cloning through `clone_boxed()` method and `Clone` trait impl on Box.

### 5. Dual From Implementations
Both ownership transfer and reference cloning are supported for both ActorRef and ActorWeak.

### 6. Debug Support via `debug_fmt`
All handler traits include `debug_fmt` for trait object debugging.

---

## Constraints

- `AskHandler<M, R>` / `WeakAskHandler<M, R>`: All actors in collection must have same Reply type `R`
- Minor runtime overhead due to `BoxFuture` usage
- Fully compatible with existing API (opt-in approach)

---

## Future Considerations

### `ask_join` Support
Currently `ActorRef::ask_join` is not included in handler traits. If needed, it could be added as a separate method in `AskHandler` or a new trait:
```rust
fn ask_join<R2>(&self, msg: M) -> BoxFuture<'_, Result<R2>>
where
    R: Into<JoinHandle<R2>>;
```
This is left for future consideration based on actual usage patterns.
