# Migrating from hand-rolled board watchers

spriff replaces the family of bespoke `watch-*.sh` / `watch-*.py` scripts (one per
persona per topic) with a single config-driven runtime. This maps the old concepts
onto spriff.

## Concept mapping

| Hand-rolled script | spriff |
|---|---|
| One `watch-<topic>-<persona>.sh` per persona per topic | One `spriff` binary + one `<name>.toml` per collaboration |
| Hard-coded board path | `board = "…"` in the config |
| `wc -l` / `shasum` board poll | Event-driven watcher + byte cursor |
| `echo`-and-exit signal | Durable per-persona cursor + `pending.flag` |
| Private `*.pending.md` / `*.pending.flag` | `<name>.<persona>.pending.*` (same idea, derived deterministically) |
| `*.watch.state` baseline | `<name>.<persona>.watch.state` cursor |
| `*.ACTION_REQUIRED.md` | `<name>.<persona>.ACTION_REQUIRED.md` (status-driven) |
| `*.pending.handled.<ts>.*` archive | same, written by `spriff ack` |
| `<persona>.watchpaths` file | `watchpaths = [...]` per `[[agents]]` |
| `--ack-pending` subcommand | `spriff ack` |
| `--status` / `--once` | `spriff status` |
| launchd `--ensure-launchd` self-heal | a launchd/systemd unit (see OPERATING.md) |
| Manual author-header matching (`Eloise\*\*`, em-dash) | canonical `## <ts> - <Author> - <subj>` (em-dash tolerated on read) |

## Porting an existing board

An old board with `## <ts> - <Author> - <subj>` headers is already compatible —
spriff's parser reads it (and tolerates the legacy ` — ` em-dash separator). To
adopt spriff for an in-flight collaboration:

1. `spriff init <name> --persona <Executor> --persona <Reviewer> --board /path/to/existing-board.md`
   (point `--board` at the existing file instead of letting spriff seed a new one).
2. Add each agent's `watchpaths` to the config.
3. Each agent runs `spriff inbox` — its cursor starts at 0, so it's caught up on
   the current live board; `ack` once to move to the head, then run the loop.
4. Optionally start a watcher per agent for proactive wakeups.

## What you stop maintaining

- 32 near-duplicate scripts → 1 binary.
- Per-script quoting/race bugs → one tested runtime (`cargo test`).
- Drifting copy-pasted board preambles → one `SKILL.md` (`spriff skill`).
- Manual baseline/state bookkeeping → the cursor model.
