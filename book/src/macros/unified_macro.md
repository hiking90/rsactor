# Message Handlers Macro

The `#[message_handlers]` attribute macro combined with `#[handler]` method attributes is the **recommended** approach for defining message handlers in rsActor. It automatically generates the necessary `Message` trait implementations from annotated methods.

## Purpose

When an actor needs to handle multiple message types, manually implementing the `Message` trait for each one is verbose. The `#[message_handlers]` macro automates this by generating all the boilerplate from simple method signatures.

## Basic Example

```rust
use rsactor::{Actor, ActorRef, message_handlers, spawn};

#[derive(Actor)]
struct CounterActor {
    value: i32,
}

// Define message types
struct Add(i32);
struct GetValue;

// Use message_handlers macro with handler attributes
#[message_handlers]
impl CounterActor {
    #[handler]
    async fn handle_add(&mut self, msg: Add, _: &ActorRef<Self>) -> i32 {
        self.value += msg.0;
        self.value
    }

    #[handler]
    async fn handle_get(&mut self, _: GetValue, _: &ActorRef<Self>) -> i32 {
        self.value
    }

    // Regular methods can coexist without the #[handler] attribute
    fn reset(&mut self) {
        self.value = 0;
    }
}
```

## How It Works

Each `#[handler]` method is transformed into a `Message<T>` trait implementation:

1. The **first parameter** after `&mut self` determines the message type `T`
2. The **return type** becomes `Message<T>::Reply`
3. The method body becomes the `handle()` implementation

The macro generates the equivalent of:

```rust
impl Message<Add> for CounterActor {
    type Reply = i32;
    async fn handle(&mut self, msg: Add, actor_ref: &ActorRef<Self>) -> Self::Reply {
        self.value += msg.0;
        self.value
    }
}
```

## Generic Actor Support

The macro fully supports generic actors:

```rust
use rsactor::{Actor, ActorRef, message_handlers, spawn};
use std::fmt::Debug;

#[derive(Debug)]
struct GenericActor<T: Send + Debug + Clone + 'static> {
    value: Option<T>,
}

impl<T: Send + Debug + Clone + 'static> Actor for GenericActor<T> {
    type Error = String;
    type Args = ();

    async fn on_start(_: Self::Args, _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(GenericActor { value: None })
    }
}

struct SetValue<T: Send + Debug + 'static>(T);
struct GetValue;

#[message_handlers]
impl<T: Send + Debug + Clone + 'static> GenericActor<T> {
    #[handler]
    async fn handle_set(&mut self, msg: SetValue<T>, _: &ActorRef<Self>) -> () {
        self.value = Some(msg.0);
    }

    #[handler]
    async fn handle_get(&mut self, _: GetValue, _: &ActorRef<Self>) -> Option<T> {
        self.value.clone()
    }
}
```

## Error Handling with `on_tell_result`

When a handler returns `Result<T, E>`, the macro automatically generates an `on_tell_result` implementation that logs errors for `tell()` calls:

```rust
#[message_handlers]
impl MyActor {
    // Returning Result<T, E> auto-generates error logging for tell() calls
    #[handler]
    async fn handle_process(&mut self, msg: ProcessData, _: &ActorRef<Self>) -> Result<(), MyError> {
        self.process(msg.data)?;
        Ok(())
    }

    // Use #[handler(no_log)] to suppress automatic error logging
    #[handler(no_log)]
    async fn handle_silent(&mut self, msg: SilentOp, _: &ActorRef<Self>) -> Result<(), MyError> {
        // Errors won't be automatically logged for tell() calls
        Ok(())
    }
}
```

## `#[derive(Actor)]` Macro

For simple actors that don't need custom initialization logic, use `#[derive(Actor)]`:

```rust
#[derive(Actor)]
struct SimpleActor {
    name: String,
    count: u32,
}

// spawn takes the struct instance directly as Args
let actor = SimpleActor { name: "test".into(), count: 0 };
let (actor_ref, handle) = spawn::<SimpleActor>(actor);
```

This generates an `Actor` impl where:
- `type Args = Self` (the struct itself)
- `type Error = std::convert::Infallible` (never fails)
- `on_start` simply returns the provided instance

## Benefits

1. **Selective Processing**: Only methods with `#[handler]` become message handlers
2. **Clean Separation**: Regular methods coexist with handlers in the same `impl` block
3. **Automatic `on_tell_result`**: Error logging for `Result` return types (suppressed via `#[handler(no_log)]`)
4. **Generic Support**: Full support for generic actors and message types
5. **Compile-Time Safety**: Message handler signatures are verified at compile time
6. **Reduced Boilerplate**: No manual `Message` trait implementations needed

## Running the Example

```bash
cargo run --example derive_macro_demo
cargo run --example unified_macro_demo
```
