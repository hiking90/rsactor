// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Deadlock Detection Tests
//!
//! Tests for runtime deadlock detection via wait-for graph cycle analysis.
//! Requires the `deadlock-detection` feature to be enabled.

use rsactor::{spawn, Actor, ActorRef, Message};

// ============================================================
// Shared utility actor — handles Ping, ForwardPing, SequentialPing
// ============================================================

#[derive(Debug)]
struct WorkerActor;

#[derive(Debug)]
struct Ping;

// Contains ActorRef, so Debug cannot be derived automatically.
struct ForwardPing(ActorRef<WorkerActor>);
struct SequentialPing(ActorRef<WorkerActor>, ActorRef<WorkerActor>);

impl Actor for WorkerActor {
    type Args = ();
    type Error = anyhow::Error;

    async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(WorkerActor)
    }
}

impl Message<Ping> for WorkerActor {
    type Reply = String;

    async fn handle(&mut self, _: Ping, _: &ActorRef<Self>) -> String {
        "pong".to_string()
    }
}

impl Message<ForwardPing> for WorkerActor {
    type Reply = String;

    async fn handle(&mut self, msg: ForwardPing, _: &ActorRef<Self>) -> String {
        msg.0.ask(Ping).await.unwrap()
    }
}

impl Message<SequentialPing> for WorkerActor {
    type Reply = (String, String);

    async fn handle(&mut self, msg: SequentialPing, _: &ActorRef<Self>) -> (String, String) {
        let r1 = msg.0.ask(Ping).await.unwrap();
        let r2 = msg.1.ask(Ping).await.unwrap();
        (r1, r2)
    }
}

// ============================================================
// Self-ask deadlock
// ============================================================

#[derive(Debug)]
struct SelfAskActor;

#[derive(Debug)]
struct TriggerSelfAsk;

impl Actor for SelfAskActor {
    type Args = ();
    type Error = anyhow::Error;

    async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(SelfAskActor)
    }
}

impl Message<TriggerSelfAsk> for SelfAskActor {
    type Reply = ();

    async fn handle(&mut self, _: TriggerSelfAsk, actor_ref: &ActorRef<Self>) -> () {
        let _ = actor_ref.ask(TriggerSelfAsk).await;
    }
}

#[tokio::test]
async fn test_self_ask_deadlock_detected() {
    let (actor_ref, join_handle) = spawn::<SelfAskActor>(());

    // The handler calls self.ask() → deadlock detection panics inside the actor task.
    // The reply channel is dropped → ask returns Err.
    let _ = actor_ref.ask(TriggerSelfAsk).await;

    let join_err = join_handle.await.unwrap_err();
    assert!(join_err.is_panic());
    let payload = join_err.into_panic();
    let msg = payload
        .downcast_ref::<String>()
        .expect("panic payload should be String");
    assert!(
        msg.contains("Deadlock detected"),
        "Expected deadlock panic, got: {msg}"
    );
}

// ============================================================
// 2-actor cycle: A → B → A
// ============================================================

#[derive(Debug)]
struct CycleActorA;
#[derive(Debug)]
struct CycleActorB;

struct StartCycle(ActorRef<CycleActorB>);

struct PingFromA(ActorRef<CycleActorA>);

#[derive(Debug)]
struct CyclePong;

impl Actor for CycleActorA {
    type Args = ();
    type Error = anyhow::Error;

    async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(CycleActorA)
    }
}

impl Actor for CycleActorB {
    type Args = ();
    type Error = anyhow::Error;

    async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(CycleActorB)
    }
}

impl Message<StartCycle> for CycleActorA {
    type Reply = String;

    async fn handle(&mut self, msg: StartCycle, actor_ref: &ActorRef<Self>) -> String {
        // Ask B, passing our ref so B can ask us back
        msg.0
            .ask(PingFromA(actor_ref.clone()))
            .await
            .unwrap_or_default()
    }
}

impl Message<CyclePong> for CycleActorA {
    type Reply = String;

    async fn handle(&mut self, _: CyclePong, _: &ActorRef<Self>) -> String {
        "pong from A".to_string()
    }
}

impl Message<PingFromA> for CycleActorB {
    type Reply = String;

    async fn handle(&mut self, msg: PingFromA, _: &ActorRef<Self>) -> String {
        // Ask A back → deadlock detected (A→B already in graph)
        msg.0.ask(CyclePong).await.unwrap_or_default()
    }
}

#[tokio::test]
async fn test_two_actor_cycle_deadlock_detected() {
    let (a_ref, a_handle) = spawn::<CycleActorA>(());
    let (b_ref, b_handle) = spawn::<CycleActorB>(());

    // A asks B, B asks A back → deadlock detected in B's task
    let _ = a_ref.ask(StartCycle(b_ref.clone())).await;

    let b_err = b_handle.await.unwrap_err();
    assert!(b_err.is_panic());
    let payload = b_err.into_panic();
    let msg = payload
        .downcast_ref::<String>()
        .expect("panic payload should be String");
    assert!(
        msg.contains("Deadlock detected"),
        "Expected deadlock panic, got: {msg}"
    );

    drop(a_ref);
    drop(b_ref);
    let _ = a_handle.await;
}

// ============================================================
// 3-actor chain: A → B → C → A
// ============================================================

#[derive(Debug)]
struct ChainActorA;
#[derive(Debug)]
struct ChainActorB;
#[derive(Debug)]
struct ChainActorC;

struct StartChain {
    b_ref: ActorRef<ChainActorB>,
    c_ref: ActorRef<ChainActorC>,
}

struct ForwardToC {
    a_ref: ActorRef<ChainActorA>,
    c_ref: ActorRef<ChainActorC>,
}

struct AskBackToA(ActorRef<ChainActorA>);

#[derive(Debug)]
struct ChainPong;

impl Actor for ChainActorA {
    type Args = ();
    type Error = anyhow::Error;

    async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(ChainActorA)
    }
}

impl Actor for ChainActorB {
    type Args = ();
    type Error = anyhow::Error;

    async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(ChainActorB)
    }
}

impl Actor for ChainActorC {
    type Args = ();
    type Error = anyhow::Error;

    async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(ChainActorC)
    }
}

impl Message<StartChain> for ChainActorA {
    type Reply = String;

    async fn handle(&mut self, msg: StartChain, actor_ref: &ActorRef<Self>) -> String {
        msg.b_ref
            .ask(ForwardToC {
                a_ref: actor_ref.clone(),
                c_ref: msg.c_ref,
            })
            .await
            .unwrap_or_default()
    }
}

impl Message<ChainPong> for ChainActorA {
    type Reply = String;

    async fn handle(&mut self, _: ChainPong, _: &ActorRef<Self>) -> String {
        "chain pong".to_string()
    }
}

impl Message<ForwardToC> for ChainActorB {
    type Reply = String;

    async fn handle(&mut self, msg: ForwardToC, _: &ActorRef<Self>) -> String {
        msg.c_ref
            .ask(AskBackToA(msg.a_ref))
            .await
            .unwrap_or_default()
    }
}

impl Message<AskBackToA> for ChainActorC {
    type Reply = String;

    async fn handle(&mut self, msg: AskBackToA, _: &ActorRef<Self>) -> String {
        // Ask A → deadlock detected (A→B→C→A cycle)
        msg.0.ask(ChainPong).await.unwrap_or_default()
    }
}

#[tokio::test]
async fn test_three_actor_chain_deadlock_detected() {
    let (a_ref, a_handle) = spawn::<ChainActorA>(());
    let (b_ref, b_handle) = spawn::<ChainActorB>(());
    let (c_ref, c_handle) = spawn::<ChainActorC>(());

    let _ = a_ref
        .ask(StartChain {
            b_ref: b_ref.clone(),
            c_ref: c_ref.clone(),
        })
        .await;

    // C should have panicked (A→B→C→A cycle)
    let c_err = c_handle.await.unwrap_err();
    assert!(c_err.is_panic());
    let payload = c_err.into_panic();
    let msg = payload
        .downcast_ref::<String>()
        .expect("panic payload should be String");
    assert!(
        msg.contains("Deadlock detected"),
        "Expected deadlock panic, got: {msg}"
    );

    drop(a_ref);
    drop(b_ref);
    drop(c_ref);
    let _ = a_handle.await;
    let _ = b_handle.await;
}

// ============================================================
// Non-cycle: A → B (no cycle, should succeed)
// ============================================================

#[tokio::test]
async fn test_non_cycle_ask_succeeds() {
    let (a_ref, a_handle) = spawn::<WorkerActor>(());
    let (b_ref, b_handle) = spawn::<WorkerActor>(());

    // A asks B via ForwardPing — no cycle, B just replies with Ping
    let result = a_ref.ask(ForwardPing(b_ref.clone())).await;
    assert_eq!(result.unwrap(), "pong");

    drop(a_ref);
    drop(b_ref);
    let _ = a_handle.await;
    let _ = b_handle.await;
}

// ============================================================
// Sequential ask: A → B then A → C (guard cleanup between asks)
// ============================================================

#[tokio::test]
async fn test_sequential_ask_no_false_positive() {
    let (a_ref, a_handle) = spawn::<WorkerActor>(());
    let (b_ref, b_handle) = spawn::<WorkerActor>(());
    let (c_ref, c_handle) = spawn::<WorkerActor>(());

    // A asks B, then asks C sequentially — guards drop between asks
    let result = a_ref
        .ask(SequentialPing(b_ref.clone(), c_ref.clone()))
        .await;
    let (r1, r2) = result.unwrap();
    assert_eq!(r1, "pong");
    assert_eq!(r2, "pong");

    drop(a_ref);
    drop(b_ref);
    drop(c_ref);
    let _ = a_handle.await;
    let _ = b_handle.await;
    let _ = c_handle.await;
}
