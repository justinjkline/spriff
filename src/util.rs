//! Small shared utilities: UTC timestamps, atomic file writes, and a bounded
//! newest-mtime walk.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Canonical timestamp used everywhere: `2026-06-28T03:10:42Z`.
pub fn utc_now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Compact timestamp safe for filenames: `20260628T031042Z`.
pub fn utc_stamp() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

/// Quote `s` as a valid TOML string value (the part after `key = `).
///
/// The reason this exists: a Windows path like `C:\Users\me\board.md` written
/// into a TOML *basic* string (double quotes) is mis-parsed — `\U` reads as the
/// start of a `\Uxxxxxxxx` unicode escape and the load fails with "too few
/// unicode value digits". TOML's own answer for paths is a *literal* string
/// (single quotes), which performs no escape processing — so backslashes survive
/// verbatim. We use a literal whenever we can; only if the value itself contains
/// a single quote or newline (which a literal cannot represent) do we fall back
/// to a basic string with `\` and `"` properly escaped.
pub fn toml_string(s: &str) -> String {
    if !s.contains('\'') && !s.contains('\n') {
        format!("'{s}'")
    } else {
        let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    }
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

/// The most recent modification time across `roots` (files and/or directories),
/// walked recursively under a bounded budget so a huge source tree can't stall a
/// supervisor poll. The `serve` supervisor uses this to notice a peer ACTIVELY
/// editing source between board posts (proactive review) without an FS-event
/// watcher of its own. Symlinks are not followed, so the walk can't cycle.
/// Returns `None` if none of the roots exist.
pub fn newest_mtime(roots: &[PathBuf]) -> Option<SystemTime> {
    // A generous cap: enough for any realistic watchpath, low enough that a
    // pathological tree degrades to "checked a slice" instead of a long stall.
    const MAX_ENTRIES: usize = 20_000;
    let mut budget = MAX_ENTRIES;
    let mut newest: Option<SystemTime> = None;
    let mut stack: Vec<PathBuf> = roots.to_vec();
    while let Some(p) = stack.pop() {
        if budget == 0 {
            break;
        }
        budget -= 1;
        // symlink_metadata: don't traverse INTO symlinked dirs (cycle-safe).
        let Ok(meta) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        if let Ok(m) = meta.modified() {
            newest = Some(match newest {
                Some(n) if n >= m => n,
                _ => m,
            });
        }
        if meta.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&p) {
                for entry in rd.flatten() {
                    stack.push(entry.path());
                }
            }
        }
    }
    newest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_string_roundtrips_a_windows_path() {
        // The regression: a backslash path in a basic string made `\U` look like
        // a unicode escape and broke config load on Windows. The value must come
        // back byte-for-byte after a real parse.
        let p = r"C:\Users\alexr\AppData\Local\Temp\h\demo.board.md";
        let toml = format!("board = {}\n", toml_string(p));
        let parsed: toml::Table = toml::from_str(&toml).expect("must parse");
        assert_eq!(parsed["board"].as_str().unwrap(), p);
    }

    #[test]
    fn toml_string_escapes_values_a_literal_cannot_hold() {
        // A single quote can't live in a literal string, so we fall back to an
        // escaped basic string — and that, too, must round-trip exactly.
        let weird = "a'b\"c\\d";
        let toml = format!("v = {}\n", toml_string(weird));
        let parsed: toml::Table = toml::from_str(&toml).expect("must parse");
        assert_eq!(parsed["v"].as_str().unwrap(), weird);
    }
}
