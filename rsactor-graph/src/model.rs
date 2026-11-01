// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! The graph model shared by every output format (§6.1).
//!
//! Every collection is sorted in [`Output::new`]: golden snapshots compare
//! bytes, so nothing may depend on `HashMap` iteration order.

use serde::Serialize;

/// Bumped whenever the JSON shape changes incompatibly, so the later runtime
/// overlay and external tools can detect a mismatch.
pub const SCHEMA_VERSION: u32 = 1;

/// One scan result.
#[derive(Debug, Serialize)]
pub struct Output {
    pub schema_version: u32,
    pub name: String,
    pub graph: Graph,
    pub report: Report,
}

impl Output {
    pub fn new(name: String, mut graph: Graph, mut report: Report) -> Self {
        graph.nodes.sort();
        graph.nodes.dedup();
        graph.edges.sort();
        graph.edges.dedup();
        report.entries.sort();
        report.entries.dedup();
        Output {
            schema_version: SCHEMA_VERSION,
            name,
            graph,
            report,
        }
    }

    /// Actor nodes, in output order.
    pub fn actors(&self) -> impl Iterator<Item = &ActorNode> {
        self.graph.nodes.iter().filter_map(|node| match node {
            Node::Actor(actor) => Some(actor),
            _ => None,
        })
    }

    pub fn node(&self, id: &str) -> Option<&Node> {
        self.graph.nodes.iter().find(|n| n.id() == id)
    }
}

#[derive(Debug, Default, Serialize)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Node {
    Actor(ActorNode),
    /// An actor declared in a crate outside the declaration scope (§3). Only
    /// its path is known.
    ExternalActor(ExternalActorNode),
    /// A sender that is not an actor: free function, associated function
    /// without `self`, method of a non-actor type, trait default method,
    /// nested `fn` (§4.4).
    Fn(FnNode),
    /// `main`.
    External(ExternalNode),
    /// "Every actor that handles M" for a type-erased handler (§4.3).
    Handlers(HandlersNode),
    /// Receiver that could not be determined. One per sender, so unresolved
    /// sends do not pile onto a single hub node (§6.1).
    Unknown(UnknownNode),
}

impl Node {
    pub fn id(&self) -> &str {
        match self {
            Node::Actor(n) => &n.id,
            Node::ExternalActor(n) => &n.id,
            Node::Fn(n) => &n.id,
            Node::External(n) => &n.id,
            Node::Handlers(n) => &n.id,
            Node::Unknown(n) => &n.id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct UnknownNode {
    pub id: String,
    /// Id of the sender whose sends end here.
    pub sender: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct ExternalActorNode {
    pub id: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct FnNode {
    pub id: String,
    pub target: String,
    /// e.g. `app::util::broadcast`, `app::Router::connect`.
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct ExternalNode {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct HandlersNode {
    pub id: String,
    /// `TellHandler` or `AskHandler` (weak variants map to these).
    pub handler: String,
    /// Message key and label.
    pub message: String,
    pub message_label: String,
    /// Reply key and label, for `AskHandler<M, R>`.
    pub reply: Option<String>,
    pub reply_label: Option<String>,
    /// Ids of the actors in the declaration scope that qualify.
    pub candidates: Vec<String>,
    /// An `open` or external actor exists, so the real receiver may be
    /// outside `candidates`.
    pub may_be_outside: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct ActorNode {
    /// Node id, `actor:<target>:<path>`.
    pub id: String,
    /// Target id of the crate that declares the actor (§6.1 node key).
    pub target: String,
    /// Full path without generic arguments, e.g. `app::router::Router`.
    pub path: String,
    /// Full path with the declared generic parameters, e.g. `app::Worker<T>`.
    pub display: String,
    /// The handled-message set cannot be enumerated (§4.2).
    pub open: bool,
    pub messages: Vec<HandledMessage>,
    pub location: Location,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct HandledMessage {
    /// Comparison key (§4.2 "메시지 key").
    pub key: String,
    /// Short form for display.
    pub label: String,
    /// Reply type label; `None` for `()`.
    pub reply: Option<String>,
    /// Reply type key, compared against `AskHandler<M, R>` (§4.3).
    pub reply_key: Option<String>,
    pub resolution: Resolution,
    /// `cfg` predicates copied from the handler, not evaluated.
    pub cfg: Vec<String>,
    pub location: Location,
}

/// One edge. `from`/`to` are node ids. Edges that differ only in location
/// are merged, with every location kept (§6.1).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Edge {
    pub kind: EdgeKind,
    /// The rsactor function or method when it is not the plain one, e.g.
    /// `spawn_with_options`, `tell_priority`.
    pub variant: Option<String>,
    pub from: String,
    pub to: String,
    /// Message key (§4.2); `None` for holds and spawn edges and for a
    /// message that could not be determined.
    pub message: Option<String>,
    /// Short form of the message for display.
    pub message_label: Option<String>,
    /// Reply type label, from the receiver's handler.
    pub reply: Option<String>,
    pub receiver_resolution: Resolution,
    pub message_resolution: Resolution,
    pub locations: Vec<Location>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Holds,
    Spawn,
    Tell,
    Ask,
}

/// How much guessing went into a judgement, strongest first (§6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    Exact,
    /// M1 assumed that an associated function returns `Self`.
    Assumed,
    /// §4.5 picked the only declared type with a matching last segment.
    Inferred,
    Unresolved,
}

impl Resolution {
    /// The weaker of two judgements.
    pub fn weakest(self, other: Resolution) -> Resolution {
        self.max(other)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Location {
    /// Path relative to the scan's base directory, `/`-separated.
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub entries: Vec<ReportEntry>,
}

impl Report {
    pub fn push(&mut self, category: Category, location: Option<Location>, detail: String) {
        self.entries.push(ReportEntry {
            category,
            location,
            sender: None,
            detail,
        });
    }

    pub fn push_from(
        &mut self,
        category: Category,
        location: Location,
        sender: &str,
        detail: String,
    ) {
        self.entries.push(ReportEntry {
            category,
            location: Some(location),
            sender: Some(sender.to_string()),
            detail,
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct ReportEntry {
    pub category: Category,
    pub location: Option<Location>,
    pub sender: Option<String>,
    pub detail: String,
}

/// Report categories (§6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// A declaration the scanner could not read: unparsable file, missing
    /// module file, handler with the wrong arity, unresolvable `impl` target.
    Declaration,
    UnresolvedEdge,
    Unclassified,
    UnsupportedReceiver,
    Ufcs,
    MacroCandidate,
    UnresolvedSpawn,
    CrossCheck,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Declaration => "declaration",
            Category::UnresolvedEdge => "unresolved-edge",
            Category::Unclassified => "unclassified",
            Category::UnsupportedReceiver => "unsupported-receiver",
            Category::Ufcs => "ufcs",
            Category::MacroCandidate => "macro-candidate",
            Category::UnresolvedSpawn => "unresolved-spawn",
            Category::CrossCheck => "cross-check",
        }
    }
}
