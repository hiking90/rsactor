// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! JSON, DOT and SVG output (§6).

use std::fmt::Write as _;
use std::io::Write as _;
use std::process::{Command, Stdio};

use crate::model::{ActorNode, Output, Resolution};

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
    for actor in output.actors() {
        let _ = writeln!(
            dot,
            "  {} [label=\"{}\"];",
            quote_id(&node_id(actor)),
            actor_label(actor)
        );
    }
    dot.push_str("}\n");
    dot
}

fn node_id(actor: &ActorNode) -> String {
    format!("{}:{}", actor.target, actor.path)
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
