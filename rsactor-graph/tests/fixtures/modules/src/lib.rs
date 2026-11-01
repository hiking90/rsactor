// Fixture: module tree and name resolution. Not compiled.
use rsactor::{message_handlers, Actor, ActorRef};

mod actors;
#[path = "custom/extra_impl.rs"]
mod extra;
mod msgs;

pub mod inline {
    use super::msgs::Ping;
    use rsactor::{message_handlers, Actor, ActorRef};

    #[derive(Actor)]
    pub struct InlineActor;

    #[message_handlers]
    impl InlineActor {
        #[handler]
        async fn on_ping(&mut self, _m: Ping, _: &ActorRef<Self>) {}
    }
}

#[cfg(test)]
mod tests {
    use rsactor::Actor;

    #[derive(Actor)]
    struct TestOnlyActor;
}

#[cfg(all(test, feature = "slow"))]
#[derive(Actor)]
struct AlsoTestOnly;
