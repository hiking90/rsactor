// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Tests for handler traits (TellHandler, AskHandler, WeakTellHandler, WeakAskHandler)

use rsactor::{
    message_handlers, spawn, Actor, ActorRef, AskHandler, TellHandler, WeakAskHandler,
    WeakTellHandler,
};

fn init_test_logger() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();
}

// ============================================================================
// Test Actors
// ============================================================================

/// First test actor that handles Ping messages
#[derive(Actor)]
struct ActorA {
    name: String,
}

/// Second test actor that also handles Ping messages (different implementation)
#[derive(Actor)]
struct ActorB {
    id: u32,
}

/// Message for fire-and-forget
struct Ping;

/// Message for request-response
struct GetName;

/// Status response type
#[derive(Debug, Clone, PartialEq)]
struct Status {
    name: String,
    alive: bool,
}

#[message_handlers]
impl ActorA {
    #[handler]
    async fn handle_ping(&mut self, _msg: Ping, _: &ActorRef<Self>) {
        // Fire-and-forget, just acknowledge
    }

    #[handler]
    async fn handle_get_name(&mut self, _msg: GetName, _: &ActorRef<Self>) -> Status {
        Status {
            name: self.name.clone(),
            alive: true,
        }
    }
}

#[message_handlers]
impl ActorB {
    #[handler]
    async fn handle_ping(&mut self, _msg: Ping, _: &ActorRef<Self>) {
        // Fire-and-forget, just acknowledge
    }

    #[handler]
    async fn handle_get_name(&mut self, _msg: GetName, _: &ActorRef<Self>) -> Status {
        Status {
            name: format!("ActorB-{}", self.id),
            alive: true,
        }
    }
}

// ============================================================================
// TellHandler Tests
// ============================================================================

#[tokio::test]
async fn test_tell_handler_from_actor_ref() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA {
        name: "test".to_string(),
    });

    // Convert ActorRef to Box<dyn TellHandler<Ping>>
    let handler: Box<dyn TellHandler<Ping>> = (&actor_ref).into();

    // Use the handler
    handler.tell(Ping).await.expect("tell should succeed");

    // Verify handler properties
    assert_eq!(handler.identity(), actor_ref.identity());
    assert!(handler.is_alive());

    // Stop via handler
    handler.stop().await.expect("stop should succeed");
    let result = handle.await.expect("actor should complete");
    assert!(result.stopped_normally());
}

#[tokio::test]
async fn test_tell_handler_multiple_actors_same_message() {
    init_test_logger();

    let (actor_a, handle_a) = spawn::<ActorA>(ActorA {
        name: "A".to_string(),
    });
    let (actor_b, handle_b) = spawn::<ActorB>(ActorB { id: 42 });

    // Create a collection of handlers for the same message type
    let handlers: Vec<Box<dyn TellHandler<Ping>>> = vec![(&actor_a).into(), (&actor_b).into()];

    // Send messages to all handlers
    for handler in &handlers {
        handler.tell(Ping).await.expect("tell should succeed");
    }

    // Verify all handlers are alive
    for handler in &handlers {
        assert!(handler.is_alive());
    }

    // Stop all actors via handlers
    for handler in &handlers {
        handler.stop().await.expect("stop should succeed");
    }

    let result_a = handle_a.await.expect("actor A should complete");
    let result_b = handle_b.await.expect("actor B should complete");
    assert!(result_a.stopped_normally());
    assert!(result_b.stopped_normally());
}

#[tokio::test]
async fn test_tell_handler_clone_boxed() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA {
        name: "clone_test".to_string(),
    });

    let handler: Box<dyn TellHandler<Ping>> = (&actor_ref).into();

    // Clone using clone_boxed
    let handler_clone = handler.clone_boxed();

    // Both should have the same identity
    assert_eq!(handler.identity(), handler_clone.identity());

    // Both should work
    handler
        .tell(Ping)
        .await
        .expect("original handler tell should succeed");
    handler_clone
        .tell(Ping)
        .await
        .expect("cloned handler tell should succeed");

    actor_ref.stop().await.expect("stop should succeed");
    let result = handle.await.expect("actor should complete");
    assert!(result.stopped_normally());
}

#[tokio::test]
async fn test_tell_handler_clone_trait() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA {
        name: "clone_trait_test".to_string(),
    });

    let handlers: Vec<Box<dyn TellHandler<Ping>>> = vec![(&actor_ref).into()];

    // Clone via Clone trait on Box<dyn TellHandler<M>>
    let handlers_clone = handlers.clone();

    assert_eq!(handlers[0].identity(), handlers_clone[0].identity());

    handlers[0].tell(Ping).await.expect("should work");
    handlers_clone[0].tell(Ping).await.expect("should work");

    actor_ref.stop().await.expect("stop should succeed");
    let result = handle.await.expect("actor should complete");
    assert!(result.stopped_normally());
}

#[tokio::test]
async fn test_tell_handler_debug() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA {
        name: "debug_test".to_string(),
    });

    let handler: Box<dyn TellHandler<Ping>> = (&actor_ref).into();

    // Test Debug formatting
    let debug_str = format!("{:?}", handler);
    assert!(debug_str.contains("TellHandler"));
    assert!(debug_str.contains("identity"));

    actor_ref.stop().await.expect("stop should succeed");
    let _ = handle.await;
}

#[tokio::test]
async fn test_tell_handler_kill() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA {
        name: "kill_test".to_string(),
    });

    let handler: Box<dyn TellHandler<Ping>> = (&actor_ref).into();

    // Kill via handler
    handler.kill().expect("kill should succeed");

    let result = handle.await.expect("actor should complete");
    assert!(result.was_killed());
}

// ============================================================================
// AskHandler Tests
// ============================================================================

#[tokio::test]
async fn test_ask_handler_from_actor_ref() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA {
        name: "ask_test".to_string(),
    });

    // Convert ActorRef to Box<dyn AskHandler<GetName, Status>>
    let handler: Box<dyn AskHandler<GetName, Status>> = (&actor_ref).into();

    // Use the handler
    let status = handler.ask(GetName).await.expect("ask should succeed");

    assert_eq!(status.name, "ask_test");
    assert!(status.alive);

    handler.stop().await.expect("stop should succeed");
    let result = handle.await.expect("actor should complete");
    assert!(result.stopped_normally());
}

#[tokio::test]
async fn test_ask_handler_multiple_actors_same_reply_type() {
    init_test_logger();

    let (actor_a, handle_a) = spawn::<ActorA>(ActorA {
        name: "ActorA".to_string(),
    });
    let (actor_b, handle_b) = spawn::<ActorB>(ActorB { id: 100 });

    // Create a collection of handlers with same message and reply types
    let handlers: Vec<Box<dyn AskHandler<GetName, Status>>> =
        vec![(&actor_a).into(), (&actor_b).into()];

    // Collect responses from all handlers
    let mut responses = Vec::new();
    for handler in &handlers {
        let status = handler.ask(GetName).await.expect("ask should succeed");
        responses.push(status);
    }

    // Verify responses
    assert_eq!(responses[0].name, "ActorA");
    assert_eq!(responses[1].name, "ActorB-100");

    // Stop all actors
    for handler in &handlers {
        handler.stop().await.expect("stop should succeed");
    }

    let _ = handle_a.await;
    let _ = handle_b.await;
}

#[tokio::test]
async fn test_ask_handler_clone() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA {
        name: "clone_ask".to_string(),
    });

    let handler: Box<dyn AskHandler<GetName, Status>> = (&actor_ref).into();
    let handler_clone = handler.clone();

    // Both should return the same result
    let status1 = handler.ask(GetName).await.expect("should work");
    let status2 = handler_clone.ask(GetName).await.expect("should work");

    assert_eq!(status1, status2);

    actor_ref.stop().await.expect("stop should succeed");
    let _ = handle.await;
}

// ============================================================================
// WeakTellHandler Tests
// ============================================================================

#[tokio::test]
async fn test_weak_tell_handler_from_actor_weak() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA {
        name: "weak_test".to_string(),
    });

    let actor_weak = ActorRef::downgrade(&actor_ref);

    // Convert ActorWeak to Box<dyn WeakTellHandler<Ping>>
    let weak_handler: Box<dyn WeakTellHandler<Ping>> = actor_weak.into();

    // Verify identity
    assert_eq!(weak_handler.identity(), actor_ref.identity());
    assert!(weak_handler.is_alive());

    // Upgrade and use
    let strong = weak_handler
        .upgrade()
        .expect("upgrade should succeed while actor is alive");
    strong.tell(Ping).await.expect("tell should succeed");

    actor_ref.stop().await.expect("stop should succeed");
    let result = handle.await.expect("actor should complete");
    assert!(result.stopped_normally());
}

#[tokio::test]
async fn test_weak_tell_handler_upgrade_after_stop() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA {
        name: "weak_upgrade_test".to_string(),
    });

    let actor_weak = ActorRef::downgrade(&actor_ref);
    let weak_handler: Box<dyn WeakTellHandler<Ping>> = actor_weak.into();

    // Stop the actor
    actor_ref.stop().await.expect("stop should succeed");
    drop(actor_ref);

    let _ = handle.await;

    // Give some time for cleanup
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Upgrade should fail now
    assert!(weak_handler.upgrade().is_none());
    assert!(!weak_handler.is_alive());
}

#[tokio::test]
async fn test_weak_tell_handler_multiple_actors() {
    init_test_logger();

    let (actor_a, handle_a) = spawn::<ActorA>(ActorA {
        name: "WeakA".to_string(),
    });
    let (actor_b, handle_b) = spawn::<ActorB>(ActorB { id: 50 });

    // Create weak handlers for multiple actors
    let weak_handlers: Vec<Box<dyn WeakTellHandler<Ping>>> = vec![
        ActorRef::downgrade(&actor_a).into(),
        ActorRef::downgrade(&actor_b).into(),
    ];

    // Upgrade and use all handlers
    for weak_handler in &weak_handlers {
        if let Some(strong) = weak_handler.upgrade() {
            strong.tell(Ping).await.expect("tell should succeed");
        }
    }

    // Stop actors
    actor_a.stop().await.expect("stop should succeed");
    actor_b.stop().await.expect("stop should succeed");

    let _ = handle_a.await;
    let _ = handle_b.await;
}

#[tokio::test]
async fn test_weak_tell_handler_batch_operations() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA {
        name: "batch_test".to_string(),
    });

    let weak_handler: Box<dyn WeakTellHandler<Ping>> = ActorRef::downgrade(&actor_ref).into();

    // Batch operations with single upgrade
    if let Some(strong) = weak_handler.upgrade() {
        strong.tell(Ping).await.expect("should work");
        strong.tell(Ping).await.expect("should work");
        strong.tell(Ping).await.expect("should work");
    }

    actor_ref.stop().await.expect("stop should succeed");
    let result = handle.await.expect("actor should complete");
    assert!(result.stopped_normally());
}

#[tokio::test]
async fn test_weak_tell_handler_clone() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA {
        name: "weak_clone".to_string(),
    });

    let weak_handler: Box<dyn WeakTellHandler<Ping>> = ActorRef::downgrade(&actor_ref).into();
    let weak_clone = weak_handler.clone();

    // Both should have same identity
    assert_eq!(weak_handler.identity(), weak_clone.identity());

    // Both should be able to upgrade
    assert!(weak_handler.upgrade().is_some());
    assert!(weak_clone.upgrade().is_some());

    actor_ref.stop().await.expect("stop should succeed");
    let _ = handle.await;
}

// ============================================================================
// WeakAskHandler Tests
// ============================================================================

#[tokio::test]
async fn test_weak_ask_handler_from_actor_weak() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA {
        name: "weak_ask".to_string(),
    });

    let weak_handler: Box<dyn WeakAskHandler<GetName, Status>> =
        ActorRef::downgrade(&actor_ref).into();

    // Upgrade and ask
    let strong = weak_handler.upgrade().expect("upgrade should succeed");
    let status = strong.ask(GetName).await.expect("ask should succeed");

    assert_eq!(status.name, "weak_ask");

    actor_ref.stop().await.expect("stop should succeed");
    let _ = handle.await;
}

#[tokio::test]
async fn test_weak_ask_handler_multiple_actors() {
    init_test_logger();

    let (actor_a, handle_a) = spawn::<ActorA>(ActorA {
        name: "WeakAskA".to_string(),
    });
    let (actor_b, handle_b) = spawn::<ActorB>(ActorB { id: 77 });

    let weak_handlers: Vec<Box<dyn WeakAskHandler<GetName, Status>>> = vec![
        ActorRef::downgrade(&actor_a).into(),
        ActorRef::downgrade(&actor_b).into(),
    ];

    // Collect responses
    let mut responses = Vec::new();
    for weak_handler in &weak_handlers {
        if let Some(strong) = weak_handler.upgrade() {
            let status = strong.ask(GetName).await.expect("ask should succeed");
            responses.push(status);
        }
    }

    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0].name, "WeakAskA");
    assert_eq!(responses[1].name, "ActorB-77");

    actor_a.stop().await.expect("stop should succeed");
    actor_b.stop().await.expect("stop should succeed");

    let _ = handle_a.await;
    let _ = handle_b.await;
}

// ============================================================================
// Interop Tests (Strong <-> Weak)
// ============================================================================

#[tokio::test]
async fn test_downgrade_from_tell_handler() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA {
        name: "downgrade_test".to_string(),
    });

    let strong_handler: Box<dyn TellHandler<Ping>> = (&actor_ref).into();

    // Downgrade strong handler to weak handler
    let weak_handler: Box<dyn WeakTellHandler<Ping>> = strong_handler.downgrade();

    assert_eq!(strong_handler.identity(), weak_handler.identity());
    assert!(weak_handler.is_alive());

    // Upgrade back and use
    let upgraded = weak_handler.upgrade().expect("should be able to upgrade");
    upgraded.tell(Ping).await.expect("tell should work");

    actor_ref.stop().await.expect("stop should succeed");
    let _ = handle.await;
}

#[tokio::test]
async fn test_downgrade_from_ask_handler() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA {
        name: "ask_downgrade".to_string(),
    });

    let strong_handler: Box<dyn AskHandler<GetName, Status>> = (&actor_ref).into();

    // Downgrade strong handler to weak handler
    let weak_handler: Box<dyn WeakAskHandler<GetName, Status>> = strong_handler.downgrade();

    assert_eq!(strong_handler.identity(), weak_handler.identity());

    // Upgrade back and ask
    let upgraded = weak_handler.upgrade().expect("should be able to upgrade");
    let status = upgraded.ask(GetName).await.expect("ask should work");

    assert_eq!(status.name, "ask_downgrade");

    actor_ref.stop().await.expect("stop should succeed");
    let _ = handle.await;
}

#[tokio::test]
async fn test_roundtrip_strong_weak_strong() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA {
        name: "roundtrip".to_string(),
    });

    // Start with strong
    let strong1: Box<dyn TellHandler<Ping>> = (&actor_ref).into();

    // Downgrade to weak
    let weak: Box<dyn WeakTellHandler<Ping>> = strong1.downgrade();

    // Upgrade back to strong
    let strong2 = weak.upgrade().expect("should upgrade");

    // All should have same identity
    assert_eq!(strong1.identity(), weak.identity());
    assert_eq!(weak.identity(), strong2.identity());

    // All should work
    strong1.tell(Ping).await.expect("should work");
    strong2.tell(Ping).await.expect("should work");

    actor_ref.stop().await.expect("stop should succeed");
    let _ = handle.await;
}

// ============================================================================
// From Trait Tests
// ============================================================================

#[tokio::test]
async fn test_from_actor_ref_ownership() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA {
        name: "ownership".to_string(),
    });

    let actor_ref_clone = actor_ref.clone();

    // From<ActorRef<T>> - moves ownership
    let handler: Box<dyn TellHandler<Ping>> = actor_ref_clone.into();

    handler.tell(Ping).await.expect("should work");

    actor_ref.stop().await.expect("stop should succeed");
    let _ = handle.await;
}

#[tokio::test]
async fn test_from_actor_ref_reference() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA {
        name: "reference".to_string(),
    });

    // From<&ActorRef<T>> - clones the reference
    let handler: Box<dyn TellHandler<Ping>> = (&actor_ref).into();

    handler.tell(Ping).await.expect("should work");

    // Original actor_ref is still usable
    actor_ref.tell(Ping).await.expect("should work");

    actor_ref.stop().await.expect("stop should succeed");
    let _ = handle.await;
}

#[tokio::test]
async fn test_from_actor_weak_ownership() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA {
        name: "weak_ownership".to_string(),
    });

    let actor_weak = ActorRef::downgrade(&actor_ref);

    // From<ActorWeak<T>> - moves ownership
    let handler: Box<dyn WeakTellHandler<Ping>> = actor_weak.into();

    assert!(handler.upgrade().is_some());

    actor_ref.stop().await.expect("stop should succeed");
    let _ = handle.await;
}

#[tokio::test]
async fn test_from_actor_weak_reference() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA {
        name: "weak_reference".to_string(),
    });

    let actor_weak = ActorRef::downgrade(&actor_ref);

    // From<&ActorWeak<T>> - clones the reference
    let handler: Box<dyn WeakTellHandler<Ping>> = (&actor_weak).into();

    // Both should work
    assert!(handler.upgrade().is_some());
    assert!(actor_weak.upgrade().is_some());

    actor_ref.stop().await.expect("stop should succeed");
    let _ = handle.await;
}

// ============================================================================
// Timeout and Blocking Tests
// ============================================================================

#[tokio::test]
async fn test_tell_handler_with_timeout() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA {
        name: "timeout_test".to_string(),
    });

    let handler: Box<dyn TellHandler<Ping>> = (&actor_ref).into();

    // Should succeed with reasonable timeout
    handler
        .tell_with_timeout(Ping, std::time::Duration::from_secs(5))
        .await
        .expect("tell_with_timeout should succeed");

    actor_ref.stop().await.expect("stop should succeed");
    let _ = handle.await;
}

#[tokio::test]
async fn test_ask_handler_with_timeout() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA {
        name: "ask_timeout".to_string(),
    });

    let handler: Box<dyn AskHandler<GetName, Status>> = (&actor_ref).into();

    // Should succeed with reasonable timeout
    let status = handler
        .ask_with_timeout(GetName, std::time::Duration::from_secs(5))
        .await
        .expect("ask_with_timeout should succeed");

    assert_eq!(status.name, "ask_timeout");

    actor_ref.stop().await.expect("stop should succeed");
    let _ = handle.await;
}

// ============================================================================
// Debug Formatting Tests
// ============================================================================

#[tokio::test]
async fn test_all_handlers_debug_formatting() {
    init_test_logger();

    let (actor_ref, handle) = spawn::<ActorA>(ActorA {
        name: "debug_all".to_string(),
    });

    let tell_handler: Box<dyn TellHandler<Ping>> = (&actor_ref).into();
    let ask_handler: Box<dyn AskHandler<GetName, Status>> = (&actor_ref).into();
    let weak_tell_handler: Box<dyn WeakTellHandler<Ping>> = ActorRef::downgrade(&actor_ref).into();
    let weak_ask_handler: Box<dyn WeakAskHandler<GetName, Status>> =
        ActorRef::downgrade(&actor_ref).into();

    // All should have debug output
    let tell_debug = format!("{:?}", tell_handler);
    let ask_debug = format!("{:?}", ask_handler);
    let weak_tell_debug = format!("{:?}", weak_tell_handler);
    let weak_ask_debug = format!("{:?}", weak_ask_handler);

    assert!(tell_debug.contains("TellHandler"));
    assert!(ask_debug.contains("AskHandler"));
    assert!(weak_tell_debug.contains("WeakTellHandler"));
    assert!(weak_ask_debug.contains("WeakAskHandler"));

    actor_ref.stop().await.expect("stop should succeed");
    let _ = handle.await;
}

// ============================================================================
// Send + Sync Tests
// ============================================================================

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn test_handlers_are_send_sync() {
    assert_send_sync::<Box<dyn TellHandler<Ping>>>();
    assert_send_sync::<Box<dyn AskHandler<GetName, Status>>>();
    assert_send_sync::<Box<dyn WeakTellHandler<Ping>>>();
    assert_send_sync::<Box<dyn WeakAskHandler<GetName, Status>>>();
}
