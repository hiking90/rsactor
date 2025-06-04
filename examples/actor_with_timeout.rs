// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Example demonstrating the use of ask_with_timeout method for actor communication
//! with timeout functionality.

use anyhow::Result;
use log::{debug, info};
use rsactor::{impl_message_handler, spawn, Actor, ActorRef, Message};
use std::time::Duration;

// Define an actor that can process requests with varying response times
struct TimeoutDemoActor {
    name: String,
}

// Implement the Actor trait
impl Actor for TimeoutDemoActor {
    type Args = String;
    type Error = anyhow::Error;

    async fn on_start(args: Self::Args, actor_ref: &ActorRef<Self>) -> Result<Self, Self::Error> {
        info!("{} actor (id: {}) started", args, actor_ref.identity());
        Ok(Self { name: args })
    }
}

// Define message types
struct FastQuery(String);
struct SlowQuery(String);
struct ConfigurableQuery {
    question: String,
    delay_ms: u64,
}

// Implement message handlers
impl Message<FastQuery> for TimeoutDemoActor {
    type Reply = String;

    async fn handle(&mut self, msg: FastQuery, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        // This is a fast handler that completes quickly
        debug!("{} handling a FastQuery: {}", self.name, msg.0);
        format!("Fast response to: {}", msg.0)
    }
}

impl Message<SlowQuery> for TimeoutDemoActor {
    type Reply = String;

    async fn handle(&mut self, msg: SlowQuery, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        // This is a slow handler that takes time to complete
        debug!(
            "{} handling a SlowQuery: {}. Will take 500ms",
            self.name, msg.0
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
        format!("Slow response to: {}", msg.0)
    }
}

impl Message<ConfigurableQuery> for TimeoutDemoActor {
    type Reply = String;

    async fn handle(&mut self, msg: ConfigurableQuery, _actor_ref: &ActorRef<Self>) -> Self::Reply {
        debug!(
            "{} handling ConfigurableQuery with delay {}ms: {}",
            self.name, msg.delay_ms, msg.question
        );
        tokio::time::sleep(Duration::from_millis(msg.delay_ms)).await;
        format!("Response after {}ms to: {}", msg.delay_ms, msg.question)
    }
}

// Use the macro to implement the MessageHandler trait
impl_message_handler!(TimeoutDemoActor, [FastQuery, SlowQuery, ConfigurableQuery]);

// Demo helper function for ask_with_timeout
async fn demonstrate_ask_with_timeout(
    actor_ref: &ActorRef<TimeoutDemoActor>,
    query: &str,
    timeout_ms: u64,
    expected_delay_ms: u64,
) {
    let timer = std::time::Instant::now();
    let query_msg = ConfigurableQuery {
        question: query.to_string(),
        delay_ms: expected_delay_ms,
    };

    let result: Result<String, _> = actor_ref
        .ask_with_timeout(query_msg, Duration::from_millis(timeout_ms))
        .await;
    match result {
        Ok(response) => {
            let elapsed = timer.elapsed().as_millis();
            info!(
                "✅ Success! Response received in {}ms: {}",
                elapsed, response
            );
        }
        Err(e) => {
            let elapsed = timer.elapsed().as_millis();
            info!("❌ Failed after {}ms. Error: {}", elapsed, e);
        }
    }
}

// Demo helper function for tell_with_timeout
async fn demonstrate_tell_with_timeout(
    actor_ref: &ActorRef<TimeoutDemoActor>,
    query: &str,
    timeout_ms: u64,
    expected_delay_ms: u64,
) {
    let timer = std::time::Instant::now();
    let query_msg = ConfigurableQuery {
        question: query.to_string(),
        delay_ms: expected_delay_ms,
    };

    let result = actor_ref
        .tell_with_timeout(query_msg, Duration::from_millis(timeout_ms))
        .await;
    match result {
        Ok(_) => {
            let elapsed = timer.elapsed().as_millis();
            info!("✅ Success! Message sent in {}ms", elapsed);
        }
        Err(e) => {
            let elapsed = timer.elapsed().as_millis();
            info!("❌ Failed to send after {}ms. Error: {}", elapsed, e);
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Setup logger for the example
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    info!("Starting actor_with_timeout example");

    // Create and spawn the actor
    let (actor_ref, join_handle) = spawn::<TimeoutDemoActor>("TimeoutDemo".to_string());

    // Fast query with plenty of time - should succeed
    info!("\n=== Test 1: Fast query with long timeout (100ms) ===");
    let result1: Result<String, _> = actor_ref
        .ask_with_timeout(
            FastQuery("What is your name?".to_string()),
            Duration::from_millis(100),
        )
        .await;
    match &result1 {
        Ok(response) => info!("✅ Success: {}", response),
        Err(e) => info!("❌ Failed: {}", e),
    }
    assert!(
        result1.is_ok(),
        "Fast query should succeed with sufficient timeout"
    );

    // Slow query with insufficient time - should timeout
    info!("\n=== Test 2: Slow query with short timeout (100ms < 500ms) ===");
    let result2: Result<String, _> = actor_ref
        .ask_with_timeout(
            SlowQuery("How old are you?".to_string()),
            Duration::from_millis(100),
        )
        .await;
    match &result2 {
        Ok(response) => info!("✅ Success: {}", response),
        Err(e) => info!("❌ Failed: {}", e),
    }
    assert!(
        result2.is_err(),
        "Slow query should timeout with insufficient time"
    );

    // Slow query with enough time - should succeed
    info!("\n=== Test 3: Slow query with sufficient timeout (1000ms > 500ms) ===");
    let result3: Result<String, _> = actor_ref
        .ask_with_timeout(
            SlowQuery("What's your favorite color?".to_string()),
            Duration::from_millis(1000),
        )
        .await;
    match &result3 {
        Ok(response) => info!("✅ Success: {}", response),
        Err(e) => info!("❌ Failed: {}", e),
    }
    assert!(
        result3.is_ok(),
        "Slow query should succeed with sufficient timeout"
    );

    // Series of configurable queries with different timeout combinations
    info!("\n=== Test 4: Multiple configurable queries with various timeouts ===");

    // Scenario 1: Timeout > Processing time (should succeed)
    demonstrate_ask_with_timeout(&actor_ref, "Query that should succeed", 200, 100).await;

    // Scenario 2: Timeout < Processing time (should timeout)
    demonstrate_ask_with_timeout(&actor_ref, "Query that should timeout", 100, 300).await;

    // Scenario 3: Timeout == Processing time (might succeed or fail depending on timing)
    demonstrate_ask_with_timeout(&actor_ref, "Query with exact timing", 200, 200).await;

    // Scenario 4: Very tight timing (likely to fail)
    demonstrate_ask_with_timeout(&actor_ref, "Query with tight timing", 50, 49).await;

    // Demo for tell_with_timeout
    info!("\n=== Test 5: Demonstrating tell_with_timeout ===");

    // Scenario 1: Message with sufficient timeout (should succeed)
    demonstrate_tell_with_timeout(
        &actor_ref,
        "Message that should be sent successfully",
        200,
        0,
    )
    .await;

    // Scenario 2: In most cases, tell_with_timeout won't timeout since sending a message is usually very fast
    // But we can still demonstrate the API
    demonstrate_tell_with_timeout(&actor_ref, "Message with very short timeout", 1, 0).await;

    // Stop the actor gracefully and wait for it to terminate
    info!("\n=== Stopping actor ===");
    actor_ref.stop().await?;
    let result = join_handle.await?;

    match result {
        rsactor::ActorResult::Completed { actor: _, killed } => {
            info!("Actor stopped successfully. Killed: {}", killed);
        }
        rsactor::ActorResult::Failed {
            error,
            killed,
            phase,
            ..
        } => {
            info!(
                "Actor stop failed: {}. Killed: {}, Phase: {}",
                error, killed, phase
            );
        }
    }
    info!("Example finished successfully");

    Ok(())
}
