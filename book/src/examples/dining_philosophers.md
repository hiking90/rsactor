# Dining Philosophers

The Dining Philosophers problem is a classic concurrency challenge that demonstrates resource sharing, deadlock prevention, and coordination between multiple actors. This example shows how rsActor can elegantly solve this problem using the actor model.

## Problem Description

Five philosophers sit around a circular table with five forks. Each philosopher alternates between thinking and eating. To eat, a philosopher needs both the fork to their left and the fork to their right. The challenge is to design a solution that:

1. Prevents deadlock (all philosophers waiting forever)
2. Prevents starvation (ensuring each philosopher gets to eat)
3. Maximizes concurrency (allows multiple philosophers to eat simultaneously when possible)

## Solution Architecture

Our solution uses two types of actors:

### 1. Table Actor
- **Role**: Centralized resource manager for forks
- **Responsibilities**:
  - Track which forks are available
  - Grant/deny fork requests
  - Handle fork releases
  - Maintain philosopher registry

### 2. Philosopher Actors
- **Role**: Individual philosophers with their own thinking/eating logic
- **Responsibilities**:
  - Think for random periods
  - Request forks from the table when hungry
  - Eat when both forks are acquired
  - Release forks after eating
  - Repeat the cycle

## Key Components

### Messages

```rust
// Fork management
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ForkSide {
    Left,
    Right,
}

// Messages to Table Actor
struct RegisterPhilosopher {
    logical_id: usize,
    philosopher_ref: ActorRef<Philosopher>,
}

struct RequestFork {
    logical_id: usize,
    side: ForkSide,
}

struct ReleaseFork {
    logical_id: usize,
    side: ForkSide,
}

// Messages to Philosopher Actor
struct StartThinking;
struct StartEating;
```

### Philosopher Actor

```rust
#[derive(Debug)]
struct Philosopher {
    id: usize,
    name: String,
    table_ref: ActorRef<Table>,
    eat_count: u32,
    has_left_fork: bool,
    has_right_fork: bool,
}

impl Actor for Philosopher {
    type Args = (usize, String, ActorRef<Table>);
    type Error = anyhow::Error;

    async fn on_start(args: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        let (id, name, table_ref) = args;
        println!("Philosopher {} ({}) is joining the table.", id, name);

        let philosopher = Self {
            id, name: name.clone(), table_ref: table_ref.clone(),
            eat_count: 0, has_left_fork: false, has_right_fork: false,
        };

        // Register with the table
        table_ref.tell(RegisterPhilosopher {
            logical_id: id,
            philosopher_ref: actor_ref.clone(),
        }).await?;

        // Start thinking
        actor_ref.tell(StartThinking).await?;
        Ok(philosopher)
    }
}
```

### Table Actor

```rust
#[derive(Debug)]
struct Table {
    philosophers: HashMap<usize, ActorRef<Philosopher>>,
    fork_availability: Vec<bool>, // true = available, false = taken
}

impl Actor for Table {
    type Args = usize; // Number of philosophers
    type Error = anyhow::Error;

    async fn on_start(num_philosophers: Self::Args, _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(Self {
            philosophers: HashMap::new(),
            fork_availability: vec![true; num_philosophers],
        })
    }
}
```

## Deadlock Prevention Strategy

The solution prevents deadlock through:

1. **Centralized Fork Management**: The Table actor manages all forks, preventing race conditions
2. **Sequential Fork Acquisition**: Philosophers request one fork at a time
3. **Fork Release on Failure**: If a philosopher can't get both forks, they release any held fork
4. **Random Timing**: Randomized thinking/eating times reduce contention patterns

## Key Implementation Details

### Fork Request Logic

```rust
impl Message<RequestFork> for Table {
    type Reply = bool; // true if fork granted, false if unavailable

    async fn handle(&mut self, msg: RequestFork, _: &ActorRef<Self>) -> Self::Reply {
        let fork_index = match msg.side {
            ForkSide::Left => msg.logical_id,
            ForkSide::Right => (msg.logical_id + 1) % self.fork_availability.len(),
        };

        if self.fork_availability[fork_index] {
            self.fork_availability[fork_index] = false;
            println!("Table: Fork {} granted to Philosopher {}", fork_index, msg.logical_id);
            true
        } else {
            println!("Table: Fork {} denied to Philosopher {} (in use)", fork_index, msg.logical_id);
            false
        }
    }
}
```

### Philosopher State Machine

1. **Thinking State**: Random duration, then try to acquire forks
2. **Fork Acquisition**:
   - Request left fork first
   - If successful, request right fork
   - If both acquired, start eating
   - If either fails, release held forks and return to thinking
3. **Eating State**: Random duration, then release both forks
4. **Repeat**: Return to thinking state

## Benefits of the Actor Model Solution

### 1. **Encapsulation**
- Each philosopher manages its own state independently
- The table encapsulates fork management logic
- No shared mutable state between actors

### 2. **Message-Based Coordination**
- All communication happens through well-defined messages
- No need for locks, mutexes, or other synchronization primitives
- Clear separation of concerns

### 3. **Fault Tolerance**
- Actor isolation means one philosopher's failure doesn't affect others
- The table actor can detect and handle philosopher failures
- System can continue operating with fewer philosophers

### 4. **Scalability**
- Easy to add more philosophers by spawning new actors
- Table actor can handle arbitrary numbers of philosophers
- Natural load distribution across async tasks

## Running the Example

```bash
cargo run --example dining_philosophers
```

You'll see output like:
```
Philosopher 0 (Socrates) is thinking.
Philosopher 1 (Plato) attempts to acquire Left fork.
Table: Fork 1 granted to Philosopher 1
Philosopher 1 (Plato) acquired Left fork. Attempting Right fork.
Table: Fork 2 granted to Philosopher 1
Philosopher 1 (Plato) acquired both forks and is eating.
...
```

## Learning Outcomes

This example demonstrates:
- **Actor coordination patterns** for resource management
- **Deadlock prevention** strategies in distributed systems
- **Message-driven state machines** for complex logic
- **Centralized vs. distributed** resource management approaches
- **Practical concurrency** problem-solving with actors

The Dining Philosophers problem showcases how the actor model can elegantly solve complex concurrency challenges while maintaining code clarity and system reliability.
