# WISDOM.md — spriff Institutional Memory

> The accumulated **why** behind design decisions, incidents, and hard-won
> principles in this repository. Each entry records: **what happened**, **what we
> decided**, **why**, and **what to do differently**.
>
> **Pillars**: [CLAUDE.md](./CLAUDE.md) | [PROTECTION.md](./PROTECTION.md) | [GUIDANCE.md](./GUIDANCE.md)

---

## How to Use This File

- **Stuck?** Search here before forming a theory — someone may have already paid for this lesson.
- **Adding an entry?** Use the next free `§N`, place it in the right section, and follow the four-part shape from [GUIDANCE.md](./GUIDANCE.md) "Pillar Update Protocol": trigger, decision/pattern, evidence, expected effect.
- **Cross-referencing?** Write `WISDOM §N` so references stay greppable.

---

## Foundational Principles

> These evergreen principles apply to every task in this repo.

### §1. The Three-Pillar System
Three living documents capture institutional knowledge:
- **WISDOM** — institutional memory: the accumulated *why*.
- **PROTECTION** — immune system: boundaries, invariants, fail-closed guards.
- **GUIDANCE** — nervous system: how intent becomes behavior.

AI agents and human contributors both lose context between sessions. The pillars are the mechanism for compounding knowledge across that gap — without them every session starts from zero. **Every significant change should sharpen at least one pillar.** If a PR ships and no pillar got better, institutional learning was lost.

### §2. Why Model Heterogeneity Wins
spriff exists because combining *diverse* predictors beats any single one — a property that predates language models. A single model is confident in the same places it is wrong, because its training shaped both. A different class of model, trained on different data with different instincts, notices what the first one couldn't. The execute↔review loop operationalizes that: the executor's momentum and the reviewer's skepticism each catch the other's misses. Design decisions should preserve, not flatten, this diversity. (See [README.md](./README.md) and [DESIGN.md](./DESIGN.md).)

### §3. Context-Efficiency Is Load-Bearing, Not Cosmetic
The whole point of the board + cursor + rollup machinery is that a reader can rejoin a long-running collaboration **without re-reading everything**. Any feature that quietly reverts to a full-board rescan erodes the core value proposition. Read the delta. This is both a design principle (here) and an invariant ([PROTECTION.md §3.2](./PROTECTION.md)).

### §4. The Board Is the Single Source of Truth; Sidecars Are Private
Shared state lives on the board and is append-disciplined. Watcher/reviewer signals that aren't part of the shared narrative live in private sidecars. Keeping these separate is what lets the watcher stay read-only ([PROTECTION.md §3.1](./PROTECTION.md)) and keeps the board's history trustworthy.

### §5. Lean Beats Clever
spriff is deliberately small and dependency-light. A new dependency or abstraction must earn its place against the standard library and what already exists. The cost of a dependency is paid forever (supply-chain surface, build time, audit burden); the benefit is usually one-time. Default to "no" and justify "yes."

### §6. Reproduce Before You Theorize
Wrong root-cause diagnoses come from reasoning off commit titles and stale local state. Pull first, reproduce the failing call, read the real error — *then* form a hypothesis. (CLAUDE.md Workflow §1, §6.)

### §7. The Repo Contract Overrides Tool Defaults
[CLAUDE.md](./CLAUDE.md) is the canonical operating contract — read it *before* starting a task here, not after. Its rules win over whatever default a harness, IDE, or CLI tool supplies. The lesson was paid for directly: a session skipped CLAUDE.md, let the tool's default attach a `Co-Authored-By: Claude` trailer to a commit (banned by CLAUDE.md "Git Commits") and pushed a behavior change to `main` without the required PR review — both irreversible once pushed. Read the contract first; author commits as the human with no AI trailer; land behavior changes via a reviewed PR; ship the docs **and the example config** in the same change ([GUIDANCE §2](./GUIDANCE.md)); and treat a bare `~`/`#`/`%` message as the CLAUDE.md shorthand it is, not a typo. ([PROTECTION §1.2, §2](./PROTECTION.md).)

---

## Section Number Registry

| §N | Title | Section |
|----|-------|---------|
| §1 | The Three-Pillar System | Foundational Principles |
| §2 | Why Model Heterogeneity Wins | Foundational Principles |
| §3 | Context-Efficiency Is Load-Bearing | Foundational Principles |
| §4 | The Board Is Source of Truth; Sidecars Are Private | Foundational Principles |
| §5 | Lean Beats Clever | Foundational Principles |
| §6 | Reproduce Before You Theorize | Foundational Principles |
| §7 | The Repo Contract Overrides Tool Defaults | Foundational Principles |
| §8 | Current-Session Wakeups Require Foreground Wait | Foundational Principles |
| §9 | Sidecar Watchers Need a Native Daemon, Not Ad-Hoc Shell | Foundational Principles |
| §10 | Live Reviewer Requests Mean the Visible Session by Default | Foundational Principles |
| §11 | Agent Provenance Survives Only If Stamped | Foundational Principles |

> Add new entries below with the next free `§N` and register them above.

### §8. Current-Session Wakeups Require Foreground Wait
Trigger: an operator expected the reviewer agent in an already-open chat to be
notified by spriff, while the setup drifted through background `watch`/detached
`wait`/supervised-child variants that did not re-enter that same chat.

Decision/pattern: treat "THIS session is the persona" as a foreground loop, not a
subscription flag. The live agent must run `spriff wait --as <persona> --timeout
600 --interval 2`, handle what prints, post, ack, and immediately re-arm. A
timeout is only a heartbeat. `spriff watch` and `spriff supervise` are valid for
sidecar/supervised workflows, but they do not resume a stopped chat model.

Evidence: `SKILL.md`, `docs/OPERATING.md`, and `README.md` now all make the same
foreground-vs-autonomous distinction and warn that `subscribed: no` is expected in
interactive mode.

Expected effect: agents stop claiming they are "watching" because a background
process exists, and operators can choose deliberately between a steerable
current-session reviewer and an autonomous separate child process.

### §9. Sidecar Watchers Need a Native Daemon, Not Ad-Hoc Shell
Trigger: an operator repeatedly asked a live reviewer to "keep watching" a Spriff
board while the foreground `wait` process was tied to one chat/tool turn and
ad-hoc `nohup`/shell loops drifted across machines (`setsid` exists on Linux, not
macOS) or died without a clear status surface.

Decision/pattern: Spriff owns the durable sidecar-watch primitive. Use
`spriff watch-daemon` when the need is "keep board/file signals fresh across chat
turn boundaries, but do not spawn a separate child agent." It is idempotent,
detached, self-restarts the underlying event-driven `watch`, writes pid/log
sidecars, and is visible in `spriff status`.

Evidence: `src/main.rs` implements `watch-daemon` with `--status`/`--stop` and a
restart loop; `tests/rendezvous.rs::watch_daemon_start_status_idempotent_signal_and_stop`
drives it end-to-end against an isolated `SPRIFF_HOME`.

Expected effect: agents stop hand-rolling brittle watcher scripts, operators can
verify the native watcher with one command, and "watching" means a durable Spriff
sidecar is actually alive instead of an invisible shell artifact.

### §10. Live Reviewer Requests Mean the Visible Session by Default
Trigger: an operator asked an already-open agent to be a Spriff reviewer and
expected to see that same agent's review activity in the chat, while the workflow
kept drifting toward hidden supervisors or sidecar-only watchers.

Decision/pattern: make current-session ownership the default when a human asks a
live chat agent to be reviewer/implementer. `spriff watch-daemon` can keep durable
sidecar signals alive, but it is not the reviewer; `spriff supervise`/`serve`
create a separate autonomous agent and require explicit opt-in.

Evidence: `SKILL.md`, `README.md`, `docs/OPERATING.md`, and `cmd_join` output now
state the default explicitly: visible live chat session first, autonomous
supervisor only when requested.

Expected effect: operators see the agent they asked doing the work; agents stop
silently replacing themselves with a hidden child process; sidecar daemons are
understood as notification durability, not identity ownership.

### §11. Agent Provenance Survives Only If Stamped
Trigger: an operator admired two shipped PRs (`mcfiddles-platform` #5925 and #5794)
and could not find which agent produced them. The work *was* done by named spriff
personas — Pamela (producer), Peter and Punchyman (reviewers) on a mission board —
but recovering that took hand-matching commit hashes against local session `.jsonl`
transcripts. Nothing in git recorded it, because the fleet's author convention
([§7](#7-the-repo-contract-overrides-tool-defaults), mcfiddles `CLAUDE.md`) authors
every commit as the human with no AI trailer.

Decision/pattern: the author rule stays — but add **additive** provenance trailers
`Spriff-Agent:` / `Spriff-Mission:`, stamped automatically by a `prepare-commit-msg`
hook that reads the `SPRIFF_AS` / `SPRIFF_COLLAB` env vars spriff already exports at
spawn (`run_agent` in `src/main.rs`). This is *not* a `Co-Authored-By: <model>`
trailer (§7's banned case): authorship stays 100% the operator; a neutral custom key
just records which of *our* agents did the work. The hook no-ops when `SPRIFF_AS` is
unset, so human commits are untouched. Because it fires unconditionally inside an
agent, it can't be silently forgotten — the fleet-wide anti-pattern the hook avoids.

Evidence: shipped as `spriff hooks install|status|uninstall`
([docs/attribution-trailers.md](docs/attribution-trailers.md), hook body in
[hooks/prepare-commit-msg](hooks/prepare-commit-msg) embedded via `include_str!`).
The installer resolves the repo's *effective* hooks dir (honoring a pinned
`core.hooksPath` — a naive `.git/hooks` install is silently ignored where it's
pinned, e.g. the mcfiddles platform clones) and **chains** any pre-existing hook
instead of clobbering it. Covered by unit tests (`effective_hooks_dir_honors_core_hookspath`,
`hook_install_is_idempotent_and_executable`, `hook_install_chains_and_uninstall_restores_foreign_hook`,
`uninstall_refuses_foreign_hook_and_handles_absent`) + a live end-to-end run
(default repo AND pinned-hooksPath repo both stamp; human commit stays clean).

Expected effect: every agent-made commit becomes creditable to its persona and
mission without touching the author line; provenance stops depending on prunable
transcripts. The one gate left to the operator is *when/where* to install (per repo
or a fleet-wide loop) — **not** a blind global `core.hooksPath`, which overrides
per-repo native hooks (e.g. mcfiddles' `pre-push.sh`) and is itself ignored by repos
that already pin one. Prefer `spriff hooks install --repo <path>`.
