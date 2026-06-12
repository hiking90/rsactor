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
    type IdleEvent = ();

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
    type IdleEvent = ();

    async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(SelfAskActor)
    }
}

impl Message<TriggerSelfAsk> for SelfAskActor {
    type Reply = ();

    async fn handle(&mut self, _: TriggerSelfAsk, actor_ref: &ActorRef<Self>) {
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
    type IdleEvent = ();

    async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(CycleActorA)
    }
}

impl Actor for CycleActorB {
    type Args = ();
    type Error = anyhow::Error;
    type IdleEvent = ();

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
    type IdleEvent = ();

    async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(ChainActorA)
    }
}

impl Actor for ChainActorB {
    type Args = ();
    type Error = anyhow::Error;
    type IdleEvent = ();

    async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(ChainActorB)
    }
}

impl Actor for ChainActorC {
    type Args = ();
    type Error = anyhow::Error;
    type IdleEvent = ();

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

// ============================================================
// Reply-time edge removal: no false positive in the window
// between B sending its reply and A's ask future resuming
// ============================================================

#[derive(Debug)]
struct FpA;
#[derive(Debug)]
struct FpB;

#[derive(Debug)]
struct FpPing;
struct FpTrigger(ActorRef<FpB>);
struct FpMsg1(ActorRef<FpA>);
struct FpMsg2(ActorRef<FpA>);

impl Actor for FpA {
    type Args = ();
    type Error = anyhow::Error;
    type IdleEvent = ();

    async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(FpA)
    }
}

impl Actor for FpB {
    type Args = ();
    type Error = anyhow::Error;
    type IdleEvent = ();

    async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(FpB)
    }
}

impl Message<FpPing> for FpA {
    type Reply = u32;

    async fn handle(&mut self, _: FpPing, _: &ActorRef<Self>) -> u32 {
        7
    }
}

impl Message<FpTrigger> for FpA {
    type Reply = u32;

    async fn handle(&mut self, msg: FpTrigger, actor_ref: &ActorRef<Self>) -> u32 {
        // A asks B from inside a handler → edge A->B registered.
        msg.0.ask(FpMsg1(actor_ref.clone())).await.unwrap()
    }
}

impl Message<FpMsg1> for FpB {
    type Reply = u32;

    async fn handle(&mut self, msg: FpMsg1, actor_ref: &ActorRef<Self>) -> u32 {
        // Queue FpMsg2 before replying; the reply to A is sent when this
        // handler returns, and B's very next message asks back into A.
        actor_ref.tell(FpMsg2(msg.0)).await.unwrap();
        1
    }
}

impl Message<FpMsg2> for FpB {
    type Reply = u32;

    async fn handle(&mut self, msg: FpMsg2, _: &ActorRef<Self>) -> u32 {
        // Regression: with caller-side-only edge removal, the stale A->B edge
        // was still in the wait-for graph here (A's ask future had not been
        // polled yet), so this ask closed a phantom cycle and panicked even
        // though A was already runnable. Reply-time edge removal (the token
        // carried in the ask envelope) must make this succeed.
        msg.0.ask(FpPing).await.unwrap()
    }
}

#[tokio::test]
async fn test_reply_send_window_has_no_false_positive() {
    let (a_ref, a_handle) = spawn::<FpA>(());
    let (b_ref, b_handle) = spawn::<FpB>(());

    let r = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        a_ref.ask(FpTrigger(b_ref.clone())),
    )
    .await
    .expect("must not hang")
    .expect("must not error");
    assert_eq!(r, 1);

    // Let B process FpMsg2 — its ask back into A must not panic.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(
        b_ref.is_alive(),
        "B must survive: the ask issued right after replying to A must not \
         trip a false-positive deadlock panic"
    );

    drop(a_ref);
    drop(b_ref);
    let _ = a_handle.await;
    let _ = b_handle.await;
}

// ============================================================
// Slow-path detection: blocking_ask from a current_thread
// runtime handler still panics immediately on a self-cycle
// ============================================================

#[derive(Debug)]
struct SlowPathActor;

#[derive(Debug)]
struct PlainPing;
#[derive(Debug)]
struct BlockingSelfAsk;

impl Actor for SlowPathActor {
    type Args = ();
    type Error = anyhow::Error;
    type IdleEvent = ();

    async fn on_start(_: (), _: &ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(SlowPathActor)
    }
}

impl Message<PlainPing> for SlowPathActor {
    type Reply = u32;

    async fn handle(&mut self, _: PlainPing, _: &ActorRef<Self>) -> u32 {
        1
    }
}

impl Message<BlockingSelfAsk> for SlowPathActor {
    type Reply = u32;

    async fn handle(&mut self, _: BlockingSelfAsk, actor_ref: &ActorRef<Self>) -> u32 {
        // On a current_thread runtime the blocking_* call takes the
        // dedicated-thread slow path. Regression: the CURRENT_ACTOR
        // task-local did not propagate to that thread, so this self-cycle
        // silently hung until the timeout instead of panicking.
        actor_ref
            .blocking_ask(PlainPing, Some(std::time::Duration::from_secs(5)))
            .unwrap_or(0)
    }
}

#[tokio::test]
async fn test_blocking_slow_path_self_cycle_panics_immediately() {
    let (actor_ref, handle) = spawn::<SlowPathActor>(());

    let start = std::time::Instant::now();
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        actor_ref.ask(BlockingSelfAsk),
    )
    .await;

    let res = handle.await;
    assert!(
        res.is_err() && res.unwrap_err().is_panic(),
        "self ask cycle through the blocking slow path must panic via deadlock detection"
    );
    assert!(
        start.elapsed() < std::time::Duration::from_secs(4),
        "detection must be immediate, not the 5s blocking timeout"
    );
}

// ============================================================
// Self-send on a full mailbox: unbounded waits panic, bounded
// waits stay recoverable
// ============================================================

#[derive(Debug)]
struct SelfSendActor {
    entered: std::sync::Arc<tokio::sync::Notify>,
    proceed: std::sync::Arc<tokio::sync::Notify>,
}

#[derive(Debug)]
struct Padding;
#[derive(Debug)]
struct TriggerSelfTell;
#[derive(Debug)]
struct TriggerSelfStop;
#[derive(Debug)]
struct TriggerTimedSelfTell;

impl Actor for SelfSendActor {
    type Args = (
        std::sync::Arc<tokio::sync::Notify>,
        std::sync::Arc<tokio::sync::Notify>,
    );
    type Error = anyhow::Error;
    type IdleEvent = ();

    async fn on_start(
        (entered, proceed): Self::Args,
        _: &ActorRef<Self>,
    ) -> Result<Self, Self::Error> {
        Ok(SelfSendActor { entered, proceed })
    }
}

impl Message<Padding> for SelfSendActor {
    type Reply = ();

    async fn handle(&mut self, _: Padding, _: &ActorRef<Self>) {}
}

impl Message<TriggerSelfTell> for SelfSendActor {
    type Reply = ();

    async fn handle(&mut self, _: TriggerSelfTell, actor_ref: &ActorRef<Self>) {
        self.entered.notify_one();
        self.proceed.notified().await;
        // The test has filled the capacity-1 mailbox: this unbounded self-send
        // can never be admitted (the loop is parked awaiting this handler) and
        // must panic under deadlock-detection instead of hanging.
        let _ = actor_ref.tell(Padding).await;
    }
}

impl Message<TriggerSelfStop> for SelfSendActor {
    type Reply = ();

    async fn handle(&mut self, _: TriggerSelfStop, actor_ref: &ActorRef<Self>) {
        self.entered.notify_one();
        self.proceed.notified().await;
        // Same hazard through stop(): StopGracefully travels the regular
        // mailbox, so a full-mailbox self-stop is equally unrecoverable.
        actor_ref.stop().await;
    }
}

impl Message<TriggerTimedSelfTell> for SelfSendActor {
    type Reply = bool;

    async fn handle(&mut self, _: TriggerTimedSelfTell, actor_ref: &ActorRef<Self>) -> bool {
        self.entered.notify_one();
        self.proceed.notified().await;
        // Bounded variant: must NOT panic — it resolves as a recoverable
        // Error::Timeout once the 50ms admission window elapses.
        actor_ref
            .tell_with_timeout(Padding, std::time::Duration::from_millis(50))
            .await
            .is_err()
    }
}

async fn run_self_send_panic_case<M>(msg: M) -> Result<(), tokio::task::JoinError>
where
    SelfSendActor: Message<M>,
    M: Send + 'static,
{
    let entered = std::sync::Arc::new(tokio::sync::Notify::new());
    let proceed = std::sync::Arc::new(tokio::sync::Notify::new());
    let (actor_ref, handle) = rsactor::spawn_with_mailbox_capacity::<SelfSendActor>(
        (entered.clone(), proceed.clone()),
        1,
    );

    actor_ref.tell(msg).await.unwrap();
    entered.notified().await;
    // Fill the capacity-1 mailbox while the handler is parked.
    actor_ref.tell::<Padding>(Padding).await.unwrap();
    proceed.notify_one();

    handle.await.map(|_| ())
}

#[tokio::test]
async fn test_self_tell_on_full_mailbox_panics() {
    let res = run_self_send_panic_case(TriggerSelfTell).await;
    assert!(
        res.is_err() && res.unwrap_err().is_panic(),
        "unbounded self-tell on a full mailbox must panic under deadlock-detection"
    );
}

#[tokio::test]
async fn test_self_stop_on_full_mailbox_panics() {
    let res = run_self_send_panic_case(TriggerSelfStop).await;
    assert!(
        res.is_err() && res.unwrap_err().is_panic(),
        "self-stop on a full mailbox must panic under deadlock-detection"
    );
}

#[tokio::test]
async fn test_timed_self_tell_on_full_mailbox_times_out_without_panic() {
    let entered = std::sync::Arc::new(tokio::sync::Notify::new());
    let proceed = std::sync::Arc::new(tokio::sync::Notify::new());
    let (actor_ref, handle) = rsactor::spawn_with_mailbox_capacity::<SelfSendActor>(
        (entered.clone(), proceed.clone()),
        1,
    );

    let asker = actor_ref.clone();
    let ask_task = tokio::spawn(async move { asker.ask(TriggerTimedSelfTell).await });

    entered.notified().await;
    actor_ref.tell(Padding).await.unwrap();
    proceed.notify_one();

    let timed_out = ask_task
        .await
        .expect("ask task must not panic")
        .expect("ask must succeed");
    assert!(
        timed_out,
        "bounded self-tell must resolve as a recoverable timeout, not a panic"
    );

    actor_ref.stop().await;
    let result = handle.await.expect("actor task must not panic");
    assert!(result.is_completed());
}
