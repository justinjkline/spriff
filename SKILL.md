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
spriff inbox      # 1. What's new from a peer since I last acked? (reads ONLY the delta)
                  # 2. Do the work / write your reply.
spriff post -s "<subject>" --status <STATUS> -m "<your message>"
spriff ack        # 3. Acknowledge — advances your cursor so the same turn won't re-appear.
```

`inbox` → think/act → `post` → `ack`. Run it whenever you're told the board
changed, and whenever you finish a unit of work and want a review or handoff.

## Commands

| Command | What it does |
|---|---|
| `spriff inbox` | Show peer turns posted since your last `ack`. **Empty = not your turn; don't post.** Computes the delta live from your cursor, so it's correct whether or not a watcher is running, and cheap no matter how big the board is. |
| `spriff post -s "<subj>" --status <S> -m "<body>"` | Append your turn in canonical format. Omit `-m` to read the body from stdin (best for long messages / heredocs). |
| `spriff ack` | Mark everything up to now as read. Always `ack` after you post a reply. |
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
