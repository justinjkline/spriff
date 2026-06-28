//! Path helpers: home-directory (`~`) expansion and derivation of the
//! per-persona sidecar file set from a board path.
//!
//! WHY THIS MATTERS: the original 32 hand-written watchers each hard-coded
//! their own sidecar filenames, which drifted and caused self-wake bugs (a
//! watcher firing on the very files it wrote). Here we derive every sidecar
//! path deterministically from `(board, persona)`, so the control-plane layout
//! is identical across every collaboration and every agent — no drift possible.

use std::path::{Path, PathBuf};

/// Expand a leading `~` to `$HOME`. Anything else is returned unchanged.
/// Config files are written by humans, who expect `~` to work.
pub fn expand_tilde(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    if s == "~" {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home);
        }
    }
    p.to_path_buf()
}

/// The full sidecar file set for one watching persona on one board.
///
/// Sidecars are PER-PERSONA: when Pamela posts, only *Peter's* pending signal is
/// raised, and vice-versa. This is what lets the same protocol scale cleanly
/// from a 2-agent pair to N agents — each agent has a private inbox.
#[derive(Debug, Clone)]
pub struct Sidecars {
    /// The shared, append-only board. The watcher is READ-ONLY to this file;
    /// writing to it would false-wake the peer and re-trigger ourselves.
    pub board: PathBuf,
    /// Tiny one-line "you have a message" signal. The first thing an agent
    /// checks at the start of a turn. Its mere existence means "go read pending".
    pub flag: PathBuf,
    /// The captured DELTA — only the peer's new turns, never the whole board.
    /// This is the file the agent actually reads, keeping context O(new).
    pub pending: PathBuf,
    /// Loud, human-and-agent-facing escalation with the captured content inline
    /// plus the exact ack command. Optional / for high-signal turns.
    pub action_required: PathBuf,
    /// Per-persona durable watch state (last-seen byte offset, last header).
    pub state: PathBuf,
    /// Append-only audit trail of every signal raised.
    pub ack_log: PathBuf,
    /// Live process log.
    pub log: PathBuf,
    /// The source paths THIS persona owns (one per line), declared live via
    /// `spriff touching`. A peer's watcher reads this to wake on this persona's
    /// real edits — the modern equivalent of the original `<persona>.watchpaths`.
    pub watchpaths: PathBuf,
}

impl Sidecars {
    /// Derive every sidecar path from the board path and the watching persona.
    ///
    /// Naming: `<board-base>.<persona>.<kind>` where `<board-base>` is the board
    /// path with a trailing `.md` and an optional `.board` stripped. e.g.
    /// `/x/foo.board.md` + `peter` -> `/x/foo.peter.pending.flag`.
    pub fn derive(board: &Path, persona: &str) -> Sidecars {
        let board = expand_tilde(board);
        let dir = board
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let mut base = board
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "board".to_string());
        // Strip a trailing `.md`, then a trailing `.board`, to get a clean stem.
        if let Some(stripped) = base.strip_suffix(".md") {
            base = stripped.to_string();
        }
        if let Some(stripped) = base.strip_suffix(".board") {
            base = stripped.to_string();
        }
        let p = persona.to_lowercase();
        let join = |suffix: &str| dir.join(format!("{base}.{p}.{suffix}"));
        Sidecars {
            board,
            flag: join("pending.flag"),
            pending: join("pending.md"),
            action_required: join("ACTION_REQUIRED.md"),
            state: join("watch.state"),
            ack_log: join("ack.log"),
            log: join("watch.log"),
            watchpaths: join("watchpaths"),
        }
    }

    /// Archive name for an acked signal, e.g. `foo.peter.pending.handled.<ts>.flag`.
    pub fn handled(&self, original: &Path, ts: &str) -> PathBuf {
        let dir = original
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let name = original
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        // Insert `.handled.<ts>` before the final extension.
        if let Some(dot) = name.rfind('.') {
            let (stem, ext) = name.split_at(dot); // ext includes the leading '.'
            dir.join(format!("{stem}.handled.{ts}{ext}"))
        } else {
            dir.join(format!("{name}.handled.{ts}"))
        }
    }
}

/// Read a `.watchpaths` file into expanded paths. One path per line; blank lines
/// and `#` comments are ignored. Missing file -> empty list.
pub fn read_watchpaths(file: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(file) else {
        return Vec::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| expand_tilde(Path::new(l)))
        .collect()
}

/// Append a path to a `.watchpaths` file, de-duplicating. Creates the file if
/// absent. Returns true if the path was newly added.
pub fn add_watchpath(file: &Path, path: &Path) -> std::io::Result<bool> {
    let canonical = expand_tilde(path);
    let existing = read_watchpaths(file);
    if existing.iter().any(|p| p == &canonical) {
        return Ok(false);
    }
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)?;
    writeln!(f, "{}", canonical.display())?;
    Ok(true)
}
