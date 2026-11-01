// Fixture: the last-segment fallback of name resolution. Not compiled.
mod msgs {
    pub struct Shutdown;
    pub struct Lonely;
}

// A glob from a crate outside the declaration scope may have brought in its
// own `Shutdown`, so the fallback must not pick `msgs::Shutdown`.
mod with_external_glob {
    use ext_crate::*;
    use rsactor::{message_handlers, Actor, ActorRef};

    #[derive(Actor)]
    pub struct A;

    #[message_handlers]
    impl A {
        #[handler]
        async fn h(&mut self, _m: Shutdown, _: &ActorRef<Self>) {}
    }
}

// Same, but the external glob is reached through another module's glob.
mod prelude_chain {
    pub use other_ext::*;
}

mod with_chained_glob {
    use crate::prelude_chain::*;
    use rsactor::{message_handlers, Actor, ActorRef};

    #[derive(Actor)]
    pub struct B;

    #[message_handlers]
    impl B {
        #[handler]
        async fn h(&mut self, _m: Shutdown, _: &ActorRef<Self>) {}
    }
}

// No glob in scope and exactly one candidate: the fallback applies and is
// marked `inferred`.
mod without_glob {
    use rsactor::{message_handlers, Actor, ActorRef};

    #[derive(Actor)]
    pub struct C;

    #[message_handlers]
    impl C {
        #[handler]
        async fn h(&mut self, _m: Lonely, _: &ActorRef<Self>) {}
    }
}
