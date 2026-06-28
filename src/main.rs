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
use std::io::{Read, Write};
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
        /// Collaboration name. Default: the single registered one, else "default".
        #[arg(long)]
        collab: Option<String>,
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
            collab,
            repo,
            agents,
        } => cmd_join(&role, as_name, with, collab, repo, agents),
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

/// The serve singleton-lock file for one persona on one board:
/// `<base>.<persona>.serve.lock`, next to the other sidecars.
fn serve_lock_path(board: &std::path::Path, persona: &str) -> PathBuf {
    let state = Sidecars::derive(board, persona).state; // <base>.<persona>.watch.state
    let s = state.to_string_lossy();
    PathBuf::from(format!("{}serve.lock", s.trim_end_matches("watch.state")))
}

/// Is `pid` a live process? Uses `kill -0` (POSIX), no extra dependency.
fn pid_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Singleton lock guarding one `serve` per (collab, persona). Removes the lock on
/// drop (normal exit / panic); a hard kill leaves a stale lock, which the next
/// acquire reclaims after checking the recorded pid is dead.
struct ServeLock {
    path: PathBuf,
}
impl Drop for ServeLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Acquire the serve singleton lock, or error if another LIVE serve holds it.
/// This is the ironclad guard against two supervisors driving the same persona
/// (which silently double-posts and races the cursor).
fn acquire_serve_lock(board: &std::path::Path, persona: &str) -> Result<ServeLock> {
    let path = serve_lock_path(board, persona);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    loop {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut f) => {
                write!(f, "{}", std::process::id())?;
                return Ok(ServeLock { path });
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let held = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|s| s.trim().parse::<u32>().ok());
                match held {
                    Some(pid) if pid_alive(pid) => anyhow::bail!(
                        "another `spriff serve` is already running as {persona} (pid {pid}). \
                         Only one supervisor per persona — stop it first (kill {pid})."
                    ),
                    // Stale lock from a hard-killed serve: reclaim and retry.
                    _ => {
                        std::fs::remove_file(&path).ok();
                    }
                }
            }
            Err(e) => return Err(e.into()),
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

/// Onboard an agent: auto-create/join the collaboration, claim the role's
/// persona, write a repo marker so later commands need no flags, and print the
/// protocol + first move. The single command an agent runs to start.
fn cmd_join(
    role: &str,
    as_name: Option<String>,
    with: Option<String>,
    collab: Option<String>,
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

    // Resolve which collaboration to join: explicit → marker/env → the single
    // registered one → "default" (created on demand). So two agents told only
    // their role land on the same board with zero coordination.
    let name = collab
        .or_else(|| {
            std::env::var("SPRIFF_COLLAB")
                .ok()
                .filter(|s| !s.is_empty())
        })
        .or_else(|| registry::marker_field("collab"))
        .or_else(|| {
            let l = registry::list();
            (l.len() == 1).then(|| l[0].clone())
        })
        .unwrap_or_else(|| "default".to_string());

    // Roster slots are FIXED: executor=0, reviewer=1. Only the *source* of each
    // name varies by role — `--as` names MY slot, `--with` names my peer's.
    // (Alice's catch: a role-dependent slot tuple double-switched and mis-assigned.)
    let (my_slot, peer_slot) = if is_impl {
        (0usize, 1usize)
    } else {
        (1usize, 0usize)
    };

    // Create it if it doesn't exist yet (first agent to join wins; idempotent).
    if !registry::config_path(&name).exists() {
        let mut roster = build_roster(agents.max(2), None, &[]);
        if let Some(n) = &as_name {
            roster[my_slot] = n.clone();
        }
        if let Some(n) = &with {
            roster[peer_slot] = n.clone();
        }
        create_collab(&name, &roster, None)?;
    }
    let cfg = Config::load(&registry::config_path(&name))?;

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

    let role_label = if is_impl { "implementer" } else { "reviewer" };
    let peers = cfg.peers(&persona).join(", ");
    println!("════════════════════════════════════════════════════════════════");
    println!("  You are {persona} — the {role_label} on collaboration '{name}'.");
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
    println!("\n⟳ GOLDEN RULE: your turn is NOT over until the task is DONE. After every post,");
    println!("  run `spriff wait` to block for your peer. NEVER go idle — a reply left unread");
    println!("  stalls the loop; nothing will re-summon you.");
    println!("✍ Post bodies via heredoc (<<'EOF' … EOF), never -m \"…\" (the shell mangles");
    println!("  backticks/$/quotes). Re-read the protocol anytime: spriff skill");

    // Live situation — the agent's actual next action, computed now. This is what
    // makes a one-line human prompt sufficient: spriff itself shows what's waiting
    // (handle it) or that nothing is (lead / block), whether first-join or resume.
    println!("\n──────────────────────────────── right now ────────────────────────────────");
    let delta = current_delta(&cfg, &persona).unwrap_or_default();
    if !delta.is_empty() {
        println!("A turn is already waiting for you — handle it now:\n");
        print_delta(&delta);
    } else if is_impl {
        println!("You lead — nothing is waiting. Do this now:");
        println!("  1. Intro + declare your files:");
        println!("       spriff post -s \"intro\" --status FYI <<'EOF'");
        println!("       <who you are + your plan>");
        println!("       EOF");
        println!("       spriff touching <path> [<path>...]");
        println!("  2. Implement a chunk, hand off, then wait:");
        println!("       spriff post -s \"<what you did>\" --status NEEDS-REVIEW <<'EOF'");
        println!("       <summary + files/lines to review>");
        println!("       EOF");
        println!("       spriff wait      # then review reply → respond → wait → … until DONE");
    } else {
        println!("Nothing waiting yet. Block until the implementer hands off:");
        println!("       spriff wait");
        println!("  then review the code they reference, respond via heredoc, `spriff ack`,");
        println!("  and `spriff wait` again — loop until DONE.");
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
    let st = state::WatchState::load(&sc.state);
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
            &kickoff_prompt(name, persona, mission.as_deref()),
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
            &wake_prompt(name, persona, turns.len(), mission.as_deref()),
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
         DONE) until it is {DEFINITION_OF_DONE}. As reviewer, REJECT a premature DONE and name \
         the precise gap; as implementer, keep closing gaps. Keep the implement<->review loop \
         going until every part is genuinely shipped.{m}"
    )
}

fn wake_prompt(name: &str, persona: &str, n: usize, mission: Option<&str>) -> String {
    format!(
        "You are {persona}, an agent on the spriff collaboration '{name}'. {n} new peer \
         turn(s) are waiting. Do exactly ONE turn, then EXIT — do NOT run `spriff wait` or \
         otherwise idle; the supervisor re-invokes you automatically when your peer next \
         posts, so idling only wastes tokens (any 'stay in the loop / spriff wait' note from \
         `spriff skill` does NOT apply while supervised). The turn: run `spriff inbox` to read \
         the new turn(s), do the work, post your reply with `spriff post -s \"...\" --status \
         <STATUS>` (body via a quoted heredoc, never -m \"...\"), then `spriff ack`.{}",
        completion_clause(mission)
    )
}

fn kickoff_prompt(name: &str, persona: &str, mission: Option<&str>) -> String {
    format!(
        "You are {persona}, an agent on the spriff collaboration '{name}'. Assess with `spriff \
         status` and `spriff inbox`. If a peer turn is waiting, handle it (read, work, `spriff \
         post`, `spriff ack`). If you are the implementer and nothing is waiting, make the \
         opening move (post your intro/plan). Do exactly ONE turn, then EXIT — do NOT run \
         `spriff wait`; the supervisor re-invokes you when your peer posts.{}",
        completion_clause(mission)
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
    fn wake_prompt_tells_supervised_agent_to_exit_not_wait() {
        let p = wake_prompt("demo", "Alice", 1, None);
        assert!(p.contains("EXIT"));
        assert!(p.contains("do NOT run `spriff wait`"));
        assert!(p.contains("spriff ack"));
    }

    #[test]
    fn serve_lock_path_is_per_persona() {
        assert_eq!(
            serve_lock_path(Path::new("/x/foo.board.md"), "Alice"),
            PathBuf::from("/x/foo.alice.serve.lock")
        );
    }

    #[test]
    fn serve_lock_is_exclusive_then_reclaimable() {
        let dir = std::env::temp_dir().join(format!("spriff-lock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let board = dir.join("t.board.md");
        std::fs::write(&board, "x").unwrap();

        let lock = acquire_serve_lock(&board, "Alice").unwrap();
        // A second acquire while the first is held (our own live pid) must fail.
        assert!(acquire_serve_lock(&board, "Alice").is_err());
        drop(lock); // release
                    // After release, acquiring again succeeds.
        assert!(acquire_serve_lock(&board, "Alice").is_ok());

        // A stale lock (dead pid) is reclaimed.
        let p = serve_lock_path(&board, "Bob");
        std::fs::write(&p, "999999999").unwrap(); // not a live pid
        assert!(acquire_serve_lock(&board, "Bob").is_ok());

        std::fs::remove_dir_all(&dir).ok();
    }
}
