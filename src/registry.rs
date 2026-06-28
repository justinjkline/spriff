//! Global collaboration registry + addressing.
//!
//! WHY: agents run `cd`'d into whatever repo they're working in. They must be
//! able to call spriff with no knowledge of where configs live. So collabs
//! are registered globally under `~/.spriff/<name>/` and addressed by NAME,
//! resolved in this priority order (most explicit wins):
//!
//!   1. an explicit `--collab <name>` flag
//!   2. the `$SPRIFF_COLLAB` environment variable
//!   3. a `.spriff` marker file found by walking up from the current dir
//!      (drop one in a repo root and every agent command "just works" there)
//!   4. if exactly one collab is registered, use it
//!
//! This makes the common case — an agent inside a project repo — a bare
//! `spriff inbox` with zero arguments.

use anyhow::{anyhow, bail, Result};
use std::path::{Path, PathBuf};

/// Root of the global registry. Override with `$SPRIFF_HOME` (handy for tests
/// and for running isolated collaborations).
pub fn root() -> PathBuf {
    if let Ok(dir) = std::env::var("SPRIFF_HOME") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".spriff")
}

/// Directory for a named collaboration.
pub fn collab_dir(name: &str) -> PathBuf {
    root().join(name)
}

/// The config path for a named collaboration.
pub fn config_path(name: &str) -> PathBuf {
    collab_dir(name).join(format!("{name}.toml"))
}

/// The default board path for a named collaboration.
pub fn board_path(name: &str) -> PathBuf {
    collab_dir(name).join(format!("{name}.board.md"))
}

/// Every registered collaboration name (dirs under the registry root with a
/// matching `<name>.toml`).
pub fn list() -> Vec<String> {
    let mut names = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root()) {
        for e in entries.flatten() {
            if e.path().is_dir() {
                if let Some(name) = e.file_name().to_str().map(str::to_string) {
                    if config_path(&name).exists() {
                        names.push(name);
                    }
                }
            }
        }
    }
    names.sort();
    names
}

/// Walk up from the current dir looking for a `.spriff` marker, returning the
/// value of `key` (`collab` or `as`). For `collab`, a bare line with no `=` is
/// also accepted as the name (back-compat with the simplest marker).
pub fn marker_field(key: &str) -> Option<String> {
    let mut dir = std::env::current_dir().ok();
    let prefix = format!("{key}=");
    while let Some(d) = dir {
        let marker = d.join(".spriff");
        if let Ok(text) = std::fs::read_to_string(&marker) {
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some(v) = line.strip_prefix(&prefix) {
                    let v = v.trim();
                    if !v.is_empty() {
                        return Some(v.to_string());
                    }
                } else if key == "collab" && !line.contains('=') {
                    return Some(line.to_string());
                }
            }
        }
        dir = d.parent().map(Path::to_path_buf);
    }
    None
}

/// Resolve the active collaboration name using the priority order above.
pub fn resolve_name(explicit: Option<String>) -> Result<String> {
    if let Some(name) = explicit {
        return Ok(name);
    }
    if let Ok(name) = std::env::var("SPRIFF_COLLAB") {
        if !name.is_empty() {
            return Ok(name);
        }
    }
    if let Some(name) = marker_field("collab") {
        return Ok(name);
    }
    let names = list();
    match names.len() {
        1 => Ok(names[0].clone()),
        0 => Err(anyhow!(
            "no collaborations registered. Agents: run `spriff join --role implementer|reviewer`."
        )),
        _ => bail!(
            "multiple collaborations registered ({}). Pass --collab <name>, set $SPRIFF_COLLAB, \
             or drop a .spriff file in your repo.",
            names.join(", ")
        ),
    }
}
