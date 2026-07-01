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
- **Fail-open for humans.** After running any chained hook, the gate
  `[ -n "$SPRIFF_AS" ] || exit 0` no-ops for commits made outside a spriff agent —
  your own manual commits (no `SPRIFF_AS`) are untouched.
- **Best-effort, never blocking.** The trailer step is `|| exit 0`: a stamping hiccup
  (e.g. a git too old for these flags) lets the commit proceed *unstamped*. Only a
  *chained* prior hook can veto a commit — because that is real prior behavior.
- **Injection-safe.** Trailer values are stripped of CR/LF before stamping, so a stray
  newline can't smuggle in an extra trailer — including the very `Co-Authored-By:
  <model>` trailer this feature exists to never emit.
- **Idempotent.** It pipes through `git interpret-trailers` with
  `--if-exists addIfDifferent`, so amends/rebases/cherry-picks don't stack duplicates.

## 4. The hook

The hook is [`hooks/prepare-commit-msg`](../hooks/prepare-commit-msg) — the single
source of truth, embedded into the binary via `include_str!` and installed verbatim.
Read it there rather than a copy here; duplicating it would drift (an earlier draft of
this doc inlined a stale copy that had lost a bug fix — exactly what "one source of
truth" is meant to prevent). Its shape, top to bottom: run any chained `.local` hook
(fail-closed) → gate on `SPRIFF_AS` → skip merge/squash → strip CR/LF from the values
→ stamp `Spriff-Agent`/`Spriff-Mission` via `git interpret-trailers` (best-effort). It
MUST stay LF-only (`.gitattributes` + a build test enforce this) so the embedded copy
runs under the POSIX sh git-for-windows uses.

## 5. Installation — `spriff hooks install`

The hook has to live in whatever repo the commit happens in. The supported path is
the `spriff hooks` command family, which asks git for the exact dir it fires hooks
from — worktree-correct (agents often run in linked worktrees, where hooks fire from
the *common* `.git/hooks`, not the per-worktree gitdir) and honoring a pinned
`core.hooksPath` (e.g. a husky repo's `.husky/`) — and **chains** any pre-existing
hook instead of clobbering it:

```sh
spriff hooks status                 # is it installed here? where's the effective hooks dir?
spriff hooks install                # install into the current repo (idempotent)
spriff hooks install --repo <path>  # …or a specific repo
spriff hooks install --force        # refresh an installed hook to this binary's version
spriff hooks uninstall              # remove; restores any hook it had displaced
```

Behavior that makes it safe to run broadly:
- **Resolves the real hooks dir.** Asks git (`rev-parse --git-path hooks`) for the
  exact dir it fires hooks from — correct inside linked worktrees (the common
  `.git/hooks`, not the per-worktree gitdir) and honoring a pinned `core.hooksPath`.
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
> per-repo hooks everywhere, and is *silently ignored* by any repo that already pins
> its own `core.hooksPath`. `spriff hooks install` resolves each repo's real hooks
> dir instead; prefer it.

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
