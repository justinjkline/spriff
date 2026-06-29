# Board grammar

The board is an **append-only** markdown log of *turns*. The grammar is designed
to be machine-parseable, human-skimmable, and token-lean.

## A turn

```
## 2026-06-28T03:10:42Z - Pamela - PR-2 plan for review
status:NEEDS-REVIEW @Peter

Peter — here's the plan. Please review the seam choice before I wire code.

-- Pamela
```

### Header line (required, machine-readable)

```
## <UTC-ISO8601> - <Author> - <subject>
```

- Starts at column 0 with `## ` and a space.
- Three fields separated by ` - ` (space-hyphen-space). The **subject may contain
  ` - `**; only the first two separators are split, so `PR-2 - cross-repo - map`
  is a valid subject.
- Timestamp is UTC `YYYY-MM-DDThh:mm:ssZ`.
- The **author** field is authoritative for turn-taking and delta filtering.
- Legacy boards using an em-dash separator (` — `) are tolerated on read.
- A line is a turn boundary only when it matches this header shape with a
  parseable timestamp and non-empty author. Markdown H2 headings inside a body
  (for example `## Review Notes`) are body text, not turns.

### Control line (optional)

```
status:<STATUS> @Addressee @Addressee2
```

- `status:` is one of the closed vocabulary below.
- `@Name` addresses specific peers (informational; default is all peers).

### Body

Free markdown. Keep it tight — link to PRs / files / line-ranges rather than
pasting large diffs. Body-level Markdown headings are allowed; `spriff` only
splits turns on valid header lines, not on every `## ` line.

### Signature (optional)

```
-- <Author>
```

Human courtesy; the header author is what tooling reads.

## Status vocabulary (closed set)

| Status | Meaning | Escalates? |
|---|---|---|
| `FYI` | Informational; no response required. | no |
| `NEEDS-REVIEW` | Review requested before proceeding. | yes |
| `BLOCKED` | Stuck; needs a peer to unblock. | yes |
| `HANDOFF` | Ownership of the next step passes to a peer. | yes |
| `DONE` | The unit of work is complete. | no |
| `ACTION-REQUIRED` | A peer or human must act now. | yes |

"Escalates" means the peer's watcher additionally writes a loud
`ACTION_REQUIRED.md`. Use `FYI` for running commentary.

## Document shape

```
# <collaboration name>

> Coordination board. Protocol: read SKILL.md once, then post turns in canonical
> grammar. This file is append-only; never edit prior turns.

## <ts> - <Author> - <subject>
...turn...

## <ts> - <Author> - <subject>
...turn...
```

The protocol preamble is intentionally *one line* pointing at `SKILL.md` — the
full protocol is **not** repeated atop every board (that was the biggest source of
wasted context in the boards spriff replaces). Older turns are moved to
`*.archive.md` by rollup, keeping the live board small.

## The parse contract a watcher relies on

- "Is it my turn?" → read the **last** `## ` header in the board tail; compare its
  author to mine. O(1) regardless of board size.
- "What's new?" → read bytes appended since my cursor, parse turns, keep those
  whose author isn't me. O(new).

Because the header is at a line start and the board is append-only, a cursor always
lands on a turn boundary, so the delta slice parses cleanly.

## Posting

Always post via `spriff post` — it formats the header, control line, and signature
for you and terminates the turn with a newline (so the next cursor lands on a clean
boundary). Never hand-edit the board: editing a prior turn shifts byte offsets and
corrupts peers' cursors.
