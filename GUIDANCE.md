# GUIDANCE.md — spriff Nervous System

> **Scope**: How intent becomes behavior in this repository — protocols and patterns.
> **Pillars**: [CLAUDE.md](./CLAUDE.md) | [WISDOM.md](./WISDOM.md) | [PROTECTION.md](./PROTECTION.md)

---

GUIDANCE answers "**how should I do this?**" — the repeatable procedures that keep
work congruent with the design. WISDOM is the *why*, PROTECTION is *what must not
break*, and this file is the *how*.

---

## 1. The Pillar Update Protocol

### When to Update
Update the pillars proactively whenever:
- A surprising failure reveals a non-obvious truth.
- A new guardrail or invariant is discovered.
- A pattern proves effective (or harmful).
- A recurring confusion is resolved.
- A correction from the user reveals a gap in documented knowledge.

### What to Include
Every pillar entry carries four elements:
1. **Trigger** — what happened that prompted this.
2. **Decision/Pattern** — what was decided, or the pattern identified.
3. **Evidence** — concrete file paths, test names, commit/PR links, or repro steps that anchor it.
4. **Expected effect** — how behavior should change going forward.

### Which Pillar Gets the Update
- **WISDOM.md** — "Why is it built this way?" Architectural decisions, strategic reasoning, past incidents.
- **PROTECTION.md** — "What could go wrong?" Boundaries, invariants, guards, safety rules.
- **GUIDANCE.md** — "How should behavior be directed?" Protocols, procedures, repeatable patterns.
- **CLAUDE.md** — Cross-session operating rules: workflow, shorthand, core principles.

### Multi-Pillar Rule
One event can teach several lessons. Update every relevant pillar and cross-link between them — e.g. a near-miss might add a PROTECTION invariant, a WISDOM entry on why it happened, and a GUIDANCE procedure to avoid it.

### Hygiene
Before adding to any pillar: (1) read the relevant section, (2) check for redundancy and merge rather than duplicate, (3) place it in the right section, (4) update or remove obsolete entries, (5) keep it tight — these are curated reference, not a changelog.

---

## 2. Adding a Feature Congruently

1. **Survey first** (CLAUDE.md Workflow §3): grep the domain nouns, read the neighboring modules and their tests, find the existing abstraction before writing a new one.
2. **Respect the invariants** in [PROTECTION.md §3](./PROTECTION.md): the board is read-only to watchers, readers consume deltas not full rescans, deps stay lean.
3. **Match the idiom**: same naming, structure, and comment density as the surrounding code — comment the *why*.
4. **Test at the right level**: unit tests for grammar/cursor/rollup/naming logic; the `tests/rendezvous.rs` end-to-end suite for behavior that crosses the binary boundary.
5. **Update every doc surface in the SAME change** — not just the prose. A new flag/command updates `SKILL.md` and `--help`; a board-format change updates `docs/BOARD-GRAMMAR.md`; a **new config knob updates `examples/example-collab.toml`**; any user-visible change updates `CHANGELOG.md`. A stale doc/example is a latent bug ([PROTECTION §4](./PROTECTION.md)); shipping the feature without it is half-done.
6. **Run the full local gate** before pushing (CLAUDE.md Workflow §5). It *is* the CI.

---

## 3. Subagent Briefing Contract

When delegating to a subagent, the brief must convey more than the diff:
1. **The vision** — what the finished change looks like when it's done well.
2. **The fit** — how this shard connects to the rest of the system.
3. **The constraints** — the governing principles and invariants it must not violate (point at the relevant pillar sections).
4. **The lane** — exactly which files it owns, so no two agents collide.

Cold-review your own output: a fresh, adversarial pass (your own or a subagent's) catches real blockers that green tests miss.

---

## 4. Open-Source Contribution Flow

This repo welcomes outside contributors; mirror their contract when you work here.
- Larger ideas (new subsystems, hosted/web features) start as a **proposal issue**, not a surprise PR.
- PRs run `fmt --check`, `clippy -D warnings`, and `test` in CI on Linux and macOS — green locally is the precondition, not the finish line.
- Security-sensitive findings go through private reporting (see [SECURITY.md](./SECURITY.md)).
- Keep the [CHANGELOG.md](./CHANGELOG.md) current for user-visible changes.
