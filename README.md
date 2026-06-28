<div align="center">

<img src="assets/spriff-logo.png" alt="spriff — where agents riff to done" width="460">

Tight execute↔review loops between heterogeneous frontier coding agents,
over a shared board, with durable cross-turn signaling.

[![CI](https://github.com/justinjkline/spriff/actions/workflows/ci.yml/badge.svg)](https://github.com/justinjkline/spriff/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org)
[![PRs welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](CONTRIBUTING.md)

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

## Why different model classes win — the evidence

The intuition above rests on a well-established result: **ensembling is close to a
free lunch when the members' errors are uncorrelated — and errors decorrelate most
when the models are genuinely different**, with different pretraining corpora,
architectures, and alignment. A model's confident mistakes are correlated with its
own training; a peer from a *different class* doesn't share those priors, so it
catches what the first is systematically blind to. That is the classic
ensemble-diversity principle — error decorrelation drives the gain — applied to
frontier LLMs.

The empirical thread runs from weak to strong forms of diversity:

- **Diversity within a single model already helps.** Sampling multiple reasoning
  paths and taking the consensus — *self-consistency* — lifts accuracy
  substantially (e.g. **+17.9%** on GSM8K).
  [Wang et al., 2022](https://arxiv.org/abs/2203.11171)
- **Multiple instances debating beat one.** Several model instances proposing and
  critiquing over rounds improve factuality and reasoning and cut hallucination.
  [Du et al., 2023](https://arxiv.org/abs/2305.14325)
- **The best model differs per problem.** Ensembling diverse LLMs via ranking and
  fusion beats any single one, precisely because "the optimal LLM can vary
  significantly per example."
  [LLM-Blender — Jiang et al., 2023](https://arxiv.org/abs/2306.02561)
- **A mix of lesser models can beat a bigger single one.** A layered
  *Mixture-of-Agents* of open models scored **65.1%** on AlpacaEval 2.0,
  **surpassing GPT-4 Omni's 57.5%** — combination beating a single frontier model.
  [Wang et al., 2024](https://arxiv.org/abs/2406.04692)
- **The idea is moving into code.** Ensemble methods are now actively studied for
  code generation and repair specifically — with both promising results and honest
  caveats.
  [Survey — Ashiga et al., 2025](https://arxiv.org/abs/2503.13505) ·
  [Wisdom and Delusion of LLM Ensembles for Code, 2025](https://arxiv.org/abs/2510.21513)

**Where spriff is different.** Most of that work ensembles *outputs* — sample,
rank, vote, or fuse after the fact — or runs symmetric debate. spriff applies the
same diversity principle to *agentic coding*, as a tight, role-asymmetric
**execute↔review loop** between different model classes: the reviewer's different
priors catch the implementer's blind spots **in flight**, turn by turn, and the
loop runs to a real Definition of Done rather than a single shot.

**We intend to measure this, not just assert it.** spriff is being evaluated on
**[SWE-Bench Pro](https://github.com/scaleapi/SWE-bench_Pro-os)** (real GitHub
issues, hidden test suites) under a controlled design — **Claude Opus 4.8 ⇄
GPT-5.5** against each model solo *and* a same-model loop — so any lift is
attributable to *heterogeneity* rather than extra compute alone. Results will be
published here. If the data disagrees, that goes here too.

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
git clone https://github.com/justinjkline/spriff
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

There's nothing to configure. Tell each agent its role and to use spriff — it
self-onboards with one command:

```sh
# In the implementer's session / repo:
spriff join --role implementer
#   → "You are Abbey — the implementer on collaboration 'default'." + the protocol.

# In the reviewer's session / repo (even a different clone):
spriff join --role reviewer
#   → "You are Alice — the reviewer on collaboration 'default'." + the protocol.
```

`join` creates the collaboration if needed, claims the right persona, and writes a
`.spriff` marker so every later command needs **no flags**. Each agent then runs
the loop:

```sh
spriff inbox          # what's new from my peer?
spriff post -s "wired the seam" --status NEEDS-REVIEW <<'EOF'
review the offset math in foo.rs:42
EOF
spriff ack            # mark read
spriff wait           # block until my peer replies, then loop
```

That's the whole thing. To run several collaborations at once, name them:
`spriff join --role implementer --collab checkout-refactor`.

### Ironclad mode — agents that can't go idle

A CLI agent isn't a daemon: left to loop on `spriff wait` it can stop, hit a turn
limit, or crash and silently strand the collaboration. `spriff serve` fixes that —
**spriff** becomes the persistent process and **re-invokes your agent for one turn
every time a peer posts**:

```sh
# Supervise each side with a headless agent invocation (spriff appends a wake prompt):
spriff serve --as Pamela -- claude -p          # implementer, driven by Claude
spriff serve --as Peter  -- codex exec         # reviewer, driven by Codex
```

A dead agent is just re-spawned on the next peer turn. Put each `spriff serve`
under launchd/systemd and the loop runs unattended for as long as you like. See
[docs/OPERATING.md](docs/OPERATING.md).

## Persona convention

Agents in a collaboration share a **first letter** and are named **alphabetically
by role** — executor lowest, reviewers ascending — so who's-who is legible at a
glance, and different collaborations get different letters:

| Collaboration | Roster |
|---|---|
| `checkout-refactor` | **Abbey** (executor) · Alice · Annie |
| `billing-audit`     | **Bailey** (executor) · Beck |

Bring your own cast: `spriff join --role implementer --as Pamela --with Peter`
(or `spriff init mytask --persona Nova --persona Nash`).

## Command reference

| Command | Purpose |
|---|---|
| `spriff join --role implementer\|reviewer` | **Agent entry point.** Auto-create/join, claim persona, write marker, print protocol + first move. |
| `spriff init <name> [--agents N] [--letter X] [--persona …]` | Create + register a collaboration explicitly. |
| `spriff list` | List registered collaborations and rosters. |
| `spriff skill` | Print the agent protocol (onboard any CLI agent). |
| `spriff serve [--as P] -- <agent-cmd>` | **Ironclad loop.** Supervise an agent: re-invoke it for one turn on every peer post, surviving agent stop/crash/timeout. |
| `spriff watch [--as P]` | Run the event-driven watcher (proactive wakeups). |
| `spriff inbox [--as P]` | Show the peer delta since your cursor. |
| `spriff wait [--as P]` | Block until a peer posts, then print their turn (agent "wait for my turn" primitive). |
| `spriff post -s … --status … <<'EOF' … EOF` | Append a turn (pipe the body via heredoc). |
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
