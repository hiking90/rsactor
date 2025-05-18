// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use rsactor::{impl_message_handler, spawn, Actor, ActorRef, ActorStopReason, Message};
use anyhow::Result;
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::sleep;
use rand::Rng;

const NUM_PHILOSOPHERS: usize = 5;
const SIMULATION_TIME_MS: u64 = 5000; // 5 seconds

// --- New Enum ---
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ForkSide {
    Left,
    Right,
}

// --- Messages for Table Actor ---

/// Message from Philosopher to Table to register.
#[derive(Debug, Clone)] // Clone needed if we were to send it multiple times, not strictly here.
struct RegisterPhilosopher {
    logical_id: usize, // The 0..N-1 ID of the philosopher
    philosopher_ref: ActorRef,
}

/// For testing purposes, only one fork is processed at a time
/// Message from Philosopher to Table to request a single fork.
#[derive(Debug, Clone)]
struct RequestFork {
    logical_id: usize,
    side: ForkSide,
}

/// Message from Philosopher to Table to release a single fork.
#[derive(Debug, Clone)]
struct ReleaseFork {
    logical_id: usize,
    side: ForkSide,
}

// --- Messages for Philosopher Actor ---

/// Self-message for a Philosopher to start thinking.
#[derive(Debug, Clone)]
struct StartThinking;

/// Self-message for a Philosopher to start eating.
#[derive(Debug, Clone)]
struct StartEating;

// --- Philosopher Actor Definition ---
struct Philosopher {
    id: usize, // Logical ID (0 to N-1)
    name: String,
    table_ref: ActorRef,
    eat_count: u32,
    has_left_fork: bool,
    has_right_fork: bool,
}

impl std::fmt::Debug for Philosopher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Philosopher")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("eat_count", &self.eat_count)
            .finish()
    }
}

impl Actor for Philosopher {
    type Error = anyhow::Error;

    async fn on_start(&mut self, actor_ref: ActorRef) -> Result<(), Self::Error> {
        println!("Philosopher {} ({}) is joining the table.", self.id, self.name);

        // Register with the table
        let register_msg = RegisterPhilosopher {
            logical_id: self.id,
            philosopher_ref: actor_ref.clone(),
        };
        if let Err(e) = self.table_ref.tell(register_msg).await {
            eprintln!("Philosopher {} ({}): Failed to send registration to table: {:?}", self.id, self.name, e);
        }

        // Start the initial thinking cycle
        if let Err(e) = actor_ref.tell(StartThinking).await {
            eprintln!("Philosopher {} ({}): Failed to send StartThinking to self: {:?}", self.id, self.name, e);
        }
        Ok(())
    }

    async fn on_stop(&mut self, _actor_ref: ActorRef, reason: &ActorStopReason) -> Result<(), Self::Error> {
        println!(
            "Philosopher {} ({}) is leaving. Eaten: {}. Reason: {:?}.",
            self.id, self.name, self.eat_count, reason
        );
        Ok(())
    }
}

// --- Philosopher Message Handlers ---

impl Message<StartThinking> for Philosopher {
    type Reply = (); // Fire-and-forget

    async fn handle(&mut self, _msg: StartThinking, actor_ref: &ActorRef) -> Self::Reply {
        println!("Philosopher {} ({}) is thinking.", self.id, self.name);

        // Simulate thinking for a random duration.
        let think_duration = rand::rng().random_range(100..1100);
        sleep(Duration::from_millis(think_duration)).await;

        println!("Philosopher {} ({}) is hungry.", self.id, self.name);

        // --- Fork Acquisition Logic ---
        // Before attempting to pick up forks, ensure the philosopher is not already holding any.
        // This is a safeguard against inconsistent states.
        if self.has_left_fork || self.has_right_fork {
            eprintln!("Philosopher {} ({}) was in inconsistent fork state before thinking. Releasing all.", self.id, self.name);
            // If holding the left fork, send a message to the table to release it.
            if self.has_left_fork {
                let release_msg = ReleaseFork { logical_id: self.id, side: ForkSide::Left };
                if let Err(e) = self.table_ref.tell(release_msg).await {
                    eprintln!("Philosopher {} ({}): Error releasing left fork (cleanup): {:?}", self.id, self.name, e);
                }
                self.has_left_fork = false;
            }
            // If holding the right fork, send a message to the table to release it.
            if self.has_right_fork {
                let release_msg = ReleaseFork { logical_id: self.id, side: ForkSide::Right };
                if let Err(e) = self.table_ref.tell(release_msg).await {
                     eprintln!("Philosopher {} ({}): Error releasing right fork (cleanup): {:?}", self.id, self.name, e);
                }
                self.has_right_fork = false;
            }
        }

        // Attempt to acquire the Left Fork by sending a request to the Table actor.
        println!("Philosopher {} ({}) attempts to acquire Left fork.", self.id, self.name);
        let req_left_fork_msg = RequestFork { logical_id: self.id, side: ForkSide::Left };
        // The 'ask' pattern is used here to wait for a reply from the Table actor
        // indicating whether the fork was successfully acquired.
        match self.table_ref.ask::<RequestFork, bool>(req_left_fork_msg).await {
            Ok(true) => { // Successfully acquired the left fork.
                self.has_left_fork = true;
                println!("Philosopher {} ({}) acquired Left fork. Attempting Right fork.", self.id, self.name);

                // Now, attempt to acquire the Right Fork.
                let req_right_fork_msg = RequestFork { logical_id: self.id, side: ForkSide::Right };
                match self.table_ref.ask::<RequestFork, bool>(req_right_fork_msg).await {
                    Ok(true) => { // Successfully acquired the right fork as well.
                        self.has_right_fork = true;
                        println!("Philosopher {} ({}) acquired both Left and Right forks.", self.id, self.name);
                        // Both forks acquired, tell self to start eating.
                        if let Err(e) = actor_ref.tell(StartEating).await {
                            eprintln!("Philosopher {} ({}): Failed to send StartEating to self: {:?}", self.id, self.name, e);
                            // If sending StartEating fails, it's crucial to release the forks
                            // to prevent deadlock and then try thinking again.
                            self.release_both_forks().await;
                            self.think_again(actor_ref).await;
                        }
                    }
                    Ok(false) => { // Failed to acquire the right fork.
                        println!("Philosopher {} ({}) failed to get Right fork. Releasing Left fork.", self.id, self.name);
                        // Must release the already acquired left fork before thinking again.
                        let release_left_msg = ReleaseFork { logical_id: self.id, side: ForkSide::Left };
                        if let Err(e) = self.table_ref.tell(release_left_msg).await {
                             eprintln!("Philosopher {} ({}): Error releasing left fork: {:?}", self.id, self.name, e);
                        }
                        self.has_left_fork = false;
                        // Go back to thinking.
                        self.think_again(actor_ref).await;
                    }
                    Err(e) => { // An error occurred while trying to acquire the right fork.
                        eprintln!("Philosopher {} ({}) error getting Right fork: {:?}. Releasing Left fork.", self.id, self.name, e);
                        // Must release the already acquired left fork.
                        let release_left_msg = ReleaseFork { logical_id: self.id, side: ForkSide::Left };
                        if let Err(e) = self.table_ref.tell(release_left_msg).await {
                            eprintln!("Philosopher {} ({}): Error releasing left fork (on error): {:?}", self.id, self.name, e);
                        }
                        self.has_left_fork = false;
                        // Go back to thinking.
                        self.think_again(actor_ref).await;
                    }
                }
            }
            Ok(false) => { // Failed to acquire the left fork.
                println!("Philosopher {} ({}) failed to get Left fork. Thinking again.", self.id, self.name);
                // No forks acquired, so just go back to thinking.
                self.think_again(actor_ref).await;
            }
            Err(e) => { // An error occurred while trying to acquire the left fork.
                eprintln!("Philosopher {} ({}) error getting Left fork: {:?}. Thinking again.", self.id, self.name, e);
                // Go back to thinking.
                self.think_again(actor_ref).await;
            }
        }
    }
}

impl Philosopher {
    async fn release_both_forks(&mut self) {
        if self.has_left_fork {
            let release_left_msg = ReleaseFork { logical_id: self.id, side: ForkSide::Left };
            if let Err(e) = self.table_ref.tell(release_left_msg).await {
                eprintln!("Philosopher {} ({}): Error releasing left fork: {:?}", self.id, self.name, e);
            }
            self.has_left_fork = false;
        }
        if self.has_right_fork {
            let release_right_msg = ReleaseFork { logical_id: self.id, side: ForkSide::Right };
            if let Err(e) = self.table_ref.tell(release_right_msg).await {
                eprintln!("Philosopher {} ({}): Error releasing right fork: {:?}", self.id, self.name, e);
            }
            self.has_right_fork = false;
        }
    }

    async fn think_again(&self, actor_ref: &ActorRef) {
        if let Err(e) = actor_ref.tell(StartThinking).await {
            eprintln!("Philosopher {} ({}): Failed to send StartThinking to self: {:?}", self.id, self.name, e);
        }
    }
}


impl Message<StartEating> for Philosopher {
    type Reply = (); // Fire-and-forget

    async fn handle(&mut self, _msg: StartEating, actor_ref: &ActorRef) -> Self::Reply {
        if !self.has_left_fork || !self.has_right_fork {
            eprintln!("Philosopher {} ({}) tried to eat without both forks! Left: {}, Right: {}. Thinking again.",
                self.id, self.name, self.has_left_fork, self.has_right_fork);
            // Release any potentially held forks just in case, though this state should not be reached.
            self.release_both_forks().await;
            self.think_again(actor_ref).await;
            return;
        }

        self.eat_count += 1;
        println!("Philosopher {} ({}) is eating. (Meal #{})", self.id, self.name, self.eat_count);

        let eat_duration = rand::rng().random_range(500..1500);
        sleep(Duration::from_millis(eat_duration)).await;

        println!("Philosopher {} ({}) finished eating. Releasing forks.", self.id, self.name);

        // Release forks
        self.release_both_forks().await;

        // Go back to thinking
        self.think_again(actor_ref).await;
    }
}

impl_message_handler!(Philosopher, [StartThinking, StartEating]);


// --- Table Actor Definition ---
struct Table {
    /// `forks[i]` is true if fork `i` is available, false if taken.
    forks: Vec<bool>,
    /// Stores references to philosopher actors, keyed by their logical ID.
    philosophers: HashMap<usize, ActorRef>,
    // self_ref: Option<ActorRef>, // Not strictly needed for Table unless it sends messages to itself
}

impl Actor for Table {
    type Error = anyhow::Error;

    async fn on_start(&mut self, _actor_ref: ActorRef) -> Result<(), Self::Error> {
        // self.self_ref = Some(actor_ref);
        println!("Table actor is ready with {} forks.", self.forks.len());
        Ok(())
    }

    async fn on_stop(&mut self, _actor_ref: ActorRef, reason: &ActorStopReason) -> Result<(), Self::Error> {
        println!("Table actor is shutting down. Reason: {:?}", reason);
        Ok(())
    }
}

// --- Table Message Handlers ---

impl Message<RegisterPhilosopher> for Table {
    type Reply = (); // Fire-and-forget

    async fn handle(&mut self, msg: RegisterPhilosopher, _: &ActorRef) -> Self::Reply {
        println!("Table: Philosopher {} (Actor ID: {}) registered.", msg.logical_id, msg.philosopher_ref.id());
        self.philosophers.insert(msg.logical_id, msg.philosopher_ref);
    }
}

impl Message<RequestFork> for Table {
    type Reply = bool; // true if acquired, false if not.

    async fn handle(&mut self, msg: RequestFork, _: &ActorRef) -> Self::Reply {
        let philosopher_id = msg.logical_id;
        let num_forks = self.forks.len();

        let fork_idx = match msg.side {
            ForkSide::Left => philosopher_id,
            ForkSide::Right => (philosopher_id + 1) % num_forks,
        };

        if fork_idx >= num_forks {
             eprintln!("Table: Invalid fork index {} requested by Philosopher {} for side {:?}. Num forks: {}", fork_idx, philosopher_id, msg.side, num_forks);
             return false; // Should not happen with correct logical_id
        }

        if self.forks[fork_idx] { // Fork is available
            self.forks[fork_idx] = false; // Mark as taken
            println!("Table: Granted fork {} ({:?}) to Philosopher {}.", fork_idx, msg.side, philosopher_id);
            true
        } else {
            println!("Table: Fork {} ({:?}) not available for Philosopher {}.", fork_idx, msg.side, philosopher_id);
            false
        }
    }
}

impl Message<ReleaseFork> for Table {
    type Reply = (); // Fire-and-forget

    async fn handle(&mut self, msg: ReleaseFork, _: &ActorRef) -> Self::Reply {
        let philosopher_id = msg.logical_id;
        let num_forks = self.forks.len();

        let fork_idx = match msg.side {
            ForkSide::Left => philosopher_id,
            ForkSide::Right => (philosopher_id + 1) % num_forks,
        };

        if fork_idx >= num_forks {
             eprintln!("Table: Invalid fork index {} attempted to release by Philosopher {} for side {:?}. Num forks: {}", fork_idx, philosopher_id, msg.side, num_forks);
             return; // Should not happen
        }

        if !self.forks[fork_idx] { // If fork was indeed taken
            self.forks[fork_idx] = true; // Mark as available
            println!("Table: Philosopher {} returned fork {} ({:?}).", philosopher_id, fork_idx, msg.side);
        } else {
            // This might happen if a philosopher tries to release a fork it didn't successfully acquire
            // or releases it multiple times.
            println!("Table: Philosopher {} tried to return fork {} ({:?}) which was already available.", philosopher_id, fork_idx, msg.side);
        }
    }
}

impl_message_handler!(Table, [RegisterPhilosopher, RequestFork, ReleaseFork]);


// --- Main Function ---
#[tokio::main]
async fn main() -> Result<()> {
    println!("Starting Dining Philosophers simulation ({} philosophers, {}ms)...", NUM_PHILOSOPHERS, SIMULATION_TIME_MS);

    // Spawn the Table actor
    let table_actor_logic = Table {
        forks: vec![true; NUM_PHILOSOPHERS], // All forks initially available
        philosophers: HashMap::new(),
    };
    let (table_ref, table_join_handle) = spawn(table_actor_logic);
    println!("Table actor spawned with ID: {}", table_ref.id());

    // Spawn Philosopher actors
    let mut philosopher_refs: Vec<ActorRef> = Vec::new();
    let mut philosopher_join_handles = Vec::new(); // Vec<tokio::task::JoinHandle<(Philosopher, ActorStopReason)>>
    let names = ["Socrates", "Plato", "Aristotle", "Descartes", "Kant", "Nietzsche", "Confucius"]; // More names

    for i in 0..NUM_PHILOSOPHERS {
        let philosopher_name = if i < names.len() { names[i] } else { "Philosopher" }.to_string();
        let philosopher_logic = Philosopher {
            id: i,
            name: format!("{} #{}", philosopher_name, i),
            table_ref: table_ref.clone(),
            eat_count: 0,
            has_left_fork: false, // Initialize new fields
            has_right_fork: false, // Initialize new fields
        };
        let (p_ref, p_join) = spawn(philosopher_logic);
        println!("Philosopher {} spawned with Actor ID: {}", i, p_ref.id());
        philosopher_refs.push(p_ref);
        philosopher_join_handles.push(p_join);
    }

    println!("All philosophers are at the table. Simulation will run for {}ms.",
        SIMULATION_TIME_MS);
    sleep(Duration::from_millis(SIMULATION_TIME_MS)).await;
    println!("Simulation time ended.");

    // Shutdown actors gracefully
    println!("\nShutting down philosophers...");
    for p_ref in philosopher_refs.iter() {
        p_ref.stop().await.unwrap_or_else(|e| {
            eprintln!("Error stopping philosopher: {:?}", e);
        });
    }

    // Wait for actors to terminate
    println!("\nWaiting for philosophers to terminate...");
    let results = futures::future::join_all(philosopher_join_handles).await;

    println!("\nShutting down table...");
    if let Err(e) = table_ref.stop().await {
        eprintln!("Error stopping table: {:?}", e);
    }

    println!("\nWaiting for table to terminate...");
     match table_join_handle.await {
        Ok((_actor_state, stop_reason)) => println!("Table terminated. Reason: {:?}", stop_reason),
        Err(e) => eprintln!("Error joining table task: {:?}", e),
    }

    println!("\n--- Final Eat Counts ---");
    for result in &results {
        if let Ok((philosopher, _stop_reason)) = result {
            println!("Philosopher {} ({}): {} meals", philosopher.id, philosopher.name, philosopher.eat_count);
        } else {
            eprintln!("Error joining philosopher task: {:?}", result);
        }
    }

    println!("------------------------");
    println!("\nSystem has been shut down. Simulation complete.");
    Ok(())
}
