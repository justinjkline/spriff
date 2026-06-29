# CLAUDE.md — spriff Operating Principles

> **Scope**: Cross-cutting principles for working in this repository.
> **Pillars**: [WISDOM.md](./WISDOM.md) | [PROTECTION.md](./PROTECTION.md) | [GUIDANCE.md](./GUIDANCE.md)
> **Doc map**: see [Project Documentation](#project-documentation) below.

---

# What This Is

spriff runs tight execute↔review loops between heterogeneous frontier coding
agents over a shared board, with durable cross-turn signaling. It is a **small,
fast, dependency-light Rust CLI** and a **public, MIT-licensed open-source
project**. Two values shape every decision here: keep it lean, and keep the
*why* legible — the code is heavily commented because the reasoning matters more
than the mechanism.

This file is the canonical operating contract. `AGENTS.md`, if present, is a
compatibility shim that points here — no split-brain docs.

---

# Project Documentation

spriff carries more first-class docs than a typical CLI, because the project *is*
a protocol for agents collaborating on a board. Read the relevant one before
working in its area — don't reconstruct what's already written down.

**The special one — read it when touching agent/board behavior:**

| Doc | What it is | Read it when |
|---|---|---|
| [SKILL.md](./SKILL.md) | **The agent collaboration protocol** — the runtime contract an agent follows to *operate* spriff: subscribe via `supervise`/`serve`, do one turn per peer turn (don't poll), `--as <persona>` on every acting command, post bodies via stdin, status markers, definition-of-done, how to review as a skeptical peer. | You change any command, flag, board interaction, turn/inbox/ack semantics, or supervision behavior — the protocol described here must stay true, and the doc must be updated in the same PR. |

**Architecture & specs:**

| Doc | What it is |
|---|---|
| [README.md](./README.md) | What spriff is and *why* heterogeneous-model loops win. The front door. |
| [DESIGN.md](./DESIGN.md) | Architecture and provenance — distilled from the 32 hand-rolled board-watcher scripts; what was kept, dropped, and why. |
| [docs/BOARD-GRAMMAR.md](./docs/BOARD-GRAMMAR.md) | The append-only board grammar — the turn format spec (machine-parseable, human-skimmable, token-lean). The source of truth for anything that reads or writes the board. |
| [docs/OPERATING.md](./docs/OPERATING.md) | Practical day-to-day operating guide (install, run, supervise). |
| [docs/MIGRATION.md](./docs/MIGRATION.md) | Concept mapping from the old bespoke `watch-*.sh`/`.py` scripts onto spriff. |

**Process & policy:**

| Doc | What it is |
|---|---|
| [CONTRIBUTING.md](./CONTRIBUTING.md) | Build/test/lint commands and the PR contract. The local gate. |
| [CHANGELOG.md](./CHANGELOG.md) | Keep current for any user-visible change. |
| [SECURITY.md](./SECURITY.md) | Private vulnerability reporting — never a public issue/PR. |
| [CODE_OF_CONDUCT.md](./CODE_OF_CONDUCT.md) | Community standards for contributors. |
| [.github/PULL_REQUEST_TEMPLATE.md](./.github/PULL_REQUEST_TEMPLATE.md) | The full pre-PR checklist. |

**Doc maintenance is part of the change.** SKILL.md, BOARD-GRAMMAR.md, and the
README describe observable contracts — if your change alters behavior they
describe, update them in the same PR (PROTECTION's silent-skip ban applies to
docs too: a stale contract doc is a latent bug).

---

# Core Principles

- **Root-Cause Fixes, No Band-Aids**: Find the real cause. No temporary patches, no TODOs left behind, no "file an issue and move on." Senior-engineer standards. The only exception is genuinely multi-phase work that is non-critical to the current task.
- **Small, Fast, Lean — No Fluff**: Production-grade, but spriff stays minimal. No gold-plating, no speculative abstraction, no enterprise theater. New dependencies need a clear justification — prefer the standard library and what's already in `Cargo.toml`.
- **Match the Surrounding Code**: Write code that reads like its neighbors — same idiom, naming, and **comment density**. This codebase comments the *why*; keep doing that. A change a reviewer can't trace back to a reason is incomplete.
- **Context-Efficiency Is a Feature**: Don't add a path that re-reads the whole board when it could read the delta. Cheap, incremental reads are a design constraint, not an optimization to defer.
- **Sub-Two-Minute Rule**: If a fix takes under two minutes, do it now. Don't file an issue. Stay alert for adjacent small wins while you're in the file.
- **Bugs Found In Flight**: When you spot an unrelated bug mid-task, fix it immediately (or spawn a subagent to) with a tight briefing — don't let it evaporate.
- **Issue Hygiene — Net-Negative Filing**: Before opening an issue, search existing issues for the same symptom/area and close or squash duplicates, stale, or already-fixed ones in the same pass. When you fix something, sweep open issues for related keywords and close what the fix resolved, linking the PR/commit. Close only what is *genuinely* resolved, with evidence — never to hit a quota.
- **Public Repo Discipline**: This is open source. Never commit secrets, tokens, local paths, or machine-specific config. Security issues go through private reporting (see [SECURITY.md](./SECURITY.md)), never a public issue or PR. Assume every commit is permanent and world-readable.

**Shorthand**:
- `~` = "Did you follow CLAUDE.md?" — re-check this file against what you just did.
- `#` = "Harvest wisdom from this context window into WISDOM.md (and the other pillars as relevant)."
- `%` = "Safe to close this session? Anything dangling?" — audit uncommitted changes, unpushed commits, running background processes/subagents, incomplete tasks, unharvested wisdom, half-applied edits. Report a punch list; say "clean — safe to close" only if truly nothing remains. As the final step of `%`, `git pull --rebase` the latest `main`.

---

# Workflow

## 1. Pull Before You Build
`git pull --rebase` before any task — coding *or* diagnosis. Stale local state produces wrong root-cause diagnoses and merge pain. Subagents working in worktrees: pull there too.

## 2. Plan Mode for Non-Trivial Work
Enter plan mode for anything that is 3+ steps or carries an architectural decision. If the work goes sideways mid-stream, stop and re-plan rather than pushing through a broken approach.

## 3. Survey Before You Build
Before any substantial change, survey the codebase for what already exists. Every new line must be congruent with the current design. Grep the domain nouns, find the existing abstraction, read the neighboring modules and tests, and check the three pillars. Don't reinvent a helper that's already there.

## 4. Subagent Strategy
- Default to parallelism: one focused task per subagent, dispatched together when independent.
- Brief the vision, not just the diff: (1) what the finished change looks like, (2) how this shard fits the whole, (3) the governing principles it must respect.
- Lane discipline: no two agents touch the same file at once.

## 5. Verification Before Done — Local Checks ARE the Gate
Never mark a task complete without proving it works. Before pushing **any** branch, run the full local gate that CI runs:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all
cargo build --release
```

CI runs exactly these on Linux and macOS — a green local run is the contract. For behavior changes, also drive the real binary end-to-end against an isolated `SPRIFF_HOME` (see [CONTRIBUTING.md](./CONTRIBUTING.md) "Manual end-to-end check" and the `tests/rendezvous.rs` suite). When piping post bodies, always use a quoted heredoc (`<<'EOF'`), never `-m "…"` — the shell mangles backticks/`$`/quotes before spriff sees them.

## 6. Consult WISDOM When Stuck
Before spinning wheels, check WISDOM.md. Reproduce the failing call and read the actual error/stack trace *before* forming a theory from commit titles or guesses.

## 7. Self-Improvement Loop
After any correction or hard-won insight: update the relevant pillar (see [GUIDANCE.md](./GUIDANCE.md) "Pillar Update Protocol"). Proactively capture non-obvious truths — WISDOM (the *why*), PROTECTION (what can fail), GUIDANCE (how behavior is directed). Harvest before context is compressed.

---

# The Three Pillars

- **WISDOM** (`WISDOM.md`) = institutional memory — the accumulated *why*.
- **PROTECTION** (`PROTECTION.md`) = immune system — boundaries, invariants, guards, fail-closed rules.
- **GUIDANCE** (`GUIDANCE.md`) = nervous system — how intent becomes behavior: protocols and patterns.

**When to consult**: WISDOM for "why is it built this way?", PROTECTION for "what can fail?", GUIDANCE for "how should I do this?".

---

# Auto-Memory Taxonomy

Persistent memory files carry `priority: system | frequent | contextual` (default `contextual`):
- **system**: invariant truth for every task — inline a 1–2 line summary in the memory index.
- **frequent**: applies to most non-trivial work — read proactively at session start.
- **contextual**: read on demand when the index hook matches. The default; pick this when unsure.

---

# Environment

Rust, stable toolchain. The repo is dependency-light by design.

- Build/test/lint commands: see Workflow §5 and [CONTRIBUTING.md](./CONTRIBUTING.md).
- The binary builds to `target/debug/spriff` (or `target/release/spriff`).
- For any manual run, isolate state with a throwaway `SPRIFF_HOME` (`export SPRIFF_HOME="$(mktemp -d)/h"`) so you never touch real board data, and clean it up after.

---

# Git Commits

- Author commits as **{NAME_OF_ACTUAL_HUMAN_ATTACHED_TO_THE_GITHUB_ACCOUNT (e.g. Justin Kline)}**. Do **not** add a "Co-Authored-By: Claude" trailer.
- Thoroughly comment and document *how it works and why it matters* — in the code and in the commit message.
- Create feature branches for non-trivial changes; never force-push `main`; never skip hooks with `--no-verify`. Pull requests require maintainer review before merging (see [CONTRIBUTING.md](./CONTRIBUTING.md)).
