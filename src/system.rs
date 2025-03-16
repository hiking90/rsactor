use std::ops::Deref;
use std::sync::Arc;

use anyhow::Result;
use tracing;
use tokio::task::JoinHandle;
use papaya::HashMap;

use crate::{
    actor::{self, IActor, Runtime},
    Errors, 
};

pub struct Inner {
    registry: HashMap<String, actor::Context>,
}

impl Inner {
    pub(crate) fn new() -> Self {
        Self {
            registry: HashMap::new(),
        }
    }

    pub(crate) fn register(&self, name: String, context: actor::Context) -> Result<(), Errors> {
        let map = self.registry.pin();
        match map.try_insert(name, context) {
            Ok(_) => Ok(()),
            Err(_) => Err(Errors::AlreadyRegistered),
        }
    }

    pub(crate) fn unregister(&self, name: &str) {
        let map = self.registry.pin();

        if let None = map.remove(name) {
            tracing::warn!("Attempted to remove non-existent actor: {}", name);
        }
    }
}

#[derive(Clone)]
pub struct System {
    inner: Arc<Inner>,
}

impl Deref for System {
    type Target = Inner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl System {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner::new()),
        }
    }

    /// Spawn an actor of this type, which is unsupervised, automatically starting
    ///
    /// * `name`: A name to give the actor. Useful for global referencing or debug printing
    /// * `handler` The implementation of Self
    /// * `startup_args`: Arguments passed to the `pre_start` call of the [Actor] to facilitate startup and
    ///   initial state creation
    ///
    /// Returns a [Ok((ActorCell, JoinHandle<()>))] upon successful start, denoting the actor reference
    /// along with the join handle which will complete when the actor terminates. Returns [Err(SpawnErr)] if
    /// the actor failed to start
    pub async fn spawn<T: IActor>(
        &self,
        name: Option<String>,
        handler: T,
    ) -> Result<(actor::Context, JoinHandle<()>), crate::Errors> {
        Runtime::<T>::spawn(self, name, handler).await
    }

    /// Spawn an actor of this type with a supervisor, automatically starting the actor
    ///
    /// * `name`: A name to give the actor. Useful for global referencing or debug printing
    /// * `handler` The implementation of Self
    /// * `startup_args`: Arguments passed to the `pre_start` call of the [Actor] to facilitate startup and
    /// initial state creation
    /// * `supervisor`: The [ActorCell] which is to become the supervisor (parent) of this actor
    ///
    /// Returns a [Ok((ActorCell, JoinHandle<()>))] upon successful start, denoting the actor reference
    /// along with the join handle which will complete when the actor terminates. Returns [Err(SpawnErr)] if
    /// the actor failed to start
    pub async fn spawn_linked<T: IActor>(
        &self,
        name: Option<String>,
        handler: T,
        supervisor: actor::Context,
    ) -> Result<(actor::Context, JoinHandle<()>), crate::Errors> {
        Runtime::<T>::spawn_linked(self, name, handler, supervisor).await
    }
}

