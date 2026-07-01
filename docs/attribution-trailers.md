# Attribution Trailers — crediting the spriff agent that made a commit

> **Status:** Shipped. `spriff hooks install|status|uninstall` implements this
> (hooksPath-aware, chaining, unit- + integration-tested). Activation per repo is
> still an explicit operator step — the hook is **not** auto-installed on `join`.

## 1. The problem this solves

spriff agents produce real commits, but the fleet's git convention
([mcfiddles `CLAUDE.md` → Git Commits], mirrored here in [WISDOM §7](../WISDOM.md))
authors **every** commit as the human operator with **no** AI trailer. That rule
is correct and stays — but it has a side effect: once a PR merges, there is no way
to tell *which agent* did the work. Provenance survives only in the local session
`.jsonl` transcripts (which get pruned) and the spriff board sidecars.

Concretely, on 2026-06-30 an operator looked at two high-quality PRs
(`mcfiddles-platform` #5925 item6/P2-core hardening and #5794 ADR-018-B2 prod MCP
bring-up) and could not find who authored them. The forensic trail *did* exist —
the work was done by named spriff personas (**Pamela** producer, **Peter** and
**Punchyman** reviewers) on the `canonical-agent-fabric` mission — but recovering
it took cross-referencing commit hashes against transcripts by hand. The identity
was never in git, so GitHub showed only `justinjkline`.

**Goal:** make the committing agent creditable on every commit, *additively*, in a
way that (a) does not reintroduce a banned `Co-Authored-By: Claude`-style AI
trailer, (b) requires no cooperation from the producing agent (can't be
forgotten), and (c) no-ops completely for the operator's own manual commits.

## 2. The trailers

Two git trailers, appended to the standard trailer block at the foot of the commit
message:

```
Spriff-Agent: Pamela
Spriff-Mission: canonical-agent-fabric
```

- **`Spriff-Agent`** — the persona that ran `git commit` (the committer), taken
  verbatim from `$SPRIFF_AS`.
- **`Spriff-Mission`** — the collaboration/board the work belongs to, taken from
  `$SPRIFF_COLLAB`.

They are [git *trailers*](https://git-scm.com/docs/git-interpret-trailers) —
`Key: Value` lines in the final paragraph — so they are greppable
(`git log --grep='Spriff-Agent: Peter'`), machine-parseable
(`git interpret-trailers --parse`), and invisible to anyone who doesn't care.

### Why this does not violate the author rule (WISDOM §7)

[WISDOM §7](../WISDOM.md) records a paid-for lesson: a session let a tool default
attach `Co-Authored-By: Claude` and pushed to `main` without review — banned.
These trailers are **not** that, and the distinction is deliberate:

| Banned (`Co-Authored-By: Claude`) | This proposal (`Spriff-Agent: Pamela`) |
|---|---|
| Names the *model vendor* ("Claude") | Names the *persona role* on our own roster |
| Uses git's `Co-Authored-By` semantics → GitHub reassigns/co-credits authorship | A neutral custom key → **authorship stays 100% the operator** |
| Advertises "an AI wrote this" | Records *which of our agents* did the work, for our own provenance |

The commit's `Author:` and `Committer:` remain the human operator, unchanged. This
is internal provenance metadata, not authorship reassignment. It is the additive
credit the author rule leaves room for — the rule bans *co-authorship to a model*,
not *labeling our own delegation layer*.

## 3. Why a hook, and why it's reliable

spriff already exports the agent's identity into the spawned process's environment.
From `src/main.rs` (`run_agent`):

```rust
cmd.args(args)
    .arg(prompt)
    .env("SPRIFF_COLLAB", name)   // the mission/board
    .env("SPRIFF_AS", persona);   // the persona (e.g. "Pamela")
```

So the identity is **already present in every spriff-spawned agent's shell**. A
`prepare-commit-msg` hook reads those two variables and stamps the trailers on
*every* commit the agent makes — including `git commit -m "…"` (which bypasses
commit templates and editors). Crucially:

- **No producer cooperation.** The agent doesn't have to remember anything; the
  hook fires unconditionally. This avoids the silent-skip failure mode the fleet
  bans everywhere else.
- **Fail-open for humans.** The hook's first line is `[ -n "$SPRIFF_AS" ] || exit 0`.
  Your own manual commits (no `SPRIFF_AS` in the environment) are untouched.
- **Idempotent.** It pipes through `git interpret-trailers`, which dedups, so
  amends/rebases/cherry-picks don't stack duplicate trailers.

## 4. The hook

Ready-to-use at [`hooks/prepare-commit-msg`](../hooks/prepare-commit-msg) in this
repo. Inline for review:

```sh
#!/bin/sh
# spriff prepare-commit-msg hook — stamp agent-provenance trailers.
#
# Fires for every commit made *inside a spriff-spawned agent* (identified by the
# SPRIFF_AS env var that `spriff serve/supervise` exports). No-ops for everyone
# else — the operator's own manual commits are never touched.
#
# Args (git-supplied): $1 = path to the commit message file
#                      $2 = commit source (message|template|merge|squash|commit|"")
set -eu

# 1. Only act inside a spriff agent. This is the human-safe gate.
[ -n "${SPRIFF_AS:-}" ] || exit 0

# 2. Skip auto-generated messages we don't own the body of.
case "${2:-}" in
  merge|squash) exit 0 ;;
esac

# 3. Stamp the trailers. `interpret-trailers` places them in the canonical
#    trailer block, and --if-exists addIfDifferent keeps it idempotent across
#    amends/rebases (identical trailer is not re-added).
set -- --in-place --if-exists addIfDifferent --if-missing add \
       --trailer "Spriff-Agent: ${SPRIFF_AS}"
[ -n "${SPRIFF_COLLAB:-}" ] && set -- "$@" --trailer "Spriff-Mission: ${SPRIFF_COLLAB}"
git interpret-trailers "$@" "$1"
```

## 5. Installation — `spriff hooks install`

The hook has to live in whatever repo the commit happens in. The supported path is
the `spriff hooks` command family, which resolves the repo's **effective** hooks dir
(honoring a pinned `core.hooksPath` — installing into a naive `.git/hooks` would be a
silent no-op in repos that pin it, like the mcfiddles platform clones) and **chains**
any pre-existing hook instead of clobbering it:

```sh
spriff hooks status                 # is it installed here? where's the effective hooks dir?
spriff hooks install                # install into the current repo (idempotent)
spriff hooks install --repo <path>  # …or a specific repo
spriff hooks install --force        # refresh an installed hook to this binary's version
spriff hooks uninstall              # remove; restores any hook it had displaced
```

Behavior that makes it safe to run broadly:
- **hooksPath-aware.** Writes to the dir git actually fires hooks from, resolved via
  `core.hooksPath` → else `.git/hooks`.
- **Never clobbers.** A foreign `prepare-commit-msg` is moved to
  `prepare-commit-msg.local` and the installed hook runs it first (preserving exit
  codes); `uninstall` restores it.
- **Idempotent.** Re-installing a spriff hook is a no-op unless `--force`.
- **Human-safe.** The hook itself no-ops when `SPRIFF_AS` is unset, so it never
  touches the operator's manual commits.

Roll it across a fleet of clones with a loop:

```sh
for d in ~/Sites/mcfiddles-ai-multi/mcfiddles-ai-*/mcfiddles-platform; do
  spriff hooks install --repo "$d"
done
```

> **Avoid** a blind global `git config --global core.hooksPath …`: it overrides
> per-repo hooks everywhere and is *silently ignored* by repos that already pin
> their own `core.hooksPath` (the platform clones do). `spriff hooks install`
> handles both correctly; prefer it.

## 6. Verification

After installing (any option), prove it end-to-end against a throwaway commit:

```sh
# Simulate a spriff-spawned agent:
SPRIFF_AS=Pamela SPRIFF_COLLAB=canonical-agent-fabric \
  git commit --allow-empty -m "test: attribution trailer"
git log -1 --format='%(trailers:key=Spriff-Agent,valueonly)'   # → Pamela
git log -1 --format='%B' | tail -3                              # trailers present

# Prove the human-safe gate: a bare commit must NOT be stamped.
git commit --allow-empty -m "test: human commit"
git log -1 --format='%B' | grep -c Spriff-Agent                 # → 0
```

Then reset the throwaway commits. A green run of both halves (stamped under
`SPRIFF_AS`, clean without it) is the acceptance gate for this change — mirroring
the fleet's "prove the guard is load-bearing" review discipline.

## 7. Scope notes / non-goals (v1)

- **Credits the committer, not the reviewers.** The env carries one persona
  (`$SPRIFF_AS`). Reviewer credit (Peter/Punchyman blessed it) already lives on the
  board; a v2 could add `Spriff-Reviewed-By` trailers sourced from the board, but
  that is out of scope here.
- **Role is not stamped.** `executor` vs `reviewer` is derivable from the roster in
  `~/.spriff/<mission>/<mission>.toml` if wanted; kept out of the trailer to keep
  the hook dependency-free (spriff §5 Lean Beats Clever).
- **No back-fill.** This only stamps commits made after activation. Historical PRs
  (#5925/#5794 etc.) stay attributable only via transcripts — accepted.
