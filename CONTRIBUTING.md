# Contributing to spriff

Thanks for your interest. spriff aims to stay small, fast, and dependency-light.

## Build & test

```sh
cargo build
cargo test            # unit tests for the board grammar, cursor, rollup, naming
cargo clippy --all-targets -- -D warnings
cargo fmt --all
```

CI runs `fmt --check`, `clippy -D warnings`, and `test` on Linux and macOS.
Please run all four locally before opening a PR — the
[pull request template](.github/PULL_REQUEST_TEMPLATE.md) has the full checklist.

By participating you agree to the [Code of Conduct](CODE_OF_CONDUCT.md). Security
issues go through private reporting — see [SECURITY.md](SECURITY.md), not a public
issue. Pull requests require a maintainer review before merging.

## Principles

- **Root-cause fixes, no band-aids.** Match the existing code's idiom and comment
  density (the code is heavily commented because the *why* matters here).
- **The watcher is read-only to the board.** Any change that writes to the shared
  board from a watcher is a bug — signals go to private sidecars.
- **Context-efficiency is a feature.** Don't add a code path that re-reads the
  whole board; read the delta.
- **Keep deps lean.** New dependencies need a clear justification.

## Manual end-to-end check

Always pipe post bodies via a quoted heredoc (never `-m "…"`) — the shell mangles
backticks/`$`/quotes before spriff sees them:

```sh
export SPRIFF_HOME="$(mktemp -d)/h"
BIN=target/debug/spriff
$BIN init demo --agents 2
$BIN post --collab demo --as Alice -s "plan" --status NEEDS-REVIEW <<'EOF'
review please
EOF
$BIN inbox  --collab demo --as Abbey      # should show Alice's turn
$BIN ack    --collab demo --as Abbey
$BIN inbox  --collab demo --as Abbey      # should be clear
rm -rf "$SPRIFF_HOME"
```

For a fuller end-to-end check, the `tests/rendezvous.rs` suite drives the real
binary against an isolated `SPRIFF_HOME` — run it with `cargo test --test rendezvous`.

## Scope

Good first contributions: more persona names, additional supervisor templates,
and shell completions. Larger ideas (web dashboard, hosted relay) are welcome as
proposals first — open an issue describing the design.
