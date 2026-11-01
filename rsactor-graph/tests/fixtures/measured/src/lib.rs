// Fixture: rules added after measuring the tool on real applications. Each
// case mirrors a shape found there. Not compiled.
use rsactor::{message_handlers, Actor, ActorRef};

pub mod msgs {
    pub struct Ping;
    pub struct Status;
}

/// A second `Ping`: without the block-level `use`, the last-segment fallback
/// has two candidates and gives up.
pub mod other {
    pub struct Ping;
}

pub struct Report {
    ok: bool,
}

impl Report {
    pub fn success() -> Self {
        Report { ok: true }
    }
    pub fn failure(_code: u32) -> Report {
        Report { ok: false }
    }
    pub fn with_code(self, _code: u32) -> Self {
        self
    }
}

#[derive(Actor)]
pub struct Target;

#[message_handlers]
impl Target {
    #[handler]
    async fn ping(&mut self, _m: msgs::Ping, _: &ActorRef<Self>) {}
    #[handler]
    async fn other_ping(&mut self, _m: other::Ping, _: &ActorRef<Self>) {}
    #[handler]
    async fn status(&mut self, _m: msgs::Status, _: &ActorRef<Self>) {}
    #[handler]
    async fn report(&mut self, _m: Report, _: &ActorRef<Self>) {}
}

pub struct Wrapper<T> {
    inner: T,
}

impl<T> Wrapper<T> {
    pub fn get(&self) -> &T {
        &self.inner
    }
}

pub fn make_target() -> ActorRef<Target> {
    todo!()
}

/// A crate-level `Result` alias: expanded to its target.
pub type Res<T> = std::result::Result<T, ()>;

pub fn lookup() -> Res<ActorRef<Target>> {
    todo!()
}

pub struct Holder {
    target: Option<ActorRef<Target>>,
}

impl Holder {
    pub fn target(&self) -> ActorRef<Target> {
        todo!()
    }

    pub async fn connect(&self) -> Result<ActorRef<Target>, ()> {
        todo!()
    }

    // `use` inside the function body.
    async fn block_use(&self, t: ActorRef<Target>) {
        use crate::msgs::Ping;
        t.tell(Ping).await.ok();
    }

    // An external glob inside the body blocks the fallback, which would
    // otherwise pick `msgs::Status`.
    async fn block_glob(&self, t: ActorRef<Target>) {
        use ext_crate::*;
        t.tell(Status).await.ok();
    }

    // Tuple scrutinees.
    async fn tuple(&self, a: Option<u32>) {
        match (a, self.target.clone()) {
            (Some(_), Some(t)) => {
                t.tell(msgs::Ping).await.ok();
            }
            _ => {}
        }
        if let (Some(_), Some(t)) = (a, self.target.as_ref()) {
            t.tell(msgs::Ping).await.ok();
        }
    }

    // `match`/`if` initializers. Diverging arms do not count; disagreeing
    // ones give `?`.
    async fn branches(&self, flag: bool) {
        let t = match self.target.clone() {
            Some(t) => t,
            None => return,
        };
        t.tell(msgs::Ping).await.ok();
        let report = if flag {
            Report::success()
        } else {
            Report::failure(1)
        };
        t.tell(report).await.ok();
        let mixed = if flag { Report::success() } else { msgs::Ping };
        t.tell(mixed).await.ok();
    }

    // Return types stated by declared signatures.
    async fn signatures(&self) {
        self.target().tell(Report::success().with_code(3)).await.ok();
        make_target().tell(msgs::Ping).await.ok();
        let t = self.connect().await.unwrap();
        t.tell(msgs::Ping).await.ok();
        let w: Wrapper<ActorRef<Target>> = todo!();
        w.get().tell(msgs::Ping).await.ok();
    }

    fn make_ref() -> ActorRef<Target> {
        todo!()
    }

    // `Self::f(..)` and a `Result` alias matched with `Ok(..)`.
    async fn self_and_alias(&self) {
        Self::make_ref().tell(msgs::Ping).await.ok();
        match lookup() {
            Ok(t) => {
                t.tell(msgs::Ping).await.ok();
            }
            Err(_) => {}
        }
    }

    // Expression macros, including `matches!`, are walked as code.
    async fn macros(&self, t: ActorRef<Target>) {
        assert!(matches!(t.ask(msgs::Ping).await, Ok(_)));
        println!("{:?}", t.ask(msgs::Ping).await);
        let _v = vec![t.tell(msgs::Ping)];
    }
}

// An associated function named `spawn` is not rsactor's.
pub struct Writer;

impl Writer {
    pub fn spawn(_a: u32) -> Self {
        Writer
    }
    pub fn open() -> Self {
        Writer::spawn(1)
    }
    pub fn again() -> Self {
        Self::spawn(2)
    }
}
