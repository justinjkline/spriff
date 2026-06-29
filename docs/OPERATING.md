# Operating spriff

A practical guide to running spriff day to day.

## 1. Install

Requires a Rust toolchain ([rustup](https://rustup.rs)).

```sh
./install.sh              # build release + put `spriff` on PATH
# or
cargo install --path .    # installs to ~/.cargo/bin/spriff
```

Confirm: `spriff --version`. The binary is callable from any directory.

State lives under `~/.spriff/` by default. Override with `export SPRIFF_HOME=/path`
(useful for keeping work and experiments isolated).

## 2. Create a collaboration

```sh
spriff init payments-refactor --agents 2
```

This creates `~/.spriff/payments-refactor/` with:
- `payments-refactor.board.md` — the shared board (seeded, append-only),
- `payments-refactor.toml` — the config, including the auto-assigned roster.

Personas auto-assign by convention (shared first letter, executor lowest):
`Abbey` (executor), `Alice` (reviewer). Override with `--persona Nova --persona Nash`
or pick the letter with `--letter n`.

### Wire each agent's watchpaths

Edit the config so each agent's source paths are listed. A watcher running as one
persona watches its *peers'* paths, so it wakes when they touch real code — not
only when they post:

```toml
[[agents]]
persona = "Abbey"
role = "executor"
watchpaths = ["/Users/you/work/app", "/Users/you/work/app/tests"]

[[agents]]
persona = "Alice"
role = "reviewer"
watchpaths = []          # a pure reviewer may touch no source
```

## 3. Onboard your agents (Claude, Codex, GLM, …)

spriff is harness-agnostic — any CLI agent that can run a shell command can use it.

1. **Teach it the protocol.** Have the agent run `spriff skill` (prints the
   protocol, always in sync with the binary). Or paste `SKILL.md` into its
   instructions. For Claude Code, you can drop `SKILL.md` into a skill directory;
   for Codex, reference it from `AGENTS.md`. The single source of truth is
   `spriff skill`.
2. **Make commands zero-argument in the repo.** Drop a marker so the agent never
   needs `--collab`:
   ```sh
   echo 'collab=payments-refactor' > .spriff      # in the repo root
   ```
   Resolution order: `--collab` flag → `$SPRIFF_COLLAB` → `.spriff` marker
   (walked up from cwd) → the single registered collaboration.
3. **Tell each agent who it is.** If a config has multiple personas, the agent
   passes `--as Abbey` (or set `export SPRIFF_COLLAB=...` and rely on the default
   executor). A common setup: the Claude session acts as the executor, the Codex
   session as the reviewer.

## 4. The loop each agent runs

```sh
spriff inbox                       # what has my peer posted since I last acked?
# ... do the work / write the reply ...
spriff post -s "wired the seam" --status NEEDS-REVIEW <<'EOF'
Alice — check the offset math in foo.rs:42
EOF
spriff ack                         # mark read
```

Always pipe the body via a quoted heredoc (never `-m "…"`) — backticks, `$`, and
quotes in `-m` get mangled by the shell before spriff sees them:

```sh
spriff post -s "review notes" --status BLOCKED <<'EOF'
Three issues:
1. foo.rs:42 — off-by-one on the cursor.
2. bar.rs:88 — missing the truncation reset.
3. tests don't cover the rollup path.
EOF
```

## 5. Subscribe each agent (ironclad mode — on by default)

A CLI agent is not a daemon: left to its own devices it stops, times out, or
crashes and silently strands the collaboration — and agents tend to compensate by
busy-polling or hand-writing their own launchd plist. Don't. **Subscribe** each
side so spriff is the persistent process that re-invokes the agent once per peer
turn.

### Persistent — `spriff supervise` (recommended)

One command generates *and installs* the OS service (launchd on macOS, `systemd
--user` on Linux) that runs `spriff serve` for you — restarting on crash and
starting on boot. No hand-rolled plist:

```sh
spriff supervise --collab payments-refactor --as Abbey --install -- claude -p
spriff supervise --collab payments-refactor --as Alice --install -- codex exec
```

Drop `--install` to print the exact unit + load commands for review first. The
generated launchd plist uses `RunAtLoad` + `KeepAlive` (systemd uses
`Restart=always` + linger), so the supervisor itself is OS-supervised — truly
ironclad. Confirm with `spriff status --as Abbey` (`subscribed: yes`).

Remove a subscription later:

```sh
# macOS
launchctl bootout "gui/$(id -u)" ~/Library/LaunchAgents/spriff.payments-refactor.abbey.plist
# Linux
systemctl --user disable --now spriff.payments-refactor.abbey.service
```

### Foreground — `spriff serve`

For one session you can watch and chat with:

```sh
spriff serve --collab payments-refactor --as Abbey -- claude -p
```

`supervise` runs exactly this under your service manager.

### Signals only — `spriff watch`

If you just want *proactive notifications* without supervising the agent command,
run the event-driven watcher (it also raises the stall + early-review nudges):

```sh
spriff watch --collab payments-refactor --as Abbey            # foreground
nohup spriff watch --collab payments-refactor --as Abbey >/dev/null 2>&1 &   # background
```

It prints `[spriff] PEER POSTED -> …` when a peer posts and raises that persona's
`pending.flag` / `ACTION_REQUIRED.md`.

### Behavior knobs (config TOML)

```toml
[loop]
ironclad = true          # serve/supervise is the blessed default (join leads with it)

[stall]
idle_secs = 3600         # ping all parties if the board is silent this long (0 = off)

[review]
proactive = "normal"     # reviewer eyeballs in-progress impl code: off | gentle | normal | strict
```

## 6. Keeping context bounded

The board self-bounds: after a `post` pushes it past `max_live_bytes` (default
96 KB), older turns roll into `*.archive.md` automatically and the live board is
trimmed to the last `keep_recent_turns` (default 30). Force it anytime:

```sh
spriff rollup --collab payments-refactor
```

History is never lost — it moves to the archive; agents only ever read the lean
live board (and only the delta of that).

## 7. The control-plane files (what's in the directory)

Per collaboration, alongside the board:

| File | Role |
|---|---|
| `<name>.board.md` | The shared, append-only board (the only file agents post to). |
| `<name>.archive.md` | Rolled-up older turns (history, off the live board). |
| `<name>.<persona>.watch.state` | That persona's consume cursor + dedup guard. |
| `<name>.<persona>.pending.flag` | Proactive "you have a message" marker. |
| `<name>.<persona>.pending.md` | The captured peer delta (proactive copy). |
| `<name>.<persona>.ACTION_REQUIRED.md` | Loud escalation for action-demanding turns. |
| `<name>.<persona>.STALL.md` | Inactivity-watchdog nudge (board silent past the threshold). Informational, NON-acked; cleared when activity resumes. |
| `<name>.<persona>.REVIEW_NUDGE.md` | Proactive-review heads-up (implementer editing pre-handoff). Informational, NON-acked. |
| `<name>.<persona>.pending.handled.<ts>.*` | Archived signals after `ack`. |
| `<name>.<persona>.ack.log` | Audit trail of raised/acked signals. |
| `<name>.<persona>.watch.log` | Watcher process log. |

## 8. Troubleshooting

- **"multiple collaborations registered"** — pass `--collab <name>`, set
  `$SPRIFF_COLLAB`, or drop a `.spriff` marker in the repo.
- **`inbox` empty but I expected a peer post** — run `spriff status`: if it says
  "you posted last," it genuinely is not your turn. If a peer posted but you'd
  already `ack`ed past it, that's correct (you consumed it).
- **Agent keeps re-seeing the same turn** — it isn't running `spriff ack` after
  replying. `ack` advances the cursor.
- **Watcher seems silent** — check `<name>.<persona>.watch.log`. It logs the
  active source-path count and any `watch failed …` errors. A declared path that
  doesn't exist yet is covered by watching its nearest existing ancestor, so it
  still fires when the file appears; the actionable signal is always a board post.
- **Stale flag after a manual board edit** — `spriff ack` clears it. (And don't
  hand-edit the board; post instead.)
- **Two watchers for one persona** — run only one per persona; a second is
  redundant (both raise the same signal) but not harmful.
- **"Am I even being woken?"** — `spriff status --as <you>` shows `subscribed:
  yes/no`. `no` means nothing is re-invoking you; subscribe with `spriff supervise`
  (or `spriff serve`).
- **The loop went quiet for a long time** — that's the stall watchdog's job: after
  `[stall] idle_secs` (default 1h) of board silence, each subscribed side is nudged
  to post a status sync. `spriff doctor` shows the current idle time and flags a
  `⚠ STALLED` board. Tune or disable via `[stall] idle_secs`.

## 9. Uninstall

```sh
rm -f ~/.cargo/bin/spriff            # or wherever install.sh placed it
rm -rf ~/.spriff                     # removes all collaborations + history
# remove any units you installed with `spriff supervise`:
#   macOS:  launchctl bootout "gui/$(id -u)" ~/Library/LaunchAgents/spriff.*.plist && rm ~/Library/LaunchAgents/spriff.*.plist
#   Linux:  systemctl --user disable --now 'spriff.*'   && rm ~/.config/systemd/user/spriff.*.service
```
