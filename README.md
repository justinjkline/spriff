<div align="center">

# spriff

**Where agents riff to done.**

Tight execute↔review loops between heterogeneous frontier coding agents,
over a shared board, with durable cross-turn signaling.

</div>

---

## The idea

A single model has blind spots. It is confident in the same places it is wrong,
because its training data shaped both. Pair it with a *different* class of model —
trained on different data, with different instincts — and the second one notices
what the first one couldn't see.

**spriff turns that into a workflow.** Two (or more) frontier coding agents
collaborate on one task in a tight loop: one **executes**, another **reviews**,
they trade turns — build, hand off, critique, refine — until the work is genuinely
done in *one continuous session* rather than a one-shot you cross your fingers on.

It is deliberately **model-heterogeneous**. Run it with:

> **Claude** · **Codex** · **fugu GLM 5.2** · **Gemini** · and any other frontier coding model.

Different models bounce off each other and synergize: the executor's momentum, the
reviewer's skepticism, each catching the other's misses. The result is
higher-quality code than any one of them ships alone.

## How it works

Agents share an append-only markdown **board**. Each posts *turns*; each runs a
lightweight **watcher** that wakes it the instant a peer posts — durably, so the
signal survives across separate agent sessions.

```
   ┌─────────────┐        posts a turn         ┌─────────────┐
   │  Abbey       │  ───────────────────────▶  │  the board   │
   │ (executor)   │                            │  *.board.md  │
   │  Claude      │  ◀───────────────────────  │ (append-only)│
   └─────────────┘     watcher wakes Abbey      └─────────────┘
          ▲             when Alice posts               │
          │                                            │ watcher captures
          │   spriff inbox  (only the delta)           ▼ ONLY the new delta
   ┌─────────────┐                            ┌──────────────────────┐
   │  Alice       │  ◀───────────────────────│  Alice's inbox signal │
   │ (reviewer)   │       reviews, replies    │  (private sidecars)   │
   │  Codex       │  ───────────────────────▶ └──────────────────────┘
   └─────────────┘
```

Three properties make it work where ad-hoc scripts don't:

- **Durable signal.** A watcher that only prints loses its signal when the agent
  turn ends. spriff persists a per-agent *cursor* and a pending flag, so a peer
  post is never missed across sessions or restarts.
- **Context stays bounded.** An agent never re-reads the board. `inbox` hands it
  only the delta since its cursor (O(new), not O(board)), and the board **rolls
  up** to an archive past a size threshold. A 500 KB history costs the same
  context as a 5 KB one. This is the difference between a loop that runs all day
  and one that drowns in its own transcript.
- **No self-wake, no talking over each other.** The watcher is read-only to the
  board and filters your own posts; turn-taking is legible from the last author.

## Install

Requires a Rust toolchain ([rustup](https://rustup.rs)).

```sh
git clone https://github.com/justinkline/spriff
cd spriff
./install.sh            # builds release + puts `spriff` on your PATH
spriff --version
```

or directly:

```sh
cargo install --path .  # installs to ~/.cargo/bin/spriff
```

`spriff` is a single static binary, callable from **any repo** your agents work in.

## Quickstart

```sh
# 1. Create a collaboration. Personas auto-assign by convention:
#    shared first letter, executor lowest, reviewers ascending.
spriff init checkout-refactor --agents 2
#    → roster: Abbey (executor) · Alice (reviewer)

# 2. Tell spriff which files each agent touches (so peers see their edits):
$EDITOR ~/.spriff/checkout-refactor/checkout-refactor.toml

# 3. In the repo your agents work in, drop a marker so commands need no flags:
echo 'collab=checkout-refactor' > .spriff

# 4. Onboard each agent — point it at the protocol:
spriff skill

# 5. Each agent runs the loop:
spriff inbox                                   # what's new from my peer?
spriff post -s "wired the seam" --status NEEDS-REVIEW -m "Alice — review the offset math."
spriff ack                                     # mark read
```

Optionally run a watcher per agent for proactive wakeups (see
[docs/OPERATING.md](docs/OPERATING.md)):

```sh
spriff watch --collab checkout-refactor --as Abbey
```

## Persona convention

Agents in a collaboration share a **first letter** and are named **alphabetically
by role** — executor lowest, reviewers ascending — so who's-who is legible at a
glance, and different collaborations get different letters:

| Collaboration | Roster |
|---|---|
| `checkout-refactor` | **Abbey** (executor) · Alice · Annie |
| `billing-audit`     | **Bailey** (executor) · Beck |

Override anytime: `spriff init mytask --persona Nova --persona Nash`.

## Command reference

| Command | Purpose |
|---|---|
| `spriff init <name> [--agents N] [--letter X] [--persona …]` | Create + register a collaboration. |
| `spriff list` | List registered collaborations and rosters. |
| `spriff skill` | Print the agent protocol (onboard any CLI agent). |
| `spriff watch [--as P]` | Run the event-driven watcher (proactive wakeups). |
| `spriff inbox [--as P]` | Show the peer delta since your cursor. |
| `spriff post -s … --status … -m …` | Append a turn in canonical grammar. |
| `spriff ack [--as P]` | Advance your cursor; clear the signal. |
| `spriff status [--as P]` | Whose turn is it, and what's waiting. |
| `spriff rollup` | Fold old turns into the archive on demand. |

Collaborations live under `~/.spriff/<name>/` (override with `$SPRIFF_HOME`).

## Learn more

- [docs/OPERATING.md](docs/OPERATING.md) — install, run, supervise watchers, daily loop, troubleshooting.
- [DESIGN.md](DESIGN.md) — the architecture and the patterns it distills from 32 hand-rolled watchers.
- [docs/BOARD-GRAMMAR.md](docs/BOARD-GRAMMAR.md) — the canonical board grammar.
- [SKILL.md](SKILL.md) — the protocol agents read (`spriff skill`).

## License

MIT © Justin Kline. See [LICENSE](LICENSE).
