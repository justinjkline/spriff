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
    /// Ironclad-by-default posture (named `[loop]` in TOML).
    #[serde(default, rename = "loop")]
    pub loop_cfg: LoopConfig,
    /// Inactivity watchdog: ping all parties when the board goes silent.
    #[serde(default)]
    pub stall: StallConfig,
    /// Proactive review: a reviewer eyeballs the implementer's in-progress code.
    #[serde(default)]
    pub review: ReviewConfig,
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

/// Ironclad-by-default: present `spriff serve` (the supervisor that re-invokes the
/// agent every turn and survives a stop/timeout/crash) as the PRIMARY way to run a
/// side, not an opt-in. The flag doesn't force a behavior on an unsupervised run —
/// it tells `join`/`doctor` to lead with the supervised path and frame the manual
/// `wait`-loop as the fallback. Set false to flip that framing back.
#[derive(Debug, Deserialize, Clone)]
pub struct LoopConfig {
    #[serde(default = "default_true")]
    pub ironclad: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct StallConfig {
    /// If the board has no new turn for this many seconds, the watchdog pings all
    /// parties to resync and recommend next steps. 0 disables it. Default one hour.
    #[serde(default = "default_stall_idle")]
    pub idle_secs: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ReviewConfig {
    /// How eagerly a reviewer is nudged to look at the implementer's in-progress
    /// code BEFORE the formal handoff: `off | gentle | normal | strict`. On by
    /// default at `normal`; `off` restores the prior "board posts only" behavior.
    #[serde(default = "default_proactive")]
    pub proactive: String,
}

/// How aggressively proactive review fires, parsed from `[review] proactive`.
/// Controls the nudge cooldown and whether it escalates loudly. PURE + testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aggressiveness {
    /// Disabled — only the prior low-key "workspace changed" notice.
    Off,
    /// Occasional, quiet nudges (long cooldown).
    Gentle,
    /// The default: prompt, quiet nudges on each edit burst.
    Normal,
    /// Frequent, LOUD (escalating) nudges — jump on every burst.
    Strict,
}

impl Aggressiveness {
    /// Parse tolerantly. Synonyms map to the four levels; an empty or unrecognized
    /// value falls back to the default (`Normal`) so a typo never silently disables
    /// the feature the way `Off` would.
    pub fn parse(s: &str) -> Aggressiveness {
        match s.trim().to_lowercase().as_str() {
            "off" | "false" | "none" | "no" | "0" | "disabled" => Aggressiveness::Off,
            "gentle" | "low" | "passive" | "light" | "1" => Aggressiveness::Gentle,
            "strict" | "high" | "aggressive" | "eager" | "3" => Aggressiveness::Strict,
            _ => Aggressiveness::Normal,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Aggressiveness::Off => "off",
            Aggressiveness::Gentle => "gentle",
            Aggressiveness::Normal => "normal",
            Aggressiveness::Strict => "strict",
        }
    }

    pub fn is_off(self) -> bool {
        matches!(self, Aggressiveness::Off)
    }

    /// Minimum gap between two proactive-review nudges, so a burst of saves can't
    /// spam the reviewer. Higher aggressiveness -> shorter cooldown.
    pub fn cooldown_secs(self) -> u64 {
        match self {
            Aggressiveness::Off => 0,
            Aggressiveness::Gentle => 600,
            Aggressiveness::Normal => 150,
            Aggressiveness::Strict => 45,
        }
    }

    /// Whether a nudge is raised LOUDLY (an escalation artifact / insistent prompt)
    /// rather than quietly. Only `strict` escalates.
    pub fn escalates(self) -> bool {
        matches!(self, Aggressiveness::Strict)
    }
}

fn default_true() -> bool {
    true
}
fn default_stall_idle() -> u64 {
    3600
}
fn default_proactive() -> String {
    "normal".to_string()
}

impl Default for LoopConfig {
    fn default() -> Self {
        LoopConfig { ironclad: true }
    }
}
impl Default for StallConfig {
    fn default() -> Self {
        StallConfig {
            idle_secs: default_stall_idle(),
        }
    }
}
impl Default for ReviewConfig {
    fn default() -> Self {
        ReviewConfig {
            proactive: default_proactive(),
        }
    }
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

    /// Is ironclad mode (supervised `serve` as the blessed default) on?
    pub fn is_ironclad(&self) -> bool {
        self.loop_cfg.ironclad
    }

    /// Seconds of board silence before the inactivity watchdog pings everyone
    /// (0 = disabled).
    pub fn stall_idle_secs(&self) -> u64 {
        self.stall.idle_secs
    }

    /// The proactive-review aggressiveness for this collaboration.
    pub fn review_aggressiveness(&self) -> Aggressiveness {
        Aggressiveness::parse(&self.review.proactive)
    }

    /// Does `persona` play the reviewer role? The explicit role wins; if no roles
    /// are declared, the roster ORDER is authoritative (slot 0 is the executor, so
    /// anyone else on the roster is a reviewer). An off-roster name is not a
    /// reviewer — proactive review must never fire for an unknown identity.
    pub fn is_reviewer(&self, persona: &str) -> bool {
        let on_roster = self
            .agents
            .iter()
            .any(|a| a.persona.eq_ignore_ascii_case(persona));
        if !on_roster {
            return false;
        }
        match self.role_of(persona) {
            Some(r) => r.eq_ignore_ascii_case("reviewer"),
            None => self
                .agents
                .first()
                .map(|a| !a.persona.eq_ignore_ascii_case(persona))
                .unwrap_or(false),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal config (board + roster only) must default ALL the new behavior
    /// knobs ON — ironclad serve, the 1-hour stall watchdog, and normal proactive
    /// review — so an existing collaboration file gains them with no edit.
    #[test]
    fn new_knobs_default_on() {
        let cfg: Config = toml::from_str(
            "board = \"/x/b.md\"\n\
             [[agents]]\npersona = \"Abbey\"\nrole = \"executor\"\n\
             [[agents]]\npersona = \"Alice\"\nrole = \"reviewer\"\n",
        )
        .unwrap();
        assert!(cfg.is_ironclad());
        assert_eq!(cfg.stall_idle_secs(), 3600);
        assert_eq!(cfg.review_aggressiveness(), Aggressiveness::Normal);
        assert!(cfg.is_reviewer("Alice"));
        assert!(!cfg.is_reviewer("Abbey"));
        assert!(!cfg.is_reviewer("Nobody")); // off-roster is never a reviewer
    }

    /// Explicit overrides parse, including disabling each feature.
    #[test]
    fn knobs_parse_overrides() {
        let cfg: Config = toml::from_str(
            "board = \"/x/b.md\"\n\
             [loop]\nironclad = false\n\
             [stall]\nidle_secs = 0\n\
             [review]\nproactive = \"strict\"\n",
        )
        .unwrap();
        assert!(!cfg.is_ironclad());
        assert_eq!(cfg.stall_idle_secs(), 0);
        assert_eq!(cfg.review_aggressiveness(), Aggressiveness::Strict);
    }

    #[test]
    fn aggressiveness_parses_and_maps() {
        use Aggressiveness::*;
        assert_eq!(Aggressiveness::parse("off"), Off);
        assert_eq!(Aggressiveness::parse("NONE"), Off);
        assert_eq!(Aggressiveness::parse("gentle"), Gentle);
        assert_eq!(Aggressiveness::parse("strict"), Strict);
        assert_eq!(Aggressiveness::parse("aggressive"), Strict);
        // Unknown / empty fall back to the default (Normal), never Off.
        assert_eq!(Aggressiveness::parse(""), Normal);
        assert_eq!(Aggressiveness::parse("wat"), Normal);
        // Only strict escalates; Off does no work.
        assert!(Strict.escalates());
        assert!(!Normal.escalates());
        assert!(Off.is_off());
        assert!(Strict.cooldown_secs() < Normal.cooldown_secs());
        assert!(Normal.cooldown_secs() < Gentle.cooldown_secs());
    }

    /// With no roles declared, the roster ORDER decides who reviews (slot 0 leads).
    #[test]
    fn is_reviewer_falls_back_to_roster_order() {
        let cfg: Config = toml::from_str(
            "board = \"/x/b.md\"\n\
             [[agents]]\npersona = \"Nova\"\n\
             [[agents]]\npersona = \"Nash\"\n",
        )
        .unwrap();
        assert!(!cfg.is_reviewer("Nova")); // slot 0 = executor
        assert!(cfg.is_reviewer("Nash")); // slot 1 = reviewer
    }
}
