// Fixture: the type environment and send classification (§4.4). Not compiled.
use rsactor::ActorRef as Ref;
use rsactor::{message_handlers, Actor, ActorRef, ActorWeak, AskHandler, TellHandler};
use std::collections::HashMap;

pub enum Command {
    Start,
    Run(u32),
    Stop { now: bool },
}
pub struct Job;
pub struct Query;
pub struct Made;
pub struct Unhandled;
pub struct LogLine;
pub struct Reply {
    pub reply_to: ActorRef<Router>,
}
pub struct Req<A: Actor> {
    pub reply_to: ActorRef<A>,
}

impl Made {
    pub fn new() -> Self {
        Made
    }
}

#[derive(Actor)]
pub struct Router;

#[message_handlers]
impl Router {
    #[handler]
    async fn command(&mut self, _m: Command, _: &ActorRef<Self>) {}
    #[handler]
    async fn job(&mut self, _m: Job, _: &ActorRef<Self>) {}
    #[handler]
    async fn made(&mut self, _m: Made, _: &ActorRef<Self>) {}
    #[handler]
    async fn query(&mut self, _m: Query, _: &ActorRef<Self>) -> u32 {
        0
    }
    #[handler]
    async fn reply(&mut self, _m: Reply, _: &ActorRef<Self>) {}
}

/// Holds a reference but is not an actor: field access still resolves (R1).
pub struct Client {
    pub inner: ActorRef<Router>,
}

/// A newtype wrapper: the scanner does not follow it, so it is reported.
pub struct RouterHandle(ActorRef<Router>);
pub type RouterRef = ActorRef<Router>;

/// Has its own `tell` and holds no actor: calls on it are dropped.
pub struct Batcher;
impl Batcher {
    pub fn tell(&self, _m: Job) {}
}

pub trait Notifier {
    fn tell(&self, line: LogLine);
}

pub struct Worker {
    router: ActorRef<Router>,
    weak: ActorWeak<Router>,
    routers: HashMap<u32, ActorRef<Router>>,
    list: Vec<ActorRef<Router>>,
    renamed: Ref<Router>,
    client: Client,
    handle: RouterHandle,
    alias: RouterRef,
    outbox: Batcher,
    asker: Box<dyn AskHandler<Query, u32>>,
}

impl Actor for Worker {
    type Args = (u32, ActorWeak<Router>);
    type Error = ();
    type IdleEvent = ();

    async fn on_start(args: Self::Args, _r: &ActorRef<Self>) -> Result<Self, ()> {
        // Tuple destructuring of `Self::Args`, then `if let Some(r) = upgrade()`.
        let (_id, weak) = args;
        if let Some(r) = weak.upgrade() {
            r.tell(Command::Start).await.ok();
        }
        todo!()
    }
}

impl Worker {
    async fn messages(&self) {
        // Enum variants of every shape → the enum.
        self.router.tell(Command::Run(1)).await.ok();
        self.router.tell(Command::Stop { now: true }).await.ok();
        // Associated function of a declared type → assumed.
        self.router.tell(Made::new()).await.ok();
        // `Box::new(..)` is not assumed to be `Box` → message `?`.
        self.router.tell(Box::new(Job)).await.ok();
        // Local binding initialised by a struct literal (M2).
        let job = Job;
        self.router.ask(job).await.ok();
        self.router.tell_priority(Job).await.ok();
    }

    async fn bindings(&self) {
        // Initializer typed through clone; used inside an async block.
        let r = self.router.clone();
        tokio::spawn(async move {
            r.tell(Job).await.ok();
        });
        // `let .. else` and `match`.
        let Some(up) = self.weak.upgrade() else { return };
        up.tell(Job).await.ok();
        match self.weak.upgrade() {
            Some(m) => m.tell(Job).await.ok(),
            None => None,
        };
        // UFCS downgrade, then upgrade.
        let w = ActorRef::downgrade(&self.router);
        w.upgrade().unwrap().tell(Job).await.ok();
    }

    async fn containers(&self) {
        for r in self.routers.values() {
            r.tell(Job).await.ok();
        }
        self.routers.get(&1).unwrap().tell(Job).await.ok();
        if let Some(r) = self.routers.get_mut(&2) {
            r.tell(Job).await.ok();
        }
        for (_k, v) in &self.routers {
            v.tell(Job).await.ok();
        }
        self.list.first().unwrap().tell(Job).await.ok();
        self.list.last().unwrap().tell(Job).await.ok();
        for x in &self.list {
            x.tell(Job).await.ok();
        }
        for (_i, x) in self.list.iter().enumerate() {
            x.tell(Job).await.ok();
        }
        self.list.iter().for_each(|x| {
            let _ = x.tell(Job);
        });
    }

    async fn fields(&self, msg: Reply, req: Req<Router>) {
        msg.reply_to.tell(Job).await.ok();
        self.client.inner.tell(Job).await.ok();
        req.reply_to.tell(Job).await.ok();
        self.renamed.tell(Job).await.ok();
    }

    async fn shadowing(&self, stream: impl Iterator<Item = u32>) {
        // The closure parameter `w` has no type and shadows the outer `w`:
        // it must not be read as `Router`.
        let w = self.router.clone();
        stream.then(|w| w.tell(Job));
        let _ = w;
        // Re-`let` in the same scope: the new binding wins.
        let s = self.router.clone();
        let s = Batcher;
        s.tell(Job);
    }

    async fn reported(&self, notifier: &impl Notifier) {
        self.handle.tell(Job).await.ok();
        self.alias.tell(Job).await.ok();
        self.outbox.tell(Job);
        // Not handled by any actor and the receiver is unknown: unclassified,
        // not a `?` edge.
        notifier.tell(LogLine);
        ActorRef::tell(&self.router, Job).await.ok();
        tokio::select! {
            _ = async {} => { self.router.tell(Job).await.ok(); }
        }
        println!("{:?}", self.router.ask(Query).await);
        // Router does not handle `Unhandled`: cross-check warning.
        self.router.tell(Unhandled).await.ok();
        self.asker.ask(Query).await.ok();
    }
}

pub async fn generic_send<R, M>(r: &R, msg: M) {
    r.tell(msg).await;
}

pub async fn unknown_receiver(get: impl Fn() -> u32) {
    // Receiver unknown, message handled by Router: a `?` edge.
    get().tell(Job).await;
}

pub async fn handlers(list: Vec<Box<dyn TellHandler<Job>>>) {
    for h in &list {
        h.tell(Job).await.ok();
    }
}
