// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! JSON, DOT and SVG output.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::io::Write as _;
use std::process::{Command, Stdio};

use crate::model::{ActorNode, Edge, EdgeKind, FnNode, HandlersNode, Node, Output, Resolution};

pub fn to_json(output: &Output) -> String {
    let mut text = serde_json::to_string_pretty(output).expect("model serialises");
    text.push('\n');
    text
}

/// Which holds edges DOT output draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldsMode {
    All,
    /// Only between nodes with no send or spawn arrow already: a holds
    /// edge next to a send arrow repeats what the send shows.
    Uncovered,
    None,
}

/// Readability options for DOT/SVG. The JSON model is never reduced by
/// these; it stays the full judgement record.
#[derive(Debug, Clone, Copy)]
pub struct DotOptions {
    /// Draw functions sharing an owner type or trait as one node.
    pub group_fns: bool,
    pub holds: HoldsMode,
}

impl Default for DotOptions {
    fn default() -> Self {
        DotOptions {
            group_fns: true,
            holds: HoldsMode::Uncovered,
        }
    }
}

pub fn to_dot(output: &Output) -> String {
    to_dot_with(output, &DotOptions::default())
}

/// Arrow classes drawn separately between the same two nodes. All sends
/// (tell and ask, every variant) share one arrow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ArrowClass {
    Holds,
    Spawn,
    Send,
}

fn arrow_class(kind: EdgeKind) -> ArrowClass {
    match kind {
        EdgeKind::Holds => ArrowClass::Holds,
        EdgeKind::Spawn => ArrowClass::Spawn,
        EdgeKind::Tell | EdgeKind::Ask => ArrowClass::Send,
    }
}

pub fn to_dot_with(output: &Output, options: &DotOptions) -> String {
    // Function nodes that share (target, owner) with at least one other
    // function are drawn as one group node.
    let mut owners: BTreeMap<(String, String), Vec<&FnNode>> = BTreeMap::new();
    if options.group_fns {
        for node in &output.graph.nodes {
            if let Node::Fn(f) = node {
                if let Some(owner) = &f.owner {
                    owners
                        .entry((f.target.clone(), owner.clone()))
                        .or_default()
                        .push(f);
                }
            }
        }
        owners.retain(|_, members| members.len() > 1);
    }
    let mut remap: HashMap<String, String> = HashMap::new();
    for ((target, owner), members) in &owners {
        let group = format!("fngroup:{target}:{owner}");
        for f in members {
            remap.insert(f.id.clone(), group.clone());
        }
    }
    // A sender's `?` node follows its sender into the group.
    for node in &output.graph.nodes {
        if let Node::Unknown(u) = node {
            if let Some(group) = remap.get(&u.sender) {
                remap.insert(u.id.clone(), format!("unknown:{group}"));
            }
        }
    }
    let id_of = |id: &str| remap.get(id).cloned().unwrap_or_else(|| id.to_string());

    let mut dot = String::new();
    let _ = writeln!(dot, "digraph {} {{", quote_id(&output.name));
    dot.push_str("  rankdir=LR;\n");
    dot.push_str("  node [shape=record, fontname=\"Helvetica\", fontsize=10];\n");
    dot.push_str("  edge [fontname=\"Helvetica\", fontsize=9];\n");
    let mut emitted: HashSet<String> = HashSet::new();
    for node in &output.graph.nodes {
        let id = id_of(node.id());
        if !emitted.insert(id.clone()) {
            continue;
        }
        let attrs = match node {
            Node::Actor(actor) => format!("label=\"{}\"", actor_label(actor)),
            Node::ExternalActor(n) => format!(
                "shape=box, style=dashed, label=\"{}\\n(outside the workspace)\"",
                escape_text(&n.path)
            ),
            Node::Fn(f) if id != f.id => {
                let owner = f.owner.clone().unwrap_or_default();
                let members = &owners[&(f.target.clone(), owner.clone())];
                format!(
                    "shape=box, style=rounded, label=\"{}\"",
                    escape_text(&group_label(&owner, members))
                )
            }
            Node::Fn(f) => format!(
                "shape=box, style=rounded, label=\"fn {}\"",
                escape_text(&f.path)
            ),
            Node::External(_) => "shape=ellipse, style=dashed, label=\"external (main)\"".into(),
            Node::Handlers(h) => format!(
                "shape=box, style=\"rounded,dashed\", label=\"{}\"",
                handlers_label(output, h)
            ),
            Node::Unknown(_) => "shape=circle, color=red, fontcolor=red, label=\"?\"".to_string(),
        };
        let _ = writeln!(dot, "  {} [{attrs}];", quote_id(&id));
    }

    let mut arrows: BTreeMap<(String, String, ArrowClass), Vec<&Edge>> = BTreeMap::new();
    for edge in &output.graph.edges {
        let key = (id_of(&edge.from), id_of(&edge.to), arrow_class(edge.kind));
        arrows.entry(key).or_default().push(edge);
    }
    let covered: HashSet<(String, String)> = arrows
        .keys()
        .filter(|(_, _, class)| *class != ArrowClass::Holds)
        .map(|(from, to, _)| (from.clone(), to.clone()))
        .collect();
    for ((from, to, class), edges) in &arrows {
        if *class == ArrowClass::Holds {
            let skip = match options.holds {
                HoldsMode::All => false,
                HoldsMode::None => true,
                HoldsMode::Uncovered => covered.contains(&(from.clone(), to.clone())),
            };
            if skip {
                continue;
            }
        }
        let _ = writeln!(
            dot,
            "  {} -> {} [{}];",
            quote_id(from),
            quote_id(to),
            arrow_attrs(*class, edges)
        );
    }
    dot.push_str("}\n");
    dot
}

/// `owner` plus the member function names, at most six of them.
fn group_label(owner: &str, members: &[&FnNode]) -> String {
    let mut names: Vec<&str> = members
        .iter()
        .map(|f| f.path.rsplit("::").next().unwrap_or(&f.path))
        .collect();
    names.sort_unstable();
    names.dedup();
    let shown: Vec<&str> = names.iter().take(6).copied().collect();
    let more = if names.len() > shown.len() {
        format!(", ... (+{})", names.len() - shown.len())
    } else {
        String::new()
    };
    format!("{owner}\n{} fns: {}{more}", names.len(), shown.join(", "))
}

fn guess_marker(resolution: Resolution) -> Option<&'static str> {
    match resolution {
        Resolution::Assumed => Some(" (assumed)"),
        Resolution::Inferred => Some(" (inferred)"),
        Resolution::Exact | Resolution::Unresolved => None,
    }
}

/// One arrow for every edge of a class between two nodes: a line
/// per distinct message, each marked when its judgement was guessed. The
/// arrow is dashed only when every line was guessed.
fn arrow_attrs(class: ArrowClass, edges: &[&Edge]) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut all_guessed = true;
    for edge in edges {
        let weakest = edge.receiver_resolution.weakest(edge.message_resolution);
        let marker = guess_marker(weakest);
        all_guessed &= marker.is_some();
        let mut line = match edge.kind {
            EdgeKind::Holds => String::new(),
            EdgeKind::Spawn => match &edge.variant {
                Some(variant) => format!("spawn ({variant})"),
                None => "spawn".to_string(),
            },
            EdgeKind::Tell | EdgeKind::Ask => {
                let verb = edge.variant.as_deref().unwrap_or(match edge.kind {
                    EdgeKind::Ask => "ask",
                    _ => "tell",
                });
                let message = edge.message_label.as_deref().unwrap_or("?");
                match &edge.reply {
                    Some(reply) => format!("{verb} {message} -> {reply}"),
                    None => format!("{verb} {message}"),
                }
            }
        };
        if let Some(marker) = marker {
            if line.is_empty() {
                // A holds edge has no text of its own.
                line = marker.trim_start().to_string();
            } else {
                line.push_str(marker);
            }
        }
        if !line.is_empty() && !lines.contains(&line) {
            lines.push(line);
        }
    }
    lines.sort();
    let mut style = match class {
        ArrowClass::Holds => vec!["dotted"],
        ArrowClass::Spawn => vec!["bold"],
        ArrowClass::Send => vec![],
    };
    if all_guessed && class != ArrowClass::Holds {
        style.push("dashed");
    }
    let mut attrs = Vec::new();
    if !style.is_empty() {
        attrs.push(format!("style=\"{}\"", style.join(",")));
    }
    if class == ArrowClass::Holds {
        attrs.push("color=gray50".to_string());
    }
    if !lines.is_empty() {
        attrs.push(format!("label=\"{}\"", escape_text(&lines.join("\n"))));
    }
    attrs.join(", ")
}

fn handlers_label(output: &Output, h: &HandlersNode) -> String {
    let signature = match &h.reply_label {
        Some(reply) => format!("{}<{}, {reply}>", h.handler, h.message_label),
        None => format!("{}<{}>", h.handler, h.message_label),
    };
    let candidates: Vec<&str> = h
        .candidates
        .iter()
        .map(|id| match output.node(id) {
            Some(Node::Actor(a)) => a.path.as_str(),
            _ => id.as_str(),
        })
        .collect();
    let mut text = format!("{signature}\ncandidates: ");
    if candidates.is_empty() {
        text.push_str("none in scope");
    } else {
        text.push_str(&candidates.join(", "));
    }
    if h.may_be_outside {
        text.push_str("\n(may also be an actor outside this list)");
    }
    escape_text(&text)
}

/// Escapes a plain (non-record) quoted label.
fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out
}

/// `{path|Msg -> Reply\lOther\l}`. Every piece goes through [`escape_record`].
fn actor_label(actor: &ActorNode) -> String {
    let mut title = escape_record(&actor.display);
    if actor.open {
        title.push_str("\\n(open: handles any message type)");
    }
    if actor.messages.is_empty() {
        return format!("{{{title}}}");
    }
    let mut lines = String::new();
    for message in &actor.messages {
        let mut line = message.label.clone();
        if let Some(reply) = &message.reply {
            let _ = write!(line, " -> {reply}");
        }
        for cfg in &message.cfg {
            let _ = write!(line, " [{cfg}]");
        }
        match message.resolution {
            Resolution::Exact => {}
            Resolution::Assumed => line.push_str(" (assumed)"),
            Resolution::Inferred => line.push_str(" (inferred)"),
            Resolution::Unresolved => line.push_str(" (unresolved)"),
        }
        lines.push_str(&escape_record(&line));
        lines.push_str("\\l");
    }
    format!("{{{title}|{lines}}}")
}

/// Escapes the characters that carry structure inside a record label.
/// `JoinHandle<String>` or `Box<dyn Calculator>` would otherwise be read as
/// port or field syntax.
pub fn escape_record(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '{' | '}' | '|' | '<' | '>' | '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out
}

fn quote_id(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

#[derive(Debug)]
pub enum SvgError {
    /// `dot` is not on `PATH`.
    DotNotFound,
    Io(std::io::Error),
    /// `dot` ran and failed.
    Dot(String),
}

impl std::fmt::Display for SvgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SvgError::DotNotFound => write!(
                f,
                "Graphviz `dot` was not found on PATH; install Graphviz \
                 (e.g. `brew install graphviz`, `apt install graphviz`)"
            ),
            SvgError::Io(error) => write!(f, "running `dot` failed: {error}"),
            SvgError::Dot(stderr) => write!(f, "`dot` failed: {stderr}"),
        }
    }
}

/// Runs the external Graphviz `dot`. Graphviz is never bundled: distributing
/// it would carry its EPL-2.0 obligations into this tool's artifacts.
pub fn dot_to_svg(dot: &str) -> Result<String, SvgError> {
    let mut child = Command::new("dot")
        .arg("-Tsvg")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => SvgError::DotNotFound,
            _ => SvgError::Io(error),
        })?;
    // Feed stdin from another thread: `dot` may start writing before it has
    // read everything, and a full stdout pipe would then block both sides.
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let input = dot.to_owned();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let output = child.wait_with_output().map_err(SvgError::Io)?;
    writer
        .join()
        .expect("stdin writer does not panic")
        .map_err(SvgError::Io)?;
    if !output.status.success() {
        return Err(SvgError::Dot(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Human-readable report for `--report`.
pub fn report_text(output: &Output) -> String {
    let mut text = String::new();
    for entry in &output.report.entries {
        let place = entry
            .location
            .as_ref()
            .map(|l| format!("{}:{}", l.file, l.line))
            .unwrap_or_else(|| "-".to_string());
        let _ = write!(text, "[{}] {place}", entry.category.as_str());
        if let Some(sender) = &entry.sender {
            let _ = write!(text, " ({sender})");
        }
        let _ = writeln!(text, ": {}", entry.detail);
    }
    text
}
