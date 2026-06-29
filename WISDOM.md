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

> Add new entries below with the next free `§N` and register them above.
