# PROTECTION.md — spriff Immune System

> **Scope**: Boundaries, invariants, and guards for this repository.
> **Pillars**: [CLAUDE.md](./CLAUDE.md) | [WISDOM.md](./WISDOM.md) | [GUIDANCE.md](./GUIDANCE.md)

---

Protection is not paranoia — it is institutional care. Everything here should be
**enforceable, testable, and fail-closed where safety matters**. The goal is to
make the dangerous thing hard, and to make it survivable when prevention fails.

> **When to update**: see [GUIDANCE.md](./GUIDANCE.md) "Pillar Update Protocol".
> A PROTECTION entry should name the **trigger**, the **risk class**, the
> **control** (what prevents it), and the **proof path** (how you verify the
> control works).

---

## 1. Destructive Action Prevention

### 1.1 Never Delete Untracked Files
**Risk class: DATA LOSS — UNRECOVERABLE.** Untracked files may be hours of work-in-progress that git cannot recover.
- **NEVER** run `git clean`, `rm -rf`, or otherwise delete untracked files without explicit user approval.
- **NEVER** run `git checkout -- .` / `git restore .` / `git reset --hard` on uncommitted changes without confirming.
- Before any destructive operation, run `git status` and list exactly what would be affected.
- If files must be removed, **commit or stash them first**. When in doubt, ask.

### 1.2 Branch Safety
- Never force-push to `main`.
- Never amend or rewrite published commits without an explicit request.
- Never skip hooks (`--no-verify`).
- Create feature branches for non-trivial changes.

### 1.3 File Overwrite Protection
- Always read a file before writing to it — prevents clobbering content you haven't seen.
- Never `Write` over an existing file without reading it first; prefer `Edit` for modifications.
- When creating a new file, confirm the path doesn't already exist.

### 1.4 Don't Delete What You Didn't Create
Before deleting or overwriting any artifact, look at it. If what you find contradicts how it was described, or you didn't create it, surface that rather than proceeding. "Looks stale/unused" is a hypothesis you cannot test from the outside — treat sibling clones, worktrees, and another session's working tree as potentially holding live, uncommitted work.

---

## 2. Secrets & Public-Repo Safety
**Risk class: SECURITY-CRITICAL — IRREVERSIBLE ONCE PUSHED.** This is a public, world-readable repository.
- Never commit secrets, API tokens, credentials, absolute machine paths, or personal/local config.
- Keep machine-specific values out of tracked files; use environment variables (e.g. `SPRIFF_HOME`) and `.gitignore`.
- Report security issues through private reporting (see [SECURITY.md](./SECURITY.md)), never in a public issue or PR.
- Assume every commit is permanent: a secret pushed and later removed is still in history and must be rotated.

---

## 3. Project Invariants

These are spriff's load-bearing design rules. Violating one is a bug even if tests are green.

### 3.1 The Watcher Is Read-Only to the Board
Any change that writes to the **shared board** from a watcher is a bug. Watcher-side signals go to **private sidecars**, never the shared board. The board is the canonical, append-disciplined surface; do not mutate it from a read path.

### 3.2 Read the Delta, Not the Whole Board
Context-efficiency is an invariant, not a nice-to-have. A new code path that re-reads the entire board where an incremental/delta read would do is a regression. Cursor and rollup state exist precisely so readers don't rescan everything.

### 3.3 Isolate Runtime State in Tests and Manual Runs
Tests and manual end-to-end checks must run against an isolated `SPRIFF_HOME` (e.g. `target/debug` binary against a `mktemp -d` home). Never let a test or manual run touch a real operator's board state.

> Add new project invariants here as they are discovered. Each should state what
> must hold, why it's load-bearing, and how a violation manifests.

---

## 4. Verification Is Fail-Closed
A change is not "done" until the local gate passes — `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --all`, `cargo build --release` (CI runs the same on Linux and macOS). Missing or skipped sources of truth must **error, not warn**: a silent skip reads as "covered everything" when it didn't. If a step was skipped or a test failed, say so plainly with the output — never report success you haven't proven.
