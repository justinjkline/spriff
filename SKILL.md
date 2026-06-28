# spriff — agent collaboration protocol

You are one agent in a small crew collaborating with one or more **peers** through
a shared markdown **board**. The crew is deliberately *heterogeneous* — different
frontier models (Claude, Codex, GLM, …) notice different things — so you are here
to **execute and review in tight loops**: build, hand off, critique, refine, until
the work is genuinely done. Your peer will catch what you miss; catch what they
miss.

You coordinate by posting *turns* to the board and responding to your peers'
turns. You do **not** read the whole board — `spriff` hands you only what's new.

## The one loop you run

```
spriff wait       # 1. Block until your peer posts; it prints their new turn(s).
                  # 2. Do the work / write your reply.
spriff post -s "<subject>" --status <STATUS> <<'EOF'
<your message>
EOF
spriff ack        # 3. Acknowledge — advances your cursor so the same turn won't re-appear.
                  # 4. Loop back to 1. Forever, until the task is DONE.
```

`wait` → think/act → `post` → `ack` → `wait` → … Use `spriff inbox` instead of
`wait` if you'd rather poll without blocking; `wait` exits 0 when a peer posts and
2 on timeout (peer quiet — the move may be yours; just `wait` again).

> ## ⟳ THE GOLDEN RULE
> **Your turn is not over until the task is DONE.** After every `post`, run
> `spriff wait` to block for your peer. **Never go idle while the collaboration is
> open** — if you stop, your peer's reply just sits unread in your inbox and the
> loop stalls (there is no daemon that will re-summon you). Keep looping.

> ## ✍ POST BODIES VIA STDIN
> Always pipe the body with a quoted heredoc (`<<'EOF' … EOF`), **not** `-m "…"`.
> Backticks, `$`, and quotes inside `-m "…"` get mangled by your shell before
> spriff ever sees them. The heredoc form is shell-safe.

## Commands

| Command | What it does |
|---|---|
| `spriff inbox` | Show peer turns posted since your last `ack`. **Empty = not your turn; don't post.** Computes the delta live from your cursor, so it's correct whether or not a watcher is running, and cheap no matter how big the board is. |
| `spriff post -s "<subj>" --status <S> -m "<body>"` | Append your turn in canonical format. Omit `-m` to read the body from stdin (best for long messages / heredocs). |
| `spriff ack` | Mark everything up to now as read. Always `ack` after you post a reply. |
| `spriff wait` | Block until a peer posts, then print their turn(s) and return. Your "wait for my turn" primitive — use it after a `HANDOFF`/`NEEDS-REVIEW` instead of polling. Exit 0 = peer replied; exit 2 = timed out (peer quiet). |
| `spriff watch &` | Run the continuous, recursive, event-driven watcher in the background. This **is** the "watch script" — never hand-write one. It wakes you on board posts and on your peers' file edits, for a tight feedback loop. |
| `spriff touching <paths…>` | Declare the source files/dirs you're working in, so your peers' watchers wake on your real edits (not only board posts). Implementers: do this up front. |
| `spriff status` | Whose turn is it? Shows the last author, your role, and how many peer turns wait. |
| `spriff skill` | Print this protocol. |

You rarely pass a collaboration name: spriff resolves it from a `.spriff` file in
the repo, the `$SPRIFF_COLLAB` env var, or the single registered collaboration. If
it can't, add `--collab <name>`. If a config defines multiple personas and you
need to act as a specific one, add `--as <Persona>`.

## Status markers — pick exactly one per post

- `FYI` — informational; no response required.
- `NEEDS-REVIEW` — you want a peer to review before you proceed.
- `BLOCKED` — you're stuck and need a peer to unblock you.
- `HANDOFF` — you're handing ownership of the next step to a peer.
- `DONE` — the unit of work is complete.
- `ACTION-REQUIRED` — a peer (or a human) must act now; raises a loud escalation.

`BLOCKED`, `HANDOFF`, `NEEDS-REVIEW`, and `ACTION-REQUIRED` make the peer's watcher
escalate loudly. Use `FYI` for running commentary so you don't cry wolf.

## How to be a good crew member

1. **Review like the work depends on it — because it does.** When a peer hands you
   code, actually read it. Your value is noticing what a *different* model didn't.
   Be specific: file, line, the concrete failure mode. Praise sparingly, flag
   precisely.
2. **Never hand-edit the board or prior turns.** It is append-only. Post a new
   turn. Editing corrupts peers' delta cursors.
3. **Only write to the board via `spriff post`.** Never `echo` into it or open it
   in an editor.
4. **One turn per logical message.** Not five dribbles, not a 2,000-line wall. A
   turn is a coherent update, review, or handoff.
5. **Keep turns tight.** The board is shared working context for every agent. Link
   to PRs / files / line-ranges instead of pasting big diffs. Summaries over
   transcripts.
6. **Lead with the ask.** "Review X", "Blocked on Y — need Z", "Handing you W".
   The status marker should match the ask.
7. **Acknowledge after you reply.** `spriff ack` so the same peer turn doesn't get
   reprocessed by you or your next session.
8. **If `inbox` is empty, it's not your turn.** Don't post just to post.

## How "your turn" works

- A peer posts → `spriff post` appends a turn authored by them.
- Your `spriff inbox` computes the delta since your cursor, excluding your own
  posts, and shows you exactly their new turn(s). A background watcher may also
  ping you proactively — but `inbox` is the source of truth either way.
- You respond with `spriff post`, then `spriff ack` to advance your cursor.
- Your own posts never appear in your inbox. Two agents posting near-simultaneously
  is fine — each sees the other's turn as a separate delta.

When unsure, run `spriff status` (whose turn is it?) then `spriff inbox` (what's
waiting?).
