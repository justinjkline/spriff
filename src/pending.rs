//! The pending-signal control plane: the durable producer/consumer queue between
//! a watcher (producer) and the local agent's next turn (consumer).
//!
//! Lifecycle:
//!   1. watcher captures the peer's delta  -> `raise()` writes flag + pending + (optional) ACTION_REQUIRED
//!   2. agent starts a turn, sees the flag  -> `read()` (or `spriff inbox`)
//!   3. agent responds on the board         -> `spriff post`
//!   4. agent acknowledges                  -> `ack()` archives flag/pending/action to *.handled.<ts>.*
//!
//! All writes are atomic and all paths are private sidecars — the shared board is
//! never touched here.

use crate::board::Turn;
use crate::paths::Sidecars;
use crate::util::{atomic_write, utc_now, utc_stamp};
use anyhow::Result;
use std::path::Path;

/// Render the captured peer turns into a delta document.
fn render_pending(persona: &str, board: &Path, turns: &[Turn]) -> String {
    let mut s = String::new();
    s.push_str(&format!("# Pending for {persona}\n\n"));
    s.push_str(&format!("- detected_at: `{}`\n", utc_now()));
    s.push_str(&format!("- board: `{}`\n", board.display()));
    s.push_str(&format!("- new_turns: {}\n\n", turns.len()));
    s.push_str("Read these new turns and respond on the board, then run `spriff ack`.\n\n");
    s.push_str("---\n\n");
    for t in turns {
        s.push_str(&t.header());
        s.push('\n');
        if !t.body.is_empty() {
            s.push('\n');
            s.push_str(&t.body);
            s.push('\n');
        }
        s.push_str("\n---\n\n");
    }
    s
}

/// Render the loud, human-facing escalation file.
fn render_action(persona: &str, board: &Path, sc: &Sidecars, turns: &[Turn]) -> String {
    let header = turns
        .last()
        .map(|t| t.header())
        .unwrap_or_else(|| "(peer update)".to_string());
    let mut s = String::new();
    s.push_str(&format!("# {persona} — Action Required\n\n"));
    s.push_str(&format!("- raised_at: `{}`\n", utc_now()));
    s.push_str(&format!("- latest: `{header}`\n"));
    s.push_str(&format!("- board: `{}`\n", board.display()));
    s.push_str(&format!("- delta: `{}`\n\n", sc.pending.display()));
    s.push_str("Open and resolve this before other work. After responding, run:\n\n");
    s.push_str("```sh\nspriff ack --collab <name>\n```\n\n");
    s.push_str("## Captured peer update\n\n");
    for t in turns {
        s.push_str(&t.header());
        s.push('\n');
        if !t.body.is_empty() {
            s.push('\n');
            s.push_str(&t.body);
            s.push('\n');
        }
        s.push('\n');
    }
    s
}

/// Raise a pending signal for `turns` (the peer's captured delta).
/// `escalate` controls whether the loud ACTION_REQUIRED file is also written.
pub fn raise(sc: &Sidecars, persona: &str, turns: &[Turn], escalate: bool) -> Result<()> {
    if turns.is_empty() {
        return Ok(());
    }
    let pending_doc = render_pending(persona, &sc.board, turns);
    atomic_write(&sc.pending, pending_doc.as_bytes())?;

    let latest = turns.last().map(|t| t.header()).unwrap_or_default();
    let flag = format!(
        "pending {} — read {} and respond, then `spriff ack`.\nlatest: {}\n",
        utc_now(),
        sc.pending.display(),
        latest,
    );
    atomic_write(&sc.flag, flag.as_bytes())?;

    if escalate {
        let action = render_action(persona, &sc.board, sc, turns);
        atomic_write(&sc.action_required, action.as_bytes())?;
    }

    // Append to the durable audit trail.
    let line = format!("[{}] raised pending: {}\n", utc_now(), latest);
    append(&sc.ack_log, &line)?;
    Ok(())
}

/// Is there an outstanding pending signal?
pub fn is_raised(sc: &Sidecars) -> bool {
    sc.flag.exists()
}

/// Acknowledge: archive flag/pending/action to `*.handled.<ts>.*`. Idempotent.
pub fn ack(sc: &Sidecars) -> Result<bool> {
    let ts = utc_stamp();
    let mut archived = false;
    for original in [&sc.flag, &sc.pending, &sc.action_required] {
        if original.exists() {
            let dest = sc.handled(original, &ts);
            std::fs::rename(original, &dest)?;
            archived = true;
        }
    }
    if archived {
        let line = format!("[{}] acked pending (archived @ {ts})\n", utc_now());
        append(&sc.ack_log, &line)?;
    }
    Ok(archived)
}

fn append(path: &Path, line: &str) -> Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    f.write_all(line.as_bytes())?;
    Ok(())
}
