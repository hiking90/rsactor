# Tell Error Logging

When using `tell()` (fire-and-forget), the handler's return value is normally discarded silently.
If the handler returns `Result<T, E>` and the result is `Err`, no indication of the failure is provided to the caller or the logs.

rsActor solves this with the `on_tell_result` hook on the `Message` trait, combined with automatic code generation via the `#[handler]` macro.

## How It Works

The `Message<T>` trait includes an `on_tell_result` method:

```rust
pub trait Message<T: Send + 'static>: Actor {
    type Reply: Send + 'static;

    fn handle(&mut self, msg: T, actor_ref: &ActorRef<Self>)
        -> impl Future<Output = Self::Reply> + Send;

    // Called only for tell() — not for ask()
    fn on_tell_result(_result: &Self::Reply, _actor_ref: &ActorRef<Self>) {
        // default: do nothing
    }
}
```

- **`tell()` path**: After the handler completes, `on_tell_result` is called with the result.
- **`ask()` path**: `on_tell_result` is **not** called — the caller receives the result directly.

## Automatic Generation with `#[handler]`

When using the `#[handler]` macro, `on_tell_result` is **automatically generated** if the return type is syntactically `Result<T, E>`. The generated code logs errors via `tracing::error!`.

```rust
#[message_handlers]
impl MyActor {
    // Return type is Result<_, _> → on_tell_result generated automatically
    #[handler]
    async fn handle_work(
        &mut self, msg: DoWork, _: &ActorRef<Self>,
    ) -> Result<(), WorkError> {
        self.do_work(msg).await
    }
}
```

This generates:

```rust
impl Message<DoWork> for MyActor {
    type Reply = Result<(), WorkError>;

    async fn handle(&mut self, msg: DoWork, actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.handle_work(msg, actor_ref).await
    }

    fn on_tell_result(result: &Self::Reply, actor_ref: &ActorRef<Self>) {
        if let Err(ref e) = result {
            tracing::error!(
                actor = %actor_ref.identity(),
                message_type = %std::any::type_name::<DoWork>(),
                "tell handler returned error: {}", e
            );
        }
    }
}
```

Non-Result return types use the default no-op:

```rust
#[handler]
async fn handle_count(&mut self, msg: GetCount, _: &ActorRef<Self>) -> u32 {
    self.count
}
// → on_tell_result not generated, default no-op applies
```

## Type Alias: `#[handler(result)]`

Rust proc macros cannot resolve type aliases. If the return type is a type alias for `Result`, auto-detection will not work:

```rust
type MyResult = Result<String, MyError>;

#[message_handlers]
impl MyActor {
    // ✗ MyResult is not syntactically Result<T, E> → no auto-detection
    #[handler]
    async fn handle_process(
        &mut self, msg: Process, _: &ActorRef<Self>,
    ) -> MyResult {
        self.process(msg).await
    }
}
```

Use `#[handler(result)]` to force generation:

```rust
#[message_handlers]
impl MyActor {
    // ✓ Explicitly marked → on_tell_result generated
    #[handler(result)]
    async fn handle_process(
        &mut self, msg: Process, _: &ActorRef<Self>,
    ) -> MyResult {
        self.process(msg).await
    }
}
```

Note: `#[handler(result)]` on a method with no return type (unit `()`) is a compile error.

## Suppressing Logging: `#[handler(no_log)]`

If a handler returns `Result` but you intentionally don't want tell errors logged, use `#[handler(no_log)]`:

```rust
#[message_handlers]
impl MyActor {
    // Result return, but errors are expected and not worth logging
    #[handler(no_log)]
    async fn handle_optional(
        &mut self, msg: TryOptional, _: &ActorRef<Self>,
    ) -> Result<(), IgnorableError> {
        self.try_optional(msg).await
    }
}
```

`#[handler(result)]` and `#[handler(no_log)]` are mutually exclusive — combining them is a compile error.

## Decision Table

| Attribute | Return Type | on_tell_result |
|---|---|---|
| `#[handler]` | `Result<T, E>` | Auto-generated (logs errors) |
| `#[handler]` | Non-Result (e.g., `u32`) | Default no-op |
| `#[handler(result)]` | Any (including type alias) | Generated (logs errors) |
| `#[handler(result)]` | `()` (no return) | **Compile error** |
| `#[handler(no_log)]` | `Result<T, E>` | Default no-op (suppressed) |
| `#[handler(result, no_log)]` | — | **Compile error** (mutually exclusive) |

## Display Bound on Error Type

The generated `on_tell_result` uses `tracing::error!("...: {}", e)`, which requires `E: Display`.
This is enforced implicitly — if `E` doesn't implement `Display`, the compiler emits:

```
error[E0277]: `MyError` doesn't implement `std::fmt::Display`
```

This is a deliberate design choice. Adding an explicit `where E: Display` bound is not possible because `E` is embedded inside `Self::Reply = Result<T, E>`, and the proc macro cannot know the concrete `E` type for type aliases.

## Manual Override

For manual `Message` trait implementations, override `on_tell_result` directly:

```rust
impl Message<MyMsg> for MyActor {
    type Reply = Result<(), MyError>;

    async fn handle(&mut self, msg: MyMsg, actor_ref: &ActorRef<Self>) -> Self::Reply {
        // ...
    }

    fn on_tell_result(result: &Self::Reply, actor_ref: &ActorRef<Self>) {
        if let Err(ref e) = result {
            tracing::error!(actor = %actor_ref.identity(), "handler error: {}", e);
        }
    }
}
```

The hook is synchronous and stateless by design:
- **No `&self`**: Actor state is not accessible, keeping the hook purely observational.
- **Not `async`**: No hidden async control flow in fire-and-forget paths.
