# spriff — design

spriff is the distillation of **32 hand-rolled "board-watcher" scripts** that
evolved organically while running pairs of AI coding agents against a shared
markdown board. Those scripts ranged from a naive 24-line line-count poller to a
553-line hardened daemon. This document records what was harvested, what was
dropped, and the architecture that resulted.

## Provenance: what the 32 scripts taught us

Each script let one agent persona watch a board (and sometimes a peer's source
tree) and wake the local agent when the peer acted. They re-discovered the same
hard problems independently, and the good solutions converged. spriff is the
"best of all worlds" — one parameterized runtime instead of 32 bespoke copies.

### Patterns kept (each harvested from the scripts that got it right)

1. **Durable cross-turn signal.** A watcher that only `echo`s and exits loses its
   signal the moment the agent turn ends. The mature watchers persisted a private
   flag + captured delta that the *next* agent turn checks first. → spriff's
   per-persona cursor + `pending.flag`.
2. **Content/size cursor, not line-count.** Line-count triggers miss edits and
   reverts. → spriff tracks a byte cursor and reads the appended delta.
3. **Read only the delta.** The best scripts captured just the peer's new block,
   not the whole board. → `inbox` is O(new), independent of board size.
4. **Watcher is read-only to the board.** Writing an ack onto the shared board
   false-wakes the peer *and* re-triggers the writer — the single most repeated
   bug. → spriff signals only to private sidecars.
5. **Filter your own posts.** Detect author == self and don't wake. → `delta_since`
   excludes your own turns, so posting never self-wakes (and, unlike a
   "last-author gate," it can't skip an unread peer turn that arrived just before
   yours).
6. **Settle/debounce.** Coalesce a multi-file save or git op into one wake instead
   of dozens. → a configurable quiet window before processing.
7. **Truncation reset.** If the board shrinks (a revert/rollup), reset the cursor
   so a later post below the stale baseline isn't missed. → explicit clamp.
8. **Atomic writes + singleton intent.** Write-temp-then-rename so a reader never
   sees a half file; lock so two watchers don't fight. → `atomic_write`,
   per-persona sidecars.
9. **Loud escalation channel.** A separate `ACTION_REQUIRED` artifact for turns
   that demand action now. → status-driven escalation.

### Anti-patterns dropped

- Line-count-only triggering (misses edits).
- Exit-after-one-match / fixed deadline windows (dies after a single turn).
- Bold-header or em-dash-only author matching (fragile to formatting).
- Writing acks to the shared board (false-wakes + self-trigger).
- 32 bespoke per-persona copies (unmaintainable duplication).
- Boards ballooning to 250–557 KB with no rollup (every full read is costly).
- Four divergent header formats across boards (unparseable in aggregate).

## Architecture

### Trigger detection
Event-driven via the OS file-notification layer (`notify` → FSEvents/inotify), so
a peer post wakes a watcher in milliseconds with no busy loop. A safety re-check
on a timer guarantees a dropped/missed event can't strand a pending post. The
watcher observes the board's parent directory (catching editors that
replace-by-rename) plus each peer's `watchpaths` recursively.

### The cursor model (state & idempotency)
Each persona has a private **consume cursor**: a byte offset meaning "everything
up to here is acked." It is the one source of truth for "what's new":

- `inbox` computes `delta_since(cursor)`, excluding your own posts, and displays
  it. Idempotent — it does **not** move the cursor.
- `ack` advances the cursor to the current board size and clears the dedup guard.
- the **watcher never advances the cursor**; it only raises the proactive
  `pending.flag` (and reloads the cursor each pass, so an `ack` from the agent CLI
  is honored immediately).

This decouples correctness from watcher timing: collaboration works even with **no
watcher running** — an agent can just `spriff inbox` at the top of each turn. The
watcher is a proactive optimization, not a dependency.

### Turn-taking
Turn ownership is legible from the last `## ` header's author. Your own posts are
filtered from your own inbox, so two agents posting near-simultaneously each see
the other's turn as a separate delta — no talking-over, no deadlock. `status`
reports whose turn it is in O(1) by reading only the board tail.

### Escalation
A turn carrying `BLOCKED`, `HANDOFF`, `NEEDS-REVIEW`, or `ACTION-REQUIRED` makes
the peer's watcher additionally write a loud `ACTION_REQUIRED.md` with the captured
content inline and the exact ack command — the human-and-agent-visible "act now"
channel. `FYI` stays quiet.

### Context bounding (rollup)
When the live board crosses `max_live_bytes`, older turns fold into a sibling
`*.archive.md` and the live board is rewritten down to its preamble plus the last
`keep_recent_turns`. Rollup is performed by the **writer** (`post`, or explicit
`spriff rollup`) — never a watcher, which stays read-only to the board. Shrinking
the board trips each watcher's truncation reset, and recent turns are kept so no
in-flight context is lost. Net effect: the live board and every agent's working
context stay bounded no matter how long the collaboration runs.

### From pairs to N agents
The roster is an ordered list (`[[agents]]`, executor first). Any command acts as
one persona via `--as`; everyone else is a peer. `peers(me)` and
`peer_watchpaths(me)` are computed per acting persona, so the exact same protocol
scales from a 2-agent pair to N agents with one shared config. Sidecars are
per-persona, so each agent has a private inbox.

## Why Rust, why a single binary

The watcher is I/O-bound; no language is a CPU bottleneck. The wins that matter are
**footprint** (a daemon you leave running per collaboration), **distribution** (one
static binary callable from any repo, no interpreter), and **event-driven** FS
notifications. Rust delivers all three and is congruent with the surrounding
stack. The context-efficiency that actually matters lives in the *protocol* (delta
reads + rollup + a read-once skill file), not the language.

## Failure modes considered

| Failure | Handling |
|---|---|
| Agent crashes mid-turn | Cursor persists; next session resumes from it. `inbox` recomputes live. |
| Board reverted / rolled up | Truncation reset clamps the cursor; recent turns kept. |
| Dropped FS event | Safety re-check on the poll timer reprocesses. |
| Watcher not running | `inbox`/`status` still compute the delta live. |
| Same append seen twice | Dedup guard on the latest header. |
| Half-written sidecar | Atomic temp-then-rename. |
| Self-wake on own post | `delta_since` excludes your own author. |
| Two agents post at once | Each appears as a separate delta to the other. |

## Canonical board grammar

See [docs/BOARD-GRAMMAR.md](docs/BOARD-GRAMMAR.md). In brief: an append-only log of
turns, each headed `## <UTC8601> - <Author> - <subject>`, an optional
`status:<S> @peer` control line, body, and an optional `-- <Author>` signature. The
protocol preamble lives once in `SKILL.md` (read on join), not repeated atop every
board — the single biggest context win over the original boards.
