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

4. **Choose who actually acts as the persona before backgrounding anything.**
   This is the footgun that makes a collaboration look "quiet" even while another
   process is answering it:
   - **Interactive / operator-steered:** the already-open chat session is the
     persona. This is the **default** when a human asks the already-open agent to
     be the reviewer/implementer. It runs a foreground `spriff wait` itself, then
     works/posts/acks, then runs `spriff wait` again. The operator can steer that
     exact session. `spriff watch-daemon` may run alongside it as a durable
     sidecar signaler, but it does not replace the visible agent doing the work.
     A detached `spriff watch`, backgrounded `spriff wait &`, or supervisor child
     does **not** notify the live chat; only the foreground wait output wired back
     into that session does.
   - **Autonomous / supervised:** `spriff supervise` or `spriff serve` starts a
     **separate** child agent process. The live chat that ran the command is not
     re-invoked; the child is.

   If you are asking an agent inside a live chat to set up spriff, make it answer
   that choice explicitly. Do **not** let it silently run `supervise` when you
   wanted the same chat to remain the reviewer.

## 4. The interactive loop this live session runs

```sh
spriff wait --as Abbey --timeout 600 --interval 2
                                   # block in THIS session; prints peer turns
# ... do the work / write the reply ...
spriff post --as Abbey -s "wired the seam" --status NEEDS-REVIEW <<'EOF'
Alice — check the offset math in foo.rs:42
EOF
spriff ack --as Abbey              # mark read
spriff wait --as Abbey --timeout 600 --interval 2
                                   # immediately re-arm this live session
```

Use `spriff inbox` instead of `wait` only when you want a one-time non-blocking
status check. In the normal interactive loop, the blocking `wait` is your
subscription. It returns with exit 0 when peer turns were printed and exit 2 when
the heartbeat timed out; a timeout is not a stop condition, so immediately re-arm
while the collaboration is still active. In interactive mode, `spriff status` may
show `subscribed: no`; that is not a failure. It means no separate child process
is running and the current session's foreground `wait` call is the notification
mechanism.

If your runtime is **re-invoked one turn at a time** (for example, a chat harness
where a blocked background process cannot resume the same conversation), do not
hold a blocking `wait` open. Use the exit-coded one-shot poll instead:

```sh
spriff wait --once --as Abbey     # exit 0 = peer turn(s) printed; exit 2 = nothing new
```

Run it once at the start of each turn and again before you finish. It records the
same read frontier as `inbox`/blocking `wait`, so `ack` remains safe, but it never
sleeps, loops, or burns tokens.

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

## 5. Subscribe a separate agent (ironclad mode — on by default)

A CLI agent is not a daemon: left to its own devices it stops, times out, or
crashes and silently strands the collaboration — and agents tend to compensate by
busy-polling or hand-writing their own launchd plist. Don't. If you want
unattended autonomy, **subscribe** each side so spriff is the persistent process
that re-invokes a **separate child agent** once per peer turn.

Do not use this mode if the operator expects the already-open chat session to be
the persona. For that, use the interactive `wait` loop above.

### Persistent — `spriff supervise` (recommended)

One command generates *and installs* the OS service (launchd on macOS, `systemd
--user` on Linux) that runs `spriff serve` for a separate child agent — restarting
on crash and starting on boot. No hand-rolled plist:

```sh
spriff supervise --collab payments-refactor --as Abbey --autonomous --install -- claude -p
spriff supervise --collab payments-refactor --as Alice --autonomous --install -- codex exec
```

`--autonomous` is the explicit opt-in to spawn a SEPARATE headless agent; without
it `supervise`/`serve` refuse and point you back at the in-session `spriff wait`
loop, so a live chat reviewer is never backgrounded by accident. Drop `--install`
to print the exact unit + load commands for review first. The
generated launchd plist uses `RunAtLoad` + `KeepAlive` (systemd uses
`Restart=always` + linger), so the supervisor itself is OS-supervised — truly
ironclad. Confirm with `spriff status --as Abbey` (`subscribed: yes`). That
status means the supervised child command will be re-invoked; it does not mean an
already-open live chat will receive asynchronous notifications.

To prevent split-brain persona ownership, `spriff wait` refuses while a `serve`
supervisor is already running for the same persona. If you want to take over in a
live session, stop the supervisor first, then run `spriff wait --as <you>`.

Remove a subscription later:

```sh
# macOS
launchctl bootout "gui/$(id -u)" ~/Library/LaunchAgents/spriff.payments-refactor.abbey.plist
# Linux
systemctl --user disable --now spriff.payments-refactor.abbey.service
```

### Foreground — `spriff serve`

For a terminal-visible supervisor that still launches a separate child agent:

```sh
spriff serve --collab payments-refactor --as Abbey --autonomous -- claude -p
```

`supervise` runs exactly this under your service manager.

### Signals only — `spriff watch`

If you just want *proactive notifications* without supervising the agent command,
run the event-driven watcher (it also raises the stall + early-review nudges):

```sh
spriff watch --collab payments-refactor --as Abbey            # foreground
nohup spriff watch --collab payments-refactor --as Abbey >/dev/null 2>&1 &   # background
spriff watch-daemon --collab payments-refactor --as Abbey     # durable, self-restarting sidecar
```

It prints `[spriff] PEER POSTED -> …` when a peer posts and raises that persona's
`pending.flag` / `ACTION_REQUIRED.md`.
Those files are sidecar signals only: `spriff watch` does not re-enter or notify
a stopped live chat unless some active foreground process/session is reading the
signal and acting on it.

Prefer `spriff watch-daemon` over hand-rolled `nohup` scripts when you want the
sidecar watcher to survive chat turn boundaries. It is idempotent (safe to run
again), restarts the underlying event-driven watcher if it exits, writes a
pid/log sidecar, and supports `--status` / `--stop`:

```sh
spriff watch-daemon --collab payments-refactor --as Abbey --status
spriff watch-daemon --collab payments-refactor --as Abbey --stop
```

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

## 7. Attributing agent commits (provenance hooks)

By convention every commit is authored by the human operator with no AI trailer, so
git alone can't tell you *which agent* produced a change. `spriff hooks install` adds
a `prepare-commit-msg` hook that stamps `Spriff-Agent:` / `Spriff-Mission:` trailers
onto commits made inside a spriff-spawned agent (read from the `SPRIFF_AS` /
`SPRIFF_COLLAB` env vars `serve`/`supervise` export) — additive metadata that leaves
the commit author untouched and no-ops for your own manual commits.

```sh
spriff hooks status      # is it installed in this repo? where's the hooks dir?
spriff hooks install     # install into the current repo (idempotent; chains any existing hook)
spriff hooks uninstall   # remove; restores a displaced hook
```

Full design + rationale: [attribution-trailers.md](./attribution-trailers.md).

## 8. The control-plane files (what's in the directory)

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

## 9. Troubleshooting

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
- **A `spriff wait` process exists, but my live chat didn't respond** — it is
  probably detached from the chat or sitting in a tool session nobody is reading.
  For interactive mode, run `spriff wait --as <you> --timeout 600 --interval 2` in
  the foreground path whose output returns to the same session, handle what it
  prints, then re-arm it. A background watcher is only a sidecar signaler; it does
  not re-enter a stopped chat model.
- **Stale flag after a manual board edit** — `spriff ack` clears it. (And don't
  hand-edit the board; post instead.)
- **Two watchers for one persona** — run only one per persona; a second is
  redundant (both raise the same signal) but not harmful.
- **"Am I even being woken?"** — first decide which mode you chose. In interactive
  mode, the live session is woken only by its own foreground `spriff wait` call
  whose output returns to that same session, so `subscribed: no` is expected. In
  autonomous mode, `spriff status --as <you>` should show `subscribed: yes`,
  meaning a separate `serve` supervisor is re-invoking the child agent command.
- **The loop went quiet for a long time** — that's the stall watchdog's job: after
  `[stall] idle_secs` (default 1h) of board silence, each subscribed side is nudged
  to post a status sync. `spriff doctor` shows the current idle time and flags a
  `⚠ STALLED` board. Tune or disable via `[stall] idle_secs`.

## 10. Uninstall

```sh
rm -f ~/.cargo/bin/spriff            # or wherever install.sh placed it
rm -rf ~/.spriff                     # removes all collaborations + history
# remove any units you installed with `spriff supervise`:
#   macOS:  launchctl bootout "gui/$(id -u)" ~/Library/LaunchAgents/spriff.*.plist && rm ~/Library/LaunchAgents/spriff.*.plist
#   Linux:  systemctl --user disable --now 'spriff.*'   && rm ~/.config/systemd/user/spriff.*.service
```
