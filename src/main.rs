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

/// The agent-facing protocol, embedded so `spriff skill` is always in sync
/// with the installed binary — one source of truth, reachable identically from
/// every CLI agent (Claude, Codex, …). No copy-pasted, drifting preambles.
const SKILL: &str = include_str!("../SKILL.md");

#[derive(Parser)]
#[command(
    name = "spriff",
    version,
    about = "Durable, event-driven coordination for collaborating AI agents over a shared markdown board."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
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
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
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
            let persona = as_persona.unwrap_or_else(|| cfg.default_persona());
            watcher::run(&cfg, &persona)
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
            let persona = as_persona.unwrap_or_else(|| cfg.default_persona());
            cmd_post(&cfg, &persona, &subject, &status, &to, message)
        }
        Cmd::Inbox {
            collab,
            config,
            as_persona,
        } => {
            let (cfg, _name) = resolve(collab, config)?;
            let persona = as_persona.unwrap_or_else(|| cfg.default_persona());
            cmd_inbox(&cfg, &persona)
        }
        Cmd::Ack {
            collab,
            config,
            as_persona,
        } => {
            let (cfg, _name) = resolve(collab, config)?;
            let persona = as_persona.unwrap_or_else(|| cfg.default_persona());
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
            let persona = as_persona.unwrap_or_else(|| cfg.default_persona());
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
    }
}

/// Resolve (config, name) from optional flags, honouring the registry priority
/// order. An explicit `--config <path>` short-circuits name resolution.
fn resolve(collab: Option<String>, config: Option<PathBuf>) -> Result<(Config, String)> {
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

fn cmd_init(
    name: &str,
    agents: usize,
    letter: Option<char>,
    personas: &[String],
    board: Option<PathBuf>,
) -> Result<()> {
    let dir = registry::collab_dir(name);
    std::fs::create_dir_all(&dir)?;
    let board_path = board.unwrap_or_else(|| registry::board_path(name));
    board::seed_board(&board_path, name)?;

    // Resolve the roster: explicit --persona names win; otherwise auto-assign by
    // convention (shared first letter, executor lowest, reviewers ascending).
    let roster: Vec<String> = if !personas.is_empty() {
        personas.to_vec()
    } else {
        let n = agents.max(2);
        let chosen = letter.unwrap_or_else(|| names::pick_letter(&used_letters()));
        names::roster(chosen, n)
    };

    // Build the config TOML by hand (we only derive Deserialize on Config).
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

    println!("Created collaboration '{name}':");
    println!("  config: {}", cfg_path.display());
    println!("  board:  {}", board_path.display());
    println!("  roster:");
    for (i, persona) in roster.iter().enumerate() {
        let role = if i == 0 { "executor" } else { "reviewer" };
        println!("    {persona}  ({role})");
    }
    println!();
    println!("Next:");
    println!(
        "  1. Edit {} to add each agent's watchpaths.",
        cfg_path.display()
    );
    println!("  2. Each agent starts its watcher:  spriff watch --collab {name} --as <persona>");
    println!("  3. In a repo, optionally:           echo 'collab={name}' > .spriff");
    println!("  4. Onboard any CLI agent:           spriff skill");
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
        &status.to_uppercase(),
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

/// Show the pending peer delta, computed LIVE from the consume cursor — works
/// whether or not a watcher is running.
fn cmd_inbox(cfg: &Config, persona: &str) -> Result<()> {
    let board_path = cfg.board_path();
    let sc = Sidecars::derive(&board_path, persona);
    let st = state::WatchState::load(&sc.state);
    let turns = board::delta_since(&board_path, st.offset, persona)?;
    if turns.is_empty() {
        if pending::is_raised(&sc) {
            println!("inbox clear — no new peer turns (stale watcher flag set; run `spriff ack` to clear).");
        } else {
            println!("inbox clear — no new peer turns. Not your turn.");
        }
        return Ok(());
    }
    println!("{} new turn(s) since your last ack:\n", turns.len());
    for t in &turns {
        println!("{}", t.header());
        if !t.body.is_empty() {
            println!("\n{}", t.body);
        }
        println!("\n---");
    }
    println!("\nRespond:  spriff post -s \"<subject>\" --status <STATUS> -m \"<reply>\"");
    println!("Then:     spriff ack");
    Ok(())
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
