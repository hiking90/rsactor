# Macros

rsActor provides several macros to reduce boilerplate code and make actor development more ergonomic. These macros handle the repetitive aspects of actor implementation while maintaining type safety and performance.

## Available Macros

### 1. `#[derive(Actor)]` - Actor Derive Macro
Automatically implements the `Actor` trait for simple structs that don't require complex initialization logic.

### 2. `impl_message_handler!` - Message Handler Macro
Generates the necessary boilerplate to wire up message handlers for an actor, supporting both regular and generic actors.

## When to Use Macros

### Use `#[derive(Actor)]` when:
- Your actor doesn't need complex initialization logic
- The actor can be created directly from its field values
- You want to minimize boilerplate for simple actors
- Error handling can use `std::convert::Infallible` (never fails)

### Use `impl_message_handler!` when:
- You have multiple message types to handle
- You want to avoid manual implementation of the `MessageHandler` trait
- You're working with generic actors
- You need consistent message routing logic

## Basic Usage Example

```rust
use rsactor::{Actor, ActorRef, Message, impl_message_handler, spawn};

// Simple actor using derive macro - no custom initialization needed
#[derive(Actor, Debug)]
struct UserAccount {
    username: String,
    balance: u64,
}

// Define messages
struct Deposit(u64);
struct GetBalance;

// Implement message handlers
impl Message<Deposit> for UserAccount {
    type Reply = u64; // Returns new balance
    async fn handle(&mut self, msg: Deposit, _: &ActorRef<Self>) -> Self::Reply {
        self.balance += msg.0;
        self.balance
    }
}

impl Message<GetBalance> for UserAccount {
    type Reply = u64;
    async fn handle(&mut self, _: GetBalance, _: &ActorRef<Self>) -> Self::Reply {
        self.balance
    }
}

// Wire up message handlers
impl_message_handler!(UserAccount, [Deposit, GetBalance]);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create actor instance directly - no complex initialization
    let account = UserAccount {
        username: "alice".to_string(),
        balance: 100
    };
    let (actor_ref, _) = spawn(account);

    let new_balance = actor_ref.ask(Deposit(50)).await?;
    println!("New balance: {}", new_balance); // 150

    Ok(())
}
```

## Key Benefits

- **Reduced Boilerplate**: Automatically generate repetitive code
- **Type Safety**: Compile-time verification of actor implementations
- **Zero Runtime Overhead**: All code generation happens at compile time
- **Easy Maintenance**: Simple to add new message types and actors

## Derive Macro Features

The `#[derive(Actor)]` macro is perfect for simple actors:

```rust
#[derive(Actor)]
struct SimpleActor {
    name: String,
    value: i32,
}
// Automatically implements Actor trait with:
// - Args = Self (takes the struct instance)
// - Error = std::convert::Infallible (never fails)
// - on_start simply returns the provided instance
```

For detailed information about each macro, see the individual sections:
- [Unified Macro](macros/unified_macro.md) - Details about the `impl_message_handler!` macro
