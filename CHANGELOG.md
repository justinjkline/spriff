# Changelog

All notable changes to spriff are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-06-28

First public release: a durable, event-driven coordination CLI for tight
execute↔review loops between heterogeneous frontier coding agents over a shared
markdown board.

### Added

**Core loop**

- Append-only markdown **board** with a canonical grammar, per-persona durable
  **consume cursors**, and `inbox`/`wait` that hand an agent only the delta since
  its cursor (O(new), not O(board)) — so context stays bounded as history grows.
- Automatic **rollup**: older turns fold into a sibling archive past a size
  threshold, keeping the live board (and every agent's context) lean.
- **`spriff serve`** — the ironclad supervisor mode: spriff stays running and
  re-invokes the agent for one turn on every peer post, surviving the agent
  stopping, timing out, or crashing, with zero idle tokens.
- **`spriff watch`** — event-driven (FSEvents/inotify) watcher that wakes an agent
  on a peer post or a peer's real source edits.

**Prompt-native rendezvous**

- `spriff join --project "<goal>"` derives a stable board slug from the goal text,
  so two agents started from the same prompt land on the same board with no manual
  coordination, and the goal seeds the mission.
- **Concurrent first-join safety** — first-join creation is serialized with a
  kernel advisory lock, so two agents launched at the same instant converge on one
  board with consistent identities and an uncorrupted roster.
- Multi-reviewer onboarding — `join --as` binds a reviewer to its own roster slot,
  so the 2nd+ reviewer in a 3+ crew can join from the natural prompt-native path.

**Review quality (the heterogeneity thesis, operationalized)**

- **Skeptical review contract** — `SKILL.md`, the `serve` wake prompt, and the
  reviewer's join brief all instruct a reviewer to try to *break* the work, judge
  the artifact against the goal (not the author's story), and advise rather than
  rubber-stamp — never a bare "LGTM".
- **Heterogeneity check** — `join --class <claude|gpt|…>` records each agent's
  model class; `doctor` warns when the implementer and reviewer share a class
  (forfeiting the error-decorrelation gain) and flags a partially-declared roster
  as unverified.
- **Review lenses** — `join --role reviewer --lens <correctness|security|…>` gives
  each reviewer in a 2+ reviewer crew a distinct lens; `serve` focuses the wake
  prompt on it and `doctor` flags redundant or missing lenses.

**Diagnostics & robustness**

- **`spriff doctor`** — registry, resolved identity + source, board/cursor state,
  whether a `serve` is running, plus warnings for a **desynced cursor**
  (`⚠CURSOR DESYNCED`) and **roster integrity** (duplicate or empty personas).
- **`spriff whoami`** — show which persona/collaboration bare commands resolve to,
  and where that identity came from.

**Project & governance**

- Community health: README grounded in ensemble-diversity research (timeless +
  2026), Code of Conduct, issue/PR templates, Dependabot, branch protection, a
  security policy, CODEOWNERS, and CI on Linux + macOS.
- End-to-end test suite (`tests/rendezvous.rs`) driving the real binary against an
  isolated `SPRIFF_HOME`.

### Changed

- `join` refuses to guess when several collaborations exist and no
  `--project`/`--collab`/marker signal is given, instead of falling back to
  `default`.
- The `--as` discipline in `SKILL.md` names the identity-sensitive commands
  precisely (rather than "every command").

### Fixed

- **Rollup cursor freeze** — a board rollup now remaps every per-persona cursor
  from old→new coordinates, and the read path clamps any cursor left pointing past
  the live board. Previously a rollup could leave a cursor stranded past the
  shrunk board, silently freezing the loop (`wait`/`inbox` reported "nothing new"
  forever while peer turns sat unread).
- **Mission divergence** — joining an existing board with a *different* goal that
  slugifies the same is a hard error with remediation, instead of two agents
  silently sharing a slug while disagreeing on the mission.
- **Roster corruption on create** — a reviewer naming the generated executor, or
  any `--as`/`--with` combination that would duplicate a persona, is now rejected
  rather than silently writing a broken roster.
- The peer rendezvous command printed on `join` carries the real key (`--collab`
  when the slug was forced explicitly), so it never points a peer at a different
  board.

[Unreleased]: https://github.com/justinjkline/spriff/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/justinjkline/spriff/releases/tag/v0.1.0
