// Fixture: sender determination and spawn forms (§4.4). Not compiled.
use rsactor::{spawn, spawn_with_options, Actor, ActorRef, SpawnOptions};

#[derive(Actor)]
pub struct Child;

#[derive(Actor)]
pub struct Derived;

pub struct Parent;

impl Actor for Parent {
    type Args = ();
    type Error = ();
    type IdleEvent = ();

    // `Actor` trait method without `self`: sends as Parent.
    async fn on_start(_a: (), _r: &ActorRef<Self>) -> Result<Self, ()> {
        let _ = spawn::<Child>(Child);
        Ok(Parent)
    }
}

impl Parent {
    // `self` method: Parent.
    fn with_self(&self) {
        let _ = rsactor::spawn_with_options::<Child>(Child, SpawnOptions::default());
    }

    // No `self`: a function node, not Parent (no false self-loop).
    pub fn assoc() {
        let _ = spawn::<Child>(Child);
    }

    // Closure and async block inside a `self` method: still Parent.
    // `tokio::spawn` itself is not an rsactor spawn.
    fn later(&self) {
        tokio::spawn(async move {
            let _ = spawn(Derived);
        });
    }

    // A nested `fn` is its own sender.
    fn outer(&self) {
        fn inner() {
            let _ = spawn::<Child>(Child);
        }
        inner();
    }
}

pub trait Boot {
    // Trait default method: a function node named after the trait method.
    fn boot(&self) {
        let _ = spawn::<Child>(Child);
    }
}

// Method of a type that is not an actor: a function node.
pub struct Helper;

impl Helper {
    pub fn go(&self) {
        let _ = spawn::<Child>(Child);
    }
}

pub fn free() {
    // Annotation decides the target.
    let (_r, _h): (ActorRef<Child>, _) = spawn(make_child_args());
    // Nothing decides it: reported.
    let args = make_child_args();
    let _ = spawn(args);
}

fn make_child_args() -> Child {
    Child
}

// `Ambig` derives `Actor` (so `Args = Ambig`) and is also `Args` of `Wrapper`:
// `spawn(Ambig { .. })` cannot be decided.
#[derive(Actor)]
pub struct Ambig {}

pub struct Wrapper;

impl Actor for Wrapper {
    type Args = Ambig;
    type Error = ();
    type IdleEvent = ();
    async fn on_start(_a: Ambig, _r: &ActorRef<Self>) -> Result<Self, ()> {
        Ok(Wrapper)
    }
}

pub fn ambiguous() {
    let _ = spawn(Ambig {});
}

// A local function named `spawn` is not rsactor's.
mod local {
    fn spawn() {}

    pub fn calls_local() {
        spawn();
    }
}

pub fn generic<A: Actor<Args = ()>>() {
    let _ = spawn::<A>(());
}
