//! spriff — durable, event-driven coordination for fleets of collaborating
//! AI agents over a shared markdown board.
//!
//! One globally-installed binary, addressable by collaboration NAME from inside
//! any repo. See README.md for the story and SKILL.md (also printed by
//! `spriff skill`) for the agent-facing protocol.

mod board;
mod config;
mod names;
mod paths;
mod pending;
mod registry;
mod state;
mod util;
mod watcher;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use config::Config;
use paths::Sidecars;
use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// The agent-facing protocol, embedded so `spriff skill` is always in sync
/// with the installed binary — one source of truth, reachable identically from
/// every CLI agent (Claude, Codex, …). No copy-pasted, drifting preambles.
const SKILL: &str = include_str!("../SKILL.md");

/// The always-on bar for declaring a collaboration's work DONE. Injected into the
/// supervisor's wake prompts and documented in SKILL.md so the crew keeps the
/// implement↔review loop going until the work is genuinely shipped.
const DEFINITION_OF_DONE: &str = "feature-complete, fully unit-tested, live-integration-tested, and PR'd (a pull request is open and CI is green)";

#[derive(Parser)]
#[command(
    name = "spriff",
    version,
    arg_required_else_help = true,
    about = "Durable, event-driven coordination for collaborating AI agents over a shared markdown board.",
    long_about = "spriff coordinates a crew of AI coding agents in tight execute<->review loops \
over a shared board.\n\nAGENTS START HERE: run `spriff join --role implementer` or \
`spriff join --role reviewer`. That one command sets up everything and prints the protocol."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// ⭐ Onboard yourself as an agent. Auto-creates/joins the collaboration,
    /// claims your persona (implementer = executor, reviewer = first reviewer),
    /// writes a repo marker so later commands need no flags, and prints the
    /// protocol + your first move. The one command an agent runs to start.
    Join {
        /// Your role: implementer (executor) or reviewer.
        #[arg(long)]
        role: String,
        /// Your persona name (e.g. Pamela). Defaults to the auto-assigned name.
        #[arg(long = "as")]
        as_name: Option<String>,
        /// Your peer's persona name (e.g. Peter), used when creating the roster.
        #[arg(long = "with")]
        with: Option<String>,
        /// The project/goal from your prompt (e.g. "fix the checkout flow"). spriff
        /// derives a STABLE board slug from it, so a peer who passes the same
        /// --project lands on the same board with zero coordination, and the text
        /// becomes the collaboration's mission. Beats relying on "default".
        #[arg(long)]
        project: Option<String>,
        /// Collaboration name (overrides --project's derived slug). Default: the
        /// single registered one; errors if ambiguous; else "default".
        #[arg(long)]
        collab: Option<String>,
        /// Your model class (e.g. claude, gpt, gemini, glm). Recorded so `doctor`
        /// can flag a same-class implementer/reviewer pairing — which forfeits most
        /// of the error-decorrelation gain that makes a heterogeneous crew win.
        #[arg(long)]
        class: Option<String>,
        /// Your review lens (e.g. correctness, security, regressions) when you're a
        /// reviewer in a 2+ reviewer crew. Distinct lenses make extra reviewers add
        /// diversity, not redundancy; spriff focuses your wake prompt on it.
        #[arg(long)]
        lens: Option<String>,
        /// Repo to mark (defaults to the current directory).
        #[arg(long)]
        repo: Option<PathBuf>,
        /// Roster size if the collaboration must be created. Default 2.
        #[arg(long, default_value_t = 2)]
        agents: usize,
    },

    /// Show which persona/collaboration your bare commands resolve to AND where
    /// that identity came from. Run this if `spriff inbox`/`wait` seems to show
    /// the wrong thing — a stale/foreign `.spriff` marker can make you act as the
    /// wrong persona (and then your peer's posts get filtered out as "your own").
    Whoami {
        #[arg(long)]
        collab: Option<String>,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long = "as")]
        as_persona: Option<String>,
    },

    /// Health-check a collaboration: registry, resolved identity + source, board
    /// state, per-persona unread/cursor, whether a `serve` supervisor is running,
    /// and roster/identity sanity warnings. Run it when something seems off.
    Doctor {
        #[arg(long)]
        collab: Option<String>,
        #[arg(long)]
        config: Option<PathBuf>,
        /// Diagnose as this persona (so the loop-preserving `--as <you>` rule
        /// works on `doctor` too, and the identity source is shown for it).
        #[arg(long = "as")]
        as_persona: Option<String>,
    },

    /// Print the agent collaboration protocol (the SKILL file). Point any CLI
    /// agent at `spriff skill` to onboard it.
    Skill,

    /// Create and register a new collaboration under ~/.spriff/<name>/.
    ///
    /// Personas are auto-assigned by convention: agents in a collaboration share
    /// a first letter, named alphabetically by role (executor lowest, reviewers
    /// ascending) — e.g. Abbey (executor), Alice, Annie. Override with --persona.
    Init {
        /// Collaboration name (used to address it everywhere).
        name: String,
        /// Number of agents (executor + reviewers). Default 2.
        #[arg(long, default_value_t = 2)]
        agents: usize,
        /// Shared first letter for the roster. Default: first letter unused by
        /// any existing collaboration, so collaborations stay visually distinct.
        #[arg(long)]
        letter: Option<char>,
        /// Explicit persona names, executor first (overrides auto-naming).
        #[arg(long = "persona")]
        personas: Vec<String>,
        /// Override the board path (defaults to the registry dir).
        #[arg(long)]
        board: Option<PathBuf>,
    },

    /// List registered collaborations.
    List,

    /// Run the event-driven watcher for a collaboration (long-running).
    Watch {
        #[arg(long)]
        collab: Option<String>,
        #[arg(long)]
        config: Option<PathBuf>,
        /// Watch as this persona (defaults to the config's `me`).
        #[arg(long = "as")]
        as_persona: Option<String>,
    },

    /// IRONCLAD loop: supervise an agent. spriff stays running and RE-INVOKES the
    /// agent command for one turn whenever a peer posts — so the loop survives the
    /// agent stopping, timing out, or crashing. The supervisor is the daemon; the
    /// agent runs per turn. Example: `spriff serve --as Alice -- codex exec`.
    Serve {
        #[arg(long)]
        collab: Option<String>,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long = "as")]
        as_persona: Option<String>,
        /// Stand down after this many seconds with no peer turn (0 = run forever).
        #[arg(long, default_value_t = 0)]
        idle_timeout: u64,
        /// Poll interval in seconds.
        #[arg(long, default_value_t = 2)]
        poll: u64,
        /// Don't make an opening/catch-up invocation at startup; only react to
        /// future peer turns.
        #[arg(long)]
        no_kickoff: bool,
        /// The agent command to run per turn (everything after `--`). spriff
        /// appends a wake prompt as the final argument. e.g. `-- claude -p`.
        #[arg(last = true, required = true)]
        agent_cmd: Vec<String>,
    },

    /// Append a turn to the board in canonical grammar.
    Post {
        #[arg(long)]
        collab: Option<String>,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long = "as")]
        as_persona: Option<String>,
        /// Short subject line for the turn header.
        #[arg(long, short = 's')]
        subject: String,
        /// Status marker: FYI | NEEDS-REVIEW | BLOCKED | HANDOFF | DONE | ACTION-REQUIRED.
        #[arg(long, default_value = "FYI")]
        status: String,
        /// Address the turn at specific peers (repeatable). Defaults to all peers.
        #[arg(long = "to")]
        to: Vec<String>,
        /// Message body. If omitted, the body is read from stdin.
        #[arg(long, short = 'm')]
        message: Option<String>,
    },

    /// Show the pending peer delta (your inbox) for a collaboration.
    Inbox {
        #[arg(long)]
        collab: Option<String>,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long = "as")]
        as_persona: Option<String>,
    },

    /// Block until a peer posts (your inbox becomes non-empty), then print the
    /// delta and exit 0. Exits 2 on timeout. This is the natural "wait for my
    /// turn" primitive for a CLI agent loop.
    Wait {
        #[arg(long)]
        collab: Option<String>,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long = "as")]
        as_persona: Option<String>,
        /// Give up after this many seconds (0 = wait forever). Default 1800.
        #[arg(long, default_value_t = 1800)]
        timeout: u64,
        /// Poll interval in seconds.
        #[arg(long, default_value_t = 2)]
        interval: u64,
    },

    /// Declare source paths you're touching, so your peers' watchers wake on your
    /// real edits (not just board posts). Appends to your watchpaths sidecar.
    Touching {
        /// One or more paths (files or dirs) you're working in.
        paths: Vec<PathBuf>,
        #[arg(long)]
        collab: Option<String>,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long = "as")]
        as_persona: Option<String>,
    },

    /// Acknowledge the current pending signal (archive it) after responding.
    Ack {
        #[arg(long)]
        collab: Option<String>,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long = "as")]
        as_persona: Option<String>,
    },

    /// Show watcher/board/pending status for a collaboration.
    Status {
        #[arg(long)]
        collab: Option<String>,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long = "as")]
        as_persona: Option<String>,
    },

    /// Roll older turns off the live board into its archive, bounding context.
    /// Runs automatically after `post` once the board crosses `max_live_bytes`;
    /// this command forces it on demand.
    Rollup {
        #[arg(long)]
        collab: Option<String>,
        #[arg(long)]
        config: Option<PathBuf>,
    },

    /// Set or show the collaboration's MISSION — the specific goal the crew drives
    /// to completion. Combined with the always-on Definition of Done (feature
    /// complete · fully tested · live-integration tested · PR'd), it keeps the
    /// implement↔review loop going until the work is genuinely shipped.
    Mission {
        /// The mission text. Omit to show the current mission.
        text: Vec<String>,
        #[arg(long)]
        collab: Option<String>,
        #[arg(long)]
        config: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Join {
            role,
            as_name,
            with,
            project,
            collab,
            class,
            lens,
            repo,
            agents,
        } => cmd_join(
            &role, as_name, with, project, collab, class, lens, repo, agents,
        ),
        Cmd::Whoami {
            collab,
            config,
            as_persona,
        } => {
            let (cfg, name) = resolve(collab, config)?;
            let (persona, source) = resolve_persona_with_source(as_persona, &cfg);
            let role = cfg.role_of(&persona);
            let on_roster = cfg
                .agents
                .iter()
                .any(|a| a.persona.eq_ignore_ascii_case(&persona));
            println!("collaboration: {name}");
            println!(
                "persona:       {persona}{}",
                match &role {
                    Some(r) => format!(" ({r})"),
                    None => String::new(),
                }
            );
            println!("identity from: {source}");
            println!(
                "roster:        [{}]",
                cfg.agents
                    .iter()
                    .map(|a| a.persona.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            if !on_roster {
                println!(
                    "⚠  WARNING: '{persona}' is NOT on the roster — your peer's posts will look"
                );
                println!(
                    "   wrong/empty. Set the right identity with `--as <name>` or $SPRIFF_AS,"
                );
                println!("   or run under `spriff serve --as <name>` (which forces it).");
            }
            Ok(())
        }
        Cmd::Doctor {
            collab,
            config,
            as_persona,
        } => cmd_doctor(collab, config, as_persona),
        Cmd::Skill => {
            print!("{SKILL}");
            Ok(())
        }
        Cmd::Init {
            name,
            agents,
            letter,
            personas,
            board,
        } => cmd_init(&name, agents, letter, &personas, board),
        Cmd::List => cmd_list(),
        Cmd::Watch {
            collab,
            config,
            as_persona,
        } => {
            let (cfg, _name) = resolve(collab, config)?;
            let persona = resolve_persona(as_persona, &cfg);
            watcher::run(&cfg, &persona)
        }
        Cmd::Serve {
            collab,
            config,
            as_persona,
            idle_timeout,
            poll,
            no_kickoff,
            agent_cmd,
        } => {
            let (cfg, name) = resolve(collab, config.clone())?;
            let persona = resolve_persona(as_persona, &cfg);
            cmd_serve(
                &cfg,
                &name,
                &persona,
                idle_timeout,
                poll,
                !no_kickoff,
                &agent_cmd,
                config,
            )
        }
        Cmd::Post {
            collab,
            config,
            as_persona,
            subject,
            status,
            to,
            message,
        } => {
            let (cfg, _name) = resolve(collab, config)?;
            let persona = resolve_persona(as_persona, &cfg);
            cmd_post(&cfg, &persona, &subject, &status, &to, message)
        }
        Cmd::Inbox {
            collab,
            config,
            as_persona,
        } => {
            let (cfg, _name) = resolve(collab, config)?;
            let persona = resolve_persona(as_persona, &cfg);
            cmd_inbox(&cfg, &persona)
        }
        Cmd::Wait {
            collab,
            config,
            as_persona,
            timeout,
            interval,
        } => {
            let (cfg, _name) = resolve(collab, config)?;
            let persona = resolve_persona(as_persona, &cfg);
            cmd_wait(&cfg, &persona, timeout, interval)
        }
        Cmd::Touching {
            paths,
            collab,
            config,
            as_persona,
        } => {
            let (cfg, _name) = resolve(collab, config)?;
            let persona = resolve_persona(as_persona, &cfg);
            cmd_touching(&cfg, &persona, &paths)
        }
        Cmd::Ack {
            collab,
            config,
            as_persona,
        } => {
            let (cfg, _name) = resolve(collab, config)?;
            let persona = resolve_persona(as_persona, &cfg);
            let board_path = cfg.board_path();
            let sc = Sidecars::derive(&board_path, &persona);
            // Advance the consume cursor to "everything up to now" and clear the
            // dedup guard, so the same peer turns won't reappear in your inbox.
            let mut st = state::WatchState::load(&sc.state);
            st.offset = board::board_size(&board_path);
            st.last_pending_header = String::new();
            st.save(&sc.state)?;
            // Archive any proactive watcher signal (flag/pending/action) too.
            let archived = pending::ack(&sc)?;
            if archived {
                println!("acked — caught up; watcher signal archived. Inbox clear.");
            } else {
                println!("acked — caught up. Inbox clear.");
            }
            Ok(())
        }
        Cmd::Status {
            collab,
            config,
            as_persona,
        } => {
            let (cfg, name) = resolve(collab, config)?;
            let persona = resolve_persona(as_persona, &cfg);
            cmd_status(&cfg, &name, &persona)
        }
        Cmd::Rollup { collab, config } => {
            let (cfg, _name) = resolve(collab, config)?;
            let n = board::rollup(&cfg.board_path(), cfg.rollup.keep_recent_turns)?;
            if n > 0 {
                println!(
                    "rolled up {n} turn(s) to {}",
                    board::archive_path(&cfg.board_path()).display()
                );
            } else {
                println!("nothing to roll up (board within keep window).");
            }
            Ok(())
        }
        Cmd::Mission {
            text,
            collab,
            config,
        } => {
            let (cfg, _name) = resolve(collab, config)?;
            let path = mission_path(&cfg.board_path());
            if text.is_empty() {
                match read_mission(&cfg.board_path()) {
                    Some(m) => {
                        println!("Mission:\n{m}\n");
                        println!("Definition of Done (always on): {DEFINITION_OF_DONE}");
                    }
                    None => println!(
                        "No mission set. Set one: spriff mission \"<goal>\".\nDefinition of Done (always on): {DEFINITION_OF_DONE}"
                    ),
                }
            } else {
                std::fs::write(&path, format!("{}\n", text.join(" ")))?;
                println!("Mission set ({}).", path.display());
                println!("The crew will drive to completion against it + the Definition of Done.");
            }
            Ok(())
        }
    }
}

/// The shared mission file for a board: `<board-base>.mission.md`.
fn mission_path(board: &std::path::Path) -> PathBuf {
    let dir = board.parent().unwrap_or_else(|| std::path::Path::new("."));
    let mut base = board
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "board".into());
    if let Some(s) = base.strip_suffix(".md") {
        base = s.to_string();
    }
    if let Some(s) = base.strip_suffix(".board") {
        base = s.to_string();
    }
    dir.join(format!("{base}.mission.md"))
}

fn read_mission(board: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(mission_path(board)).ok()?;
    let t = text.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// The per-persona model-class sidecar: `<base>.<persona>.class`. Written by
/// `join --class` so an agent can declare its model class without rewriting the
/// shared config TOML (same sidecar pattern as the cursor/flag files).
fn class_path(board: &std::path::Path, persona: &str) -> PathBuf {
    let state = Sidecars::derive(board, persona).state; // <base>.<persona>.watch.state
    let s = state.to_string_lossy();
    PathBuf::from(format!("{}class", s.trim_end_matches("watch.state")))
}

/// An agent's declared model class: the live `join --class` sidecar if present,
/// else the seed `class` field in the config roster, else None.
fn resolve_class(cfg: &Config, board: &std::path::Path, persona: &str) -> Option<String> {
    if let Ok(t) = std::fs::read_to_string(class_path(board, persona)) {
        let t = t.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    cfg.agents
        .iter()
        .find(|a| a.persona.eq_ignore_ascii_case(persona))
        .and_then(|a| a.class.clone())
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
}

/// The per-persona review-lens sidecar: `<base>.<persona>.lens` (same pattern as
/// the class sidecar, written by `join --lens`).
fn lens_path(board: &std::path::Path, persona: &str) -> PathBuf {
    let state = Sidecars::derive(board, persona).state;
    let s = state.to_string_lossy();
    PathBuf::from(format!("{}lens", s.trim_end_matches("watch.state")))
}

/// A reviewer's declared review lens: the live `join --lens` sidecar if present,
/// else the seed `lens` field in the config roster, else None.
fn resolve_lens(cfg: &Config, board: &std::path::Path, persona: &str) -> Option<String> {
    if let Ok(t) = std::fs::read_to_string(lens_path(board, persona)) {
        let t = t.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    cfg.agents
        .iter()
        .find(|a| a.persona.eq_ignore_ascii_case(persona))
        .and_then(|a| a.lens.clone())
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
}

/// Advise on review-lens coverage across a crew's REVIEWERS. PURE + testable.
/// Lenses only matter once there are 2+ reviewers — then distinct lenses make
/// the extra reviewer add diversity rather than redundancy (a redundant reviewer
/// is the "more agents, worse" failure mode). `reviewers` is (persona, lens).
/// Returns a warning on a shared lens, a nudge if lenses are missing, else None.
fn lens_advisory(reviewers: &[(String, Option<String>)]) -> Option<String> {
    if reviewers.len() < 2 {
        return None; // one reviewer: nothing to diversify.
    }
    let norm = |l: &Option<String>| {
        l.as_deref()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
    };
    for i in 0..reviewers.len() {
        for j in (i + 1)..reviewers.len() {
            if let (Some(a), Some(b)) = (norm(&reviewers[i].1), norm(&reviewers[j].1)) {
                if a == b {
                    return Some(format!(
                        "{} and {} share review lens '{a}': redundant — give each reviewer a \
                         DISTINCT lens (e.g. correctness / security / regressions) so extra \
                         reviewers add diversity, not duplicate coverage",
                        reviewers[i].0, reviewers[j].0
                    ));
                }
            }
        }
    }
    if reviewers.iter().any(|(_, l)| norm(l).is_none()) {
        return Some(
            "2+ reviewers without distinct lenses — assign each a lens \
             (`spriff join --role reviewer --lens <correctness|security|regressions|…>`) so \
             they cover different failure modes instead of overlapping"
                .to_string(),
        );
    }
    None
}

/// The outcome of the model-class heterogeneity check over a roster.
#[derive(Debug, PartialEq, Eq)]
enum Heterogeneity {
    /// Two agents share a model class — the actionable problem (carries the message).
    Collision(String),
    /// Some agents declared a class and some didn't — the check is INCONCLUSIVE,
    /// NOT clean (carries the personas missing a class). A single unknown in a
    /// two-agent crew leaves the same-class risk unassessed. (Alice's catch.)
    Unverified(Vec<String>),
    /// No classes declared at all — the feature simply isn't in use (a soft nudge,
    /// not a warning, so `doctor` stays quiet for crews that don't use classes).
    Undeclared,
    /// Every agent declared a class and all are distinct — verified heterogeneous.
    Healthy,
}

/// Classify a roster's model-class diversity. PURE + testable. The premise
/// (Condorcet independence; the ambiguity decomposition) is that the gain comes
/// from DECORRELATED errors, which decorrelate most across different model
/// classes — so a same-class implementer/reviewer pair forfeits most of it, and a
/// *partially* declared roster can't be certified clean. `roster` is
/// (persona, class); the collision message names the first colliding pair in
/// roster order (deterministic).
fn heterogeneity_status(roster: &[(String, Option<String>)]) -> Heterogeneity {
    let norm = |c: &Option<String>| {
        c.as_deref()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
    };
    for i in 0..roster.len() {
        for j in (i + 1)..roster.len() {
            if let (Some(na), Some(nb)) = (norm(&roster[i].1), norm(&roster[j].1)) {
                if na == nb {
                    return Heterogeneity::Collision(format!(
                        "{} and {} share model class '{na}': a same-class pair forfeits most of \
                         the error-decorrelation gain — pair different model classes (e.g. one \
                         Claude, one GPT) so the reviewer fails differently than the implementer",
                        roster[i].0, roster[j].0
                    ));
                }
            }
        }
    }
    let missing: Vec<String> = roster
        .iter()
        .filter(|(_, c)| norm(c).is_none())
        .map(|(p, _)| p.clone())
        .collect();
    if missing.is_empty() {
        Heterogeneity::Healthy
    } else if missing.len() == roster.len() {
        Heterogeneity::Undeclared
    } else {
        Heterogeneity::Unverified(missing)
    }
}

/// Do a stored mission and supplied `--project` text name the SAME goal?
/// Lenient on case and on surrounding/collapsed whitespace — those are the same
/// goal phrased slightly differently. Strict on everything else, so two prompts
/// that slugify to the same board but mean different things (Alice's example:
/// `"a/b"` vs `"a b"`, both → slug `a-b`) are correctly seen as DIFFERENT.
fn mission_eq(a: &str, b: &str) -> bool {
    fn norm(s: &str) -> String {
        s.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    }
    norm(a) == norm(b)
}

/// The exact command a peer runs to land on THIS same board — PURE, so the
/// rendezvous-key logic is unit-tested. The KEY is the slug: when `--collab` was
/// explicit the goal text would slugify to a *different* board, so the peer
/// command must carry `--collab {name}` (the goal rides along only as shared
/// context); otherwise the goal text itself slugifies back to `{name}` and is the
/// key. (Alice's escape-hatch sync catch — a peer command that points elsewhere
/// silently breaks the whole prompt-native rendezvous.)
fn peer_join_command(other_role: &str, name: &str, project: &str, collab_explicit: bool) -> String {
    if collab_explicit {
        format!("spriff join --role {other_role} --collab {name} --project \"{project}\"")
    } else {
        format!("spriff join --role {other_role} --project \"{project}\"")
    }
}

/// What `join --project` should do with the mission once it has resolved a board.
#[derive(Debug, PartialEq, Eq)]
enum MissionPlan {
    /// No mission yet — seed it from the supplied project text.
    Seed,
    /// A mission already names this goal (or the slug was forced) — leave it.
    Keep,
}

/// Decide the mission action when `--project` resolved a board — PURE (no FS), so
/// the seed/keep/reject logic is fully unit-tested.
///
/// The bug this guards (Alice's silent-divergence catch): seeding the mission
/// only on *create* meant a second agent whose `--project` slugified onto an
/// existing board would join *displaying its own goal* while the board's mission
/// was the first agent's. Two agents "synchronized" on different goals. So:
///   * no mission yet            → `Seed` (first agent's goal becomes the mission);
///   * mission names this goal   → `Keep`;
///   * `--collab` forced the slug→ `Keep` (the operator joined intentionally);
///   * mission names a DIFFERENT goal → hard-error with explicit remediation.
fn plan_mission(
    existing: Option<&str>,
    project: &str,
    collab_explicit: bool,
    name: &str,
) -> Result<MissionPlan> {
    match existing {
        None => Ok(MissionPlan::Seed),
        Some(_) if collab_explicit => Ok(MissionPlan::Keep),
        Some(m) if mission_eq(m, project) => Ok(MissionPlan::Keep),
        Some(m) => anyhow::bail!(
            "project \"{project}\" maps to existing board '{name}', but that board's \
             mission is \"{m}\". Two agents would rendezvous on the same board while \
             disagreeing on the goal. Use the exact project text, pass --collab {name} \
             to join it intentionally, or choose a more specific --project.",
        ),
    }
}

/// Run `f` while holding an exclusive, kernel-arbitrated lock keyed on the collab
/// name, so concurrent first-joins of the SAME collaboration serialize: exactly
/// one process is the creator (deterministic roster), the rest observe the
/// finished config and join. Crash-safe — the OS releases the lock if the holder
/// dies — and free of any read-then-act TOCTOU. The lock file lives at the
/// registry root (`<SPRIFF_HOME>/.<name>.create.lock`) and is never unlinked, so
/// every joiner flocks the same inode. (Alice's concurrent-rendezvous catch.)
fn with_create_lock<T>(name: &str, f: impl FnOnce() -> Result<T>) -> Result<T> {
    use fs2::FileExt;
    let root = registry::root();
    std::fs::create_dir_all(&root).ok();
    let lock_path = root.join(format!(".{name}.create.lock"));
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("opening create-lock {}", lock_path.display()))?;
    // Blocks until we hold the lock; a concurrent joiner waits here, then sees the
    // config the winner wrote and takes the join path.
    file.lock_exclusive()
        .with_context(|| format!("locking create-lock {}", lock_path.display()))?;
    let result = f();
    let _ = FileExt::unlock(&file); // also released on drop / process death.
    result
}

/// The serve singleton-lock file for one persona on one board:
/// `<base>.<persona>.serve.lock`, next to the other sidecars.
fn serve_lock_path(board: &std::path::Path, persona: &str) -> PathBuf {
    let state = Sidecars::derive(board, persona).state; // <base>.<persona>.watch.state
    let s = state.to_string_lossy();
    PathBuf::from(format!("{}serve.lock", s.trim_end_matches("watch.state")))
}

/// Singleton lock guarding one `serve` per (collab, persona), backed by an OS
/// advisory lock (flock via fs2). The KERNEL arbitrates exclusivity, so there is
/// no path-based read-then-unlink TOCTOU at all, and a crashed/killed process has
/// its lock released automatically by the OS — no stale-file reclaim needed.
/// (Alice's fix: "an OS advisory lock held for the lifetime of `ServeLock`".)
struct ServeLock {
    // Holding the File holds the kernel lock; dropping it releases the lock.
    // The lock file itself is intentionally never unlinked, so every serve flocks
    // the SAME inode (unlinking + recreating would defeat flock semantics).
    _file: std::fs::File,
}

/// Acquire the serve singleton lock, or error if another LIVE serve holds it.
fn acquire_serve_lock(board: &std::path::Path, persona: &str) -> Result<ServeLock> {
    use fs2::FileExt;
    use std::io::Write;
    let path = serve_lock_path(board, persona);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    // NOTE: this is a LOCAL advisory lock (flock). It coordinates processes on
    // the same host; network filesystems (NFS/SMB) can have weaker/flakier lock
    // semantics, so keep ~/.spriff (the lock dir) on a local FS. (Alice's caveat.)
    let file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false) // don't truncate on open; we set_len(0) only after locking
        .open(&path)
        .with_context(|| format!("opening serve lock {}", path.display()))?;

    // Retry briefly before declaring a duplicate: a REAL serve holds the lock for
    // its whole lifetime, so retrying still fails against it — but a momentary
    // holder (e.g. `spriff doctor`'s non-destructive lock PROBE) is gone within
    // milliseconds, so the grace window lets a legitimate serve start win the
    // race. (Alice's fix for the doctor-probe race.)
    let mut last = file.try_lock_exclusive();
    for _ in 0..10 {
        if last.is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
        last = file.try_lock_exclusive();
    }
    match last {
        Ok(()) => {
            // We hold the kernel lock. Record our pid as DIAGNOSTIC text only —
            // ownership is decided by the kernel lock, never by this content.
            file.set_len(0).ok();
            let _ = (&file).write_all(std::process::id().to_string().as_bytes());
            Ok(ServeLock { _file: file })
        }
        Err(_) => {
            let pid = std::fs::read_to_string(&path)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "?".into());
            anyhow::bail!(
                "another `spriff serve` is already running as {persona} (pid {pid}). \
                 Only one supervisor per persona — stop it first."
            )
        }
    }
}

/// Resolve (config, name) from optional flags, honouring the registry priority
/// order. An explicit `--config <path>` short-circuits name resolution.
fn resolve(collab: Option<String>, config: Option<PathBuf>) -> Result<(Config, String)> {
    // Precedence: explicit `--config` wins outright. `$SPRIFF_CONFIG` (how a
    // `serve --config` supervisor propagates a non-registry config to its child)
    // applies ONLY when neither `--config` nor an explicit `--collab` is given —
    // so a deliberate `--collab other` always outranks an inherited env config.
    // (Alice's non-blocking note.)
    let config = config.or_else(|| {
        if collab.is_some() {
            return None;
        }
        std::env::var("SPRIFF_CONFIG")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
    });
    if let Some(path) = config {
        let cfg = Config::load(&path)?;
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "collab".into());
        return Ok((cfg, name));
    }
    let name = registry::resolve_name(collab)?;
    let path = registry::config_path(&name);
    let cfg = Config::load(&path)
        .with_context(|| format!("loading collaboration '{name}' ({})", path.display()))?;
    Ok((cfg, name))
}

/// Resolve a roster: explicit `--persona` names win; otherwise auto-assign by
/// convention (shared first letter, executor lowest, reviewers ascending).
fn build_roster(agents: usize, letter: Option<char>, personas: &[String]) -> Vec<String> {
    if !personas.is_empty() {
        personas.to_vec()
    } else {
        let n = agents.max(2);
        let chosen = letter.unwrap_or_else(|| names::pick_letter(&used_letters()));
        names::roster(chosen, n)
    }
}

/// Create + register a collaboration: seed the board, write the config TOML.
/// Idempotent enough to be safe if two agents race to create the same one (the
/// board is only seeded if absent; the config content is deterministic).
fn create_collab(name: &str, roster: &[String], board: Option<PathBuf>) -> Result<PathBuf> {
    let dir = registry::collab_dir(name);
    std::fs::create_dir_all(&dir)?;
    let board_path = board.unwrap_or_else(|| registry::board_path(name));
    board::seed_board(&board_path, name)?;

    let mut toml = String::new();
    toml.push_str(&format!("# spriff collaboration: {name}\n"));
    toml.push_str(&format!("board = \"{}\"\n\n", board_path.display()));
    for (i, persona) in roster.iter().enumerate() {
        let role = if i == 0 { "executor" } else { "reviewer" };
        toml.push_str("[[agents]]\n");
        toml.push_str(&format!("persona = \"{persona}\"\n"));
        toml.push_str(&format!("role = \"{role}\"\n"));
        toml.push_str(
            "watchpaths = []   # add this agent's source paths so peers see their edits\n\n",
        );
    }
    toml.push_str("[watch]\nsettle_ms = 600\npoll_ms = 3000\n\n");
    toml.push_str("[rollup]\nmax_live_bytes = 98304\nkeep_recent_turns = 30\n");

    let cfg_path = registry::config_path(name);
    std::fs::write(&cfg_path, toml)?;
    Ok(board_path)
}

fn cmd_init(
    name: &str,
    agents: usize,
    letter: Option<char>,
    personas: &[String],
    board: Option<PathBuf>,
) -> Result<()> {
    let roster = build_roster(agents, letter, personas);
    let board_path = create_collab(name, &roster, board)?;
    let cfg_path = registry::config_path(name);

    println!("Created collaboration '{name}':");
    println!("  config: {}", cfg_path.display());
    println!("  board:  {}", board_path.display());
    println!("  roster:");
    for (i, persona) in roster.iter().enumerate() {
        let role = if i == 0 { "executor" } else { "reviewer" };
        println!("    {persona}  ({role})");
    }
    println!();
    println!("Agents can now self-onboard in any repo with:");
    println!(
        "    spriff join --role implementer   (acts as {})",
        roster[0]
    );
    if roster.len() > 1 {
        println!(
            "    spriff join --role reviewer      (acts as {})",
            roster[1]
        );
    }
    Ok(())
}

/// The persona to act as, plus a human description of WHERE that identity came
/// from: `--as` flag → `$SPRIFF_AS` → `.spriff` marker `as=` → the executor.
/// The source matters because a stale/foreign marker can silently make an agent
/// act as the wrong persona (e.g. a reviewer resolving as the executor because a
/// shared repo's marker names someone else) — `spriff whoami` surfaces it.
fn resolve_persona_with_source(explicit: Option<String>, cfg: &Config) -> (String, String) {
    if let Some(p) = explicit {
        return (p, "--as flag".to_string());
    }
    if let Ok(p) = std::env::var("SPRIFF_AS") {
        if !p.is_empty() {
            return (p, "$SPRIFF_AS env".to_string());
        }
    }
    if let Some(p) = registry::marker_field("as") {
        return (p, ".spriff marker (walked up from cwd)".to_string());
    }
    (
        cfg.default_persona(),
        "default = the collaboration's executor (no --as/env/marker found)".to_string(),
    )
}

fn resolve_persona(explicit: Option<String>, cfg: &Config) -> String {
    resolve_persona_with_source(explicit, cfg).0
}

/// Derive a STABLE board slug from free-text project/goal: lowercase, runs of
/// non-alphanumerics collapse to a single '-', trimmed, capped. The same project
/// text always yields the same slug, so two agents who pass the same --project
/// land on the same board with no other coordination.
fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !out.is_empty() && !dash {
            out.push('-');
            dash = true;
        }
    }
    let slug: String = out.trim_matches('-').chars().take(48).collect();
    let slug = slug.trim_end_matches('-').to_string();
    if slug.is_empty() {
        "project".to_string()
    } else {
        slug
    }
}

/// Which roster slot a joining agent claims. PURE + testable. A reviewer is NOT
/// necessarily slot 1: in a 3+ crew, a reviewer who names itself with `--as` binds
/// to THAT reviewer's slot so the 2nd, 3rd, … reviewer can actually join. The
/// executor slot (0) is never a valid reviewer target, so an `--as` that resolves
/// to slot 0 falls back to the default and the caller's validation rejects it.
/// (Alice's catch: join hardcoded reviewer → slot 1, so a second reviewer's
/// `--as Annie` failed against slot 1 = Alice.)
fn resolve_my_slot(
    is_review: bool,
    as_name: Option<&str>,
    default_slot: usize,
    roster: &[String],
) -> usize {
    if is_review {
        if let Some(idx) =
            as_name.and_then(|n| roster.iter().position(|p| p.eq_ignore_ascii_case(n)))
        {
            if idx != 0 {
                return idx;
            }
        }
    }
    default_slot
}

/// The board to join when the agent gave no explicit signal: the single
/// registered collaboration, or "default" if none — but if SEVERAL exist, refuse
/// to guess (which would silently join the wrong board) and ask for disambiguation.
fn resolve_join_default() -> Result<String> {
    let l = registry::list();
    match l.len() {
        0 => Ok("default".to_string()),
        1 => Ok(l[0].clone()),
        _ => anyhow::bail!(
            "several collaborations exist ({}) and you gave no project. Pass \
             --project \"<your goal>\" (recommended — your peer passes the same and you meet) \
             or --collab <name>.",
            l.join(", ")
        ),
    }
}

/// Onboard an agent: auto-create/join the collaboration, claim the role's
/// persona, write a repo marker so later commands need no flags, and print the
/// protocol + first move. The single command an agent runs to start.
#[allow(clippy::too_many_arguments)]
fn cmd_join(
    role: &str,
    as_name: Option<String>,
    with: Option<String>,
    project: Option<String>,
    collab: Option<String>,
    class: Option<String>,
    lens: Option<String>,
    repo: Option<PathBuf>,
    agents: usize,
) -> Result<()> {
    let role_norm = role.to_lowercase();
    let is_impl = matches!(
        role_norm.as_str(),
        "implementer" | "executor" | "impl" | "exec" | "dev" | "builder"
    );
    let is_review = matches!(role_norm.as_str(), "reviewer" | "review" | "qa" | "critic");
    if !is_impl && !is_review {
        anyhow::bail!("unknown role '{role}'. Use --role implementer or --role reviewer.");
    }
    // A review lens is meaningless for the implementer — reject it loudly rather
    // than silently writing a lens an implementer would then carry into serve/status
    // surfaces. (Alice's catch: --lens leaked onto non-reviewers.)
    if lens.is_some() && !is_review {
        anyhow::bail!(
            "--lens is for reviewers only — role '{role}' is the implementer, and a review lens \
             has no meaning for the agent doing the building."
        );
    }

    // Resolve which board to join, in priority order:
    //   1. explicit --collab
    //   2. --project text -> a STABLE slug (so two agents who pass the same project
    //      from their prompts deterministically meet on the same board)
    //   3. $SPRIFF_COLLAB / `.spriff` marker (an already-established context)
    //   4. the single registered collaboration
    //   5. "default" ONLY when nothing is registered; if several exist and the
    //      agent gave no signal, STOP and ask for --project/--collab rather than
    //      silently joining the wrong board.
    // Was the slug forced explicitly? If so, `--project` is just a mission label
    // and we do NOT enforce mission match (the operator chose this board on
    // purpose). Capture before `collab` is moved into the resolution below.
    let collab_explicit = collab.is_some();
    let name = if let Some(c) = collab {
        c
    } else if let Some(p) = &project {
        slugify(p)
    } else if let Some(c) = std::env::var("SPRIFF_COLLAB")
        .ok()
        .filter(|s| !s.is_empty())
    {
        c
    } else if let Some(c) = registry::marker_field("collab") {
        c
    } else {
        resolve_join_default()?
    };

    // Roster slots are FIXED: executor=0, reviewer=1. Only the *source* of each
    // name varies by role — `--as` names MY slot, `--with` names my peer's.
    // (Alice's catch: a role-dependent slot tuple double-switched and mis-assigned.)
    let (my_slot, peer_slot) = if is_impl {
        (0usize, 1usize)
    } else {
        (1usize, 0usize)
    };

    // Create-or-join, serialized so two agents launched at the SAME instant from
    // the same --project can't both run create_collab. Without the lock that race
    // is real and nasty: both see `created = true`, both pick a roster letter from
    // `used_letters()` (whose result depends on whether the *other* process has
    // registered yet), and the second create_collab overwrites the config — which
    // can leave one agent's `.spriff` marker pointing at a persona that is no
    // longer on the roster (off-roster → every later command for that agent
    // breaks). The kernel advisory lock makes exactly one process the creator
    // (deterministic roster) and the rest observe the finished config and join.
    // Mission reconciliation runs under the SAME lock so read-None-then-seed is
    // atomic with creation. (Alice's concurrent-rendezvous catch.)
    let (cfg, created) = with_create_lock(&name, || {
        let created = !registry::config_path(&name).exists();
        if created {
            let mut roster = build_roster(agents.max(2), None, &[]);
            // Apply --with FIRST: it can rename the peer/executor slot, and the
            // role-conflict check below must see the post-with executor name.
            if let Some(n) = &with {
                roster[peer_slot] = n.clone();
            }
            // A reviewer whose --as names the executor slot (slot 0) is a ROLE
            // CONFLICT — a reviewer can't be the implementer. Without this, --as
            // matching slot 0 fell back to slot 1 and then OVERWROTE the first
            // reviewer with the executor's name (`Abbey, Abbey, Annie`), and
            // validation passed because slot 1 was now also that name. (Alice.)
            if is_review {
                if let Some(n) = &as_name {
                    if roster
                        .first()
                        .map(|e| e.eq_ignore_ascii_case(n))
                        .unwrap_or(false)
                    {
                        anyhow::bail!(
                            "--as {n} is the implementer on a new '{name}' crew but --role is \
                             reviewer — a reviewer can't claim the implementer's slot. Use a \
                             reviewer name, or pass --with <implementer> to name the implementer."
                        );
                    }
                }
            }
            // Resolve MY slot against the GENERATED roster so a reviewer that
            // CREATES the board (reviewer #2 winning the create race) lands in its
            // own slot instead of clobbering the first reviewer. (Alice's catch.)
            let create_slot = resolve_my_slot(is_review, as_name.as_deref(), my_slot, &roster);
            if let Some(n) = &as_name {
                roster[create_slot] = n.clone();
            }
            // Final roster personas MUST be distinct — a duplicate makes identity
            // ambiguous and silently passes --as validation. (Alice's catch.)
            let mut seen = std::collections::HashSet::new();
            for p in &roster {
                if !seen.insert(p.to_lowercase()) {
                    anyhow::bail!(
                        "creating '{name}' would put two agents named '{p}' on the roster ({}). \
                         Personas must be distinct — check --as/--with.",
                        roster.join(", ")
                    );
                }
            }
            create_collab(&name, &roster, None)?;
        }
        let cfg = Config::load(&registry::config_path(&name))?;
        // Seed the goal once / keep if it matches / hard-error on divergence.
        if let Some(p) = &project {
            let board = cfg.board_path();
            match plan_mission(read_mission(&board).as_deref(), p, collab_explicit, &name)? {
                MissionPlan::Seed => {
                    std::fs::write(mission_path(&board), format!("{}\n", p)).ok();
                }
                MissionPlan::Keep => {}
            }
        }
        Ok((cfg, created))
    })?;

    // Re-resolve MY slot now that the roster is known: a reviewer that named
    // itself with --as binds to that reviewer's slot, so the 2nd+ reviewer in a 3+
    // crew can join (not just slot 1). (Alice's catch.)
    let roster_personas: Vec<String> = cfg.agents.iter().map(|a| a.persona.clone()).collect();
    let my_slot = resolve_my_slot(is_review, as_name.as_deref(), my_slot, &roster_personas);

    // The canonical persona for my role IS the roster slot. Identity must stay
    // canonical or every downstream invariant (peers, sidecars, addressees, turn
    // filtering) breaks — so validate any explicit names against the roster and
    // hard-error on a mismatch rather than writing an off-roster marker.
    let slot_persona = cfg
        .agents
        .get(my_slot)
        .map(|a| a.persona.clone())
        .ok_or_else(|| anyhow::anyhow!("collaboration '{name}' has no slot for role '{role}'"))?;
    if let Some(n) = &as_name {
        if !n.eq_ignore_ascii_case(&slot_persona) {
            anyhow::bail!(
                "--as {n} doesn't match the {role} on '{name}' (that role is {slot_persona}). \
                 Pick the right --role, or the canonical name.",
            );
        }
    }
    if let Some(n) = &with {
        if let Some(peer) = cfg.agents.get(peer_slot) {
            if !n.eq_ignore_ascii_case(&peer.persona) {
                anyhow::bail!(
                    "--with {n} doesn't match the peer on '{name}' (that's {}).",
                    peer.persona
                );
            }
        }
    }
    let persona = slot_persona;

    // Write the repo marker so bare `spriff` commands here act as this persona.
    let repo = repo
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let marker = repo.join(".spriff");
    std::fs::write(&marker, format!("collab={name}\nas={persona}\n"))
        .with_context(|| format!("writing marker {}", marker.display()))?;

    // Record this agent's declared model class (sidecar, so no config rewrite) so
    // `doctor` can flag a same-class implementer/reviewer pairing — heterogeneity
    // is the whole point, and a same-class pair forfeits most of the gain.
    if let Some(c) = &class {
        let c = c.trim();
        if !c.is_empty() {
            let p = class_path(&cfg.board_path(), &persona);
            std::fs::write(&p, format!("{c}\n"))
                .with_context(|| format!("writing class sidecar {}", p.display()))?;
        }
    }
    // Record this reviewer's review lens (sidecar) so `serve` can focus its wake
    // prompt on it and `doctor` can flag two reviewers covering the same lens.
    if let Some(l) = &lens {
        let l = l.trim();
        if !l.is_empty() {
            let p = lens_path(&cfg.board_path(), &persona);
            std::fs::write(&p, format!("{l}\n"))
                .with_context(|| format!("writing lens sidecar {}", p.display()))?;
        }
    }

    let role_label = if is_impl { "implementer" } else { "reviewer" };
    let peers = cfg.peers(&persona).join(", ");
    let other_role = if is_impl { "reviewer" } else { "implementer" };
    println!("════════════════════════════════════════════════════════════════");
    println!("  You are {persona} — the {role_label} on collaboration '{name}'.");
    if let Some(p) = &project {
        println!(
            "  Project: \"{p}\"  ({} board slug '{name}')",
            if created {
                "created"
            } else {
                "joined existing"
            }
        );
        println!("  → Your peer joins the SAME board with:");
        println!(
            "      {}",
            peer_join_command(other_role, &name, p, collab_explicit)
        );
        // In the --collab override case the goal text is only a label, so if it
        // disagrees with this board's mission, say so loudly — the rendezvous key
        // is the explicit slug, not the text. (Alice's escape-hatch catch.)
        if collab_explicit {
            if let Some(m) = read_mission(&cfg.board_path()) {
                if !mission_eq(&m, p) {
                    println!(
                        "  ⚠ this board's mission is \"{m}\" — your --project \"{p}\" is just a \
                         label here; the rendezvous key is --collab {name}."
                    );
                }
            }
        }
    }
    println!(
        "  Your peer(s): {}",
        if peers.is_empty() {
            "(none yet)".into()
        } else {
            peers
        }
    );
    println!("  Marker written: {}", marker.display());
    println!("  (bare `spriff` commands in this repo now act as {persona})");
    println!("════════════════════════════════════════════════════════════════\n");
    print!("{SKILL}");
    let me = persona.as_str();
    println!("\n═══════════ YOUR JOB — run this loop, and NEVER stop on your own ═══════════");
    println!("Two rules that keep the loop from silently breaking:");
    println!(
        "  • On every command that ACTS AS YOU — wait, inbox, post, ack, status, doctor, watch,"
    );
    println!(
        "    serve — pass `--as {me}`. (Bare resolution can mis-resolve you via a shared repo"
    );
    println!("    marker, and your peer's posts then look empty. skill/list/init take no --as.)");
    println!(
        "  • Always write post bodies with a heredoc (<<'EOF' … EOF), never -m \"…\" (the shell"
    );
    println!("    mangles backticks/$/quotes before spriff sees them).");
    println!();
    if is_impl {
        println!("You are the IMPLEMENTER. Loop until the work meets the Definition of Done:");
        println!("  1. Implement a coherent chunk.");
        println!("  2. spriff post --as {me} -s \"<what you did>\" --status NEEDS-REVIEW <<'EOF'");
        println!("       <summary + the files/lines your reviewer should scrutinize>");
        println!("     EOF");
        println!("  3. spriff wait --as {me}      ← BLOCKS until your reviewer replies");
        println!("  4. Read the review, address EVERY point, then GO BACK TO STEP 1.");
    } else {
        println!(
            "You are the REVIEWER. This loop IS your whole job — repeat it until the work is DONE:"
        );
        if let Some(l) = resolve_lens(&cfg, &cfg.board_path(), &persona) {
            println!(
                "  (Your review lens: {l} — concentrate your scrutiny there; peers cover other angles.)"
            );
        }
        println!("  1. spriff wait --as {me}      ← BLOCKS cheaply until the implementer posts");
        println!("  2. Try to BREAK it: read the diff against the goal, run the tests + linters,");
        println!("     hunt the failing case. Skeptical by default — don't bless it.");
        println!("  3. spriff post --as {me} -s \"review: <area>\" --status <NEEDS-REVIEW|BLOCKED|DONE> <<'EOF'");
        println!(
            "       <a specific defect: file:line + the failing case — never a bare \"LGTM\";"
        );
        println!("        if you truly found none, say what you tried to break and why it holds>");
        println!("     EOF");
        println!("  4. spriff ack --as {me}");
        println!("  5. GO BACK TO STEP 1. Never stop after one review; reject a premature DONE.");
        println!("     You're a different model than the implementer — your job is catching what they can't.");
    }
    println!(
        "\n⚠ Keeping this loop running is YOUR responsibility. If you stop, your peer's posts pile"
    );
    println!("  up unread and nothing re-summons you — that's what \"the loop broke\" means. Don't stop.");
    println!("  Re-read the full protocol anytime: spriff skill");

    // Live situation: whatever is already waiting, handle now, then continue looping.
    println!("\n──────────────────────────────── right now ────────────────────────────────");
    let delta = current_delta(&cfg, &persona).unwrap_or_default();
    if !delta.is_empty() {
        println!(
            "{} turn(s) already waiting — handle them now, then continue the loop above:\n",
            delta.len()
        );
        print_delta(&delta);
    } else if is_impl {
        println!("Nothing waiting — you lead. Start at step 1 above (implement → post NEEDS-REVIEW → wait).");
    } else {
        println!("Nothing waiting yet. Start at step 1 above:  spriff wait --as {me}");
    }
    Ok(())
}

/// First letters already used by existing collaborations' executors.
fn used_letters() -> Vec<char> {
    registry::list()
        .iter()
        .filter_map(|n| Config::load(&registry::config_path(n)).ok())
        .filter_map(|c| c.agents.first().and_then(|a| a.persona.chars().next()))
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn cmd_list() -> Result<()> {
    let names = registry::list();
    if names.is_empty() {
        println!("no collaborations registered. Create one: spriff init <name> --agents 2");
        return Ok(());
    }
    println!(
        "registered collaborations (under {}):",
        registry::root().display()
    );
    for name in names {
        match Config::load(&registry::config_path(&name)).ok() {
            Some(c) => {
                let roster: Vec<String> = c
                    .agents
                    .iter()
                    .enumerate()
                    .map(|(i, a)| {
                        if i == 0 {
                            format!("{}*", a.persona)
                        } else {
                            a.persona.clone()
                        }
                    })
                    .collect();
                println!("  {name}  [{}]   (* = executor)", roster.join(", "));
            }
            None => println!("  {name}  (config unreadable)"),
        }
    }
    Ok(())
}

fn cmd_post(
    cfg: &Config,
    persona: &str,
    subject: &str,
    status: &str,
    to: &[String],
    message: Option<String>,
) -> Result<()> {
    // Validate the status up front (before reading stdin) so a typo fails fast
    // and loudly instead of being silently posted to the board.
    let status = board::normalize_status(status)?;
    let body = match message {
        Some(m) => m,
        None => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            s
        }
    };
    // Default addressees = all peers.
    let addressees: Vec<String> = if to.is_empty() {
        cfg.peers(persona)
    } else {
        to.to_vec()
    };
    let ts = util::utc_now();
    let board_path = cfg.board_path();
    board::append_turn(
        &board_path,
        &ts,
        persona,
        subject,
        &status,
        &addressees,
        &body,
    )?;
    println!("posted to {} as {persona}: {subject}", board_path.display());

    // Auto-rollup: keep the live board (and everyone's context) bounded. The
    // writer does this, not the watcher, so watchers stay read-only to the board.
    if board::board_size(&board_path) > cfg.rollup.max_live_bytes {
        let n = board::rollup(&board_path, cfg.rollup.keep_recent_turns)?;
        if n > 0 {
            println!("(auto-rolled {n} older turn(s) to the archive to bound context)");
        }
    }
    Ok(())
}

/// The peer delta since this persona's cursor, computed LIVE — works whether or
/// not a watcher is running.
fn current_delta(cfg: &Config, persona: &str) -> Result<Vec<board::Turn>> {
    let board_path = cfg.board_path();
    let sc = Sidecars::derive(&board_path, persona);
    let mut st = state::WatchState::load(&sc.state);
    // Safety net for a cursor that points PAST the live board — e.g. a board rolled
    // up or truncated by an OLD spriff before cursor-remap existed, or an external
    // edit. Left alone, such a cursor freezes the loop forever (`delta_since`
    // returns nothing, so `wait`/`inbox` say "not your turn" while real peer turns
    // sit unread below it). Clamp to the board end and persist so it self-heals.
    let size = board::board_size(&board_path);
    if st.offset > size {
        st.offset = size;
        let _ = st.save(&sc.state);
    }
    board::delta_since(&board_path, st.offset, persona)
}

/// Print the captured peer turns plus the canonical "what to do next" footer.
fn print_delta(turns: &[board::Turn]) {
    println!("{} new turn(s) since your last ack:\n", turns.len());
    for t in turns {
        println!("{}", t.header());
        if !t.body.is_empty() {
            println!("\n{}", t.body);
        }
        println!("\n---");
    }
    println!(
        "\nRespond (pipe the body via stdin/heredoc — avoids shell-quoting on backticks/$/quotes):"
    );
    println!("    spriff post -s \"<subject>\" --status <STATUS> <<'EOF'");
    println!("    <your reply>");
    println!("    EOF");
    println!("Ack:      spriff ack");
    println!(
        "Continue: spriff wait        # ⟳ STAY IN THE LOOP — do NOT stop until the work is DONE"
    );
}

fn cmd_inbox(cfg: &Config, persona: &str) -> Result<()> {
    let sc = Sidecars::derive(&cfg.board_path(), persona);
    let turns = current_delta(cfg, persona)?;
    if turns.is_empty() {
        if pending::is_raised(&sc) {
            println!("inbox clear — no new peer turns (stale watcher flag set; run `spriff ack` to clear).");
        } else {
            println!("inbox clear — no new peer turns. Not your turn.");
        }
        return Ok(());
    }
    print_delta(&turns);
    Ok(())
}

/// Declare source paths this persona is touching, by appending to its watchpaths
/// sidecar. A peer's `spriff watch` reads this and wakes on real edits there.
fn cmd_touching(cfg: &Config, persona: &str, paths: &[PathBuf]) -> Result<()> {
    if paths.is_empty() {
        anyhow::bail!("give at least one path: spriff touching <path> [<path>...]");
    }
    let sc = Sidecars::derive(&cfg.board_path(), persona);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut added = 0;
    for p in paths {
        // Resolve relative paths against cwd so the entry is unambiguous, but do
        // not require the path to exist yet (an agent may declare it up front).
        let abs = if p.is_absolute() {
            p.clone()
        } else {
            cwd.join(p)
        };
        if paths::add_watchpath(&sc.watchpaths, &abs)? {
            println!("  watching: {}", abs.display());
            added += 1;
        }
    }
    println!("declared {added} new path(s) for {persona}.");
    println!("Peers already running `spriff watch` pick these up automatically within one");
    println!("poll cycle (no restart needed); others get them when they start watching.");
    Ok(())
}

/// Block until a peer posts (the delta becomes non-empty), then print it. Exits
/// 0 on a peer turn, 2 on timeout. The natural "wait for my turn" agent primitive.
fn cmd_wait(cfg: &Config, persona: &str, timeout_secs: u64, interval_secs: u64) -> Result<()> {
    let start = Instant::now();
    let interval = Duration::from_secs(interval_secs.max(1));
    eprintln!("[spriff] waiting for a peer turn as {persona}…");
    loop {
        let turns = current_delta(cfg, persona)?;
        if !turns.is_empty() {
            print_delta(&turns);
            return Ok(());
        }
        if timeout_secs > 0 && start.elapsed().as_secs() >= timeout_secs {
            eprintln!("[spriff] no peer turn within {timeout_secs}s — still your move, or your peer is quiet.");
            std::process::exit(2);
        }
        std::thread::sleep(interval);
    }
}

/// Supervise an agent: re-invoke `agent_cmd` for one turn whenever a peer posts.
///
/// THIS is what makes the loop ironclad. A CLI agent is not a daemon — left to
/// loop on `spriff wait` it can stop, time out, or crash and silently strand the
/// collaboration. Here spriff is the persistent process (itself OS-supervisable
/// via launchd/systemd) and the agent is invoked per turn, so a dead agent is
/// just re-spawned on the next peer turn. The agent does ONE turn and exits; we
/// dedup on the latest peer header so an agent that forgets to `ack` is not
/// re-invoked in a spin.
#[allow(clippy::too_many_arguments)]
fn cmd_serve(
    cfg: &Config,
    name: &str,
    persona: &str,
    idle_timeout: u64,
    poll: u64,
    kickoff: bool,
    agent_cmd: &[String],
    config: Option<PathBuf>,
) -> Result<()> {
    // Identity validation at the persistent entry point: refuse to supervise an
    // off-roster persona (it would act as someone the collaboration doesn't know).
    if !cfg
        .agents
        .iter()
        .any(|a| a.persona.eq_ignore_ascii_case(persona))
    {
        let roster: Vec<&str> = cfg.agents.iter().map(|a| a.persona.as_str()).collect();
        anyhow::bail!(
            "persona '{persona}' is not on '{name}' roster [{}]. Use --as <one of them>.",
            roster.join(", ")
        );
    }

    // Singleton: refuse to start if another live serve already drives this
    // persona (the duplicate-supervisor case that silently double-posts). Held
    // for the lifetime of this function; released on exit.
    let _lock = acquire_serve_lock(&cfg.board_path(), persona)?;

    let interval = Duration::from_secs(poll.max(1));
    const MAX_ATTEMPTS: u32 = 3;
    let mission = read_mission(&cfg.board_path());
    // A reviewer's declared lens (if any) focuses its supervised wake prompt — and
    // ONLY a reviewer's: an implementer must never get review-lens prompt text.
    let lens = if cfg.role_of(persona).as_deref() == Some("reviewer") {
        resolve_lens(cfg, &cfg.board_path(), persona)
    } else {
        None
    };
    eprintln!(
        "[spriff] serving {persona} on '{name}': invoking `{}` per peer turn (poll {poll}s, idle_timeout {idle_timeout}s){}",
        agent_cmd.join(" "),
        if mission.is_some() { " [drive-to-completion mission set]" } else { "" }
    );

    // Kickoff: an opening invocation so an implementer can LEAD and a reviewer can
    // catch up on anything already waiting. Completion is judged below, not here.
    if kickoff {
        eprintln!("[spriff] kickoff invocation…");
        run_agent(
            agent_cmd,
            name,
            persona,
            config.as_deref(),
            &kickoff_prompt(name, persona, mission.as_deref(), lens.as_deref()),
        );
    }

    let mut idle_since = Instant::now();
    let mut current_header = String::new();
    let mut attempts: u32 = 0;

    loop {
        let turns = current_delta(cfg, persona)?;
        let Some(latest) = turns.last() else {
            // Nothing waiting. Stand down if idle long enough.
            if idle_timeout > 0 && idle_since.elapsed().as_secs() >= idle_timeout {
                eprintln!("[spriff] no peer turn for {idle_timeout}s — standing down.");
                return Ok(());
            }
            std::thread::sleep(interval);
            continue;
        };
        let header = latest.header();
        if header != current_header {
            current_header = header.clone();
            attempts = 0;
        }
        if attempts >= MAX_ATTEMPTS {
            // The agent keeps failing to handle this turn. Don't spin or double-post:
            // back off loudly, then retry (never silently strand it).
            eprintln!("[spriff] WARNING: agent failed to handle this turn {MAX_ATTEMPTS}x; backing off. Turn: {header}");
            std::thread::sleep(interval * 5);
            attempts = 0;
            continue;
        }
        attempts += 1;

        eprintln!(
            "[spriff] peer turn -> invoking agent (attempt {attempts}, {} new)",
            turns.len()
        );
        run_agent(
            agent_cmd,
            name,
            persona,
            config.as_deref(),
            &wake_prompt(
                name,
                persona,
                turns.len(),
                mission.as_deref(),
                lens.as_deref(),
            ),
        );

        // COMPLETION POLICY (not "did the process exit 0"): did the turn actually
        // get consumed? Only then mark it handled — a failed/crashed invocation is
        // retried, never marked served. Cursor-based, so it's also restart-safe.
        let after = current_delta(cfg, persona)?;
        let still_pending = after.last().map(|t| t.header()) == Some(header.clone());
        if !still_pending {
            idle_since = Instant::now();
            current_header.clear();
            attempts = 0;
            continue;
        }
        // NOT consumed. We deliberately do NOT auto-ack on "the latest board post
        // is mine" — that heuristic can't distinguish a real, complete reply from a
        // progress note, a partial/failure post, or an intro, so it would silently
        // consume an unaddressed turn and HIDE work. (Alice's catch.) Instead we
        // retry with loud linear backoff; the wake prompt requires the agent to run
        // `spriff ack` only when it genuinely handled the turn, which is the single
        // authoritative completion signal.
        eprintln!("[spriff] turn not acked (attempt {attempts}); retrying after backoff.");
        std::thread::sleep(interval * attempts);
    }
}

/// Run the agent command once, appending the wake prompt as the final argument
/// and exporting the agent's identity (SPRIFF_COLLAB/SPRIFF_AS) so its `spriff`
/// commands resolve correctly regardless of the child's working directory.
/// Inherits stdio so the operator sees the agent work. Returns process success.
fn run_agent(
    agent_cmd: &[String],
    name: &str,
    persona: &str,
    config: Option<&std::path::Path>,
    prompt: &str,
) -> bool {
    let Some((prog, args)) = agent_cmd.split_first() else {
        eprintln!("[spriff] empty agent command");
        return false;
    };
    let mut cmd = std::process::Command::new(prog);
    cmd.args(args)
        .arg(prompt)
        .env("SPRIFF_COLLAB", name)
        .env("SPRIFF_AS", persona);
    // Propagate an explicit --config so the child's bare `spriff` commands resolve
    // the same non-registry config (Alice's catch: child only got SPRIFF_COLLAB).
    if let Some(path) = config {
        cmd.env("SPRIFF_CONFIG", path);
    }
    match cmd.status() {
        Ok(s) if s.success() => true,
        Ok(s) => {
            eprintln!("[spriff] agent exited with {s}");
            false
        }
        Err(e) => {
            eprintln!("[spriff] failed to run agent `{prog}`: {e}");
            false
        }
    }
}

/// The drive-to-completion clause shared by both supervisor prompts: the
/// always-on Definition of Done plus the collaboration's specific mission.
fn completion_clause(mission: Option<&str>) -> String {
    let m = match mission {
        Some(m) => format!(" MISSION: {m}."),
        None => String::new(),
    };
    format!(
        " This is a DRIVE-TO-COMPLETION collaboration: do NOT declare the work DONE (--status \
         DONE) until it is {DEFINITION_OF_DONE}. As reviewer, be the fresh, skeptical, \
         different-model eyes: actively try to BREAK the work and either name a specific defect \
         (file:line / the failing case) or say what you checked and why it holds — never a bare \
         'LGTM'; judge the artifact against the goal, not the author's explanation; advise, don't \
         rubber-stamp; and REJECT a premature DONE, naming the precise gap. As implementer, you \
         own the artifact — keep closing gaps. Keep the implement<->review loop going until every \
         part is genuinely shipped.{m}"
    )
}

/// Focus clause appended to a reviewer's supervisor prompt when it has a declared
/// review lens, so a multi-reviewer crew covers distinct failure modes rather than
/// overlapping (the "more agents only help if diverse" lesson).
fn lens_clause(lens: Option<&str>) -> String {
    match lens {
        Some(l) if !l.trim().is_empty() => format!(
            " YOUR REVIEW LENS is '{}': concentrate your scrutiny there — other reviewers cover \
             other angles, so depth on your lens beats shallow breadth.",
            l.trim()
        ),
        _ => String::new(),
    }
}

fn wake_prompt(
    name: &str,
    persona: &str,
    n: usize,
    mission: Option<&str>,
    lens: Option<&str>,
) -> String {
    format!(
        "You are {persona}, an agent on the spriff collaboration '{name}'. {n} new peer \
         turn(s) are waiting. Do exactly ONE turn, then EXIT — do NOT run `spriff wait` or \
         otherwise idle; the supervisor re-invokes you automatically when your peer next \
         posts, so idling only wastes tokens (any 'stay in the loop / spriff wait' note from \
         `spriff skill` does NOT apply while supervised). The turn: run `spriff inbox` to read \
         the new turn(s), do the work, post your reply with `spriff post -s \"...\" --status \
         <STATUS>` (body via a quoted heredoc, never -m \"...\"), then `spriff ack`.{}{}",
        completion_clause(mission),
        lens_clause(lens),
    )
}

fn kickoff_prompt(name: &str, persona: &str, mission: Option<&str>, lens: Option<&str>) -> String {
    format!(
        "You are {persona}, an agent on the spriff collaboration '{name}'. Assess with `spriff \
         status` and `spriff inbox`. If a peer turn is waiting, handle it (read, work, `spriff \
         post`, `spriff ack`). If you are the implementer and nothing is waiting, make the \
         opening move (post your intro/plan). Do exactly ONE turn, then EXIT — do NOT run \
         `spriff wait`; the supervisor re-invokes you when your peer posts.{}{}",
        completion_clause(mission),
        lens_clause(lens),
    )
}

fn cmd_status(cfg: &Config, name: &str, persona: &str) -> Result<()> {
    let board_path = cfg.board_path();
    let sc = Sidecars::derive(&board_path, persona);
    let st = state::WatchState::load(&sc.state);
    let size = board::board_size(&board_path);
    let last = board::last_turn_header(&board_path);
    let new = board::delta_since(&board_path, st.offset, persona)?.len();

    println!("collaboration: {name}");
    match cfg.role_of(persona) {
        Some(role) => println!("  persona:    {persona} ({role})"),
        None => println!("  persona:    {persona}"),
    }
    if let Some(c) = resolve_class(cfg, &board_path, persona) {
        println!("  class:      {c}");
    }
    // A lens is a reviewer-only concept — never surface one for the implementer.
    if cfg.role_of(persona).as_deref() == Some("reviewer") {
        if let Some(l) = resolve_lens(cfg, &board_path, persona) {
            println!("  lens:       {l}");
        }
    }
    println!("  board:      {} ({} bytes)", board_path.display(), size);
    println!("  cursor:     offset={}", st.offset);
    match last {
        Some((ts, author, subject)) => {
            let mine = author.to_lowercase() == persona.to_lowercase();
            println!("  last turn:  {ts} - {author} - {subject}");
            println!(
                "  your turn?  {}",
                if mine {
                    "no (you posted last)"
                } else {
                    "YES — peer posted last"
                }
            );
        }
        None => println!("  last turn:  (board empty)"),
    }
    if new > 0 {
        println!("  inbox:      {new} new peer turn(s) waiting — run `spriff inbox`");
    } else {
        println!("  inbox:      clear");
    }
    Ok(())
}

/// Is a `serve` supervisor currently holding the lock for this persona? Probes
/// the advisory lock non-destructively: if we CAN'T take it, someone holds it.
fn is_serve_running(board: &std::path::Path, persona: &str) -> bool {
    use fs2::FileExt;
    let path = serve_lock_path(board, persona);
    if !path.exists() {
        return false;
    }
    match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
    {
        // try_lock succeeded => nobody held it (we now hold it; drop releases it).
        Ok(f) => f.try_lock_exclusive().is_err(),
        Err(_) => false,
    }
}

/// Roster/identity warnings for a resolved persona (pure + testable).
fn identity_warnings(cfg: &Config, name: &str, persona: &str) -> Vec<String> {
    let mut w = Vec::new();
    let on_roster = cfg
        .agents
        .iter()
        .any(|a| a.persona.eq_ignore_ascii_case(persona));
    if !on_roster {
        w.push(format!(
            "resolved persona '{persona}' is NOT on the '{name}' roster — peer posts will look empty/wrong (use --as or $SPRIFF_AS)"
        ));
    }
    w
}

/// Health-check: aggregate the state an operator needs when something seems off —
/// registry, the cwd's resolved identity (the #1 footgun), board + per-persona
/// unread/cursor, whether a `serve` is running, and roster/identity warnings.
/// `as_persona` lets the loop-preserving `--as <you>` rule work on `doctor` too.
fn cmd_doctor(
    collab: Option<String>,
    config: Option<PathBuf>,
    as_persona: Option<String>,
) -> Result<()> {
    let mut warnings: Vec<String> = Vec::new();
    println!("spriff doctor\n=============");

    // Registry overview.
    println!("\nregistry: {}", registry::root().display());
    let names = registry::list();
    if names.is_empty() {
        println!("  (no collaborations registered)");
    }
    for n in &names {
        match Config::load(&registry::config_path(n)) {
            Ok(c) => {
                let roster: Vec<&str> = c.agents.iter().map(|a| a.persona.as_str()).collect();
                println!("  {n}  [{}]", roster.join(", "));
            }
            Err(_) => {
                println!("  {n}  (config unreadable)");
                warnings.push(format!("config for '{n}' is unreadable"));
            }
        }
    }

    // The active collaboration for THIS directory + identity (the common footgun).
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    println!("\nthis directory: {cwd}");
    match resolve(collab, config) {
        Err(e) => println!("  (no active collaboration: {e})"),
        Ok((cfg, name)) => {
            // Honour an explicit --as so doctor can confirm the safe invocation
            // pattern the join brief mandates (and show "identity from --as flag").
            let (persona, source) = resolve_persona_with_source(as_persona, &cfg);
            println!("  resolves to: '{name}' as '{persona}' (identity from {source})");
            warnings.extend(identity_warnings(&cfg, &name, &persona));

            let board = cfg.board_path();
            println!(
                "  board: {} ({} bytes)",
                board.display(),
                board::board_size(&board)
            );
            if let Some(m) = read_mission(&board) {
                let line = m.lines().next().unwrap_or("");
                let shown: String = line.chars().take(80).collect();
                println!(
                    "  mission: {shown}{}",
                    if line.len() > 80 { "…" } else { "" }
                );
            }
            println!("  agents:");
            let mut roster_classes: Vec<(String, Option<String>)> = Vec::new();
            let mut reviewer_lenses: Vec<(String, Option<String>)> = Vec::new();
            for a in &cfg.agents {
                let sc = Sidecars::derive(&board, &a.persona);
                let st = state::WatchState::load(&sc.state);
                let unread = board::delta_since(&board, st.offset, &a.persona)
                    .map(|t| t.len())
                    .unwrap_or(0);
                let serving = if is_serve_running(&board, &a.persona) {
                    " · serve RUNNING"
                } else {
                    ""
                };
                let role = a.role.clone().unwrap_or_default();
                let class = resolve_class(&cfg, &board, &a.persona);
                let class_str = class
                    .as_deref()
                    .map(|c| format!(" · class={c}"))
                    .unwrap_or_default();
                let is_reviewer = role.eq_ignore_ascii_case("reviewer");
                // Lens is a reviewer-only concept — only resolve/show it for reviewers.
                let lens = if is_reviewer {
                    resolve_lens(&cfg, &board, &a.persona)
                } else {
                    None
                };
                let lens_str = lens
                    .as_deref()
                    .map(|l| format!(" · lens={l}"))
                    .unwrap_or_default();
                println!(
                    "    {} ({role}): {unread} unread · cursor={}{serving}{class_str}{lens_str}",
                    a.persona, st.offset
                );
                roster_classes.push((a.persona.clone(), class));
                if is_reviewer {
                    reviewer_lenses.push((a.persona.clone(), lens));
                }
            }
            // Heterogeneity: collision is a warning; a PARTIAL declaration is
            // ALSO a warning (the check is inconclusive, not clean — Alice's
            // catch); an all-undeclared roster is just a soft nudge.
            match heterogeneity_status(&roster_classes) {
                Heterogeneity::Collision(msg) => warnings.push(msg),
                Heterogeneity::Unverified(missing) => warnings.push(format!(
                    "heterogeneity UNVERIFIED — {} ha{} no model class declared; a single \
                     unknown leaves the same-class risk unassessed. Declare it: `spriff join \
                     --role <r> --class <claude|gpt|…>`",
                    missing.join(", "),
                    if missing.len() == 1 { "s" } else { "ve" }
                )),
                Heterogeneity::Undeclared => println!(
                    "  heterogeneity: model classes not declared — `spriff join --role <r> \
                     --class <claude|gpt|…>` lets spriff flag a same-class pairing"
                ),
                Heterogeneity::Healthy => {}
            }
            // Review lenses: only relevant with 2+ reviewers — flag a shared lens
            // (redundant coverage) or missing lenses (overlap risk).
            if let Some(adv) = lens_advisory(&reviewer_lenses) {
                warnings.push(adv);
            }
        }
    }

    println!();
    if warnings.is_empty() {
        println!("✓ no problems detected");
    } else {
        println!("⚠ {} warning(s):", warnings.len());
        for w in &warnings {
            println!("  - {w}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn mission_path_derivation() {
        assert_eq!(
            mission_path(Path::new("/x/foo.board.md")),
            PathBuf::from("/x/foo.mission.md")
        );
        assert_eq!(
            mission_path(Path::new("/x/bar.md")),
            PathBuf::from("/x/bar.mission.md")
        );
    }

    #[test]
    fn class_path_derivation() {
        // Sidecars lowercase the persona, so write (join --class) and read
        // (doctor/status) resolve to the same file regardless of name casing.
        assert_eq!(
            class_path(Path::new("/x/foo.board.md"), "Abbey"),
            PathBuf::from("/x/foo.abbey.class")
        );
    }

    #[test]
    fn heterogeneity_status_classifies_all_four_outcomes() {
        let pair = |a: &str, ca: Option<&str>, b: &str, cb: Option<&str>| {
            vec![
                (a.to_string(), ca.map(str::to_string)),
                (b.to_string(), cb.map(str::to_string)),
            ]
        };
        // Distinct, both declared -> Healthy.
        assert_eq!(
            heterogeneity_status(&pair("Abbey", Some("claude"), "Alice", Some("gpt"))),
            Heterogeneity::Healthy
        );
        // Same class (case/space-insensitive) -> Collision naming both + the class.
        match heterogeneity_status(&pair("Abbey", Some("Claude"), "Alice", Some(" claude "))) {
            Heterogeneity::Collision(w) => {
                assert!(w.contains("Abbey") && w.contains("Alice") && w.contains("claude"));
            }
            other => panic!("expected Collision, got {other:?}"),
        }
        // PARTIAL: one declared, one not -> Unverified naming the missing peer
        // (the bug Alice caught: this must NOT read as clean).
        assert_eq!(
            heterogeneity_status(&pair("Abbey", Some("claude"), "Alice", None)),
            Heterogeneity::Unverified(vec!["Alice".to_string()])
        );
        // None declared -> Undeclared (soft nudge, not a warning).
        assert_eq!(
            heterogeneity_status(&pair("Abbey", None, "Alice", None)),
            Heterogeneity::Undeclared
        );
        // An empty-string class counts as undeclared, not a match.
        assert_eq!(
            heterogeneity_status(&pair("Abbey", Some("  "), "Alice", Some("  "))),
            Heterogeneity::Undeclared
        );
    }

    #[test]
    fn completion_clause_injects_dod_and_mission() {
        let with = completion_clause(Some("ship checkout"));
        assert!(with.contains("DRIVE-TO-COMPLETION"));
        assert!(with.contains("feature-complete"));
        assert!(with.contains("MISSION: ship checkout."));
        let without = completion_clause(None);
        assert!(without.contains("DRIVE-TO-COMPLETION"));
        assert!(!without.contains("MISSION:"));
    }

    #[test]
    fn completion_clause_carries_the_skeptical_review_contract() {
        // The research-backed review discipline must reach even headless `serve`
        // agents via the supervisor prompt: try to break it, no bare rubber-stamp,
        // judge the artifact not the author's story, advise rather than average.
        let c = completion_clause(None);
        assert!(
            c.contains("try to BREAK"),
            "must tell the reviewer to break it"
        );
        assert!(c.contains("LGTM"), "must forbid a bare LGTM");
        assert!(
            c.contains("not the author") || c.contains("artifact against the goal"),
            "must frame review as artifact-vs-goal, not the author's explanation"
        );
        assert!(
            c.contains("own the artifact"),
            "must keep ownership asymmetric"
        );
    }

    #[test]
    fn wake_prompt_tells_supervised_agent_to_exit_not_wait() {
        let p = wake_prompt("demo", "Alice", 1, None, None);
        assert!(p.contains("EXIT"));
        assert!(p.contains("do NOT run `spriff wait`"));
        assert!(p.contains("spriff ack"));
    }

    #[test]
    fn wake_prompt_focuses_a_reviewer_on_its_lens() {
        // No lens -> no lens clause; a lens -> the supervised reviewer is told to
        // concentrate there (so a multi-reviewer crew covers distinct angles).
        assert!(!wake_prompt("demo", "Alice", 1, None, None).contains("REVIEW LENS"));
        let p = wake_prompt("demo", "Alice", 1, None, Some("security"));
        assert!(p.contains("REVIEW LENS is 'security'"));
        // Blank lens is treated as none.
        assert!(!wake_prompt("demo", "Alice", 1, None, Some("  ")).contains("REVIEW LENS"));
    }

    #[test]
    fn lens_advisory_only_fires_for_multi_reviewer_crews() {
        let rev = |a: &str, la: Option<&str>, b: &str, lb: Option<&str>| {
            vec![
                (a.to_string(), la.map(str::to_string)),
                (b.to_string(), lb.map(str::to_string)),
            ]
        };
        // Fewer than two reviewers -> never advise.
        assert!(lens_advisory(&[("Alice".to_string(), None)]).is_none());
        assert!(lens_advisory(&[("Alice".to_string(), Some("security".to_string()))]).is_none());
        // Two reviewers, distinct lenses -> healthy.
        assert!(lens_advisory(&rev(
            "Alice",
            Some("security"),
            "Annie",
            Some("correctness")
        ))
        .is_none());
        // Same lens (case/space-insensitive) -> warn naming both + the lens.
        let w = lens_advisory(&rev("Alice", Some("Security"), "Annie", Some(" security ")))
            .expect("shared lens must warn");
        assert!(w.contains("Alice") && w.contains("Annie") && w.contains("security"));
        // A missing lens among 2+ reviewers -> nudge to assign distinct lenses.
        assert!(lens_advisory(&rev("Alice", Some("security"), "Annie", None)).is_some());
    }

    #[test]
    fn serve_lock_path_is_per_persona() {
        assert_eq!(
            serve_lock_path(Path::new("/x/foo.board.md"), "Alice"),
            PathBuf::from("/x/foo.alice.serve.lock")
        );
    }

    #[test]
    fn serve_lock_is_exclusive_then_releasable() {
        let dir = std::env::temp_dir().join(format!("spriff-lock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let board = dir.join("t.board.md");
        std::fs::write(&board, "x").unwrap();

        let lock = acquire_serve_lock(&board, "Alice").unwrap();
        // A second acquire while the kernel lock is held must fail.
        assert!(acquire_serve_lock(&board, "Alice").is_err());
        drop(lock); // releases the OS lock
                    // After release, acquiring again succeeds.
        assert!(acquire_serve_lock(&board, "Alice").is_ok());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn slugify_is_stable_and_clean() {
        assert_eq!(
            slugify("spriff doctor health-check"),
            "spriff-doctor-health-check"
        );
        assert_eq!(
            slugify("  Fix the Checkout Flow!! "),
            "fix-the-checkout-flow"
        );
        assert_eq!(slugify("a/b\\c"), "a-b-c");
        // The whole point: the same text always yields the same slug.
        assert_eq!(slugify("My Project"), slugify("my   project"));
        // Never empty.
        assert_eq!(slugify("***"), "project");
    }

    #[test]
    fn resolve_my_slot_binds_reviewer_to_its_named_slot() {
        let roster = vec![
            "Abbey".to_string(),
            "Alice".to_string(),
            "Annie".to_string(),
        ];
        // Implementer keeps slot 0.
        assert_eq!(resolve_my_slot(false, Some("Abbey"), 0, &roster), 0);
        // The bug fix: a reviewer naming the SECOND reviewer binds to slot 2.
        assert_eq!(resolve_my_slot(true, Some("Annie"), 1, &roster), 2);
        // First reviewer -> slot 1; case-insensitive.
        assert_eq!(resolve_my_slot(true, Some("alice"), 1, &roster), 1);
        // No --as -> default first-reviewer slot.
        assert_eq!(resolve_my_slot(true, None, 1, &roster), 1);
        // A reviewer naming the executor (slot 0) falls back to default so the
        // caller's cross-role validation rejects it; an unknown name does too.
        assert_eq!(resolve_my_slot(true, Some("Abbey"), 1, &roster), 1);
        assert_eq!(resolve_my_slot(true, Some("Zed"), 1, &roster), 1);
    }

    #[test]
    fn mission_eq_is_lenient_on_form_strict_on_meaning() {
        // Same goal, different surface form (case + whitespace) -> equal.
        assert!(mission_eq(
            "Fix the checkout flow",
            "fix   the checkout flow"
        ));
        assert!(mission_eq("  ship it  ", "ship it"));
        // Alice's collision case: two prompts that slugify to the SAME board
        // (`a-b`) but mean different things must NOT be treated as the same goal.
        assert_eq!(slugify("a/b"), slugify("a b")); // same board…
        assert!(!mission_eq("a/b", "a b")); // …different goal.
    }

    #[test]
    fn peer_join_command_uses_the_real_rendezvous_key() {
        // --project was the key: the peer passes the same --project (slugifies
        // back to the same board) and must NOT be handed --collab.
        let c = peer_join_command("reviewer", "fix-checkout", "fix checkout", false);
        assert_eq!(c, "spriff join --role reviewer --project \"fix checkout\"");
        // --collab forced the slug: the goal text would slugify ELSEWHERE, so the
        // peer command MUST carry --collab to land on this board (Alice's catch).
        let c = peer_join_command("implementer", "a-b", "totally different", true);
        assert!(c.contains("--collab a-b"));
        assert_eq!(
            c,
            "spriff join --role implementer --collab a-b --project \"totally different\""
        );
        // The bug regression guard: the override command must NOT be the bare
        // --project form that points the peer at a different board.
        assert_ne!(
            c,
            "spriff join --role implementer --project \"totally different\""
        );
    }

    #[test]
    fn plan_mission_seed_keep_reject() {
        // No mission yet -> seed it from the project text.
        assert_eq!(
            plan_mission(None, "fix checkout", false, "fix-checkout").unwrap(),
            MissionPlan::Seed
        );
        // Mission already names this goal (case/space-insensitive) -> keep.
        assert_eq!(
            plan_mission(
                Some("Fix Checkout"),
                "fix   checkout",
                false,
                "fix-checkout"
            )
            .unwrap(),
            MissionPlan::Keep
        );
        // Mission names a DIFFERENT goal on the same slug -> hard error that names
        // both goals and the remediation paths.
        let err = plan_mission(Some("a/b"), "a b", false, "a-b")
            .unwrap_err()
            .to_string();
        assert!(err.contains("a b") && err.contains("a/b") && err.contains("a-b"));
        assert!(err.contains("--collab"));
        // …unless --collab forced the slug: the operator joined intentionally.
        assert_eq!(
            plan_mission(Some("a/b"), "a b", true, "a-b").unwrap(),
            MissionPlan::Keep
        );
    }

    #[test]
    fn identity_warnings_flags_off_roster_only() {
        let cfg: Config = toml::from_str(
            "board = \"/x/b.md\"\n[[agents]]\npersona = \"Abbey\"\n[[agents]]\npersona = \"Alice\"\n",
        )
        .unwrap();
        // On-roster (case-insensitive) -> no warning.
        assert!(identity_warnings(&cfg, "demo", "Alice").is_empty());
        assert!(identity_warnings(&cfg, "demo", "abbey").is_empty());
        // Off-roster -> exactly one warning naming the persona.
        let w = identity_warnings(&cfg, "demo", "Vera");
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("Vera"));
    }

    #[test]
    fn serve_lock_acquirable_when_file_exists_but_unlocked() {
        // Simulates a crashed/killed serve: the lock FILE persists with a stale
        // pid, but no process holds the kernel lock — so a fresh serve acquires it
        // with no path-based reclaim (the OS released the dead process's lock).
        let dir = std::env::temp_dir().join(format!("spriff-lock2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let board = dir.join("t.board.md");
        std::fs::write(&board, "x").unwrap();

        let p = serve_lock_path(&board, "Bob");
        std::fs::create_dir_all(p.parent().unwrap()).ok();
        std::fs::write(&p, "999999").unwrap(); // leftover file, nobody flocking it
        assert!(acquire_serve_lock(&board, "Bob").is_ok());

        std::fs::remove_dir_all(&dir).ok();
    }
}
