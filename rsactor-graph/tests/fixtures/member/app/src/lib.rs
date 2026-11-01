// Fixture: a target crate that uses an actor from a workspace member (§3).
// Not compiled.
use core_actors::Start;
use rsactor::{message_handlers, Actor, ActorRef};

#[derive(Actor)]
pub struct Frontend;

#[message_handlers]
impl Frontend {
    #[handler]
    async fn start(&mut self, _m: Start, _: &ActorRef<Self>) {}
}
