# rsactor-graph

A static analysis tool that draws the actor topology of an
[rsActor](https://github.com/hiking90/rsactor) application: which actor types
exist, which messages each one handles, which actors hold references to which,
who spawns whom, and who sends what to whom.

It parses source files with `syn`. It does not compile the code and does no
type inference. Every edge records how it was judged, and a call it cannot
judge is listed in a report instead of being guessed (see
[Limitations](#limitations)).

## Requirements

- Rust 1.85 or newer, the same MSRV as rsActor.
- Graphviz `dot` on `PATH`, for SVG output only. JSON and DOT output need
  nothing else. Graphviz is never bundled; install it yourself
  (`brew install graphviz`, `apt install graphviz`, ...).

## Installation

The tool is not published on crates.io yet. From a checkout of this
repository:

```sh
cargo install --path rsactor-graph
# or, without installing
cargo run -p rsactor-graph -- --help
```

## Usage

```sh
# The lib and bins of the package in the current directory, as DOT on stdout
rsactor-graph

# Another project, as SVG
rsactor-graph --manifest-path ../my-app/Cargo.toml --format svg -o topology.svg

# One example of this repository
rsactor-graph --example dining_philosophers --format svg -o dining.svg

# Only the neighbourhood of one actor
rsactor-graph --manifest-path ../my-app/Cargo.toml --focus Router --format svg -o router.svg

# List what could not be judged
rsactor-graph --manifest-path ../my-app/Cargo.toml --report > /dev/null
```

```text
Options:
  --manifest-path <PATH>  Cargo.toml of the workspace or package
  --package <NAME>        Workspace member to scan (default: the root package)
  --example <NAME>        Scan one example instead of the lib and bins
  --format <FORMAT>       json, dot or svg [default: dot]
  -o, --output <FILE>     Write to FILE instead of stdout
  --report                Print the judgement report to stderr
  --focus <ACTOR>         Keep only nodes near ACTOR (full path or unique suffix)
  --depth <N>             Edges to follow from --focus [default: 1]

DOT/SVG only (JSON always carries the full model):
  --holds <MODE>          all, uncovered or none [default: uncovered]
                          uncovered: hide a holds edge where a send or spawn
                          arrow already joins the same two nodes
  --no-group-fns          Draw every function sender separately instead of
                          one node per owning type, trait or module
```

With `--format svg`, if `dot` is not on `PATH`, the DOT text is written next
to the requested file (`-o out.svg` gives `out.dot`; without `-o` it goes to
stdout) and the tool exits with status 1.

### What is scanned

- Without `--example`, the lib and bin targets of the package are merged into
  one graph. With `--example NAME`, only that example is scanned.
- Declarations (actor types, handled messages, struct fields, function
  signatures) are also read from the package's lib and from the libs of the
  workspace members it depends on. Crates outside the workspace are not read;
  an actor from such a crate appears as a node with its path only.
- The rsactor crate itself is never scanned; its API is built into the tool.
  Run in this repository without `--example`, the tool finds no actors and
  lists the examples that contain some.
- `#[cfg(test)]` modules and `tests/` targets are skipped. Other `cfg`
  attributes are not evaluated; a `cfg` on a handler is shown in its label.

## Reading the graph

| Node | Meaning |
|---|---|
| Record with a path and a message list | An actor type and the messages it handles (`Msg -> Reply`, `[cfg(...)]`). `(open: handles any message type)` marks a blanket `impl<M> Message<M>` |
| Rounded box `fn path` | A function that sends or spawns but is not an actor method: a free function, an associated function without `self`, a method of a non-actor type, a trait default method |
| Rounded box `owner` + `N fns: ...` | Several such functions sharing a type, trait or module, drawn as one node (`--no-group-fns` splits them) |
| Dashed ellipse `external (main)` | `main` of a bin or example |
| Dashed box with `(outside the workspace)` | An actor declared in a crate that is not scanned |
| Dashed rounded box `TellHandler<M>` / `AskHandler<M, R>` | A type-erased handler and the actors in scope that could be behind it |
| Red circle `?` | The receiver of a send could not be determined (one per sender) |

| Edge | Meaning |
|---|---|
| Solid arrow | Sends. One arrow per pair, one line per message: `tell M`, `ask M -> Reply`; other send methods appear by name (`tell_priority M`) |
| Bold arrow | `spawn` (or `spawn (spawn_with_options)`) |
| Gray dotted arrow | The actor holds an `ActorRef`/`ActorWeak`/handler of the target in a field or in `type Args` |
| `?` as the message | The message type could not be determined |
| `(assumed)` | The message is `T::f(..)` with no visible signature, assumed to return `T` |
| `(inferred)` | A type name was resolved through the only declared type with that name |
| Dashed arrow | Every line on the arrow is `(assumed)` or `(inferred)` |

## Report

`--report` prints one line per entry to stderr:
`[category] file:line (sender): detail`.

| Category | Meaning |
|---|---|
| `declaration` | A declaration could not be read: unparsable file, missing module file, handler without exactly 3 parameters, unresolvable `impl` target, `ActorRef<T>` held with a generic `T` |
| `unresolved-edge` | Drawn, but its receiver or message is `?` |
| `unclassified` | Neither the receiver nor a message some actor handles could be determined; not drawn |
| `unsupported-receiver` | The receiver's type is one the rules do not follow: a newtype wrapper, a type from an unscanned crate, a generic parameter |
| `ufcs` | A call written as `ActorRef::tell(&r, m)`; not judged |
| `macro-candidate` | A send-like call inside a macro whose arguments do not parse as expressions, such as `tokio::select!` |
| `unresolved-spawn` | An rsactor spawn whose actor type could not be determined |
| `cross-check` | The judged receiver does not handle the judged message. The source compiled, so this means the tool misjudged something |

## JSON

`--format json` writes the whole model: a `schema_version`, every node and
edge with its source locations, a separate `receiver_resolution` and
`message_resolution` (`exact`, `assumed`, `inferred` or `unresolved`) on each
edge, and the report. The DOT/SVG options `--holds` and `--no-group-fns` do
not change it; `--focus` does, because it narrows the scope you asked for.

## Limitations

The tool reads types that are written down or that follow from declarations.
It does not judge:

- values whose type comes from inference across APIs it does not read: a
  generic or `impl Trait` parameter, a value taken out of a channel or other
  external-crate generic, an untyped parameter of a closure that is not passed
  to a known iterator adapter (`for_each`, `map`, `filter`, `filter_map`,
  `find`, `any`, `all`);
- fields of newtypes from other crates, such as axum's
  `State(state): State<AppState>`;
- sends written as UFCS calls, sends inside macros with their own syntax
  (`select!`), and sends produced by user macros;
- a derive or trait imported under another name (`use rsactor::Actor as A`).

Such calls end up as `?` or in the report; the tool never draws an edge it had
to guess.

## Development

```sh
cargo test -p rsactor-graph
# after an intended output change, regenerate the example snapshots and review the diff
RSACTOR_GRAPH_BLESS=1 cargo test -p rsactor-graph --test examples
```

The trees under `tests/fixtures/` are scanner input only; they are not
compiled.
