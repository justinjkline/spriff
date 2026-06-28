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
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Join {
            role,
            collab,
            repo,
            agents,
        } => cmd_join(&role, collab, repo, agents),
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

/// The persona to act as: `--as` flag → `$SPRIFF_AS` → `.spriff` marker `as=` →
/// the collaboration's executor. `spriff join` writes the marker, so after
/// joining an agent's bare commands act as the right persona automatically.
fn resolve_persona(explicit: Option<String>, cfg: &Config) -> String {
    if let Some(p) = explicit {
        return p;
    }
    if let Ok(p) = std::env::var("SPRIFF_AS") {
        if !p.is_empty() {
            return p;
        }
    }
    if let Some(p) = registry::marker_field("as") {
        return p;
    }
    cfg.default_persona()
}

/// Onboard an agent: auto-create/join the collaboration, claim the role's
/// persona, write a repo marker so later commands need no flags, and print the
/// protocol + first move. The single command an agent runs to start.
fn cmd_join(
    role: &str,
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

    // Create it if it doesn't exist yet (first agent to join wins; idempotent).
    if !registry::config_path(&name).exists() {
        let roster = build_roster(agents, None, &[]);
        create_collab(&name, &roster, None)?;
    }
    let cfg = Config::load(&registry::config_path(&name))?;

    // Claim the role's persona: implementer = executor (index 0), reviewer =
    // first reviewer (index 1).
    let persona = if is_impl {
        cfg.agents.first()
    } else {
        cfg.agents.get(1)
    }
    .map(|a| a.persona.clone())
    .ok_or_else(|| anyhow::anyhow!("collaboration '{name}' has no slot for role '{role}'"))?;

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
    println!("\n──────────────────────────── your first move ────────────────────────────");
    if is_impl {
        println!("1. Introduce yourself + declare the files you're touching:");
        println!("     spriff post -s \"intro\" --status FYI <<'EOF'");
        println!("     <who you are + your plan>");
        println!("     EOF");
        println!(
            "     spriff touching <path> [<path>...]   # so your reviewer is woken on your edits"
        );
        println!("2. Implement a coherent chunk, then hand off for review:");
        println!("     spriff post -s \"<what you did>\" --status NEEDS-REVIEW <<'EOF'");
        println!("     <summary + files/lines to scrutinize>");
        println!("     EOF");
        println!("3. spriff wait   → review their reply → respond → spriff wait → … until DONE.");
    } else {
        println!("1. Introduce yourself:");
        println!("     spriff post -s \"intro\" --status FYI <<'EOF'");
        println!("     <who you are + your review bar>");
        println!("     EOF");
        println!(
            "2. Block until the implementer hands off, review the code they reference, reply:"
        );
        println!("     spriff wait");
        println!("     spriff post -s \"review: <area>\" --status NEEDS-REVIEW <<'EOF'");
        println!("     <file:line + the concrete issue, or LGTM with reasoning>");
        println!("     EOF");
        println!("     spriff ack");
        println!("3. spriff wait again → review next handoff → … until DONE.");
    }
    println!("\n⟳ THE GOLDEN RULE: your turn is NOT over until the task is DONE. After every");
    println!("  post, run `spriff wait` to block for your peer — never go idle, or the loop");
    println!("  stalls (your peer's reply will just sit unread in your inbox).");
    println!("✍ Always pipe post bodies via stdin/heredoc (<<'EOF'), never -m \"…\" — backticks,");
    println!("  $, and quotes in -m get mangled by the shell.");
    println!("  Re-read the protocol anytime: spriff skill");
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
    println!(
        "declared {added} new path(s) for {persona} in {}",
        sc.watchpaths.display()
    );
    println!("Your peers will be woken on edits there once they run `spriff watch`.");
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
