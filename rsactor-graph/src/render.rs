// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! JSON, DOT and SVG output (§6).

use std::fmt::Write as _;
use std::io::Write as _;
use std::process::{Command, Stdio};

use crate::model::{ActorNode, Edge, EdgeKind, HandlersNode, Node, Output, Resolution};

pub fn to_json(output: &Output) -> String {
    let mut text = serde_json::to_string_pretty(output).expect("model serialises");
    text.push('\n');
    text
}

pub fn to_dot(output: &Output) -> String {
    let mut dot = String::new();
    let _ = writeln!(dot, "digraph {} {{", quote_id(&output.name));
    dot.push_str("  rankdir=LR;\n");
    dot.push_str("  node [shape=record, fontname=\"Helvetica\", fontsize=10];\n");
    dot.push_str("  edge [fontname=\"Helvetica\", fontsize=9];\n");
    for node in &output.graph.nodes {
        let id = quote_id(node.id());
        let attrs = match node {
            Node::Actor(actor) => format!("label=\"{}\"", actor_label(actor)),
            Node::ExternalActor(n) => format!(
                "shape=box, style=dashed, label=\"{}\\n(outside the workspace)\"",
                escape_text(&n.path)
            ),
            Node::Fn(n) => format!(
                "shape=box, style=rounded, label=\"fn {}\"",
                escape_text(&n.path)
            ),
            Node::External(_) => "shape=ellipse, style=dashed, label=\"external (main)\"".into(),
            Node::Handlers(h) => format!(
                "shape=box, style=\"rounded,dashed\", label=\"{}\"",
                handlers_label(output, h)
            ),
        };
        let _ = writeln!(dot, "  {id} [{attrs}];");
    }
    for group in edge_groups(&output.graph.edges) {
        let first = group[0];
        let _ = writeln!(
            dot,
            "  {} -> {} [{}];",
            quote_id(&first.from),
            quote_id(&first.to),
            edge_attrs(&group)
        );
    }
    dot.push_str("}\n");
    dot
}

/// Edges sharing (from, to, kind, receiver resolution, message resolution)
/// are drawn as one arrow listing every message (§6.2). `edges` is sorted,
/// so members of a group are adjacent.
fn edge_groups(edges: &[Edge]) -> Vec<Vec<&Edge>> {
    let key = |e: &Edge| {
        (
            e.from.clone(),
            e.to.clone(),
            e.kind,
            e.receiver_resolution,
            e.message_resolution,
        )
    };
    let mut groups: Vec<Vec<&Edge>> = Vec::new();
    for edge in edges {
        match groups.last_mut() {
            Some(group) if key(group[0]) == key(edge) => group.push(edge),
            _ => groups.push(vec![edge]),
        }
    }
    groups
}

fn edge_attrs(group: &[&Edge]) -> String {
    let first = group[0];
    let weakest = first.receiver_resolution.weakest(first.message_resolution);
    let guessed = matches!(weakest, Resolution::Assumed | Resolution::Inferred);
    let mut lines: Vec<String> = Vec::new();
    for edge in group {
        let line = match edge.kind {
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
                let message = edge.message.as_deref().unwrap_or("?");
                match &edge.reply {
                    Some(reply) => format!("{verb} {message} -> {reply}"),
                    None => format!("{verb} {message}"),
                }
            }
        };
        if !line.is_empty() && !lines.contains(&line) {
            lines.push(line);
        }
    }
    if guessed {
        lines.push(format!("({weakest:?})").to_lowercase());
    }
    let mut style = match first.kind {
        EdgeKind::Holds => vec!["dotted"],
        EdgeKind::Spawn => vec!["bold"],
        EdgeKind::Tell | EdgeKind::Ask => vec![],
    };
    if guessed && first.kind != EdgeKind::Holds {
        style.push("dashed");
    }
    let mut attrs = Vec::new();
    if !style.is_empty() {
        attrs.push(format!("style=\"{}\"", style.join(",")));
    }
    if first.kind == EdgeKind::Holds {
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

/// Escapes the characters that carry structure inside a record label (§6.2).
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

/// Runs the external Graphviz `dot` (§6.3). Graphviz is never bundled.
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
