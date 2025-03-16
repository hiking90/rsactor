
use crate::{IMessage, IState};

pub mod id;
pub mod context;
mod properties;
mod messages;
mod runtime;
mod supervision;

pub(crate) use properties::*;
pub(crate) use messages::*;
pub use context::*;
pub use id::*;
pub(crate) use supervision::*;
pub use runtime::*;

pub(crate) fn get_panic_string(e: Box<dyn std::any::Any + Send>) -> String {
    match e.downcast::<String>() {
        Ok(v) => *v,
        Err(e) => match e.downcast::<&str>() {
            Ok(v) => v.to_string(),
            _ => "Unknown panic occurred which couldn't be coerced to a string"
                .to_string(),
        },
    }
}

pub trait IActor: Sized + Sync + Send + 'static {
    /// The message type for this actor
    type Msg: IMessage;

    /// The type of state this actor manages internally
    type State: IState;

    /// Invoked when an actor is being started by the system.
    ///
    /// Any initialization inherent to the actor's role should be
    /// performed here hence why it returns the initial state.
    ///
    /// Panics in `pre_start` do not invoke the
    /// supervision strategy and the actor won't be started. [Actor]::`spawn`
    /// will return an error to the caller
    ///
    /// * `myself` - A handle to the [ActorCell] representing this actor
    /// * `args` - Arguments that are passed in the spawning of the actor which might
    ///   be necessary to construct the initial state
    ///
    /// Returns an initial [Actor::State] to bootstrap the actor
    fn pre_start(
        &self,
        myself: Context,
    ) -> impl Future<Output = Result<Self::State, crate::Errors>> + Send;

    /// Invoked after an actor has started.
    ///
    /// Any post initialization can be performed here, such as writing
    /// to a log file, emitting metrics.
    ///
    /// Panics in `post_start` follow the supervision strategy.
    ///
    /// * `myself` - A handle to the [ActorCell] representing this actor
    /// * `state` - A mutable reference to the internal actor's state
    #[allow(unused_variables)]
    fn post_start(
        &self,
        myself: Context,
        state: &mut Self::State,
    ) -> impl Future<Output = Result<(), crate::Errors>> + Send {
        async { Ok(()) }
    }

    /// Invoked after an actor has been stopped to perform final cleanup. In the
    /// event the actor is terminated with [Signal::Kill] or has self-panicked,
    /// `post_stop` won't be called.
    ///
    /// Panics in `post_stop` follow the supervision strategy.
    ///
    /// * `myself` - A handle to the [ActorCell] representing this actor
    /// * `state` - A mutable reference to the internal actor's last known state
    #[allow(unused_variables)]
    fn post_stop(
        &self,
        myself: Context,
        state: &mut Self::State,
    ) -> impl Future<Output = Result<(), crate::Errors>> + Send {
        async { Ok(()) }
    }

    /// Handle the incoming message from the event processing loop. Unhandled panickes will be
    /// captured and sent to the supervisor(s)
    ///
    /// * `myself` - A handle to the [ActorCell] representing this actor
    /// * `message` - The message to process
    /// * `state` - A mutable reference to the internal actor's state
    #[allow(unused_variables)]
    fn handle(
        &self,
        myself: Context,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> impl Future<Output = Result<(), crate::Errors>> + Send {
        async { Ok(()) }
    }

    /// Handle the incoming supervision event. Unhandled panics will be captured and
    /// sent the the supervisor(s). The default supervision behavior is to exit the
    /// supervisor on any child exit. To override this behavior, implement this function.
    ///
    /// * `myself` - A handle to the [ActorCell] representing this actor
    /// * `message` - The message to process
    /// * `state` - A mutable reference to the internal actor's state
    #[allow(unused_variables)]
    fn handle_supervisor_evt(
        &self,
        myself: Context,
        message: SupervisionEvent,
        state: &mut Self::State,
    ) -> impl Future<Output = Result<(), crate::Errors>> + Send {
        async move {
            match message {
                SupervisionEvent::Terminated(who, _, _)
                | SupervisionEvent::Failed(who, _) => {
                    myself.stop(None);
                }
                _ => {}
            }
            Ok(())
        }
    }
}
