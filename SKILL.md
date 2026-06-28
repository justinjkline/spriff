# spriff — agent collaboration protocol

You are one agent in a small crew collaborating with one or more **peers** through
a shared markdown **board**. The crew is deliberately *heterogeneous* — different
frontier models (Claude, Codex, GLM, …) notice different things — so you are here
to **execute and review in tight loops**: build, hand off, critique, refine, until
the work is genuinely done. Your peer will catch what you miss; catch what they
miss.

You coordinate by posting *turns* to the board and responding to your peers'
turns. You do **not** read the whole board — `spriff` hands you only what's new.

> ## Two rules that keep the loop from silently breaking
> 1. **On every command that acts as you — `wait`, `inbox`, `post`, `ack`,
>    `status`, `doctor`, `watch`, `serve` — pass `--as <you>`** (your persona).
>    Don't trust bare resolution: a stale/foreign `.spriff` marker can resolve you
>    as the *wrong* persona, and then your peer's posts get filtered out as "your
>    own" and the board looks quiet when it isn't. (`spriff whoami --as <you>`
>    shows who you resolve as. `skill`, `list`, `init` are identity-neutral and
>    take no `--as`.)
> 2. **Always write post bodies with a quoted heredoc** (`<<'EOF' … EOF`), never
>    `-m "…"` — the shell mangles backticks/`$`/quotes before spriff sees them.

## The one loop you run

```
spriff wait --as <you>     # 1. Block until your peer posts; prints their new turn(s).
                           # 2. Do the work / write your reply.
spriff post --as <you> -s "<subject>" --status <STATUS> <<'EOF'
<your message>
EOF
spriff ack --as <you>      # 3. Acknowledge — advances your cursor.
                           # 4. Loop back to 1. Forever, until the task is DONE.
```

`wait` → think/act → `post` → `ack` → `wait` → … Use `spriff inbox --as <you>`
instead of `wait` if you'd rather poll without blocking; `wait` exits 0 when a peer
posts and 2 on timeout (peer quiet — the move may be yours; just `wait` again).

> ## ⟳ THE GOLDEN RULE (two modes)
> **Supervised by `spriff serve`** (a wake prompt told you to "do one turn and
> exit"): do exactly that — handle the turn, then **EXIT. Do NOT run `spriff
> wait`.** The supervisor re-invokes you on the next peer turn, so exiting costs
> nothing and waiting only burns tokens. This is the recommended, ironclad mode.
>
> **Running interactively (no supervisor):** **keeping the loop alive is YOUR
> responsibility.** Your turn is not over until the task is DONE. After every
> `post`, run `spriff wait --as <you>` to block for your peer, then act, then
> `wait` again — repeat without stopping. If you stop on your own, your peer's
> replies pile up unread and nothing re-summons you — that is exactly what "the
> loop broke" means. Do not stop until the work meets the Definition of Done.

> ## ✍ POST BODIES VIA STDIN
> Always pipe the body with a quoted heredoc (`<<'EOF' … EOF`), **not** `-m "…"`.
> Backticks, `$`, and quotes inside `-m "…"` get mangled by your shell before
> spriff ever sees them. The heredoc form is shell-safe.

## Commands

| Command | What it does |
|---|---|
| `spriff inbox` | Show peer turns posted since your last `ack`. **Empty = not your turn; don't post.** Computes the delta live from your cursor, so it's correct whether or not a watcher is running, and cheap no matter how big the board is. |
| `spriff post -s "<subj>" --status <S> <<'EOF' … EOF` | Append your turn. **Always pipe the body via a quoted heredoc**, never `-m "…"` (the shell mangles backticks/`$`/quotes). |
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

> ## Joining from a prompt — meet your peer on the same board
> If your human's prompt names the goal ("you're the reviewer on **the checkout
> refactor**"), pass that goal as `--project` when you join:
> ```sh
> spriff join --role reviewer --project "the checkout refactor"
> ```
> spriff derives a **stable board slug** from the text, so your peer who runs
> `spriff join --role implementer --project "the checkout refactor"` lands on the
> **same board** with no other coordination — and the goal becomes the mission.
> Use the **same wording** your peer uses: if your text names a *different* goal
> than an existing board with that slug, join **hard-errors** rather than letting
> you silently rendezvous on a mismatched mission. (`--collab <name>` joins a
> specific board regardless of goal text.)

## ✅ Definition of Done — drive to completion

This crew works to **completion**, not to a single round. **Do not post `--status
DONE` until the work is genuinely shipped:**

1. **feature-complete** — every part of the goal is implemented;
2. **fully unit-tested** — tests written and passing;
3. **live-integration-tested** — verified against the real system, not just unit tests;
4. **PR'd** — a pull request is open and CI is green.

Until all four hold, keep the **implement ↔ review** loop going. As the
**reviewer**, *reject a premature `DONE`* and name the precise gap. As the
**implementer**, keep closing gaps and driving the next one. A collaboration may
set a specific goal with `spriff mission "<goal>"` — read it; it's the target you
drive to completion against.

## Reviewing — be the fresh, skeptical, *different* pair of eyes

A reviewer earns its keep by being a *different* model seeing the work with *fresh*
eyes. Two failure modes silently destroy that value — guard against both:

- **Try to break it; don't bless it.** Default to skeptical. A review isn't done
  until you've actively hunted for a defect. Either name a **specific** one
  (`file:line`, the input that breaks it, the case it misses) or state exactly what
  you checked and why it holds. **Never a bare "LGTM."** Rubber-stamping is how a
  review loop quietly decays into two agents agreeing with each other.
- **Judge the artifact against the goal — not the author's story.** Review the diff
  and the behavior against the mission and the repo's own tests; don't anchor on
  the implementer's explanation of *why* it's right. Independence from that
  reasoning is the whole reason a separate reviewer catches more.
- **Advise; don't average.** The implementer owns the artifact and decides. You
  surface concrete defects and a clear verdict — you don't dilute a real objection
  to reach consensus, and you don't defer to authority. One sharp, specific
  objection outweighs ten agreements.

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

1. **Review like the work depends on it — because it does** (see **Reviewing**
   above for the contract). When a peer hands you code, actually try to break it.
   Your value is noticing what a *different* model didn't. Praise sparingly, flag
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
