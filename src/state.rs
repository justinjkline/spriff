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
            }
        }
        st
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let body = format!(
            "offset={}\nlast_pending_header={}\nupdated_at={}\n",
            self.offset,
            self.last_pending_header,
            crate::util::utc_now(),
        );
        // Atomic: write to a temp then rename, so a reader never sees a half file.
        crate::util::atomic_write(path, body.as_bytes())
    }
}
