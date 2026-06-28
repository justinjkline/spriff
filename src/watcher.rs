//! The event-driven watcher: the long-running daemon that wakes the local agent
//! when a peer posts to the board or touches watched source.
//!
//! Design distilled from the 32 original scripts:
//!   * EVENT-DRIVEN, not a busy poll: FS events (FSEvents/inotify via `notify`)
//!     wake us in milliseconds. A safety re-check on a timer guarantees a missed
//!     event can never strand a pending post.
//!   * SETTLE DEBOUNCE: after activity we wait for a quiet window so a multi-file
//!     save or a git operation coalesces into ONE wake, not dozens. (from peter)
//!   * SELF-POST FILTER: the captured delta excludes turns you authored, so
//!     posting never self-wakes you — without a "last-author gate," which could
//!     otherwise skip an unread peer turn posted just before yours. (from prancer)
//!   * READ-ONLY TO THE BOARD: we only ever read it; signals go to private
//!     sidecars. Writing to the board would false-wake the peer and re-trigger
//!     ourselves — the single most repeated bug in the originals. (from eloise)
//!   * DURABLE SIGNAL: we persist offset + a pending flag, so the signal
//!     survives across agent turns and watcher restarts.
//!   * TRUNCATION RESET: if the board shrinks below our baseline (a revert), we
//!     reset so a later post below the stale baseline is never missed. (from eloise)

use crate::board;
use crate::config::Config;
use crate::paths::Sidecars;
use crate::pending;
use crate::state::WatchState;
use crate::util::utc_now;
use anyhow::Result;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::time::{Duration, Instant};

pub fn run(cfg: &Config, persona: &str) -> Result<()> {
    let board_path = cfg.board_path();
    let sc = Sidecars::derive(&board_path, persona);

    // Cursor semantics: `offset` is the per-persona CONSUME cursor — everything
    // up to here has been acked. A fresh join (no state file) starts at 0 so the
    // agent is caught up on the current (post-rollup) live board. A restart loads
    // the persisted cursor. We only clamp if the board shrank while we were down.
    let mut st = WatchState::load(&sc.state);
    let size_now = board::board_size(&board_path);
    if st.offset > size_now {
        st.offset = size_now;
    }
    st.save(&sc.state)?;

    // ---- set up FS notifications ------------------------------------------------
    let (tx, rx) = channel();
    let mut watcher: RecommendedWatcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })?;

    // Watch the board's PARENT dir (watching the file directly misses editors
    // that replace-by-rename) ...
    if let Some(parent) = board_path.parent() {
        let _ = watcher.watch(parent, RecursiveMode::NonRecursive);
    }
    // ... plus every peer source path, recursively.
    for p in cfg.peer_watchpaths(persona) {
        if p.exists() {
            let _ = watcher.watch(&p, RecursiveMode::Recursive);
        }
    }

    log(
        &sc.log,
        &format!(
            "armed persona={persona} board={} offset={} settle_ms={} poll_ms={} peers={}",
            board_path.display(),
            st.offset,
            cfg.watch.settle_ms,
            cfg.watch.poll_ms,
            cfg.peers(persona).len()
        ),
    );
    eprintln!(
        "[spriff] watching {} as {persona} (offset {})",
        board_path.display(),
        st.offset
    );

    let settle = Duration::from_millis(cfg.watch.settle_ms);
    let poll = Duration::from_millis(cfg.watch.poll_ms);

    loop {
        // Block until a real FS event OR the safety-poll timeout. Either way we
        // proceed to (settle, then) process — the timeout is the periodic safety
        // re-check that guarantees a dropped event can't strand a pending post.
        match rx.recv_timeout(poll) {
            Ok(_event) => {}
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        // Settle: wait for the filesystem to go quiet for `settle`, draining any
        // events that arrive during the nap so a burst coalesces into ONE wake.
        // Then process immediately (latency ≈ settle, not poll).
        let mut last_event = Instant::now();
        while last_event.elapsed() < settle {
            std::thread::sleep(settle.saturating_sub(last_event.elapsed()));
            while rx.try_recv().is_ok() {
                last_event = Instant::now();
            }
        }

        if let Err(e) = process_board(persona, &sc) {
            log(&sc.log, &format!("ERROR processing board: {e}"));
        }
    }

    Ok(())
}

/// One pass over the board: detect a peer delta and raise a durable signal.
///
/// Reloads the consume cursor from disk each pass so an `ack` from the agent's
/// CLI side is honoured immediately. The watcher NEVER advances the cursor on a
/// peer post (only `ack` consumes); it just raises the proactive signal. This is
/// what decouples correctness from watcher timing — `inbox` recomputes the same
/// delta live, so collaboration works even with no watcher running.
pub fn process_board(persona: &str, sc: &Sidecars) -> Result<()> {
    let board_path = &sc.board;
    let mut st = WatchState::load(&sc.state);
    let size = board::board_size(board_path);

    // Truncation / rollup: board shrank below the cursor — clamp and exit.
    if size < st.offset {
        st.offset = size;
        st.save(&sc.state)?;
        log(&sc.log, &format!("board shrank to {size}; cursor clamped"));
        return Ok(());
    }

    // Capture only peer turns since the cursor. `delta_since` already excludes
    // our own posts, so posting can never self-wake us — no last-author gate
    // (which could otherwise skip an unread peer turn posted just before ours).
    let turns = board::delta_since(board_path, st.offset, persona)?;
    if turns.is_empty() {
        return Ok(());
    }

    // Dedup: same newest header already flagged and still pending -> don't spam.
    let latest_header = turns.last().map(|t| t.header()).unwrap_or_default();
    if latest_header == st.last_pending_header && pending::is_raised(sc) {
        return Ok(());
    }

    // Escalate loudly when any captured turn carries an action-demanding status.
    let escalate = turns.iter().any(|t| {
        let b = t.body.to_uppercase();
        b.contains("ACTION-REQUIRED")
            || b.contains("STATUS:BLOCKED")
            || b.contains("STATUS:HANDOFF")
            || b.contains("STATUS:NEEDS-REVIEW")
    });

    pending::raise(sc, persona, &turns, escalate)?;
    st.last_pending_header = latest_header.clone();
    st.save(&sc.state)?; // cursor intentionally NOT advanced — only `ack` consumes.

    log(
        &sc.log,
        &format!(
            "raised pending: {} new turn(s); latest: {latest_header}",
            turns.len()
        ),
    );
    eprintln!(
        "[spriff] PEER POSTED -> {} ({} new turn(s))",
        sc.flag.display(),
        turns.len()
    );
    Ok(())
}

fn log(path: &Path, msg: &str) {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "[{}] {msg}", utc_now());
    }
}
