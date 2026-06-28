//! Small shared utilities: UTC timestamps and atomic file writes.

use anyhow::{Context, Result};
use std::path::Path;

/// Canonical timestamp used everywhere: `2026-06-28T03:10:42Z`.
pub fn utc_now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Compact timestamp safe for filenames: `20260628T031042Z`.
pub fn utc_stamp() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

/// Write a file atomically: write to a sibling temp file, then rename over the
/// target. A concurrent reader (e.g. an agent opening `pending.md`) therefore
/// always sees either the old complete file or the new complete file — never a
/// half-written one. This was a real corruption source in the original scripts
/// that wrote pending files in place.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let pid = std::process::id();
    let tmp = path.with_extension(format!("tmp.{pid}"));
    std::fs::write(&tmp, bytes).with_context(|| format!("writing temp {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("renaming {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}
