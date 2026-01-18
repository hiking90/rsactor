// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Tests for ActorControl and WeakActorControl traits

use rsactor::{message_handlers, spawn, Actor, ActorControl, ActorRef, WeakActorControl};

fn init_test_logger() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_test_writer()
        .try_init();
}

// ============================================================================
// Test Actors
// ============================================================================

/// First test actor
#[derive(Actor, Default)]
struct ActorA;

/// Second test actor (different type)
#[derive(Actor, Default)]
struct ActorB;

/// Message for fire-and-forget
struct Ping;

#[message_handlers]
impl ActorA {
    #[handler]
    async fn handle_ping(&mut self, _msg: Ping, _: &ActorRef<Self>) {}
}

#[message_handlers]
impl ActorB {
    #[handler]
    async fn handle_ping(&mut self, _msg: Ping, _: &ActorRef<Self>) {}
}

// ============================================================================
// ActorControl Tests
// ============================================================================

#[tokio::test]
async fn test_actor_control_from_actor_ref() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA);

    // Convert ActorRef to Box<dyn ActorControl>
    let control: Box<dyn ActorControl> = (&actor_ref).into();

    // Verify control properties
    assert_eq!(control.identity(), actor_ref.identity());
    assert!(control.is_alive());

    // Stop via control
    control.stop().await.expect("stop should succeed");
    let result = handle.await.expect("actor should complete");
    assert!(result.stopped_normally());
}

#[tokio::test]
async fn test_actor_control_multiple_actors() {
    init_test_logger();

    let (actor_a, handle_a) = spawn::<ActorA>(ActorA);
    let (actor_b, handle_b) = spawn::<ActorB>(ActorB);

    // Create a collection of controls for different actor types
    // This is the key advantage of ActorControl - no message type parameter!
    let controls: Vec<Box<dyn ActorControl>> = vec![(&actor_a).into(), (&actor_b).into()];

    // Verify all controls are alive
    for control in &controls {
        assert!(control.is_alive());
    }

    // Stop all actors via controls
    for control in &controls {
        control.stop().await.expect("stop should succeed");
    }

    let result_a = handle_a.await.expect("actor A should complete");
    let result_b = handle_b.await.expect("actor B should complete");
    assert!(result_a.stopped_normally());
    assert!(result_b.stopped_normally());
}

#[tokio::test]
async fn test_actor_control_stop() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA);

    let control: Box<dyn ActorControl> = (&actor_ref).into();

    // Stop via control
    control.stop().await.expect("stop should succeed");

    let result = handle.await.expect("actor should complete");
    assert!(result.stopped_normally());
    assert!(!result.was_killed());
}

#[tokio::test]
async fn test_actor_control_kill() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA);

    let control: Box<dyn ActorControl> = (&actor_ref).into();

    // Kill via control
    control.kill().expect("kill should succeed");

    let result = handle.await.expect("actor should complete");
    assert!(result.was_killed());
}

#[tokio::test]
async fn test_actor_control_clone() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA);

    let control: Box<dyn ActorControl> = (&actor_ref).into();

    // Clone via clone_boxed and Clone trait
    let control_clone1 = control.clone_boxed();
    let control_clone2 = control.clone();

    // All should have the same identity
    assert_eq!(control.identity(), control_clone1.identity());
    assert_eq!(control.identity(), control_clone2.identity());

    // All should report alive
    assert!(control.is_alive());
    assert!(control_clone1.is_alive());
    assert!(control_clone2.is_alive());

    actor_ref.stop().await.expect("stop should succeed");
    let result = handle.await.expect("actor should complete");
    assert!(result.stopped_normally());
}

#[tokio::test]
async fn test_actor_control_downgrade() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA);

    let control: Box<dyn ActorControl> = (&actor_ref).into();

    // Downgrade to weak control
    let weak_control: Box<dyn WeakActorControl> = control.downgrade();

    // Both should have the same identity
    assert_eq!(control.identity(), weak_control.identity());
    assert!(weak_control.is_alive());

    actor_ref.stop().await.expect("stop should succeed");
    let _ = handle.await;
}

#[tokio::test]
async fn test_actor_control_downgrade_upgrade_roundtrip() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA);

    // Start with strong control
    let control1: Box<dyn ActorControl> = (&actor_ref).into();

    // Downgrade to weak
    let weak_control = control1.downgrade();

    // Upgrade back to strong
    let control2 = weak_control.upgrade().expect("upgrade should succeed");

    // All should have the same identity
    assert_eq!(control1.identity(), weak_control.identity());
    assert_eq!(weak_control.identity(), control2.identity());

    // Can use the upgraded control to stop
    control2.stop().await.expect("stop should succeed");
    let result = handle.await.expect("actor should complete");
    assert!(result.stopped_normally());
}

// ============================================================================
// WeakActorControl Tests
// ============================================================================

#[tokio::test]
async fn test_weak_actor_control_upgrade() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA);

    let weak_control: Box<dyn WeakActorControl> = ActorRef::downgrade(&actor_ref).into();

    // Upgrade should succeed while actor is alive
    let strong = weak_control.upgrade().expect("upgrade should succeed");
    assert_eq!(strong.identity(), actor_ref.identity());

    actor_ref.stop().await.expect("stop should succeed");
    let _ = handle.await;
}

#[tokio::test]
async fn test_weak_actor_control_upgrade_after_stop() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA);

    let weak_control: Box<dyn WeakActorControl> = ActorRef::downgrade(&actor_ref).into();

    // Stop and drop the actor
    actor_ref.stop().await.expect("stop should succeed");
    drop(actor_ref);

    let _ = handle.await;

    // Give some time for cleanup
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Upgrade should fail now
    assert!(weak_control.upgrade().is_none());
    assert!(!weak_control.is_alive());
}

#[tokio::test]
async fn test_weak_actor_control_clone() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA);

    let weak_control: Box<dyn WeakActorControl> = ActorRef::downgrade(&actor_ref).into();
    let weak_clone = weak_control.clone();

    // Both should have same identity
    assert_eq!(weak_control.identity(), weak_clone.identity());

    // Both should be able to upgrade
    assert!(weak_control.upgrade().is_some());
    assert!(weak_clone.upgrade().is_some());

    actor_ref.stop().await.expect("stop should succeed");
    let _ = handle.await;
}

// ============================================================================
// Debug and Send+Sync Tests
// ============================================================================

#[tokio::test]
async fn test_actor_control_debug() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA);

    let control: Box<dyn ActorControl> = (&actor_ref).into();
    let weak_control: Box<dyn WeakActorControl> = ActorRef::downgrade(&actor_ref).into();

    // Test Debug formatting
    let control_debug = format!("{:?}", control);
    let weak_debug = format!("{:?}", weak_control);

    assert!(control_debug.contains("ActorControl"));
    assert!(weak_debug.contains("WeakActorControl"));

    actor_ref.stop().await.expect("stop should succeed");
    let _ = handle.await;
}

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn test_actor_control_is_send_sync() {
    assert_send_sync::<Box<dyn ActorControl>>();
    assert_send_sync::<Box<dyn WeakActorControl>>();
}
