// Tests for tell() handler error logging via on_tell_result

use rsactor::{message_handlers, spawn, Actor, ActorRef, Message};
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

// ---- Shared test error type ----

#[derive(Debug, Clone)]
struct TestError(String);

impl fmt::Display for TestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TestError: {}", self.0)
    }
}

// ---- Actor with auto-detected Result handler ----

#[derive(Debug)]
struct AutoDetectActor;

impl Actor for AutoDetectActor {
    type Args = ();
    type Error = anyhow::Error;

    async fn on_start(
        _args: Self::Args,
        _actor_ref: &ActorRef<Self>,
    ) -> std::result::Result<Self, Self::Error> {
        Ok(AutoDetectActor)
    }
}

struct FailingMessage;
struct SucceedingMessage;
struct NonResultMessage;

#[message_handlers]
impl AutoDetectActor {
    #[handler]
    async fn handle_failing(
        &mut self,
        _msg: FailingMessage,
        _: &ActorRef<Self>,
    ) -> Result<(), TestError> {
        Err(TestError("something went wrong".to_string()))
    }

    #[handler]
    async fn handle_succeeding(
        &mut self,
        _msg: SucceedingMessage,
        _: &ActorRef<Self>,
    ) -> Result<String, TestError> {
        Ok("success".to_string())
    }

    #[handler]
    async fn handle_non_result(&mut self, _msg: NonResultMessage, _: &ActorRef<Self>) -> u32 {
        42
    }
}

// ---- Actor for tell-err test (own statics) ----

static TELL_ERR_CALLED: AtomicBool = AtomicBool::new(false);
static TELL_ERR_WAS_ERR: AtomicBool = AtomicBool::new(false);
static TELL_ERR_COUNT: AtomicU32 = AtomicU32::new(0);

#[derive(Debug)]
struct TellErrActor;

impl Actor for TellErrActor {
    type Args = ();
    type Error = anyhow::Error;
    async fn on_start(_: (), _: &ActorRef<Self>) -> std::result::Result<Self, Self::Error> {
        Ok(TellErrActor)
    }
}

struct TellErrMsg;

impl Message<TellErrMsg> for TellErrActor {
    type Reply = Result<(), TestError>;

    async fn handle(&mut self, _msg: TellErrMsg, _: &ActorRef<Self>) -> Self::Reply {
        Err(TestError("tell error".to_string()))
    }

    fn on_tell_result(result: &Self::Reply, _actor_ref: &ActorRef<Self>) {
        TELL_ERR_COUNT.fetch_add(1, Ordering::SeqCst);
        TELL_ERR_CALLED.store(true, Ordering::SeqCst);
        TELL_ERR_WAS_ERR.store(result.is_err(), Ordering::SeqCst);
    }
}

// ---- Actor for tell-ok test (own statics) ----

static TELL_OK_CALLED: AtomicBool = AtomicBool::new(false);
static TELL_OK_WAS_ERR: AtomicBool = AtomicBool::new(false);
static TELL_OK_COUNT: AtomicU32 = AtomicU32::new(0);

#[derive(Debug)]
struct TellOkActor;

impl Actor for TellOkActor {
    type Args = ();
    type Error = anyhow::Error;
    async fn on_start(_: (), _: &ActorRef<Self>) -> std::result::Result<Self, Self::Error> {
        Ok(TellOkActor)
    }
}

struct TellOkMsg;

impl Message<TellOkMsg> for TellOkActor {
    type Reply = Result<String, TestError>;

    async fn handle(&mut self, _msg: TellOkMsg, _: &ActorRef<Self>) -> Self::Reply {
        Ok("ok".to_string())
    }

    fn on_tell_result(result: &Self::Reply, _actor_ref: &ActorRef<Self>) {
        TELL_OK_COUNT.fetch_add(1, Ordering::SeqCst);
        TELL_OK_CALLED.store(true, Ordering::SeqCst);
        TELL_OK_WAS_ERR.store(result.is_err(), Ordering::SeqCst);
    }
}

// ---- Actor for ask-does-not-invoke test (own statics) ----

static ASK_NO_INVOKE_CALLED: AtomicBool = AtomicBool::new(false);
static ASK_NO_INVOKE_COUNT: AtomicU32 = AtomicU32::new(0);

#[derive(Debug)]
struct AskNoInvokeActor;

impl Actor for AskNoInvokeActor {
    type Args = ();
    type Error = anyhow::Error;
    async fn on_start(_: (), _: &ActorRef<Self>) -> std::result::Result<Self, Self::Error> {
        Ok(AskNoInvokeActor)
    }
}

struct AskNoInvokeMsg;

impl Message<AskNoInvokeMsg> for AskNoInvokeActor {
    type Reply = Result<(), TestError>;

    async fn handle(&mut self, _msg: AskNoInvokeMsg, _: &ActorRef<Self>) -> Self::Reply {
        Err(TestError("ask error".to_string()))
    }

    fn on_tell_result(_result: &Self::Reply, _actor_ref: &ActorRef<Self>) {
        ASK_NO_INVOKE_COUNT.fetch_add(1, Ordering::SeqCst);
        ASK_NO_INVOKE_CALLED.store(true, Ordering::SeqCst);
    }
}

// ---- Actor with #[handler(no_log)] ----

#[derive(Debug)]
struct NoLogActor;

impl Actor for NoLogActor {
    type Args = ();
    type Error = anyhow::Error;
    async fn on_start(_: (), _: &ActorRef<Self>) -> std::result::Result<Self, Self::Error> {
        Ok(NoLogActor)
    }
}

struct NoLogMessage;

#[message_handlers]
impl NoLogActor {
    #[handler(no_log)]
    async fn handle_no_log(
        &mut self,
        _msg: NoLogMessage,
        _: &ActorRef<Self>,
    ) -> Result<(), TestError> {
        Err(TestError("suppressed error".to_string()))
    }
}

// ---- Actor with #[handler(result)] for type alias ----

type MyResult = Result<String, TestError>;

#[derive(Debug)]
struct ForceResultActor;

impl Actor for ForceResultActor {
    type Args = ();
    type Error = anyhow::Error;
    async fn on_start(_: (), _: &ActorRef<Self>) -> std::result::Result<Self, Self::Error> {
        Ok(ForceResultActor)
    }
}

struct ForceResultMessage;

#[message_handlers]
impl ForceResultActor {
    #[handler(result)]
    async fn handle_force_result(
        &mut self,
        _msg: ForceResultMessage,
        _: &ActorRef<Self>,
    ) -> MyResult {
        Err(TestError("forced result error".to_string()))
    }
}

// ---- Tests ----

#[tokio::test]
async fn tell_error_with_auto_detected_result_does_not_panic() {
    let (actor_ref, _handle) = spawn::<AutoDetectActor>(());

    actor_ref.tell(FailingMessage).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Actor should still be alive and responsive
    let result: Result<String, TestError> = actor_ref.ask(SucceedingMessage).await.unwrap();
    assert!(result.is_ok());
}

#[tokio::test]
async fn tell_ok_with_auto_detected_result_does_not_panic() {
    let (actor_ref, _handle) = spawn::<AutoDetectActor>(());

    actor_ref.tell(SucceedingMessage).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let result: Result<String, TestError> = actor_ref.ask(SucceedingMessage).await.unwrap();
    assert_eq!(result.unwrap(), "success");
}

#[tokio::test]
async fn tell_non_result_handler_completes_without_side_effects() {
    let (actor_ref, _handle) = spawn::<AutoDetectActor>(());

    actor_ref.tell(NonResultMessage).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let value: u32 = actor_ref.ask(NonResultMessage).await.unwrap();
    assert_eq!(value, 42);
}

#[tokio::test]
async fn ask_does_not_invoke_on_tell_result() {
    let (actor_ref, _handle) = spawn::<AskNoInvokeActor>(());

    let result: Result<(), TestError> = actor_ref.ask(AskNoInvokeMsg).await.unwrap();
    assert!(result.is_err());

    assert!(!ASK_NO_INVOKE_CALLED.load(Ordering::SeqCst));
    assert_eq!(ASK_NO_INVOKE_COUNT.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn tell_invokes_on_tell_result_with_err() {
    let (actor_ref, _handle) = spawn::<TellErrActor>(());

    actor_ref.tell(TellErrMsg).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert!(TELL_ERR_CALLED.load(Ordering::SeqCst));
    assert!(TELL_ERR_WAS_ERR.load(Ordering::SeqCst));
    assert_eq!(TELL_ERR_COUNT.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn tell_invokes_on_tell_result_with_ok() {
    let (actor_ref, _handle) = spawn::<TellOkActor>(());

    actor_ref.tell(TellOkMsg).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert!(TELL_OK_CALLED.load(Ordering::SeqCst));
    assert!(!TELL_OK_WAS_ERR.load(Ordering::SeqCst));
    assert_eq!(TELL_OK_COUNT.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn handler_no_log_suppresses_on_tell_result() {
    let (actor_ref, _handle) = spawn::<NoLogActor>(());

    actor_ref.tell(NoLogMessage).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Actor still alive — on_tell_result was not generated (default no-op used)
    let result: Result<(), TestError> = actor_ref.ask(NoLogMessage).await.unwrap();
    assert!(result.is_err());
}

#[tokio::test]
async fn handler_result_forces_on_tell_result_for_type_alias() {
    let (actor_ref, _handle) = spawn::<ForceResultActor>(());

    actor_ref.tell(ForceResultMessage).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let result: MyResult = actor_ref.ask(ForceResultMessage).await.unwrap();
    assert!(result.is_err());
}
