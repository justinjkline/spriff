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
spriff post -s "wired the seam" --status NEEDS-REVIEW -m "Alice — check the offset math in foo.rs:42"
spriff ack                         # mark read
```

Long messages: omit `-m` and pipe the body via stdin / heredoc:

```sh
spriff post -s "review notes" --status BLOCKED <<'EOF'
Three issues:
1. foo.rs:42 — off-by-one on the cursor.
2. bar.rs:88 — missing the truncation reset.
3. tests don't cover the rollup path.
EOF
```

## 5. Proactive wakeups (the watcher)

`inbox` works with no watcher running. To get *proactive* notifications (so an
agent is told the moment its peer posts), run one watcher per agent.

### Foreground (simplest)

```sh
spriff watch --collab payments-refactor --as Abbey
```

It prints `[spriff] PEER POSTED -> …` when a peer posts and raises that persona's
`pending.flag` / `ACTION_REQUIRED.md`.

### Background

```sh
nohup spriff watch --collab payments-refactor --as Abbey >/dev/null 2>&1 &
```

### Supervised on macOS (launchd)

Create `~/Library/LaunchAgents/dev.spriff.payments-abbey.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>dev.spriff.payments-abbey</string>
  <key>ProgramArguments</key>
  <array>
    <string>/Users/you/.cargo/bin/spriff</string>
    <string>watch</string><string>--collab</string><string>payments-refactor</string>
    <string>--as</string><string>Abbey</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>10</integer>
</dict></plist>
```

```sh
launchctl bootstrap "gui/$(id -u)" ~/Library/LaunchAgents/dev.spriff.payments-abbey.plist
launchctl kickstart -k "gui/$(id -u)/dev.spriff.payments-abbey"
```

### Supervised on Linux (systemd --user)

`~/.config/systemd/user/spriff-payments-abbey.service`:

```ini
[Unit]
Description=spriff watcher (payments-refactor / Abbey)
[Service]
ExecStart=%h/.cargo/bin/spriff watch --collab payments-refactor --as Abbey
Restart=always
RestartSec=5
[Install]
WantedBy=default.target
```

```sh
systemctl --user enable --now spriff-payments-abbey.service
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
- **Watcher seems silent** — check `<name>.<persona>.watch.log`. Ensure the
  peer's `watchpaths` exist; non-existent paths are skipped.
- **Stale flag after a manual board edit** — `spriff ack` clears it. (And don't
  hand-edit the board; post instead.)
- **Two watchers for one persona** — run only one per persona; a second is
  redundant (both raise the same signal) but not harmful.

## 9. Uninstall

```sh
rm -f ~/.cargo/bin/spriff            # or wherever install.sh placed it
rm -rf ~/.spriff                     # removes all collaborations + history
# remove any launchd/systemd units you created
```
