// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! `rsactor-graph` command line (§5).

use std::path::PathBuf;
use std::process::ExitCode;

use rsactor_graph::render::{self, SvgError};
use rsactor_graph::workspace::{Selection, Workspace};

const USAGE: &str = "\
Draws the actor topology of an rsactor application.

Usage: rsactor-graph [OPTIONS]

Options:
  --manifest-path <PATH>  Cargo.toml of the workspace or package
  --package <NAME>        Workspace member to scan (default: the root package)
  --example <NAME>        Scan one example instead of the lib and bins
  --format <FORMAT>       json, dot or svg [default: dot]
  -o, --output <FILE>     Write to FILE instead of stdout
  --report                Print the judgement report to stderr
  -h, --help              Print this help
";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Json,
    Dot,
    Svg,
}

struct Args {
    selection: Selection,
    format: Format,
    output: Option<PathBuf>,
    report: bool,
}

fn parse_args() -> Result<Option<Args>, String> {
    let mut args = Args {
        selection: Selection::default(),
        format: Format::Dot,
        output: None,
        report: false,
    };
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        let mut value = |name: &str| {
            iter.next()
                .ok_or_else(|| format!("{name} requires a value"))
        };
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "--manifest-path" => args.selection.manifest_path = Some(value(&arg)?.into()),
            "--package" | "-p" => args.selection.package = Some(value(&arg)?),
            "--example" => args.selection.example = Some(value(&arg)?),
            "--format" => {
                args.format = match value(&arg)?.as_str() {
                    "json" => Format::Json,
                    "dot" => Format::Dot,
                    "svg" => Format::Svg,
                    other => return Err(format!("unknown format `{other}`")),
                }
            }
            "-o" | "--output" => args.output = Some(value(&arg)?.into()),
            "--report" => args.report = true,
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    Ok(Some(args))
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(Some(args)) => args,
        Ok(None) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("error: {error}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &Args) -> Result<(), String> {
    let workspace = Workspace::load(&args.selection)?;
    let input = workspace.input(&args.selection)?;
    let output = rsactor_graph::scan(&input);

    if args.report {
        eprint!("{}", render::report_text(&output));
    }
    if output.actors().next().is_none() && args.selection.example.is_none() {
        return Err(no_actors_guidance(&workspace, &args.selection, &input.name));
    }

    let text = match args.format {
        Format::Json => render::to_json(&output),
        Format::Dot => render::to_dot(&output),
        Format::Svg => {
            let dot = render::to_dot(&output);
            match render::dot_to_svg(&dot) {
                Ok(svg) => svg,
                Err(SvgError::DotNotFound) => {
                    // Keep the DOT so the work is not lost (§6.3).
                    let kept = match &args.output {
                        Some(path) => {
                            let dot_path = path.with_extension("dot");
                            write(&Some(dot_path.clone()), &dot)?;
                            format!("DOT written to {}", dot_path.display())
                        }
                        None => {
                            print!("{dot}");
                            "DOT written to stdout".to_string()
                        }
                    };
                    return Err(format!("{}; {kept}", SvgError::DotNotFound));
                }
                Err(error) => return Err(error.to_string()),
            }
        }
    };
    write(&args.output, &text)
}

fn write(path: &Option<PathBuf>, text: &str) -> Result<(), String> {
    match path {
        Some(path) => std::fs::write(path, text)
            .map_err(|error| format!("cannot write {}: {error}", path.display())),
        None => {
            print!("{text}");
            Ok(())
        }
    }
}

/// No actor in the lib and bins (always the case for the rsactor repo itself,
/// §3): list the examples that do contain actors.
fn no_actors_guidance(workspace: &Workspace, selection: &Selection, name: &str) -> String {
    let mut with_actors = Vec::new();
    if let Ok(examples) = workspace.examples(selection) {
        for example in examples {
            let example_selection = Selection {
                example: Some(example.clone()),
                ..selection.clone()
            };
            if let Ok(input) = workspace.input(&example_selection) {
                if rsactor_graph::scan(&input).actors().next().is_some() {
                    with_actors.push(example);
                }
            }
        }
    }
    let mut message = format!("no actor types found in the lib and bins of `{name}`");
    if !with_actors.is_empty() {
        message.push_str("\nexamples with actors:\n");
        for example in &with_actors {
            message.push_str(&format!("  {example}\n"));
        }
        message.push_str("run again with --example NAME");
    }
    message
}
