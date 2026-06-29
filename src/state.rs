//! Durable per-persona watch state.
//!
//! This is the cross-turn memory that makes spriff reliable where the naive
//! scripts were not: a watcher that only `echo`s and exits loses its signal the
//! moment the agent turn ends. We persist the baseline (last-seen byte offset)
//! and the last header we raised a pending for, so:
//!   * the same peer post is never flagged twice (dedup), and
//!   * a watcher that crashes and restarts resumes exactly where it left off.

use anyhow::Result;
use std::path::Path;

#[derive(Debug, Default, Clone)]
pub struct WatchState {
    /// Byte offset into the board of everything already processed.
    pub offset: u64,
    /// The header of the most recent pending we raised (dedup guard).
    pub last_pending_header: String,
    /// Byte offset of the board END as of the agent's most recent READ
    /// (`inbox` / `wait` — the commands that actually SHOW the agent its turns;
    /// count-only pollers like `status`/`doctor` deliberately do NOT record it).
    /// This is the "read frontier": how far the
    /// agent has actually been SHOWN. `ack` advances the consume cursor only to
    /// here, never to the live board end — so a peer turn that lands AFTER the
    /// agent's read but BEFORE its `ack` (the mid-turn race) is preserved as
    /// unread instead of being silently swallowed. 0 = "no read recorded yet"
    /// (a bare `ack` with no prior read then becomes a safe no-op rather than
    /// jumping past unseen turns).
    pub read_frontier: u64,
}

impl WatchState {
    pub fn load(path: &Path) -> WatchState {
        let mut st = WatchState::default();
        let Ok(text) = std::fs::read_to_string(path) else {
            return st;
        };
        for line in text.lines() {
            if let Some(v) = line.strip_prefix("offset=") {
                st.offset = v.trim().parse().unwrap_or(0);
            } else if let Some(v) = line.strip_prefix("last_pending_header=") {
                st.last_pending_header = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("read_frontier=") {
                st.read_frontier = v.trim().parse().unwrap_or(0);
            }
        }
        st
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let body = format!(
            "offset={}\nlast_pending_header={}\nread_frontier={}\nupdated_at={}\n",
            self.offset,
            self.last_pending_header,
            self.read_frontier,
            crate::util::utc_now(),
        );
        // Atomic: write to a temp then rename, so a reader never sees a half file.
        crate::util::atomic_write(path, body.as_bytes())
    }
}
