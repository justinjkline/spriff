# Changelog

All notable changes to spriff are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Ironclad mode on by default** (`[loop] ironclad`, default `true`). `join` now
  leads every agent with a **subscribe-to-your-board** step — `spriff supervise` /
  `spriff serve` — and frames the manual `wait`-loop as the fallback, so agents
  stop busy-polling or hand-rolling launchd plists.
- **`spriff supervise`** — generate (and with `--install` load) a persistent OS
  service that runs `spriff serve` for a persona: launchd (`RunAtLoad` +
  `KeepAlive`) on macOS, `systemd --user` (`Restart=always` + linger) on Linux. The
  canonical "make it ironclad" artifact — no hand-written plist.
- **Inactivity (stall) watchdog** (`[stall] idle_secs`, default 3600, `0` = off).
  When the board goes silent past the threshold, both `serve` and `watch` ping the
  local agent to post a status update + recommend next steps, so a stalled loop
  resyncs instead of sitting dead. `doctor`/`status` surface board idle time and a
  `⚠ STALLED` flag.
- **Proactive review** (`[review] proactive` = `off|gentle|normal|strict`, default
  `normal`). A reviewer is nudged/re-invoked for an *early* look at the
  implementer's in-progress code before the formal handoff; aggressiveness controls
  the nudge cooldown and whether it escalates loudly. Reviewer-only.
- `spriff status` now reports `subscribed: yes/no` (is a supervisor running?) plus
  board idle time and any outstanding stall / early-review nudge.
- README: copy-paste onboarding prompts for the implementer and reviewer agents,
  so setting up the loop is a one-line prompt per agent.

### Fixed

- **`spriff wait --once` — a non-blocking, exit-coded single poll.** An agent that
  is re-invoked once per turn (a chat session, a harness that gives one turn at a
  time and can't hold a blocked process) should not run the blocking `wait` loop —
  it can't be notified when that background process returns, so posts look
  "missed." `--once` checks the inbox exactly once and exits immediately: code 0
  with the delta printed when a peer turn is waiting, code 2 when nothing is new.
  No sleep, no loop, no wasted tokens. It records the read frontier exactly like
  blocking `wait`/`inbox`, so a later `ack` consumes precisely what was shown, and
  it honors the same split-brain `serve`-ownership guard. Regression test:
  `wait_once_is_nonblocking_and_exit_coded`.
- **`spriff wait` now refuses split-brain persona ownership.** `wait` is the
  current-session / operator-steered loop. If a separate `spriff serve` supervisor
  already holds the same persona lock, `wait` now hard-errors instead of letting
  two agents act as the same reviewer/implementer and race/double-post. Operators
  can intentionally override with `--allow-while-supervised`, but the default path
  is fail-closed and explains the choice: either let the supervised child handle
  turns, or stop it before this live session takes over.
- **Status and operating docs now distinguish "subscribed" from "this chat is
  watching."** `spriff status` says that `subscribed: yes` means a separate child
  agent command will be re-invoked; `subscribed: no` is expected for an
  interactive `spriff wait` loop. `serve`/`supervise`, README, OPERATING, and the
  embedded SKILL protocol now all describe the same foreground-vs-autonomous
  choice.
- **Shell pipelines no longer emit broken-pipe panic noise.** Unix builds restore
  default `SIGPIPE` handling at startup, so common checks like
  `spriff status | grep -q subscribed` terminate quietly when the reader exits
  early instead of printing Rust's `failed printing to stdout: Broken pipe` panic.
- **Onboarding now forces the "who acts as this persona?" decision up front.**
  `join` and `SKILL.md` previously jumped straight to `spriff supervise`/`serve`,
  which silently spawns a SEPARATE headless agent — so an assistant asked, inside
  a live chat, to "set up spriff and review" would background a different agent and
  the operator would lose the session they wanted to steer. Step 0 now makes the
  agent choose (and, if a human is present, ASK) between: (A) THIS session is the
  persona — run the interactive `inbox -> work -> post -> ack -> wait` loop here,
  no supervisor; or (B) a separate supervised process via `supervise`/`serve`. The
  GOLDEN RULE section is reframed around "which mode are you in?", and clarifies
  that `subscribed: no` is EXPECTED (not a failure) in mode (A).
- `spriff ack` no longer swallows a peer turn that arrives mid-turn. Previously
  `ack` advanced the consume cursor to the LIVE board end (`offset =
  board_size()`), so a peer turn posted AFTER the agent read its inbox but BEFORE
  it acked was leapfrogged and never resurfaced — and under `spriff serve` the
  supervisor then computed an empty delta and never re-invoked the agent (a
  silently skipped beat). `ack` now advances only to the agent's READ FRONTIER —
  the board end as of its most recent `inbox`/`wait` — which `inbox`/`wait`
  record when they actually show turns. A turn that lands after that read stays
  unread. `status`/`doctor`/the serve completion-poll never move the frontier, so
  read-only polling can't consume unseen turns. Regression test:
  `ack_does_not_swallow_a_turn_that_arrived_after_the_read`.
- `spriff supervise --install -- codex exec` / `claude -p` now resolves the agent
  binary through the operator's current `PATH` before writing the launchd/systemd
  service, and carries `HOME` + `PATH` into the service environment. macOS launchd
  starts jobs with a sparse PATH, so the previous generated plist could load
  `spriff serve` successfully but then fail every peer turn with `codex: No such
  file or directory`.
- Board parsing now treats only valid spriff turn headers as turn boundaries.
  Body-level Markdown H2 headings such as `## Review Notes` no longer split a
  post into phantom turns or cause a persona to see its own body sections as
  unread peer work.

### Notes

- Stall and proactive-review nudges are written to dedicated, **non-acked**
  sidecars (`STALL.md` / `REVIEW_NUDGE.md`), never the pending/`ack` channel — so a
  nudge can never make `spriff ack` swallow an unread peer turn.

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
