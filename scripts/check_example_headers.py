#!/usr/bin/env python3
"""Check that API paths named in an example's `//!` header are used by that example.

Example headers say what the example demonstrates, but nothing verifies them:
examples are not rendered by `cargo doc`, their `//!` blocks are never doctested,
and clippy stays silent about message types left behind by a refactor. So a header
can advertise an API the example dropped long ago -- see issue #119, where
`examples/actor_task.rs` promised an `mpsc::channel` that had been deleted 15
months earlier and survived a dozen later edits to the same file.

Rule enforced here: every `a::b`-shaped path in a `//!` header must appear
somewhere in the example's code. Deliberate exceptions -- an API a header names
in order to warn against it -- are listed in the allowlist file.

This catches drift at the token level only. A header that describes the wrong
direction of a data flow, or the wrong reason for a pattern, still needs a human
reader; see CLAUDE.md on documentation being part of the contract.

Usage: python3 scripts/check_example_headers.py [example.rs ...]
"""

from __future__ import annotations

import pathlib
import re
import sys

REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent
ALLOWLIST = REPO_ROOT / "scripts" / "example-header-allowlist.txt"

# Rust paths such as `mpsc::channel` or `SpawnOptions::with_idle`. Single
# identifiers are deliberately not checked: headers legitimately mention other
# examples by name, which would produce noise without catching more drift.
PATH_RE = re.compile(r"\b[A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)+\b")


def load_allowlist() -> set[tuple[str, str]]:
    """Read `<example path> <path token>` pairs that are exempt from the rule."""
    allowed: set[tuple[str, str]] = set()
    if not ALLOWLIST.exists():
        return allowed
    for line in ALLOWLIST.read_text().splitlines():
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        example, _, token = line.partition(" ")
        if not token.strip():
            sys.exit(f"{ALLOWLIST}: malformed entry (want '<file> <token>'): {line!r}")
        allowed.add((example, token.strip()))
    return allowed


def check(path: pathlib.Path, allowed: set[tuple[str, str]]) -> list[str]:
    """Return the header paths that never occur in this example's code."""
    lines = path.read_text().splitlines()
    header = " ".join(line[3:] for line in lines if line.startswith("//!"))
    code = "\n".join(line for line in lines if not line.startswith("//!"))
    rel = path.relative_to(REPO_ROOT).as_posix()
    return sorted(
        {
            token
            for token in PATH_RE.findall(header)
            if token not in code and (rel, token) not in allowed
        }
    )


def main(argv: list[str]) -> int:
    paths = (
        [pathlib.Path(a).resolve() for a in argv]
        if argv
        else sorted((REPO_ROOT / "examples").glob("*.rs"))
    )
    allowed = load_allowlist()
    failed = False

    for path in paths:
        missing = check(path, allowed)
        if missing:
            failed = True
            rel = path.relative_to(REPO_ROOT).as_posix()
            print(f"{rel}: header names {', '.join(missing)}, but the code never uses it")

    if failed:
        print(
            "\nThe `//!` header and the code disagree. Fix whichever is wrong -- and if\n"
            "the header names an API on purpose without using it (to warn against it,\n"
            f"say), add a '<file> <token>' line to {ALLOWLIST.relative_to(REPO_ROOT)}.",
            file=sys.stderr,
        )
        return 1

    print(f"checked {len(paths)} example header(s): all named APIs are used")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
