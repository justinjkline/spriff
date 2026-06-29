//! Informational, NON-acked nudges: the inactivity (stall) watchdog and the
//! proactive-review heads-up.
//!
//! These are deliberately NOT part of the pending/`ack` control plane. `spriff
//! ack` advances the consume cursor to the current board size, so if a nudge
//! reused the pending flag, acking it could silently swallow a real peer turn
//! that landed after the nudge — exactly the "hide work" footgun the design is
//! paranoid about. So a nudge gets its own sidecar: it never touches the cursor,
//! is overwritten on update, and is cleared by the watcher when activity resumes.
//! Its consumers are read-only: the operator's terminal, `status`, and `doctor`.

use crate::paths::Sidecars;
use crate::util::{atomic_write, utc_now};
use anyhow::Result;
use std::path::{Path, PathBuf};

/// Raise the loud stall nudge: the whole board has been silent past the
/// threshold, so prompt the local agent to break the silence with a status sync.
pub fn raise_stall(sc: &Sidecars, persona: &str, idle_secs: i64) -> Result<()> {
    let mins = idle_secs / 60;
    let mut s = String::new();
    s.push_str(&format!("# {persona} — Collaboration STALLED\n\n"));
    s.push_str(&format!("- raised_at: `{}`\n", utc_now()));
    s.push_str(&format!("- board: `{}`\n", sc.board.display()));
    s.push_str(&format!("- idle: ~{mins} min with no new turn\n\n"));
    s.push_str(
        "The board has gone quiet — nobody has posted in a while, so the loop is stalled.\n\
         Break the silence: post a brief STATUS update so every party resyncs.\n\n",
    );
    s.push_str("Do ONE turn:\n\n");
    s.push_str("1. `spriff status` and `spriff inbox` — reassess where things stand.\n");
    s.push_str("2. `spriff post --status FYI` a short update covering:\n");
    s.push_str("   • where the work stands, • what (if anything) is blocking you,\n");
    s.push_str("   • your recommended next step.\n");
    s.push_str("   If you're waiting on your peer, say so explicitly and @them.\n");
    s.push_str(
        "   If the work meets the Definition of Done, open the PR and post `--status DONE`.\n\n",
    );
    s.push_str("This file is informational; it clears itself once the board moves again.\n");
    atomic_write(&sc.stall, s.as_bytes())
}

/// Remove the stall nudge (activity resumed). Best-effort.
pub fn clear_stall(sc: &Sidecars) {
    let _ = std::fs::remove_file(&sc.stall);
}

/// Raise the proactive-review nudge: the implementer is actively editing watched
/// source before a formal handoff, so prompt the reviewer to take an early look.
/// `escalate` is the loud (strict) variant.
pub fn raise_review(sc: &Sidecars, persona: &str, files: &[PathBuf], escalate: bool) -> Result<()> {
    let mut s = String::new();
    let banner = if escalate {
        "Action: EARLY REVIEW (strict)"
    } else {
        "Heads-up: early review"
    };
    s.push_str(&format!("# {persona} — {banner}\n\n"));
    s.push_str(&format!("- raised_at: `{}`\n", utc_now()));
    s.push_str(&format!("- board: `{}`\n", sc.board.display()));
    s.push_str("- changed source:\n");
    for f in files {
        s.push_str(&format!("  - `{}`\n", f.display()));
    }
    s.push('\n');
    s.push_str(
        "Your implementer is changing code ahead of a formal handoff. Take an EARLY look:\n\
         read the in-progress diff, and if something's worth flagging, post a concise\n\
         `--status FYI` observation (file:line + the specific concern) so they can\n\
         course-correct before the formal review. If nothing stands out, note briefly\n\
         what you checked. This is a heads-up, not the formal review — don't block a\n\
         handoff that hasn't happened yet.\n\n",
    );
    s.push_str("This file is informational; it clears itself once a board post arrives.\n");
    atomic_write(&sc.review_nudge, s.as_bytes())
}

/// Remove the proactive-review nudge (a handoff arrived / activity moved on).
pub fn clear_review(sc: &Sidecars) {
    let _ = std::fs::remove_file(&sc.review_nudge);
}

/// Is a given nudge artifact currently raised? (Used by `status`/`doctor`.)
pub fn exists(path: &Path) -> bool {
    path.exists()
}
