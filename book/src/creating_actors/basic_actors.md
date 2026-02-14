## Basic Actors

A basic actor is the foundation of the rsActor system. It encapsulates state and processes messages sequentially, ensuring thread safety through the actor model.

### Essential Components

Every basic actor requires:
1. **Actor Struct**: Holds the actor's state
2. **Actor Trait Implementation**: Defines initialization and lifecycle
3. **Message Types**: Define communication protocol
4. **Message Handlers**: Process incoming messages

### Complete Example

```rust
use rsactor::{Actor, ActorRef, Message, impl_message_handler, spawn};
use anyhow::Result;

// 1. Define the actor struct
#[derive(Debug)]
struct BankAccount {
    account_id: String,
    balance: u64,
}

// 2. Implement the Actor trait
impl Actor for BankAccount {
    type Args = (String, u64); // (account_id, initial_balance)
    type Error = anyhow::Error;

    async fn on_start(args: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        let (account_id, initial_balance) = args;
        println!("BankAccount {} (ID: {}) opened with balance: ${}",
                 account_id, actor_ref.identity(), initial_balance);

        Ok(BankAccount { account_id, balance: initial_balance })
    }
}

// 3. Define message types
struct Deposit(u64);
struct Withdraw(u64);
struct GetBalance;

// 4. Implement message handlers
impl Message<Deposit> for BankAccount {
    type Reply = u64; // Returns new balance

    async fn handle(&mut self, msg: Deposit, _: &ActorRef<Self>) -> Self::Reply {
        self.balance += msg.0;
        println!("Account {}: Deposited ${}, new balance: ${}",
                 self.account_id, msg.0, self.balance);
        self.balance
    }
}

impl Message<Withdraw> for BankAccount {
    type Reply = Result<u64, String>; // Returns new balance or error

    async fn handle(&mut self, msg: Withdraw, _: &ActorRef<Self>) -> Self::Reply {
        if self.balance >= msg.0 {
            self.balance -= msg.0;
            println!("Account {}: Withdrew ${}, new balance: ${}",
                     self.account_id, msg.0, self.balance);
            Ok(self.balance)
        } else {
            Err(format!("Insufficient funds: requested ${}, available ${}",
                        msg.0, self.balance))
        }
    }
}

impl Message<GetBalance> for BankAccount {
    type Reply = u64;

    async fn handle(&mut self, _: GetBalance, _: &ActorRef<Self>) -> Self::Reply {
        self.balance
    }
}

// 5. Wire up message handlers
impl_message_handler!(BankAccount, [Deposit, Withdraw, GetBalance]);

// Usage example
#[tokio::main]
async fn main() -> Result<()> {
    let (account_ref, _) = spawn::<BankAccount>(
        ("Alice".to_string(), 1000)
    );

    // Perform operations
    let new_balance = account_ref.ask(Deposit(500)).await?;
    println!("After deposit: ${}", new_balance);

    match account_ref.ask(Withdraw(200)).await? {
        Ok(balance) => println!("After withdrawal: ${}", balance),
        Err(error) => println!("Withdrawal failed: {}", error),
    }

    let final_balance = account_ref.ask(GetBalance).await?;
    println!("Final balance: ${}", final_balance);

    Ok(())
}
### Key Design Patterns

**State Management**: Actors encapsulate state privately, ensuring thread safety without locks.

**Error Handling**: Use `Result` types in message replies to handle business logic errors gracefully.

**Message Design**: Keep messages focused on single operations for clarity and maintainability.

**Type Safety**: Leverage Rust's type system to ensure compile-time verification of message handling.

This pattern scales from simple stateful services to complex business logic processors while maintaining the benefits of the actor model.
}
```

This covers the creation of a standard actor. The actor runs in its own Tokio task, processes messages sequentially, and manages its state privately.
