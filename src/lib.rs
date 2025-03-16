use tokio::time;
use anyhow::Result;

pub mod message;
pub mod actor;
pub mod errors;
pub mod system;

pub use message::*;
pub use errors::*;
pub use system::*;
pub use actor::IActor;

/// Represents the state of an actor. Must be safe
/// to send between threads (same bounds as a [Message])
pub trait IState: std::any::Any + Send + 'static {}
impl<T: std::any::Any + Send + 'static> IState for T {}

/// Execute the future up to a timeout
///
/// * `dur`: The duration of time to allow the future to execute for
/// * `future`: The future to execute
///
/// Returns [Ok(_)] if the future succeeded before the timeout, [Err(super::Timeout)] otherwise
pub async fn timeout<F, T>(dur: time::Duration, future: F) -> Result<T, Errors>
where
    F: Future<Output = T>,
{
    tokio::time::timeout(dur, future)
        .await
        .map_err(|_| Errors::Timeout)
}
