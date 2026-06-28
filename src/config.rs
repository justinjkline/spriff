//! Per-collaboration configuration.
//!
//! ONE parameterized runtime replaces the bespoke per-persona scripts: all the
//! variation that used to live in copy-pasted shell (board path, the roster of
//! agents, what code of theirs to watch, timing) now lives in a small TOML file.
//! The binary is constant; only this file changes per collaboration.
//!
//! The roster is an ordered list of agents (executor first, reviewers after).
//! Any command acts AS one persona via `--as`; everyone else is a peer. This is
//! what lets the same protocol scale from a 2-agent pair to N agents with one
//! shared config.

use crate::paths::expand_tilde;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    /// Absolute (or `~`-relative) path to the shared markdown board.
    pub board: PathBuf,
    /// The roster, ordered: index 0 is the executor, the rest are reviewers.
    #[serde(default)]
    pub agents: Vec<Agent>,
    #[serde(default)]
    pub watch: WatchConfig,
    #[serde(default)]
    pub rollup: RollupConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Agent {
    pub persona: String,
    /// "executor" | "reviewer" (informational; order is what's authoritative).
    #[serde(default)]
    pub role: Option<String>,
    /// Source paths owned by this agent, watched recursively by its peers.
    #[serde(default)]
    pub watchpaths: Vec<PathBuf>,
    /// Model class driving this agent (e.g. "claude", "gpt", "gemini", "glm").
    /// Optional and informational — used by `doctor` to flag a same-class pairing,
    /// which forfeits most of the error-decorrelation gain heterogeneity buys. A
    /// live `spriff join --class <x>` sidecar takes precedence over this seed.
    #[serde(default)]
    pub class: Option<String>,
    /// Review lens for a reviewer in a multi-reviewer crew (e.g. "correctness",
    /// "security", "regressions"). Distinct lenses make extra reviewers add
    /// *diversity* instead of redundancy. A live `spriff join --lens <x>` sidecar
    /// takes precedence over this seed.
    #[serde(default)]
    pub lens: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct WatchConfig {
    /// Quiet window (ms) the filesystem must be still before a burst of changes
    /// is treated as "settled" and a single wake is raised. Coalesces a
    /// multi-file save / git operation into ONE signal instead of dozens.
    #[serde(default = "default_settle_ms")]
    pub settle_ms: u64,
    /// Safety re-check interval (ms). Even with FS events, we re-scan on this
    /// cadence so a missed/dropped event can never strand a pending peer post.
    #[serde(default = "default_poll_ms")]
    pub poll_ms: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RollupConfig {
    /// When the live board exceeds this many bytes, older turns are folded into
    /// a sibling `*.archive.md` so the live board — and therefore every agent's
    /// working context — stays bounded no matter how long the collaboration runs.
    #[serde(default = "default_rollup_bytes")]
    pub max_live_bytes: u64,
    /// How many of the most recent turns to KEEP live during a rollup.
    #[serde(default = "default_keep_turns")]
    pub keep_recent_turns: usize,
}

fn default_settle_ms() -> u64 {
    600
}
fn default_poll_ms() -> u64 {
    3000
}
fn default_rollup_bytes() -> u64 {
    96 * 1024
}
fn default_keep_turns() -> usize {
    30
}

impl Default for WatchConfig {
    fn default() -> Self {
        WatchConfig {
            settle_ms: default_settle_ms(),
            poll_ms: default_poll_ms(),
        }
    }
}

impl Default for RollupConfig {
    fn default() -> Self {
        RollupConfig {
            max_live_bytes: default_rollup_bytes(),
            keep_recent_turns: default_keep_turns(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Config> {
        let path = expand_tilde(path);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let cfg: Config =
            toml::from_str(&text).with_context(|| format!("parsing TOML {}", path.display()))?;
        Ok(cfg)
    }

    /// The fully-expanded board path.
    pub fn board_path(&self) -> PathBuf {
        expand_tilde(&self.board)
    }

    /// The configured role of a persona ("executor"/"reviewer"), if any.
    pub fn role_of(&self, persona: &str) -> Option<String> {
        let p = persona.to_lowercase();
        self.agents
            .iter()
            .find(|a| a.persona.to_lowercase() == p)
            .and_then(|a| a.role.clone())
    }

    /// The default persona to act as: the executor (first in the roster).
    pub fn default_persona(&self) -> String {
        self.agents
            .first()
            .map(|a| a.persona.clone())
            .unwrap_or_else(|| "agent".to_string())
    }

    /// Every other agent's persona (the peers of `me`).
    pub fn peers(&self, me: &str) -> Vec<String> {
        let me_lc = me.to_lowercase();
        self.agents
            .iter()
            .filter(|a| a.persona.to_lowercase() != me_lc)
            .map(|a| a.persona.clone())
            .collect()
    }

    /// Every peer source path `me` should watch (expanded). A watcher running as
    /// `me` observes the code its peers own, not its own.
    pub fn peer_watchpaths(&self, me: &str) -> Vec<PathBuf> {
        let me_lc = me.to_lowercase();
        self.agents
            .iter()
            .filter(|a| a.persona.to_lowercase() != me_lc)
            .flat_map(|a| a.watchpaths.iter())
            .map(|p| expand_tilde(p))
            .collect()
    }
}
