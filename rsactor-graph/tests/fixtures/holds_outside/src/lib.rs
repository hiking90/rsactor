// Fixture: an actor outside the workspace makes handler candidates
// incomplete (§4.3). Not compiled.
use rsactor::{message_handlers, Actor, ActorRef, TellHandler};

pub struct Ping;

#[derive(Actor)]
pub struct Local {
    remote: ActorRef<remote_crate::Remote>,
    pinger: Box<dyn TellHandler<Ping>>,
}

#[message_handlers]
impl Local {
    #[handler]
    async fn ping(&mut self, _m: Ping, _: &ActorRef<Self>) {}
}
