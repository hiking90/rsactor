// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use crate::actor::IdleEventStream;
use crate::error::{Channel, Error, Operation, Result};
use crate::Identity;
use crate::{Actor, ControlSignal, MailboxMessage, MailboxSender, Message};
use futures::stream::{BoxStream, Stream, StreamExt};
use std::fmt;
use std::time::Duration;
use tokio::runtime::{Handle, RuntimeFlavor};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, warn};

/// If the calling thread is currently inside a multi-thread Tokio runtime,
/// returns a `Handle` to it. Otherwise (no runtime, or current_thread runtime)
/// returns `None`.
///
/// The blocking_* APIs use this to take a near-zero-cost fast path: instead of
/// spawning a new OS thread and a fresh single-thread runtime, they call
/// `block_in_place` + `Handle::block_on` and reuse the caller's runtime.
/// `block_in_place` requires a multi-thread runtime, so current_thread is
/// excluded here and falls back to the original "new thread + new runtime"
/// path. The comparison deliberately allows *only* `MultiThread`: should tokio
/// ever add another flavor to the `#[non_exhaustive]` enum, the safe failure
/// mode is the slow path, not a `block_in_place` panic.
#[inline]
fn current_multi_thread_handle() -> Option<Handle> {
    let handle = Handle::try_current().ok()?;
    (handle.runtime_flavor() == RuntimeFlavor::MultiThread).then_some(handle)
}

/// Fast-path helper shared by every `blocking_*` API: runs `fut` to completion
/// on the caller's own multi-thread runtime via `block_in_place` + `block_on`.
/// No new thread or runtime is created.
#[inline]
fn block_on_current_runtime<R>(handle: Handle, fut: impl std::future::Future<Output = R>) -> R {
    tokio::task::block_in_place(|| handle.block_on(fut))
}

/// Guard for every `blocking_*` slow path: panics — via Tokio's own internal
/// "can this thread block?" assertion — when the calling thread is currently
/// driving async tasks (a `current_thread` runtime worker, or any thread
/// inside `Handle::block_on`/`Runtime::block_on`).
///
/// Parking such a thread (e.g. in `join()` on a dedicated blocking thread)
/// freezes its runtime: every task on it — including the target actor's
/// message loop — stops being polled, so the wait can never be satisfied and
/// even `kill()` cannot intervene. Tokio's loud panic
/// ("Cannot block the current thread from within a runtime") is strictly
/// better than that silent, unrecoverable hang.
///
/// Threads that are allowed to block — no runtime at all, or a
/// `spawn_blocking` pool thread of any runtime — pass through untouched.
/// Tokio does not expose this assertion as a queryable API, so a `try_send`
/// on a fresh non-full channel is used purely for its context check: it never
/// actually blocks and its only effect is the panic in async contexts.
fn assert_current_thread_can_block() {
    if Handle::try_current().is_ok() {
        let (tx, _rx) = mpsc::channel::<()>(1);
        let _ = tx.blocking_send(());
    }
}

/// Slow-path helper shared by `blocking_*` APIs: spawns a dedicated thread,
/// builds a single-thread Tokio runtime on it, and runs `fut` to completion.
///
/// Used only when the caller is *not* inside a multi-thread runtime (i.e. when
/// the fast `block_in_place` path is unavailable). Centralizes the
/// thread/runtime/join boilerplate so all slow paths report errors identically:
/// a failure to spawn the thread or to build the temporary runtime becomes
/// `Error::Runtime` carrying the underlying [`std::io::Error`] as its
/// [`source`](std::error::Error::source).
///
/// With `deadlock-detection` enabled, the calling actor's task-local identity
/// is captured here and re-established inside the new runtime. This is
/// defensive: the blockability guard below panics before any caller that is
/// actually driving actor tasks (and could therefore see a task-local
/// identity) reaches this point, so the capture is expected to be `None` on
/// every surviving path — it is kept so a future change to the guard or the
/// dispatch conditions cannot silently disable cycle detection. A deadlock
/// panic raised on the dedicated thread is resumed on the caller's thread
/// (preserving the documented panic-on-detection contract); any other
/// worker-thread panic is reported as `Error::Runtime` with `panic_msg` and
/// the panic payload in its details.
fn run_blocking_with_runtime<F, R>(identity: Identity, panic_msg: &'static str, fut: F) -> Result<R>
where
    F: std::future::Future<Output = Result<R>> + Send + 'static,
    R: Send + 'static,
{
    // `join.join()` below parks the calling thread; refuse loudly if that
    // thread is currently driving a runtime (see the guard's docs).
    assert_current_thread_can_block();

    // Capture the current actor (if any) *before* hopping threads — tokio
    // task-locals do not propagate to new threads or runtimes.
    #[cfg(feature = "deadlock-detection")]
    let caller = crate::CURRENT_ACTOR.try_with(|id| *id).ok();

    let join = std::thread::Builder::new()
        .name("rsactor-blocking".to_string())
        .spawn(move || -> Result<R> {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .map_err(|e| Error::Runtime {
                    identity,
                    details: "Failed to build blocking runtime".to_string(),
                    source: Some(std::sync::Arc::new(e)),
                })?;
            #[cfg(feature = "deadlock-detection")]
            {
                match caller {
                    Some(id) => runtime.block_on(crate::CURRENT_ACTOR.scope(id, fut)),
                    None => runtime.block_on(fut),
                }
            }
            #[cfg(not(feature = "deadlock-detection"))]
            {
                runtime.block_on(fut)
            }
        })
        .map_err(|e| Error::Runtime {
            identity,
            details: "Failed to spawn blocking thread".to_string(),
            source: Some(std::sync::Arc::new(e)),
        })?;

    join.join().unwrap_or_else(|payload| {
        let panic_text = payload
            .downcast_ref::<&str>()
            .copied()
            .map(str::to_owned)
            .or_else(|| payload.downcast_ref::<String>().cloned());
        // Deadlock detection promises an *immediate panic* on an ask cycle;
        // swallowing it into Error::Runtime would silently downgrade that
        // contract on the slow path.
        #[cfg(feature = "deadlock-detection")]
        if panic_text
            .as_deref()
            .is_some_and(|m| m.starts_with(crate::DEADLOCK_PANIC_PREFIX))
        {
            std::panic::resume_unwind(payload);
        }
        Err(Error::Runtime {
            identity,
            details: match panic_text {
                Some(text) => format!("{panic_msg}: {text}"),
                None => panic_msg.to_string(),
            },
            source: None,
        })
    })
}

#[cfg(feature = "metrics")]
use crate::metrics::MetricsCollector;
#[cfg(feature = "metrics")]
use std::sync::Arc;

/// A type-safe reference to an actor of type `T`.
///
/// `ActorRef<T>` provides type-safe message passing to actors, ensuring that only
/// messages that the actor can handle are sent, and that reply types are correctly typed.
/// It provides compile-time type safety through Rust's type system and trait bounds.
///
/// ## Type Safety Benefits
///
/// - **Compile-Time Message Validation**: Only messages implementing [`Message<M>`] for actor `T` are accepted
/// - **Automatic Reply Type Inference**: Return types are inferred from trait implementations
/// - **Zero Runtime Overhead**: Type safety is enforced at compile time with no performance cost
/// - **IDE Support**: Full autocomplete and type checking support
/// - **Prevention of Runtime Type Errors**: Eliminates downcasting failures and type mismatches
///
/// ## Message Passing Methods
///
/// - **Asynchronous Methods**:
///   - [`ask`](ActorRef::ask): Send a message and await a typed reply.
///   - [`ask_with_timeout`](ActorRef::ask_with_timeout): Send a message and await a typed reply with a timeout.
///   - [`tell`](ActorRef::tell): Send a message without waiting for a reply.
///   - [`tell_with_timeout`](ActorRef::tell_with_timeout): Send a message without waiting for a reply with a timeout.
///
/// - **Blocking Methods**:
///   - [`blocking_ask`](ActorRef::blocking_ask): Send a message and block until a typed reply is received (no runtime context required).
///   - [`blocking_tell`](ActorRef::blocking_tell): Send a message and block until it is sent (no runtime context required).
///
///   The `blocking_*` methods can be called from any thread and from within a
///   multi-thread Tokio runtime. The exact mechanism (`block_in_place` +
///   `block_on`, `blocking_send` / `blocking_recv`, or a temporary runtime on a
///   dedicated thread) depends on the calling context — see each method's
///   "Execution paths" section for details.
///
/// - **Control Methods**:
///   - [`stop`](ActorRef::stop): Gracefully stop the actor.
///   - [`kill`](ActorRef::kill): Immediately terminate the actor.
///
/// - **Utility Methods**:
///   - [`identity`](ActorRef::identity): Get the unique ID of the actor.
///   - [`is_alive`](ActorRef::is_alive): Check if the actor is still running.
///   - [`ask_join`](ActorRef::ask_join): Send a message expecting a JoinHandle and await its completion.
///
/// ## Recommended Usage
///
/// Use [`ActorRef<T>`] by default for all actor communication. It provides compile-time
/// guarantees that prevent type-related runtime errors.
///
/// **When to use `ActorRef<T>`**:
/// - Default choice for actor communication
/// - When you know the actor type at compile time
/// - When you want compile-time message validation
/// - When working with strongly-typed actor systems
pub struct ActorRef<T: Actor> {
    /// The unique identifier for this actor instance
    id: Identity,
    /// Channel for sending messages to the actor's mailbox
    sender: MailboxSender<T>,
    /// Optional channel for sending priority messages to the actor.
    /// `None` when the actor was spawned without `SpawnOptions::with_priority()`.
    pub(crate) priority_sender: Option<MailboxSender<T>>,
    /// Channel for sending control signals (e.g., terminate) to the actor
    pub(crate) terminate_sender: mpsc::Sender<ControlSignal>,
    /// Optional channel for registering new idle-event streams with the actor's runtime.
    /// Each subscribed stream is merged into the runtime's `SelectAll` and its
    /// events drive [`Actor::on_idle`](crate::Actor::on_idle).
    /// `None` when the actor was spawned without `SpawnOptions::with_idle()`.
    pub(crate) idle_subscribe_sender: Option<mpsc::Sender<IdleEventStream<T>>>,
    /// Per-actor metrics collector (when metrics feature is enabled)
    #[cfg(feature = "metrics")]
    pub(crate) metrics: Arc<MetricsCollector>,
}

// Manual `Debug` rather than `#[derive(Debug)]`: every field is unconditionally
// `Debug` (tokio's `Sender`/`WeakSender` are `Debug` for all `T`), so the derive's
// implicit `T: Debug` bound would needlessly deny `Debug` to `ActorRef<T>` for
// actor types that are not themselves `Debug`.
impl<T: Actor> fmt::Debug for ActorRef<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("ActorRef");
        s.field("id", &self.id)
            .field("sender", &self.sender)
            .field("priority_sender", &self.priority_sender)
            .field("terminate_sender", &self.terminate_sender)
            .field("idle_subscribe_sender", &self.idle_subscribe_sender);
        #[cfg(feature = "metrics")]
        s.field("metrics", &self.metrics);
        s.finish()
    }
}

impl<T: Actor> ActorRef<T> {
    /// Creates a new type-safe ActorRef.
    pub(crate) fn new(
        id: Identity,
        sender: MailboxSender<T>,
        priority_sender: Option<MailboxSender<T>>,
        terminate_sender: mpsc::Sender<ControlSignal>,
        idle_subscribe_sender: Option<mpsc::Sender<IdleEventStream<T>>>,
        #[cfg(feature = "metrics")] metrics: Arc<MetricsCollector>,
    ) -> Self {
        ActorRef {
            id,
            sender,
            priority_sender,
            terminate_sender,
            idle_subscribe_sender,
            #[cfg(feature = "metrics")]
            metrics,
        }
    }

    /// Returns the unique ID of the actor.
    pub fn identity(&self) -> Identity {
        self.id
    }

    /// Returns `true` if this actor was spawned with the priority channel enabled
    /// via [`SpawnOptions::with_priority`](crate::SpawnOptions::with_priority).
    ///
    /// When this returns `false`, calls to [`tell_priority`](Self::tell_priority),
    /// [`ask_priority`](Self::ask_priority) and their blocking counterparts return
    /// [`Error::PriorityChannelNotEnabled`](crate::Error::PriorityChannelNotEnabled).
    #[inline]
    pub fn has_priority_channel(&self) -> bool {
        self.priority_sender.is_some()
    }

    /// Returns `true` if this actor was spawned with the idle-event channel enabled
    /// via [`SpawnOptions::with_idle`](crate::SpawnOptions::with_idle).
    ///
    /// When this returns `false`, [`Actor::on_idle`](crate::Actor::on_idle) is never
    /// driven and calls to [`subscribe_idle`](Self::subscribe_idle) return
    /// [`Error::IdleChannelNotEnabled`](crate::Error::IdleChannelNotEnabled).
    #[inline]
    pub fn has_idle_channel(&self) -> bool {
        self.idle_subscribe_sender.is_some()
    }

    /// Test-only: drops this `ActorRef`'s strong priority sender without affecting
    /// the regular mailbox or the terminate channel.
    ///
    /// Used to write deterministic tests for the
    /// [`ActorWeak::upgrade`](crate::ActorWeak::upgrade) policy where every strong
    /// priority sender is dropped while the actor is still alive on its other
    /// channels. Production code has no reason to drop one channel without the
    /// others — clone or drop the entire `ActorRef` instead.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn drop_priority_sender_for_test(&mut self) {
        self.priority_sender = None;
    }

    /// Checks if the actor is still alive by verifying if its channels are open.
    ///
    /// The optional priority channel is intentionally **not** consulted: it is a
    /// secondary channel, and the actor is considered alive as long as the regular
    /// mailbox and the terminate channel remain open.
    ///
    /// **Note**: This is a heuristic, point-in-time check. The channels are
    /// closed only after the actor's loop exits, so this can briefly report
    /// `true` after `on_stop` has already run, and a `true` result does not
    /// guarantee that a subsequent send will succeed. A successful send is the
    /// only definitive liveness signal.
    #[inline]
    pub fn is_alive(&self) -> bool {
        // Both channels must be open for the actor to be considered alive
        !self.sender.is_closed() && !self.terminate_sender.is_closed()
    }

    /// Creates a weak, type-safe reference to this actor.
    ///
    /// The returned [`ActorWeak<T>`] can be used to check if the actor is still alive
    /// and optionally upgrade back to a strong [`ActorRef<T>`] without keeping the actor alive.
    /// When the `metrics` feature is enabled, metrics are preserved via strong reference
    /// for post-mortem analysis.
    ///
    /// This is an inherent `&self` method (callable as `actor_ref.downgrade()`
    /// or `ActorRef::downgrade(&actor_ref)`). Inherent methods win method
    /// resolution, so the call stays unambiguous even with the
    /// [`TellHandler`](crate::TellHandler) / [`AskHandler`](crate::AskHandler) /
    /// [`ActorControl`](crate::ActorControl) traits in scope, all of which also
    /// declare a `downgrade`; reach those via trait objects or fully-qualified
    /// syntax.
    pub fn downgrade(&self) -> ActorWeak<T> {
        ActorWeak {
            id: self.id,
            sender: self.sender.downgrade(),
            priority_sender: self.priority_sender.as_ref().map(|s| s.downgrade()),
            terminate_sender: self.terminate_sender.downgrade(),
            idle_subscribe_sender: self.idle_subscribe_sender.as_ref().map(|s| s.downgrade()),
            #[cfg(feature = "metrics")]
            metrics: self.metrics.clone(),
        }
    }

    /// Resolves once the actor has fully stopped — its runtime loop has exited
    /// (after `on_stop` ran) and the mailbox channel has closed.
    ///
    /// Unlike awaiting the `JoinHandle` returned by [`spawn`](crate::spawn),
    /// this works from any clone of the `ActorRef` and is type-erasable (see
    /// [`ActorControl::wait_stopped`](crate::ActorControl::wait_stopped)), but
    /// it only signals completion — it cannot return the
    /// [`ActorResult`](crate::ActorResult).
    ///
    /// Returns immediately if the actor has already stopped. Note that holding
    /// this `ActorRef` does not keep the actor alive forever: an actor also
    /// stops when every *other* strong ref is dropped only if this one is
    /// dropped too, so pair this with [`stop`](Self::stop) / [`kill`](Self::kill)
    /// rather than waiting for ref-drop termination from a live ref.
    pub async fn wait_stopped(&self) {
        self.sender.closed().await
    }

    /// Registers a [`Stream`] of idle events to be merged into the actor's
    /// runtime. Each event yielded by the stream is dispatched to
    /// [`Actor::on_idle`](crate::Actor::on_idle).
    ///
    /// Streams are owned by the runtime — their internal state (timer schedules,
    /// channel buffers, etc.) survives across runtime-loop iterations, so a
    /// stream like [`IntervalStream`](https://docs.rs/tokio-stream) fires reliably even
    /// when the surrounding `select!` arm is cancelled by a higher-priority
    /// branch. This is the chief reason to prefer subscriptions over open-coded
    /// idle loops.
    ///
    /// **Do not capture a strong `ActorRef` of this same actor inside the
    /// stream.** Because the runtime task owns the stream, a stream that owns
    /// a strong self-reference forms a permanent keep-alive cycle: dropping
    /// every external `ActorRef` then never terminates the actor (the
    /// ref-drop shutdown path can't fire), and once the last external ref is
    /// gone there is no handle left to call [`stop`](Self::stop)/[`kill`](Self::kill)
    /// with — the task leaks for the life of the runtime. Capture an
    /// [`ActorWeak`] (via [`downgrade`](Self::downgrade)) and upgrade per
    /// event instead.
    ///
    /// Subscription can be called any number of times, from any context with
    /// access to an [`ActorRef`] — typically from [`Actor::on_start`] for the
    /// initial sources, and from message handlers to attach additional sources
    /// dynamically.
    ///
    /// This method is synchronous and never awaits. It uses [`try_send`] under
    /// the hood so that subscriptions inside `on_start` cannot deadlock the
    /// runtime: at that moment the lifecycle is still awaiting `on_start`, so
    /// the subscribe channel's receiver is not yet being polled and any
    /// `.await` would block forever. The bounded buffer (capacity
    /// [`IDLE_SUBSCRIBE_CHANNEL_CAPACITY`](crate::IDLE_SUBSCRIBE_CHANNEL_CAPACITY)
    /// by default, configurable via
    /// [`SpawnOptions::with_idle_capacity`](crate::SpawnOptions::with_idle_capacity))
    /// absorbs bursts; an explicit `Err` is returned if it is exceeded.
    ///
    /// [`try_send`]: tokio::sync::mpsc::Sender::try_send
    ///
    /// # Errors
    ///
    /// Every failure returns an [`IdleSubscribeError`] that hands the stream
    /// back to the caller (via
    /// [`take_stream`](IdleSubscribeError::take_stream) /
    /// [`into_parts`](IdleSubscribeError::into_parts)), so an exclusive,
    /// non-reconstructible source (e.g. the only receiver of a data channel)
    /// is never silently destroyed.
    /// The carried [`error`](IdleSubscribeError::error) is one of:
    ///
    /// - [`Error::IdleChannelNotEnabled`] when the actor was spawned without
    ///   [`SpawnOptions::with_idle`](crate::SpawnOptions::with_idle). The idle channel is
    ///   **off by default**; enable it explicitly to use `on_idle`. This is a configuration
    ///   error and is terminal.
    /// - [`Error::ChannelFull`] when the bounded subscribe buffer is at capacity.
    ///   This is transient ([`Error::is_retryable`] returns `true`) **only for
    ///   callers outside this actor** — the buffer drains exclusively between
    ///   the actor's own handler invocations, so retrying in a loop from inside
    ///   `on_start` or one of this actor's handlers can never succeed (see
    ///   [`Error::ChannelFull`] for the livelock hazard). Instead, spawn with a
    ///   larger [`with_idle_capacity`](crate::SpawnOptions::with_idle_capacity),
    ///   merge sources into one stream before subscribing, or batch
    ///   subscriptions across separate handler invocations.
    /// - [`Error::Send`] when the actor is no longer alive (channel closed) — terminal.
    ///   The subscribe channel is closed as soon as the actor *begins* stopping
    ///   (graceful stop, kill, or an `on_idle` failure), so most termination-racing
    ///   calls fail here rather than succeed.
    ///
    /// # Ok is "accepted", not "installed"
    ///
    /// `Ok(())` means the stream was enqueued for installation. A call that
    /// races the very start of the actor's shutdown can still return `Ok` for
    /// a stream that is never polled — it is discarded (and its drop runs)
    /// when the actor finishes stopping; the shutdown drain logs the count at
    /// debug level.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Inside `on_start`:
    /// actor_ref.subscribe_idle(
    ///     tokio_stream::wrappers::IntervalStream::new(
    ///         tokio::time::interval(std::time::Duration::from_secs(1))
    ///     ).map(|_| Tick)
    /// )?;
    /// ```
    pub fn subscribe_idle<S>(&self, stream: S) -> std::result::Result<(), IdleSubscribeError<T>>
    where
        S: Stream<Item = T::IdleEvent> + Send + 'static,
    {
        let boxed: IdleEventStream<T> = stream.boxed();

        let Some(idle_subscribe_sender) = self.idle_subscribe_sender.as_ref() else {
            warn!("subscribe_idle called on actor without an idle channel");
            return Err(IdleSubscribeError::new(
                Error::IdleChannelNotEnabled {
                    identity: self.identity(),
                },
                boxed,
            ));
        };

        // `try_send` hands the stream back on failure; thread it through to
        // the caller instead of dropping it — idle streams often wrap
        // exclusive resources that cannot be reconstructed for a retry.
        idle_subscribe_sender.try_send(boxed).map_err(|e| match e {
            mpsc::error::TrySendError::Full(stream) => IdleSubscribeError::new(
                Error::ChannelFull {
                    identity: self.identity(),
                    channel: Channel::IdleSubscribe,
                },
                stream,
            ),
            mpsc::error::TrySendError::Closed(stream) => IdleSubscribeError::new(
                Error::Send {
                    identity: self.identity(),
                    details: "Idle subscribe channel closed (actor is no longer alive)",
                },
                stream,
            ),
        })
    }

    /// Sends a message to the actor without awaiting a reply (fire-and-forget).
    ///
    /// The message is sent to the actor's mailbox for processing.
    /// This method returns once the message is admitted into the mailbox —
    /// **when the mailbox is full, it waits for space** rather than returning
    /// an error (use [`tell_with_timeout`](Self::tell_with_timeout) for a
    /// bounded wait).
    ///
    /// Type safety: Only messages that the actor `T` can handle via [`Message<M>`] trait are accepted.
    ///
    /// # Deadlock warning (self-send)
    ///
    /// Calling this on the actor's **own** `ActorRef` from inside one of its
    /// handlers or lifecycle hooks while its mailbox is full is an
    /// unrecoverable deadlock: the only consumer of the mailbox is the actor's
    /// runtime loop, which is parked awaiting that very handler, so admission
    /// can never succeed and [`kill`](Self::kill) cannot interrupt the wait.
    /// With the `deadlock-detection` feature enabled this panics immediately
    /// instead of hanging. Prefer [`tell_with_timeout`](Self::tell_with_timeout)
    /// (bounded, recoverable) or send from a spawned task.
    #[cfg_attr(feature = "tracing", tracing::instrument(
        level = "debug",
        name = "actor_tell",
        fields(
            actor_id = %self.identity(),
            message_type = %std::any::type_name::<M>()
        ),
        skip(self, msg)
    ))]
    pub async fn tell<M>(&self, msg: M) -> Result<()>
    where
        M: Send + 'static,
        T: Message<M>,
    {
        self.tell_inner(msg, true).await
    }

    /// Shared implementation of [`tell`](Self::tell) and
    /// [`tell_with_timeout`](Self::tell_with_timeout).
    ///
    /// `detect_self_deadlock` arms the deadlock-detection check for unbounded
    /// admission waits: a no-timeout tell issued from inside this actor's own
    /// handler while its mailbox is full can never be admitted (the loop that
    /// would free a slot is parked awaiting that very handler), so under the
    /// `deadlock-detection` feature it panics instead of hanging forever.
    /// `tell_with_timeout` passes `false` — its admission wait is bounded by
    /// the caller's timeout and therefore recoverable, not a deadlock.
    async fn tell_inner<M>(&self, msg: M, detect_self_deadlock: bool) -> Result<()>
    where
        M: Send + 'static,
        T: Message<M>,
    {
        #[cfg(not(feature = "deadlock-detection"))]
        let _ = detect_self_deadlock;

        let envelope = MailboxMessage::Envelope {
            payload: Box::new(msg),
            reply_channel: None,     // reply_channel is None for tell
            actor_ref: self.clone(), // Include the actor ref for context
            ask_edge: Default::default(),
        };

        debug!("Sending tell message (fire-and-forget)");

        // Deadlock detection: probe admission first so a full mailbox is
        // observable before committing to an unbounded wait. `Closed` falls
        // through to `send().await`, which fails fast on the standard error
        // path below.
        #[cfg(feature = "deadlock-detection")]
        let envelope = match self.sender.try_send(envelope) {
            Ok(()) => {
                debug!("Tell message sent successfully");
                return Ok(());
            }
            Err(mpsc::error::TrySendError::Full(env)) => {
                if detect_self_deadlock {
                    self.panic_if_self_send_on_full("tell");
                }
                env
            }
            Err(mpsc::error::TrySendError::Closed(env)) => env,
        };

        let result = if self.sender.send(envelope).await.is_err() {
            crate::dead_letter::record::<M>(
                self.identity(),
                crate::dead_letter::DeadLetterReason::ActorStopped,
                "tell",
            );
            Err(Error::Send {
                identity: self.identity(),
                details: "Mailbox channel closed",
            })
        } else {
            Ok(())
        };

        match &result {
            Ok(_) => debug!("Tell message sent successfully"),
            Err(e) => warn!(error = %e, "Failed to send tell message"),
        }

        result
    }

    /// Deadlock-detection helper: panics when an *unbounded* admission wait on
    /// this actor's own full mailbox is attempted from inside one of its own
    /// handlers or lifecycle hooks. The mailbox's only consumer is the actor's
    /// runtime loop, which is parked awaiting the very code making this call —
    /// the wait can never be satisfied and `kill()` cannot interrupt it, so
    /// this is a guaranteed deadlock, not a heuristic.
    #[cfg(feature = "deadlock-detection")]
    fn panic_if_self_send_on_full(&self, operation: &str) {
        let is_self = crate::CURRENT_ACTOR
            .try_with(|id| id.id == self.id.id)
            .unwrap_or(false);
        if is_self {
            let prefix = crate::DEADLOCK_PANIC_PREFIX;
            panic!(
                "{prefix}: {operation} to self on a full mailbox from inside actor {}'s \
                 own handler or lifecycle hook.\n\
                 The mailbox's only consumer is this actor's runtime loop, which is parked \
                 awaiting the current handler, so admission can never succeed and kill() cannot \
                 interrupt the wait. Use tell_with_timeout, a larger mailbox capacity, or send \
                 from a spawned task.",
                self.id
            );
        }
    }

    /// Sends a message to the actor without awaiting a reply (fire-and-forget) with a timeout.
    ///
    /// Similar to [`ActorRef::tell`], but allows specifying a timeout for the send operation.
    /// The message is sent to the actor's mailbox, and this method will return once
    /// the message is sent or timeout if the send operation doesn't complete
    /// within the specified duration.
    #[cfg_attr(feature = "tracing", tracing::instrument(
        level = "debug",
        name = "actor_tell_with_timeout",
        fields(
            actor_id = %self.identity(),
            message_type = %std::any::type_name::<M>(),
            timeout_ms = timeout.as_millis()
        ),
        skip(self, msg)
    ))]
    pub async fn tell_with_timeout<M>(&self, msg: M, timeout: Duration) -> Result<()>
    where
        T: Message<M>,
        M: Send + 'static,
    {
        debug!(
            timeout_ms = timeout.as_millis(),
            "Sending tell message with timeout"
        );

        // `detect_self_deadlock = false`: a full-mailbox self-send through this
        // method is bounded by `timeout` and resolves as a recoverable
        // `Error::Timeout`, so it is not a deadlock.
        let result = tokio::time::timeout(timeout, self.tell_inner(msg, false))
            .await
            .map_err(|_| {
                crate::dead_letter::record::<M>(
                    self.identity(),
                    crate::dead_letter::DeadLetterReason::Timeout,
                    "tell",
                );
                Error::Timeout {
                    identity: self.identity(),
                    timeout,
                    operation: Operation::Tell,
                }
            })?;

        match &result {
            Ok(_) => debug!("Tell with timeout completed successfully"),
            Err(e) => warn!(error = %e, "Tell with timeout failed"),
        }

        result
    }

    /// Sends a message to the actor and awaits a reply.
    ///
    /// The message is sent to the actor's mailbox, and this method will wait for
    /// the actor to process the message and send a reply.
    ///
    /// Type safety: The return type `R` is automatically inferred from the [`Message<M>`] trait
    /// implementation, ensuring compile-time type safety for replies.
    #[cfg_attr(feature = "tracing", tracing::instrument(
        level = "debug",
        name = "actor_ask",
        fields(
            actor_id = %self.identity(),
            message_type = %std::any::type_name::<M>(),
            reply_type = %std::any::type_name::<T::Reply>()
        ),
        skip(self, msg)
    ))]
    pub async fn ask<M>(&self, msg: M) -> Result<T::Reply>
    where
        T: Message<M>,
        M: Send + 'static,
        T::Reply: Send + 'static,
    {
        self.ask_inner(msg, Operation::Ask).await
    }

    /// Shared implementation of [`ask`](Self::ask) and the `blocking_ask`
    /// fast path. `operation` labels dead letters, deadlock-detection edges,
    /// and [`Error::Timeout`] so the reported operation always matches the
    /// public API the caller actually invoked, regardless of execution path.
    async fn ask_inner<M>(&self, msg: M, operation: Operation) -> Result<T::Reply>
    where
        T: Message<M>,
        M: Send + 'static,
        T::Reply: Send + 'static,
    {
        // `operation` only labels the deadlock-detection edge and dead-letter
        // records here (this helper never builds `Error::Timeout`), so reduce
        // it to its string form once.
        let operation = operation.as_str();

        // Deadlock detection: register this `ask` as a wait-for edge and hold
        // the guard until the reply is received (or this future is dropped).
        // Panics if it would close an ask cycle. The envelope carries a token
        // so the callee removes the edge the moment it sends the reply —
        // waiting for this future to resume would leave a stale-edge window
        // that produces false-positive cycle panics.
        #[cfg(feature = "deadlock-detection")]
        let _guard = crate::register_ask_edge(self.identity(), operation);
        #[cfg(feature = "deadlock-detection")]
        let ask_edge = _guard.as_ref().map(|g| g.token());
        #[cfg(not(feature = "deadlock-detection"))]
        let ask_edge = ();

        let (reply_tx, reply_rx) = oneshot::channel();
        let envelope = MailboxMessage::Envelope {
            payload: Box::new(msg),
            reply_channel: Some(reply_tx),
            actor_ref: self.clone(), // Include the actor ref for context
            ask_edge,
        };

        debug!("Sending ask message and waiting for reply");

        if self.sender.send(envelope).await.is_err() {
            crate::dead_letter::record::<M>(
                self.identity(),
                crate::dead_letter::DeadLetterReason::ActorStopped,
                operation,
            );

            warn!("Failed to send ask message: mailbox channel closed");

            return Err(Error::Send {
                identity: self.identity(),
                details: "Mailbox channel closed",
            });
        }

        match reply_rx.await {
            Ok(reply_any) => {
                // Successfully received reply from actor
                match reply_any.downcast::<T::Reply>() {
                    Ok(reply) => {
                        debug!("Ask reply received successfully");
                        Ok(*reply)
                    }
                    Err(_) => {
                        warn!(
                            expected_type = %std::any::type_name::<T::Reply>(),
                            "Ask reply type downcast failed"
                        );
                        Err(Error::Downcast {
                            identity: self.identity(),
                            expected_type: std::any::type_name::<T::Reply>(),
                        })
                    }
                }
            }
            Err(_recv_err) => {
                crate::dead_letter::record::<M>(
                    self.identity(),
                    crate::dead_letter::DeadLetterReason::ReplyDropped,
                    operation,
                );

                warn!("Ask reply channel closed unexpectedly");
                Err(Error::Receive {
                    identity: self.identity(),
                    details: "Reply channel closed unexpectedly",
                })
            }
        }
    }

    /// Sends a message to the actor and awaits a reply with a timeout.
    ///
    /// Similar to [`ActorRef::ask`], but allows specifying a timeout for the operation.
    /// The message is sent to the actor's mailbox, and this method will wait for
    /// the actor to process the message and send a reply, or timeout if the reply
    /// doesn't arrive within the specified duration.
    #[cfg_attr(feature = "tracing", tracing::instrument(
        level = "debug",
        name = "actor_ask_with_timeout",
        fields(
            actor_id = %self.identity(),
            message_type = %std::any::type_name::<M>(),
            reply_type = %std::any::type_name::<T::Reply>(),
            timeout_ms = timeout.as_millis()
        ),
        skip(self, msg)
    ))]
    pub async fn ask_with_timeout<M>(&self, msg: M, timeout: Duration) -> Result<T::Reply>
    where
        T: Message<M>,
        M: Send + 'static,
        T::Reply: Send + 'static,
    {
        self.ask_timeout_inner(msg, timeout, Operation::Ask).await
    }

    /// Shared implementation of [`ask_with_timeout`](Self::ask_with_timeout)
    /// and the `blocking_ask` fast path. See [`ask_inner`](Self::ask_inner)
    /// for the role of `operation`.
    async fn ask_timeout_inner<M>(
        &self,
        msg: M,
        timeout: Duration,
        operation: Operation,
    ) -> Result<T::Reply>
    where
        T: Message<M>,
        M: Send + 'static,
        T::Reply: Send + 'static,
    {
        debug!(
            timeout_ms = timeout.as_millis(),
            "Sending ask message with timeout"
        );

        let result = tokio::time::timeout(timeout, self.ask_inner(msg, operation))
            .await
            .map_err(|_| {
                crate::dead_letter::record::<M>(
                    self.identity(),
                    crate::dead_letter::DeadLetterReason::Timeout,
                    operation.as_str(),
                );
                Error::Timeout {
                    identity: self.identity(),
                    timeout,
                    operation,
                }
            })?;

        match &result {
            Ok(_) => debug!("Ask with timeout completed successfully"),
            Err(e) => warn!(error = %e, "Ask with timeout failed"),
        }

        result
    }

    /// Sends a message through the priority channel without awaiting a reply (fire-and-forget).
    ///
    /// The priority channel bypasses the regular mailbox queue: at every iteration of the
    /// actor's runtime loop it is polled with higher priority than the regular mailbox,
    /// but lower priority than the `kill()` (terminate) signal. This is intended for short,
    /// infrequent control-plane messages such as health checks or pause/resume signals.
    ///
    /// `timeout` is **mandatory** and applies to the admission of the message into the
    /// priority channel. The priority channel has a fixed capacity of 1, so a wedged
    /// actor would otherwise block the sender indefinitely; the timeout makes that
    /// failure mode visible.
    ///
    /// # Errors
    ///
    /// - [`Error::PriorityChannelNotEnabled`] if the actor was spawned without
    ///   [`SpawnOptions::with_priority`](crate::SpawnOptions::with_priority).
    ///   This is a configuration error and is **not** recorded as a dead letter.
    /// - [`Error::Send`] if the actor has already stopped (recorded as a dead letter).
    /// - [`Error::Timeout`] if admission did not complete within `timeout` (recorded as
    ///   a dead letter).
    #[cfg_attr(feature = "tracing", tracing::instrument(
        level = "debug",
        name = "actor_tell_priority",
        fields(
            actor_id = %self.identity(),
            message_type = %std::any::type_name::<M>(),
            timeout_ms = timeout.as_millis()
        ),
        skip(self, msg)
    ))]
    pub async fn tell_priority<M>(&self, msg: M, timeout: Duration) -> Result<()>
    where
        M: Send + 'static,
        T: Message<M>,
    {
        let Some(priority_sender) = self.priority_sender.as_ref() else {
            warn!("tell_priority called on actor without a priority channel");
            return Err(Error::PriorityChannelNotEnabled {
                identity: self.identity(),
            });
        };

        let envelope = MailboxMessage::Envelope {
            payload: Box::new(msg),
            reply_channel: None,
            actor_ref: self.clone(),
            ask_edge: Default::default(),
        };

        debug!(
            timeout_ms = timeout.as_millis(),
            "Sending priority tell message"
        );

        let send_fut = priority_sender.send(envelope);
        let result = match tokio::time::timeout(timeout, send_fut).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => {
                crate::dead_letter::record::<M>(
                    self.identity(),
                    crate::dead_letter::DeadLetterReason::ActorStopped,
                    "tell_priority",
                );
                Err(Error::Send {
                    identity: self.identity(),
                    details: "Priority channel closed",
                })
            }
            Err(_) => {
                crate::dead_letter::record::<M>(
                    self.identity(),
                    crate::dead_letter::DeadLetterReason::Timeout,
                    "tell_priority",
                );
                Err(Error::Timeout {
                    identity: self.identity(),
                    timeout,
                    operation: Operation::TellPriority,
                })
            }
        };

        match &result {
            Ok(_) => debug!("Priority tell message sent successfully"),
            Err(e) => warn!(error = %e, "Priority tell failed"),
        }

        result
    }

    /// Sends a message through the priority channel and awaits a typed reply.
    ///
    /// `timeout` is **mandatory** and applies to the entire round-trip (admission into
    /// the priority channel plus reply wait). The reply travels through a dedicated
    /// `oneshot` channel separate from the priority slot, so a single in-flight priority
    /// message does not block another `ask_priority` from issuing a fresh request as
    /// soon as the actor pulls the previous one off the channel.
    ///
    /// # Errors
    ///
    /// - [`Error::PriorityChannelNotEnabled`] if the actor was spawned without the
    ///   priority channel enabled (not recorded as a dead letter).
    /// - [`Error::Send`] if the actor has already stopped (recorded as a dead letter).
    /// - [`Error::Timeout`] if the round-trip did not complete within `timeout`
    ///   (recorded as a dead letter).
    /// - [`Error::Receive`] if the reply channel was dropped before a response arrived
    ///   (recorded as a dead letter).
    /// - [`Error::Downcast`] if the handler returned a value of an unexpected type.
    #[cfg_attr(feature = "tracing", tracing::instrument(
        level = "debug",
        name = "actor_ask_priority",
        fields(
            actor_id = %self.identity(),
            message_type = %std::any::type_name::<M>(),
            reply_type = %std::any::type_name::<T::Reply>(),
            timeout_ms = timeout.as_millis()
        ),
        skip(self, msg)
    ))]
    pub async fn ask_priority<M>(&self, msg: M, timeout: Duration) -> Result<T::Reply>
    where
        T: Message<M>,
        M: Send + 'static,
        T::Reply: Send + 'static,
    {
        self.ask_priority_inner(msg, timeout, Operation::AskPriority)
            .await
    }

    /// Shared implementation of [`ask_priority`](Self::ask_priority) and
    /// [`blocking_ask_priority`](Self::blocking_ask_priority). `operation`
    /// labels dead letters, deadlock-detection edges, and [`Error::Timeout`]
    /// so the reported operation always matches the public API the caller
    /// actually invoked, regardless of execution path.
    async fn ask_priority_inner<M>(
        &self,
        msg: M,
        timeout: Duration,
        operation: Operation,
    ) -> Result<T::Reply>
    where
        T: Message<M>,
        M: Send + 'static,
        T::Reply: Send + 'static,
    {
        let Some(priority_sender) = self.priority_sender.as_ref() else {
            warn!("ask_priority called on actor without a priority channel");
            return Err(Error::PriorityChannelNotEnabled {
                identity: self.identity(),
            });
        };

        // String label for the deadlock-detection edge and dead-letter records;
        // the typed `operation` is kept for the `Error::Timeout` below.
        let operation_label = operation.as_str();

        // Deadlock detection: a priority ask still parks the calling actor's
        // message loop while it awaits the reply, so a cycle through the priority
        // channel stalls just like a regular `ask` (here bounded by `timeout`).
        // Register the edge before sending and hold the guard across the wait so
        // the cycle is detected immediately instead of only timing out.
        #[cfg(feature = "deadlock-detection")]
        let _guard = crate::register_ask_edge(self.identity(), operation_label);
        #[cfg(feature = "deadlock-detection")]
        let ask_edge = _guard.as_ref().map(|g| g.token());
        #[cfg(not(feature = "deadlock-detection"))]
        let ask_edge = ();

        let result = tokio::time::timeout(timeout, async {
            let (reply_tx, reply_rx) = oneshot::channel();
            let envelope = MailboxMessage::Envelope {
                payload: Box::new(msg),
                reply_channel: Some(reply_tx),
                actor_ref: self.clone(),
                ask_edge,
            };

            if priority_sender.send(envelope).await.is_err() {
                crate::dead_letter::record::<M>(
                    self.identity(),
                    crate::dead_letter::DeadLetterReason::ActorStopped,
                    operation_label,
                );
                return Err(Error::Send {
                    identity: self.identity(),
                    details: "Priority channel closed",
                });
            }

            match reply_rx.await {
                Ok(reply_any) => match reply_any.downcast::<T::Reply>() {
                    Ok(reply) => Ok(*reply),
                    Err(_) => Err(Error::Downcast {
                        identity: self.identity(),
                        expected_type: std::any::type_name::<T::Reply>(),
                    }),
                },
                Err(_) => {
                    crate::dead_letter::record::<M>(
                        self.identity(),
                        crate::dead_letter::DeadLetterReason::ReplyDropped,
                        operation_label,
                    );
                    Err(Error::Receive {
                        identity: self.identity(),
                        details: "Reply channel closed unexpectedly",
                    })
                }
            }
        })
        .await;

        match result {
            Ok(inner) => inner,
            Err(_) => {
                crate::dead_letter::record::<M>(
                    self.identity(),
                    crate::dead_letter::DeadLetterReason::Timeout,
                    operation_label,
                );
                Err(Error::Timeout {
                    identity: self.identity(),
                    timeout,
                    operation,
                })
            }
        }
    }

    /// Blocking equivalent of [`tell_priority`](Self::tell_priority). `timeout` is
    /// mandatory for the same reason as the async version.
    ///
    /// # Execution paths
    ///
    /// - **Priority slot empty (every context)**: a lock-free `try_send`
    ///   admits the message immediately — no blocking, no thread spawn, no
    ///   `block_in_place`.
    /// - **Priority slot full, worker of a multi-thread Tokio runtime
    ///   (including `async fn`)**: uses [`tokio::task::block_in_place`] +
    ///   [`Handle::block_on`](tokio::runtime::Handle::block_on) to await
    ///   admission on the caller's runtime. No new thread or runtime is created.
    /// - **Priority slot full, any other context that may block** (no
    ///   runtime, or a `spawn_blocking` thread of a `current_thread`
    ///   runtime): spawns a short-lived dedicated thread with a temporary
    ///   single-thread runtime to await admission with the timeout. Contexts
    ///   that may *not* block panic instead — see [§ Panics](#panics).
    ///
    /// # Worker-pool caveat
    ///
    /// On a multi-thread runtime the `block_in_place` path holds the calling
    /// worker thread for the duration of the call. Avoid invoking from
    /// runtimes with very few workers; prefer [`tell_priority`](Self::tell_priority)
    /// directly from async code where possible.
    ///
    /// # Panics
    ///
    /// The conditions below require a full priority slot — the `try_send`
    /// hot path never blocks, so it never panics.
    ///
    /// - When called from inside a [`LocalSet`](https://docs.rs/tokio/latest/tokio/task/struct.LocalSet.html)
    ///   running on a multi-thread runtime: the runtime handle reports
    ///   multi-thread flavor, but `block_in_place` is not permitted there.
    /// - When called from an async context that `block_in_place` cannot
    ///   rescue — a task on a `current_thread` runtime, or any thread driving
    ///   a `current_thread` runtime's `Runtime::block_on`/`Handle::block_on`
    ///   (a *multi-thread* runtime's `block_on` driver takes the
    ///   `block_in_place` path and is fine) — this panics with Tokio's
    ///   "Cannot block the current thread from within a runtime". Parking such
    ///   a thread would starve every actor on that runtime for the full
    ///   timeout; the loud panic surfaces the bug instead. Call from a
    ///   [`spawn_blocking`](tokio::task::spawn_blocking) thread, or use the
    ///   async [`tell_priority`](Self::tell_priority), instead.
    /// - When the caller's own multi-thread runtime was built **without a time
    ///   driver** (no `enable_time()` / `enable_all()`): the fast path awaits
    ///   the admission timeout on that runtime, so `tokio::time::timeout`
    ///   panics because no timer is available. The slow-path temporary runtime
    ///   always enables timers, so only the caller's own runtime is affected.
    ///
    /// # Deadlock warning
    ///
    /// Never call this (or any `blocking_*` method) from inside the actor's own
    /// message handler — directly on `self`, or transitively via a cycle that
    /// routes back into this actor. If the priority slot is full, the call parks
    /// the actor's message loop synchronously while waiting for admission that
    /// only that same loop could make room for — an unrecoverable hang that
    /// `kill()` cannot interrupt (note that `deadlock-detection` only tracks
    /// `ask` cycles, not `tell`). Prefer the async
    /// [`tell_priority`](Self::tell_priority) from handler code.
    pub fn blocking_tell_priority<M>(&self, msg: M, timeout: Duration) -> Result<()>
    where
        M: Send + 'static,
        T: Message<M>,
    {
        let Some(priority_sender) = self.priority_sender.as_ref() else {
            return Err(Error::PriorityChannelNotEnabled {
                identity: self.identity(),
            });
        };

        let envelope = MailboxMessage::Envelope {
            payload: Box::new(msg),
            reply_channel: None,
            actor_ref: self.clone(),
            ask_edge: Default::default(),
        };

        // Hot path (every context): the priority slot is empty — a single
        // lock-free `try_send` completes the call. In particular this skips
        // `block_in_place` on multi-thread-runtime workers, which migrates
        // the whole worker loop to another thread and back even when the
        // send itself is immediately ready.
        let envelope = match priority_sender.try_send(envelope) {
            Ok(()) => return Ok(()),
            Err(mpsc::error::TrySendError::Closed(_)) => {
                crate::dead_letter::record::<M>(
                    self.identity(),
                    crate::dead_letter::DeadLetterReason::ActorStopped,
                    "blocking_tell_priority",
                );
                warn!("Failed to send blocking priority tell: priority channel closed");
                return Err(Error::Send {
                    identity: self.identity(),
                    details: "Priority channel closed",
                });
            }
            Err(mpsc::error::TrySendError::Full(env)) => env,
        };

        // Priority slot occupied: await admission with the timeout.
        let identity = self.identity();
        let priority_sender = priority_sender.clone();
        let admit = async move {
            match tokio::time::timeout(timeout, priority_sender.send(envelope)).await {
                Ok(Ok(())) => {
                    debug!("Blocking priority tell message sent successfully");
                    Ok(())
                }
                Ok(Err(_)) => {
                    crate::dead_letter::record::<M>(
                        identity,
                        crate::dead_letter::DeadLetterReason::ActorStopped,
                        "blocking_tell_priority",
                    );
                    warn!("Failed to send blocking priority tell: priority channel closed");
                    Err(Error::Send {
                        identity,
                        details: "Priority channel closed",
                    })
                }
                Err(_) => {
                    crate::dead_letter::record::<M>(
                        identity,
                        crate::dead_letter::DeadLetterReason::Timeout,
                        "blocking_tell_priority",
                    );
                    warn!(
                        timeout_ms = timeout.as_millis(),
                        "Blocking priority tell timed out awaiting slot admission"
                    );
                    Err(Error::Timeout {
                        identity,
                        timeout,
                        operation: Operation::BlockingTellPriority,
                    })
                }
            }
        };

        // On a multi-thread-runtime worker, reuse the caller's runtime;
        // otherwise the bounded wait needs a timer, so spawn a dedicated
        // thread with a temporary runtime.
        match current_multi_thread_handle() {
            Some(handle) => block_on_current_runtime(handle, admit),
            None => run_blocking_with_runtime(
                identity,
                "Priority blocking thread terminated unexpectedly",
                admit,
            ),
        }
    }

    /// Blocking equivalent of [`ask_priority`](Self::ask_priority). `timeout` is
    /// mandatory for the same reason as the async version.
    ///
    /// # Execution paths
    ///
    /// - **Worker of a multi-thread Tokio runtime (including `async fn`)**: uses
    ///   [`tokio::task::block_in_place`] + [`Handle::block_on`](tokio::runtime::Handle::block_on)
    ///   to reuse the caller's runtime. No new thread or runtime is created.
    /// - **Any other context that may block** (no runtime, or a
    ///   `spawn_blocking` thread of a `current_thread` runtime): spawns a
    ///   short-lived dedicated thread with a temporary single-thread runtime
    ///   to await the reply with the timeout. Contexts that may *not* block
    ///   panic instead — see [§ Panics](#panics).
    ///
    /// Unlike the `tell` variants, `ask` cannot take a `try_send` fast path
    /// because a sync `recv_timeout` for the reply channel is unavailable.
    ///
    /// # Worker-pool caveat
    ///
    /// On a multi-thread runtime the `block_in_place` path holds the calling
    /// worker thread for the duration of the call. Avoid invoking from
    /// runtimes with very few workers.
    ///
    /// # Panics
    ///
    /// - When called from inside a [`LocalSet`](https://docs.rs/tokio/latest/tokio/task/struct.LocalSet.html)
    ///   running on a multi-thread runtime: the runtime handle reports
    ///   multi-thread flavor, but `block_in_place` is not permitted there.
    /// - When called from an async context that `block_in_place` cannot
    ///   rescue — a task on a `current_thread` runtime, or any thread driving
    ///   a `current_thread` runtime's `Runtime::block_on`/`Handle::block_on`
    ///   (a *multi-thread* runtime's `block_on` driver takes the
    ///   `block_in_place` path and is fine) — this panics with Tokio's
    ///   "Cannot block the current thread from within a runtime". Parking such
    ///   a thread would starve every actor on that runtime for the full
    ///   timeout; the loud panic surfaces the bug instead. Call from a
    ///   [`spawn_blocking`](tokio::task::spawn_blocking) thread, or use the
    ///   async [`ask_priority`](Self::ask_priority), instead.
    ///
    /// # Deadlock warning
    ///
    /// Never call this (or any `blocking_*` method) from inside the actor's own
    /// message handler — directly on `self`, or transitively via a cycle that
    /// asks back into this actor. The call parks the actor's message loop
    /// synchronously while waiting for a reply that only that same loop could
    /// produce — an unrecoverable hang that `kill()` cannot interrupt. Enable
    /// the `deadlock-detection` feature to turn such a cycle into an immediate
    /// panic instead, or use the async [`ask_priority`](Self::ask_priority) from
    /// handler code.
    pub fn blocking_ask_priority<M>(&self, msg: M, timeout: Duration) -> Result<T::Reply>
    where
        T: Message<M>,
        M: Send + 'static,
        T::Reply: Send + 'static,
    {
        if self.priority_sender.is_none() {
            return Err(Error::PriorityChannelNotEnabled {
                identity: self.identity(),
            });
        }

        // Fast path: caller is inside a multi-thread runtime. Reuse it.
        if let Some(handle) = current_multi_thread_handle() {
            return block_on_current_runtime(
                handle,
                self.ask_priority_inner(msg, timeout, Operation::BlockingAskPriority),
            );
        }

        let self_clone = self.clone();
        run_blocking_with_runtime(
            self.identity(),
            "Priority blocking thread terminated unexpectedly",
            async move {
                self_clone
                    .ask_priority_inner(msg, timeout, Operation::BlockingAskPriority)
                    .await
            },
        )
    }

    /// Sends an immediate termination signal to the actor.
    ///
    /// The actor stops as soon as it returns to its message loop: any messages still
    /// queued in the mailbox are discarded without being processed (the physical
    /// drain — and their dead-letter records — happens after `on_stop(killed = true)`
    /// returns), and the actor's final result indicates it was killed.
    ///
    /// **Cooperative, not preemptive.** `kill()` cannot interrupt a message handler
    /// (or [`on_idle`](crate::Actor::on_idle)) that is *already executing* — the
    /// in-flight future runs until its next return before the terminate signal is
    /// observed. A handler blocked forever on an `.await`, or spinning in a
    /// synchronous/CPU-bound loop, therefore blocks `kill()` as well; the terminate
    /// signal is delivered but cannot be acted on until control returns to the loop.
    /// The same applies to [`on_start`](crate::Actor::on_start), only more so:
    /// [`spawn`](crate::spawn) hands out the `ActorRef` before `on_start` runs,
    /// but the runtime loop that observes terminate signals does not exist until
    /// `on_start` returns — a `kill()` issued while `on_start` awaits a resource
    /// that never arrives is enqueued yet never observed, `wait_stopped()` never
    /// resolves, and the `JoinHandle` never completes. Bound `on_start`'s own
    /// awaits with timeouts. Guard against all of these with timeouts on external
    /// operations, the `deadlock-detection` feature for `ask` cycles, and by
    /// isolating blocking work (e.g. `tokio::task::spawn_blocking` or a separate
    /// process).
    ///
    /// **First signal wins.** If a graceful stop is already in progress — the
    /// actor has dequeued a [`stop`](Self::stop) signal and is draining /
    /// running `on_stop(killed = false)` — a `kill()` arriving afterwards is
    /// enqueued but never observed: the actor completes the graceful stop and
    /// its result reports `killed: false`. Honoring the late kill would mean
    /// interrupting or re-running an `on_stop` that is already executing.
    ///
    /// This method is idempotent: a `Full` or `Closed` terminate channel is treated as
    /// "termination already in flight" — the desired terminal state is met either way.
    /// Both conditions are logged via `tracing::warn!` for diagnostics.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(level = "info", name = "actor_kill", skip(self))
    )]
    pub fn kill(&self) {
        info!(actor_id = %self.identity(), "Killing actor");

        // Use the dedicated terminate_sender with try_send
        match self.terminate_sender.try_send(ControlSignal::Terminate) {
            Ok(_) => {
                info!(
                    "Kill signal enqueued; takes effect when the actor next polls control \
                     signals (no effect if a graceful stop is already in progress)"
                );
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                // The channel has capacity 1, so Full means a Terminate is already queued.
                warn!("Failed to send Terminate to actor {}: terminate mailbox is full. Actor is likely already being terminated.", self.identity());
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                // The channel is closed; the actor is already stopped or has finished.
                warn!("Failed to send Terminate to actor {}: terminate mailbox closed. Actor might already be stopped.", self.identity());
            }
        }
    }

    /// Sends a graceful stop signal to the actor.
    ///
    /// The actor will process all messages currently in its mailbox and then stop.
    /// New messages sent after this call might be ignored or fail.
    /// The actor's final result will indicate normal completion.
    ///
    /// The returned future resolves once the stop signal is **enqueued**, not
    /// when the actor has finished stopping. Await the `JoinHandle` from
    /// [`spawn`](crate::spawn) or [`wait_stopped`](Self::wait_stopped) to
    /// observe completion. When the mailbox is full, this waits for space like
    /// [`tell`](Self::tell) does.
    ///
    /// This method is idempotent: a closed mailbox channel is treated as "stop already
    /// in flight" — the desired terminal state is met either way. The condition is
    /// logged via `tracing::warn!` for diagnostics. Conversely, a [`kill`](Self::kill)
    /// that arrives *after* the actor has begun this graceful stop is never observed
    /// (first signal wins; the result reports `killed: false`).
    ///
    /// # Deadlock warning (self-stop)
    ///
    /// Calling this on the actor's **own** `ActorRef` from inside one of its
    /// handlers or lifecycle hooks while its mailbox is full is an
    /// unrecoverable deadlock, identical to a full-mailbox self-[`tell`](Self::tell):
    /// the loop that would free a slot is parked awaiting that handler, and
    /// [`kill`](Self::kill) cannot interrupt it. With the `deadlock-detection`
    /// feature enabled this panics immediately instead of hanging.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(level = "info", name = "actor_stop", skip(self))
    )]
    pub async fn stop(&self) {
        let stop_msg = MailboxMessage::StopGracefully(self.clone());

        // Deadlock detection: probe admission first; see `tell_inner`. A
        // `Closed` result falls through to `send().await`, which fails fast on
        // the standard already-stopped path below.
        #[cfg(feature = "deadlock-detection")]
        let stop_msg = match self.sender.try_send(stop_msg) {
            Ok(()) => {
                info!(actor_id = %self.identity(), "Actor stop signal sent successfully");
                return;
            }
            Err(mpsc::error::TrySendError::Full(m)) => {
                self.panic_if_self_send_on_full("stop");
                m
            }
            Err(mpsc::error::TrySendError::Closed(m)) => m,
        };

        match self.sender.send(stop_msg).await {
            Ok(_) => {
                info!(actor_id = %self.identity(), "Actor stop signal sent successfully");
            }
            Err(_) => {
                // Mailbox channel is closed; the actor is already stopping or has stopped.
                warn!("Failed to send StopGracefully to actor {}: mailbox closed. Actor might already be stopped or stopping.", self.identity());
            }
        }
    }

    /// Synchronous version of [`ActorRef::tell`] that blocks until the message is sent.
    ///
    /// This method can be used from any thread, including non-async contexts,
    /// and from within `async fn` on a multi-thread Tokio runtime.
    ///
    /// # Execution paths
    ///
    /// - **Mailbox has room (every context)**: a lock-free `try_send` completes
    ///   the call immediately — no blocking, no thread spawn, no
    ///   `block_in_place`.
    /// - **Mailbox full, worker of a multi-thread Tokio runtime (including
    ///   `async fn`)**: uses [`tokio::task::block_in_place`] +
    ///   [`Handle::block_on`](tokio::runtime::Handle::block_on) to await
    ///   admission on the caller's runtime. No new thread or runtime is created.
    /// - **Mailbox full, `timeout: Some(_)`, any other context that may
    ///   block** (no runtime, or a `spawn_blocking` thread of a
    ///   `current_thread` runtime): spawns a short-lived dedicated thread with
    ///   a temporary single-thread runtime to await admission with the
    ///   timeout. Contexts that may *not* block panic instead — see
    ///   [§ Panics](#panics).
    /// - **Mailbox full, `timeout: None`, same blockable contexts**: uses
    ///   Tokio's `blocking_send` directly; blocks indefinitely until admitted.
    ///
    /// # Performance Considerations
    ///
    /// The slow-path fallbacks (mailbox full outside a multi-thread runtime)
    /// incur:
    /// - **Thread creation**: ~50-200μs depending on the platform
    /// - **Tokio runtime creation**: ~1-10μs for a single-threaded runtime
    ///
    /// All other execution paths above are sub-microsecond.
    ///
    /// # Worker-pool caveat
    ///
    /// On a multi-thread runtime the `block_in_place` path holds the calling
    /// worker thread for the duration of the call. Avoid invoking from
    /// runtimes with very few workers; prefer [`tell`](Self::tell) directly
    /// from async code where possible.
    ///
    /// # Panics
    ///
    /// The conditions below require a full mailbox — the `try_send` hot path
    /// never blocks, so it never panics.
    ///
    /// - When called from inside a [`LocalSet`](https://docs.rs/tokio/latest/tokio/task/struct.LocalSet.html)
    ///   running on a multi-thread runtime: the runtime handle reports
    ///   multi-thread flavor, but `block_in_place` is not permitted there.
    /// - When called from an async context that `block_in_place` cannot
    ///   rescue — a task on a `current_thread` runtime, or any thread driving
    ///   a `current_thread` runtime's `Runtime::block_on`/`Handle::block_on`
    ///   (a *multi-thread* runtime's `block_on` driver takes the
    ///   `block_in_place` path and is fine) — this panics with Tokio's
    ///   "Cannot block the current thread from within a runtime". Parking such
    ///   a thread would freeze the only thread able to drive the target actor,
    ///   turning the call into a silent deadlock of every actor on that
    ///   runtime that not even `kill()` could break; the loud panic surfaces
    ///   the bug instead. Call from a
    ///   [`spawn_blocking`](tokio::task::spawn_blocking) thread, or use the
    ///   async [`tell`](Self::tell), instead.
    /// - When a timeout is supplied and the caller's own multi-thread runtime
    ///   was built **without a time driver** (no `enable_time()` /
    ///   `enable_all()`): the fast path awaits the admission timeout on that
    ///   runtime, so `tokio::time::timeout` panics because no timer is
    ///   available. The timeout-less path and the slow-path temporary runtime
    ///   are unaffected.
    ///
    /// # Deadlock warning
    ///
    /// Never call this (or any `blocking_*` method) from inside the actor's own
    /// message handler — directly on `self`, or transitively via a cycle that
    /// routes back into this actor. If the target mailbox is full, the call
    /// parks the actor's message loop synchronously while waiting for admission
    /// that only that same loop could make room for — an unrecoverable hang that
    /// `kill()` cannot interrupt (note that `deadlock-detection` only tracks
    /// `ask` cycles, not `tell`; with that feature enabled, a timeout-less
    /// blocking self-send on a full mailbox panics instead of hanging). Prefer
    /// the async [`tell`](Self::tell) from handler code.
    #[cfg_attr(feature = "tracing", tracing::instrument(
        level = "debug",
        name = "actor_blocking_tell",
        fields(
            actor_id = %self.identity(),
            message_type = %std::any::type_name::<M>(),
            timeout_ms = timeout.map(|t| t.as_millis())
        ),
        skip(self, msg)
    ))]
    pub fn blocking_tell<M>(&self, msg: M, timeout: Option<Duration>) -> Result<()>
    where
        M: Send + 'static,
        T: Message<M>,
    {
        let envelope = MailboxMessage::Envelope {
            payload: Box::new(msg),
            reply_channel: None,     // reply_channel is None for tell
            actor_ref: self.clone(), // Include the actor ref for context
            ask_edge: Default::default(),
        };

        debug!("Sending blocking tell message (fire-and-forget)");

        // Hot path (every context): room in the mailbox — a single lock-free
        // `try_send` completes the call. In particular this skips
        // `block_in_place` on multi-thread-runtime workers, which migrates
        // the whole worker loop to another thread and back (microseconds of
        // scheduler churn) even when the send itself is immediately ready.
        let envelope = match self.sender.try_send(envelope) {
            Ok(()) => {
                debug!("Blocking tell message sent successfully");
                return Ok(());
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                crate::dead_letter::record::<M>(
                    self.identity(),
                    crate::dead_letter::DeadLetterReason::ActorStopped,
                    "blocking_tell",
                );
                warn!("Failed to send blocking tell message: mailbox channel closed");
                return Err(Error::Send {
                    identity: self.identity(),
                    details: "Mailbox channel closed",
                });
            }
            Err(mpsc::error::TrySendError::Full(env)) => {
                // A timeout-less blocking self-send on a full mailbox is the
                // same unrecoverable deadlock as the async `tell` case.
                #[cfg(feature = "deadlock-detection")]
                if timeout.is_none() {
                    self.panic_if_self_send_on_full("blocking_tell");
                }
                env
            }
        };

        // Mailbox full: await admission.
        let identity = self.identity();
        match timeout {
            Some(timeout) => {
                let sender = self.sender.clone();
                let admit = async move {
                    match tokio::time::timeout(timeout, sender.send(envelope)).await {
                        Ok(Ok(())) => {
                            debug!("Blocking tell message sent successfully");
                            Ok(())
                        }
                        Ok(Err(_)) => {
                            crate::dead_letter::record::<M>(
                                identity,
                                crate::dead_letter::DeadLetterReason::ActorStopped,
                                "blocking_tell",
                            );
                            warn!("Failed to send blocking tell message: mailbox channel closed");
                            Err(Error::Send {
                                identity,
                                details: "Mailbox channel closed",
                            })
                        }
                        Err(_) => {
                            crate::dead_letter::record::<M>(
                                identity,
                                crate::dead_letter::DeadLetterReason::Timeout,
                                "blocking_tell",
                            );
                            warn!(
                                timeout_ms = timeout.as_millis(),
                                "Blocking tell timed out awaiting mailbox admission"
                            );
                            Err(Error::Timeout {
                                identity,
                                timeout,
                                operation: Operation::BlockingTell,
                            })
                        }
                    }
                };
                // On a multi-thread-runtime worker, reuse the caller's runtime;
                // otherwise the bounded wait needs a timer, so spawn a
                // dedicated thread with a temporary runtime.
                match current_multi_thread_handle() {
                    Some(handle) => block_on_current_runtime(handle, admit),
                    None => run_blocking_with_runtime(
                        identity,
                        "Timeout thread terminated unexpectedly",
                        admit,
                    ),
                }
            }
            None => {
                if let Some(handle) = current_multi_thread_handle() {
                    return block_on_current_runtime(handle, async {
                        self.sender.send(envelope).await.map_err(|_| {
                            crate::dead_letter::record::<M>(
                                identity,
                                crate::dead_letter::DeadLetterReason::ActorStopped,
                                "blocking_tell",
                            );
                            warn!("Failed to send blocking tell message: mailbox channel closed");
                            Error::Send {
                                identity,
                                details: "Mailbox channel closed",
                            }
                        })
                    });
                }

                // `blocking_send` carries tokio's own guard: in any async
                // context (a `current_thread` runtime worker, or a thread
                // driving `block_on`) it panics with "Cannot block the current
                // thread from within a runtime" instead of parking — and
                // thereby starving — the runtime that must drive the target
                // actor. Do NOT route around that panic with a dedicated
                // thread: the park would turn the loud failure into a silent,
                // kill-proof deadlock of every actor on the caller's runtime.
                let result = self.sender.blocking_send(envelope).map_err(|_| {
                    crate::dead_letter::record::<M>(
                        identity,
                        crate::dead_letter::DeadLetterReason::ActorStopped,
                        "blocking_tell",
                    );
                    Error::Send {
                        identity,
                        details: "Mailbox channel closed",
                    }
                });

                match &result {
                    Ok(_) => debug!("Blocking tell message sent successfully"),
                    Err(e) => warn!(error = %e, "Failed to send blocking tell message"),
                }

                result
            }
        }
    }

    /// Synchronous version of [`ActorRef::ask`] that blocks until the reply is received.
    ///
    /// This method can be used from any thread, including non-async contexts,
    /// and from within `async fn` on a multi-thread Tokio runtime.
    ///
    /// # Execution paths
    ///
    /// - **Worker of a multi-thread Tokio runtime (including `async fn`)**: uses
    ///   [`tokio::task::block_in_place`] + [`Handle::block_on`](tokio::runtime::Handle::block_on)
    ///   to reuse the caller's runtime. No new thread or runtime is created.
    /// - **`timeout: Some(_)`, any other context that may block** (no
    ///   runtime, or a `spawn_blocking` thread of a `current_thread`
    ///   runtime): spawns a short-lived dedicated thread with a temporary
    ///   single-thread runtime to await the reply with the timeout. Unlike
    ///   the `tell` variants, no `try_send` fast path is possible because a
    ///   sync `recv_timeout` for the reply channel is unavailable. Contexts
    ///   that may *not* block panic instead — see [§ Panics](#panics).
    /// - **`timeout: None`, same blockable contexts**: uses Tokio's
    ///   `blocking_send` and `blocking_recv` directly; blocks indefinitely
    ///   until the reply arrives.
    ///
    /// # Performance Considerations
    ///
    /// The slow-path fallbacks (dedicated thread + temporary runtime) incur:
    /// - **Thread creation**: ~50-200μs depending on the platform
    /// - **Tokio runtime creation**: ~1-10μs for a single-threaded runtime
    ///
    /// All other execution paths above are sub-microsecond plus the actor's
    /// own reply latency.
    ///
    /// # Worker-pool caveat
    ///
    /// On a multi-thread runtime the `block_in_place` path holds the calling
    /// worker thread for the duration of the call. Avoid invoking from
    /// runtimes with very few workers; prefer [`ask`](Self::ask) directly
    /// from async code where possible.
    ///
    /// # Panics
    ///
    /// - When called from inside a [`LocalSet`](https://docs.rs/tokio/latest/tokio/task/struct.LocalSet.html)
    ///   running on a multi-thread runtime: the runtime handle reports
    ///   multi-thread flavor, but `block_in_place` is not permitted there.
    /// - When called from an async context that `block_in_place` cannot
    ///   rescue — a task on a `current_thread` runtime, or any thread driving
    ///   a `current_thread` runtime's `Runtime::block_on`/`Handle::block_on`
    ///   (a *multi-thread* runtime's `block_on` driver takes the
    ///   `block_in_place` path and is fine) — this panics with Tokio's
    ///   "Cannot block the current thread from within a runtime". Parking such
    ///   a thread would freeze the only thread able to drive the target actor,
    ///   turning the call into a silent deadlock of every actor on that
    ///   runtime that not even `kill()` could break; the loud panic surfaces
    ///   the bug instead. Call from a
    ///   [`spawn_blocking`](tokio::task::spawn_blocking) thread, or use the
    ///   async [`ask`](Self::ask), instead.
    /// - When the caller's own multi-thread runtime was built **without a time
    ///   driver** (no `enable_time()` / `enable_all()`): the fast path awaits
    ///   the admission timeout on that runtime, so `tokio::time::timeout`
    ///   panics because no timer is available. The slow-path temporary runtime
    ///   always enables timers, so only the caller's own runtime is affected.
    ///
    /// # Deadlock warning
    ///
    /// Never call this (or any `blocking_*` method) from inside the actor's own
    /// message handler — directly on `self`, or transitively via a cycle that
    /// asks back into this actor. The call parks the actor's message loop
    /// synchronously while waiting for a reply that only that same loop could
    /// produce — an unrecoverable hang that `kill()` cannot interrupt. Enable
    /// the `deadlock-detection` feature to turn such a cycle into an immediate
    /// panic instead, or use the async [`ask`](Self::ask) from handler code.
    #[cfg_attr(feature = "tracing", tracing::instrument(
        level = "debug",
        name = "actor_blocking_ask",
        fields(
            actor_id = %self.identity(),
            message_type = %std::any::type_name::<M>(),
            reply_type = %std::any::type_name::<T::Reply>(),
            timeout_ms = timeout.map(|t| t.as_millis())
        ),
        skip(self, msg)
    ))]
    pub fn blocking_ask<M>(&self, msg: M, timeout: Option<Duration>) -> Result<T::Reply>
    where
        T: Message<M>,
        M: Send + 'static,
        T::Reply: Send + 'static,
    {
        // Fast path: caller is inside a multi-thread runtime. Reuse it via
        // `block_in_place` + `Handle::block_on` instead of spawning a new
        // thread and runtime. Safe to call from async contexts on a
        // multi-thread runtime.
        if let Some(handle) = current_multi_thread_handle() {
            return block_on_current_runtime(handle, async {
                match timeout {
                    Some(t) => self.ask_timeout_inner(msg, t, Operation::BlockingAsk).await,
                    None => self.ask_inner(msg, Operation::BlockingAsk).await,
                }
            });
        }

        match timeout {
            Some(timeout_duration) => self.blocking_ask_with_timeout_impl(msg, timeout_duration),
            None => self.blocking_ask_no_timeout(msg),
        }
    }

    /// Internal implementation of blocking_ask without timeout.
    fn blocking_ask_no_timeout<M>(&self, msg: M) -> Result<T::Reply>
    where
        T: Message<M>,
        M: Send + 'static,
        T::Reply: Send + 'static,
    {
        debug!("Sending blocking ask message and waiting for reply");

        // `blocking_send`/`blocking_recv` carry tokio's own guard: in any
        // async context (a `current_thread` runtime worker, or a thread
        // driving `block_on`) they panic with "Cannot block the current
        // thread from within a runtime" instead of parking — and thereby
        // starving — the runtime that must drive the target actor. Do NOT
        // route around that panic with a dedicated thread: the park would
        // turn the loud failure into a silent, kill-proof deadlock of every
        // actor on the caller's runtime. The panic fires before the message
        // is admitted, so it has no side effects.
        let (reply_tx, reply_rx) = oneshot::channel();
        let envelope = MailboxMessage::Envelope {
            payload: Box::new(msg),
            reply_channel: Some(reply_tx),
            actor_ref: self.clone(), // Include the actor ref for context
            ask_edge: Default::default(),
        };

        self.sender.blocking_send(envelope).map_err(|_| {
            crate::dead_letter::record::<M>(
                self.identity(),
                crate::dead_letter::DeadLetterReason::ActorStopped,
                "blocking_ask",
            );

            warn!("Failed to send blocking ask message: mailbox channel closed");

            Error::Send {
                identity: self.identity(),
                details: "Mailbox channel closed",
            }
        })?;

        match reply_rx.blocking_recv() {
            Ok(reply_any) => {
                // Successfully received reply from actor
                match reply_any.downcast::<T::Reply>() {
                    Ok(reply) => {
                        debug!("Blocking ask reply received successfully");
                        Ok(*reply)
                    }
                    Err(_) => {
                        warn!(
                            expected_type = %std::any::type_name::<T::Reply>(),
                            "Blocking ask reply type downcast failed"
                        );
                        Err(Error::Downcast {
                            identity: self.identity(),
                            expected_type: std::any::type_name::<T::Reply>(),
                        })
                    }
                }
            }
            Err(_recv_err) => {
                crate::dead_letter::record::<M>(
                    self.identity(),
                    crate::dead_letter::DeadLetterReason::ReplyDropped,
                    "blocking_ask",
                );

                warn!("Blocking ask reply channel closed unexpectedly");
                Err(Error::Receive {
                    identity: self.identity(),
                    details: "Reply channel closed unexpectedly",
                })
            }
        }
    }

    /// Internal implementation of blocking_ask with timeout using a separate thread and runtime.
    fn blocking_ask_with_timeout_impl<M>(&self, msg: M, timeout: Duration) -> Result<T::Reply>
    where
        T: Message<M>,
        M: Send + 'static,
        T::Reply: Send + 'static,
    {
        let self_clone = self.clone();
        let identity = self.identity();

        run_blocking_with_runtime(
            identity,
            "Timeout thread terminated unexpectedly",
            async move {
                self_clone
                    .ask_timeout_inner(msg, timeout, Operation::BlockingAsk)
                    .await
            },
        )
    }

    /// Sends a message to an actor expecting a JoinHandle reply and awaits its completion.
    ///
    /// This method is specifically designed for the pattern where message handlers spawn
    /// long-running asynchronous tasks using `tokio::spawn` and return the `JoinHandle`.
    /// Instead of manually handling the `JoinHandle`, this method automatically awaits
    /// the spawned task and returns the final result.
    ///
    /// # Common Pattern
    ///
    /// This method supports the following actor pattern:
    ///
    /// ```rust,no_run
    /// use rsactor::{Actor, ActorRef, message_handlers};
    /// use tokio::task::JoinHandle;
    /// use std::time::Duration;
    ///
    /// #[derive(Actor)]
    /// struct WorkerActor;
    ///
    /// struct HeavyTask {
    ///     data: String,
    /// }
    ///
    /// #[message_handlers]
    /// impl WorkerActor {
    ///     #[handler]
    ///     async fn handle_heavy_task(
    ///         &mut self,
    ///         msg: HeavyTask,
    ///         _: &ActorRef<Self>
    ///     ) -> JoinHandle<String> {
    ///         let data = msg.data.clone();
    ///         // Spawn a long-running task to avoid blocking the actor
    ///         tokio::spawn(async move {
    ///             tokio::time::sleep(Duration::from_secs(5)).await;
    ///             format!("Processed: {}", data)
    ///         })
    ///     }
    /// }
    /// ```
    ///
    /// # Usage Examples
    ///
    /// See the `examples/ask_join_demo.rs` file for a complete working example that demonstrates:
    /// - Handlers that return `JoinHandle<T>` from spawned tasks
    /// - Using `ask_join` vs regular `ask` with manual await
    /// - Error handling for panicked and cancelled tasks
    /// - Concurrent task execution patterns
    ///
    /// # Error Handling
    ///
    /// This method can return errors in several scenarios:
    /// - Communication errors (actor not available, channels closed): [`Error::Send`], [`Error::Receive`]
    /// - Task execution errors (task panicked, cancelled): [`Error::Join`]
    /// - Type casting errors: [`Error::Downcast`]
    ///
    /// The [`Error::Join`] variant includes the original [`tokio::task::JoinError`]
    /// which provides detailed information about task failures, such as whether
    /// the task was cancelled or panicked.
    ///
    /// # Cancellation safety
    ///
    /// Dropping this future mid-flight (e.g. wrapping it in
    /// [`tokio::time::timeout`]) does **not** abort the task the handler
    /// spawned: the `JoinHandle` is merely dropped, which detaches the task —
    /// it runs to completion in the background and its result is discarded.
    /// If you need the spawned work to stop when the caller gives up, use a
    /// plain [`ask`](Self::ask) to receive the `JoinHandle` and call
    /// [`abort`](tokio::task::JoinHandle::abort) on it yourself.
    ///
    /// # Type Safety
    ///
    /// This method enforces compile-time type safety:
    /// - The message type `M` must be handled by actor `T`
    /// - The handler must return `tokio::task::JoinHandle<R>`
    /// - The final return type `R` is automatically inferred and type-checked
    #[cfg_attr(feature = "tracing", tracing::instrument(
        level = "debug",
        name = "actor_ask_join",
        fields(
            actor_id = %self.identity(),
            message_type = %std::any::type_name::<M>(),
            result_type = %std::any::type_name::<R>()
        ),
        skip(self, msg)
    ))]
    pub async fn ask_join<M, R>(&self, msg: M) -> crate::Result<R>
    where
        T: Message<M, Reply = tokio::task::JoinHandle<R>>,
        M: Send + 'static,
        R: Send + 'static,
    {
        tracing::debug!("Sending ask_join message and waiting for task completion");

        let join_handle = self.ask(msg).await?;

        tracing::debug!("Received JoinHandle, awaiting task completion");

        let result = join_handle.await.map_err(|join_error| crate::Error::Join {
            identity: self.identity(),
            source: std::sync::Arc::new(join_error),
        })?;

        tracing::debug!("Task completed successfully");

        Ok(result)
    }

    // ==================== Metrics API ====================

    /// Returns a snapshot of all metrics for this actor.
    ///
    /// The snapshot includes message count, processing times, priority message
    /// counts, uptime, and last activity timestamp.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let metrics = actor_ref.metrics();
    /// println!("Processed {} messages", metrics.message_count);
    /// println!("Avg time: {:?}", metrics.avg_processing_time);
    /// ```
    #[cfg(feature = "metrics")]
    pub fn metrics(&self) -> crate::MetricsSnapshot {
        self.metrics.snapshot()
    }

    /// Returns the number of **regular-mailbox** messages processed by this
    /// actor. Priority messages are counted separately by
    /// [`priority_message_count`](Self::priority_message_count); sum the two
    /// for an all-channel total.
    #[cfg(feature = "metrics")]
    #[inline]
    pub fn message_count(&self) -> u64 {
        self.metrics.message_count()
    }

    /// Returns the average message processing time.
    ///
    /// Returns `Duration::ZERO` if no messages have been processed yet.
    #[cfg(feature = "metrics")]
    #[inline]
    pub fn avg_processing_time(&self) -> std::time::Duration {
        self.metrics.avg_processing_time()
    }

    /// Returns the maximum message processing time observed.
    #[cfg(feature = "metrics")]
    #[inline]
    pub fn max_processing_time(&self) -> std::time::Duration {
        self.metrics.max_processing_time()
    }

    /// Returns the total number of priority messages processed by this actor.
    ///
    /// Always `0` for actors spawned without
    /// [`SpawnOptions::with_priority`](crate::SpawnOptions::with_priority).
    #[cfg(feature = "metrics")]
    #[inline]
    pub fn priority_message_count(&self) -> u64 {
        self.metrics.priority_message_count()
    }

    /// Returns the average priority message processing time.
    #[cfg(feature = "metrics")]
    #[inline]
    pub fn avg_priority_processing_time(&self) -> std::time::Duration {
        self.metrics.avg_priority_processing_time()
    }

    /// Returns the maximum priority message processing time observed.
    #[cfg(feature = "metrics")]
    #[inline]
    pub fn max_priority_processing_time(&self) -> std::time::Duration {
        self.metrics.max_priority_processing_time()
    }

    /// Returns the total number of idle events dispatched to
    /// [`Actor::on_idle`](crate::Actor::on_idle).
    ///
    /// Always `0` for actors spawned without
    /// [`SpawnOptions::with_idle`](crate::SpawnOptions::with_idle).
    #[cfg(feature = "metrics")]
    #[inline]
    pub fn idle_event_count(&self) -> u64 {
        self.metrics.idle_event_count()
    }

    /// Returns the average `on_idle` processing time per dispatched event.
    #[cfg(feature = "metrics")]
    #[inline]
    pub fn avg_idle_processing_time(&self) -> std::time::Duration {
        self.metrics.avg_idle_processing_time()
    }

    /// Returns the maximum `on_idle` processing time observed.
    #[cfg(feature = "metrics")]
    #[inline]
    pub fn max_idle_processing_time(&self) -> std::time::Duration {
        self.metrics.max_idle_processing_time()
    }

    /// Returns the time elapsed since the actor started.
    #[cfg(feature = "metrics")]
    #[inline]
    pub fn uptime(&self) -> std::time::Duration {
        self.metrics.uptime()
    }

    /// Returns the timestamp of the last completed unit of work — a message
    /// (regular or priority) or an idle event.
    ///
    /// Returns `None` if no work has completed yet.
    #[cfg(feature = "metrics")]
    #[inline]
    pub fn last_activity(&self) -> Option<std::time::SystemTime> {
        self.metrics.last_activity()
    }
}

impl<T: Actor> Clone for ActorRef<T> {
    #[inline]
    fn clone(&self) -> Self {
        ActorRef {
            id: self.id,
            sender: self.sender.clone(),
            priority_sender: self.priority_sender.clone(),
            terminate_sender: self.terminate_sender.clone(),
            idle_subscribe_sender: self.idle_subscribe_sender.clone(),
            #[cfg(feature = "metrics")]
            metrics: self.metrics.clone(),
        }
    }
}

/// Error returned by [`ActorRef::subscribe_idle`], handing the rejected
/// stream back to the caller.
///
/// Idle streams frequently wrap exclusive, non-reconstructible resources —
/// the only receiver of a data channel, a file/OS event watcher, a network
/// subscription. Returning ownership on failure means a retryable rejection
/// ([`Error::ChannelFull`]) can actually be retried *with the same stream*,
/// and a terminal one ([`Error::Send`], [`Error::IdleChannelNotEnabled`])
/// still lets the caller route the events elsewhere instead of losing the
/// source.
///
/// Converts into [`Error`] (dropping the stream) via `From`, so
/// `subscribe_idle(...)?` keeps working in functions returning
/// [`Result`](crate::Result) or `anyhow::Result`.
pub struct IdleSubscribeError<T: Actor> {
    /// Why the subscription was rejected.
    pub error: Error,
    /// The stream that was not installed, recoverable via
    /// [`take_stream`](Self::take_stream) / [`into_parts`](Self::into_parts).
    /// Behind a `Mutex` purely so this error is `Sync` (a `BoxStream` is only
    /// `Send`), which `?`-conversion into types like `anyhow::Error` requires.
    stream: std::sync::Mutex<Option<BoxStream<'static, T::IdleEvent>>>,
}

impl<T: Actor> IdleSubscribeError<T> {
    fn new(error: Error, stream: BoxStream<'static, T::IdleEvent>) -> Self {
        Self {
            error,
            stream: std::sync::Mutex::new(Some(stream)),
        }
    }

    /// Takes back ownership of the rejected stream for reuse (e.g. a retry,
    /// or routing the events elsewhere). Returns `None` only if it was
    /// already taken.
    pub fn take_stream(&self) -> Option<BoxStream<'static, T::IdleEvent>> {
        self.stream
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }

    /// Splits this into the underlying [`Error`] and the rejected stream
    /// (`None` only if [`take_stream`](Self::take_stream) was called first).
    pub fn into_parts(self) -> (Error, Option<BoxStream<'static, T::IdleEvent>>) {
        let stream = self
            .stream
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (self.error, stream)
    }
}

impl<T: Actor> fmt::Debug for IdleSubscribeError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let taken = self
            .stream
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_none();
        f.debug_struct("IdleSubscribeError")
            .field("error", &self.error)
            .field(
                "stream",
                &format_args!(
                    "{}BoxStream<{}>",
                    if taken { "taken " } else { "" },
                    std::any::type_name::<T::IdleEvent>()
                ),
            )
            .finish()
    }
}

impl<T: Actor> fmt::Display for IdleSubscribeError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)
    }
}

impl<T: Actor> std::error::Error for IdleSubscribeError<T> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // `Display` already delegates to `self.error`, so returning
        // `&self.error` here would print the same message twice in chain
        // reports (anyhow's "Caused by:"). Skip straight to the inner
        // error's own source; `self.error` stays reachable via the public
        // field.
        std::error::Error::source(&self.error)
    }
}

impl<T: Actor> From<IdleSubscribeError<T>> for Error {
    fn from(e: IdleSubscribeError<T>) -> Self {
        e.error
    }
}

/// A weak, type-safe reference to an actor of type `T`.
///
/// `ActorWeak<T>` is a weak reference that does not prevent the actor from being dropped
/// and can be upgraded to a strong [`ActorRef<T>`] if the actor is still alive.
/// Like [`ActorRef<T>`], it maintains compile-time type safety for the actor type `T`.
///
/// ## Creating `ActorWeak<T>`
///
/// `ActorWeak<T>` instances are created by calling [`downgrade`](ActorRef::downgrade) on an
/// existing [`ActorRef<T>`]:
///
/// ```ignore
/// let weak_ref = ActorRef::downgrade(&actor_ref);
/// ```
///
/// ## Upgrading to `ActorRef<T>`
///
/// An `ActorWeak<T>` can be upgraded to an `ActorRef<T>` using the [`upgrade`](ActorWeak::upgrade) method:
///
/// ```ignore
/// if let Some(strong_ref) = weak_ref.upgrade() {
///     // Successfully upgraded, actor is still alive
///     strong_ref.tell(MyMessage).await?;
/// } else {
///     // Actor is no longer alive
/// }
/// ```
pub struct ActorWeak<T: Actor> {
    /// The unique identifier for this actor instance
    id: Identity,
    /// Weak reference to the actor's mailbox sender
    sender: tokio::sync::mpsc::WeakSender<MailboxMessage<T>>,
    /// Weak reference to the actor's optional priority sender (None if priority disabled)
    priority_sender: Option<tokio::sync::mpsc::WeakSender<MailboxMessage<T>>>,
    /// Weak reference to the actor's terminate signal sender
    terminate_sender: tokio::sync::mpsc::WeakSender<ControlSignal>,
    /// Weak reference to the actor's optional idle-subscribe channel sender
    /// (None if the idle channel was not enabled via `SpawnOptions::with_idle()`).
    idle_subscribe_sender: Option<tokio::sync::mpsc::WeakSender<IdleEventStream<T>>>,
    /// Strong reference to metrics (survives actor drop for post-mortem analysis)
    #[cfg(feature = "metrics")]
    metrics: Arc<MetricsCollector>,
}

// Manual `Debug` for the same reason as [`ActorRef`]: avoid the derive's
// spurious `T: Debug` bound on an actor type that need not be `Debug`.
impl<T: Actor> fmt::Debug for ActorWeak<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("ActorWeak");
        s.field("id", &self.id)
            .field("sender", &self.sender)
            .field("priority_sender", &self.priority_sender)
            .field("terminate_sender", &self.terminate_sender)
            .field("idle_subscribe_sender", &self.idle_subscribe_sender);
        #[cfg(feature = "metrics")]
        s.field("metrics", &self.metrics);
        s.finish()
    }
}

impl<T: Actor> ActorWeak<T> {
    /// Attempts to upgrade the weak reference to a strong, type-safe reference.
    ///
    /// Returns `Some(ActorRef<T>)` if the actor is still alive, or `None` if the actor
    /// has been dropped.
    ///
    /// The priority and idle-subscribe channels are treated as *secondary* channels: if the
    /// original actor was spawned with one of them enabled but every strong sender for it has
    /// been dropped, `upgrade()` still succeeds. The resulting [`ActorRef`] reports the channel
    /// as absent ([`has_priority_channel()`](ActorRef::has_priority_channel) /
    /// [`has_idle_channel()`](ActorRef::has_idle_channel) returning `false`), and calls on it
    /// fail with [`Error::PriorityChannelNotEnabled`](crate::Error::PriorityChannelNotEnabled)
    /// / [`Error::IdleChannelNotEnabled`](crate::Error::IdleChannelNotEnabled).
    ///
    /// The mailbox and terminate channels are the *primary* channels and must both upgrade for
    /// `upgrade()` to succeed. In practice their strong-sender counts move in lockstep — every
    /// [`ActorRef`] clone owns one of each — so they live and die together; the joint check is
    /// defensive rather than restrictive.
    ///
    /// Besides the reference counts, this also fails once the actor's runtime
    /// loop has exited and closed its channel receivers (graceful stop, kill,
    /// or a lifecycle failure) — even while strong [`ActorRef`] clones still
    /// linger in registries elsewhere. A reference to a fully stopped actor
    /// can only fail every call, so handing one out would just defer the error
    /// and make `upgrade()` disagree with [`ActorRef::is_alive`].
    #[inline]
    pub fn upgrade(&self) -> Option<ActorRef<T>> {
        // Try to upgrade the two primary channel senders (mailbox, terminate).
        // The priority and idle-subscribe channels, when present, are best-effort.
        let sender = self.sender.upgrade()?;
        let terminate_sender = self.terminate_sender.upgrade()?;
        // Strong refs lingering after the runtime loop exited keep the
        // refcounts positive, but the closed receivers reveal the actor is
        // gone for good.
        if sender.is_closed() || terminate_sender.is_closed() {
            return None;
        }
        let priority_sender = self.priority_sender.as_ref().and_then(|s| s.upgrade());
        let idle_subscribe_sender = self
            .idle_subscribe_sender
            .as_ref()
            .and_then(|s| s.upgrade());

        Some(ActorRef {
            id: self.id,
            sender,
            priority_sender,
            terminate_sender,
            idle_subscribe_sender,
            #[cfg(feature = "metrics")]
            metrics: self.metrics.clone(),
        })
    }

    /// Returns the unique ID of the actor this weak reference points to.
    pub fn identity(&self) -> Identity {
        self.id
    }

    /// Checks if the actor might still be alive.
    ///
    /// Returns `false` if the actor is definitely dead: every strong reference
    /// has been dropped, or the actor's runtime loop has exited and closed its
    /// channels (graceful stop, kill, or a lifecycle failure) — the latter
    /// catches fully stopped actors whose strong [`ActorRef`] clones still
    /// linger in registries, matching [`ActorRef::is_alive`].
    ///
    /// **Note**: This is a heuristic check — the actor can stop between this
    /// call and the next operation. For definitive actor state, always use
    /// [`upgrade`](ActorWeak::upgrade) and check the returned `Option`.
    #[inline]
    pub fn is_alive(&self) -> bool {
        // Mirror the exact check `upgrade()` performs (primary senders
        // upgradable + receivers not closed) so the two calls cannot
        // disagree. The priority and idle-subscribe channels are secondary
        // and intentionally not consulted.
        match (self.sender.upgrade(), self.terminate_sender.upgrade()) {
            (Some(sender), Some(terminate_sender)) => {
                !sender.is_closed() && !terminate_sender.is_closed()
            }
            _ => false,
        }
    }

    /// Returns a snapshot of all metrics for this actor.
    ///
    /// `ActorWeak` holds the metrics by strong reference precisely so they
    /// survive the actor: this accessor keeps working after the actor has
    /// fully stopped (when [`upgrade`](Self::upgrade) already returns `None`),
    /// enabling post-mortem analysis from a retained weak handle.
    #[cfg(feature = "metrics")]
    pub fn metrics(&self) -> crate::MetricsSnapshot {
        self.metrics.snapshot()
    }
}

impl<T: Actor> Clone for ActorWeak<T> {
    #[inline]
    fn clone(&self) -> Self {
        ActorWeak {
            id: self.id,
            sender: self.sender.clone(),
            priority_sender: self.priority_sender.clone(),
            terminate_sender: self.terminate_sender.clone(),
            idle_subscribe_sender: self.idle_subscribe_sender.clone(),
            #[cfg(feature = "metrics")]
            metrics: self.metrics.clone(),
        }
    }
}
