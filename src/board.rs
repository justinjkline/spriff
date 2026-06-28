//! The canonical board grammar: parsing turns, extracting the delta since a
//! byte offset, reading the last author cheaply, and appending a new turn.
//!
//! GRAMMAR (one format, machine-parseable AND human-skimmable):
//!
//! ```text
//! ## 2026-06-28T03:10Z - Pamela - PR-2 plan for review     <- header: `## <ts> - <author> - <subject>`
//! status:NEEDS-REVIEW @Peter                               <- optional control line
//!
//! <body…>
//!
//! -- Pamela                                                <- optional human signature
//! ```
//!
//! The watcher only ever needs the LAST `## ` header to answer "is it my turn?",
//! and only the bytes APPENDED since last time to capture the delta. Neither
//! cost depends on board size — a 500 KB board reads as cheaply as a 5 KB one.

use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

/// One posted message on the board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub ts: String,
    pub author: String,
    pub subject: String,
    pub body: String,
}

impl Turn {
    /// The canonical one-line header for this turn.
    pub fn header(&self) -> String {
        format!("## {} - {} - {}", self.ts, self.author, self.subject)
    }
}

/// Parse a `## ` header line into (ts, author, subject).
///
/// Tolerates the legacy em-dash separator (` — `) by normalizing it to ` - `,
/// so boards migrated from the old scripts still parse. If the line has fewer
/// than three fields, the whole remainder becomes the subject with an unknown
/// author (so a malformed header never silently vanishes).
fn parse_header(line: &str) -> (String, String, String) {
    let stripped = line.trim_start_matches('#').trim();
    let normalized = stripped.replace(" — ", " - ");
    let parts: Vec<&str> = normalized.splitn(3, " - ").collect();
    match parts.as_slice() {
        [ts, author, subject] => (
            ts.trim().to_string(),
            author.trim().to_string(),
            subject.trim().to_string(),
        ),
        [ts, author] => (
            ts.trim().to_string(),
            author.trim().to_string(),
            String::new(),
        ),
        _ => (String::new(), "unknown".to_string(), normalized),
    }
}

/// Return the byte offsets (into `content`) at which each `## ` header line
/// starts. Walks line starts only, so a `## ` inside a code fence in a body is
/// not mistaken for a header unless it begins the line (acceptable, documented).
fn header_offsets(content: &str) -> Vec<usize> {
    let mut offsets = Vec::new();
    let mut line_start = 0usize;
    while line_start <= content.len() {
        let rest = &content[line_start..];
        if rest.starts_with("## ") {
            offsets.push(line_start);
        }
        match rest.find('\n') {
            Some(rel) => {
                line_start += rel + 1;
                if line_start > content.len() {
                    break;
                }
            }
            None => break,
        }
    }
    offsets
}

/// Parse every turn in a markdown string.
pub fn parse_turns(content: &str) -> Vec<Turn> {
    let offsets = header_offsets(content);
    let mut turns = Vec::with_capacity(offsets.len());
    for (i, &start) in offsets.iter().enumerate() {
        let end = offsets.get(i + 1).copied().unwrap_or(content.len());
        let block = &content[start..end];
        // First line is the header; the remainder is the body.
        let (header_line, body) = match block.find('\n') {
            Some(nl) => (&block[..nl], block[nl + 1..].trim_end()),
            None => (block.trim_end(), ""),
        };
        let (ts, author, subject) = parse_header(header_line);
        turns.push(Turn {
            ts,
            author,
            subject,
            body: body.to_string(),
        });
    }
    turns
}

/// The current size of the board in bytes (0 if missing). This is our cheap,
/// append-only baseline: the byte offset of "everything we've already seen".
pub fn board_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// Read only the bytes appended since `offset` and parse the turns in them,
/// keeping only those NOT authored by `me`.
///
/// This is the heart of context-efficiency: an agent never re-reads the whole
/// board, only the genuinely new content. For an append-only board, `offset`
/// always lands on a turn boundary, so the slice parses cleanly.
pub fn delta_since(path: &Path, offset: u64, me: &str) -> Result<Vec<Turn>> {
    let size = board_size(path);
    if size <= offset {
        return Ok(Vec::new());
    }
    let mut f = OpenOptions::new()
        .read(true)
        .open(path)
        .with_context(|| format!("opening board {}", path.display()))?;
    f.seek(SeekFrom::Start(offset))?;
    let mut buf = Vec::with_capacity((size - offset) as usize);
    f.read_to_end(&mut buf)?;
    let slice = String::from_utf8_lossy(&buf);
    let me_lc = me.to_lowercase();
    let turns = parse_turns(&slice)
        .into_iter()
        .filter(|t| t.author.to_lowercase() != me_lc)
        .collect();
    Ok(turns)
}

/// Read the last `## ` header from the tail of the board, returning
/// (ts, author, subject). Reads at most the final 64 KiB.
pub fn last_turn_header(path: &Path) -> Option<(String, String, String)> {
    let size = board_size(path);
    if size == 0 {
        return None;
    }
    let window = 64 * 1024u64;
    let start = size.saturating_sub(window);
    let mut f = OpenOptions::new().read(true).open(path).ok()?;
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    let tail = String::from_utf8_lossy(&buf);
    let turns = parse_turns(&tail);
    turns
        .last()
        .map(|t| (t.ts.clone(), t.author.clone(), t.subject.clone()))
}

/// The closed status vocabulary (see docs/BOARD-GRAMMAR.md). Kept here, next to
/// the grammar, so validation and any future status-aware logic share one source.
pub const STATUSES: [&str; 6] = [
    "FYI",
    "NEEDS-REVIEW",
    "BLOCKED",
    "HANDOFF",
    "DONE",
    "ACTION-REQUIRED",
];

/// Normalize and validate a `--status` value against the closed vocabulary.
/// Returns the canonical upper-case form, or an error listing the valid options —
/// so a typo like `REVEIW` is rejected loudly instead of silently posted.
pub fn normalize_status(s: &str) -> Result<String> {
    let upper = s.trim().to_uppercase();
    if STATUSES.contains(&upper.as_str()) {
        Ok(upper)
    } else {
        anyhow::bail!(
            "invalid status '{s}'. Valid statuses: {}.",
            STATUSES.join(", ")
        )
    }
}

/// Append a new turn to the board in canonical format.
///
/// The post is the ONLY write the framework makes to the board, and it is done
/// by the posting agent's own `spriff post` invocation — never by a watcher.
/// We always terminate with a newline so the next `delta_since` offset lands on
/// a clean turn boundary.
pub fn append_turn(
    path: &Path,
    ts: &str,
    author: &str,
    subject: &str,
    status: &str,
    addressees: &[String],
    body: &str,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    // Enforce the status vocabulary at the single write seam, so NO caller path
    // (not just `cmd_post`) can land an invalid status on the board. (Hardening
    // from Alice's review: validate here, not only at the CLI.)
    let status = normalize_status(status)?;
    let mut control = format!("status:{status}");
    for who in addressees {
        control.push_str(&format!(" @{who}"));
    }
    let mut block = String::new();
    block.push_str(&format!("\n## {ts} - {author} - {subject}\n"));
    block.push_str(&control);
    block.push('\n');
    block.push('\n');
    block.push_str(body.trim_end());
    block.push_str(&format!("\n\n-- {author}\n"));

    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("appending to board {}", path.display()))?;
    f.write_all(block.as_bytes())?;
    Ok(())
}

/// The archive path for a board: `<base>.archive.md` alongside it. Rolled-up
/// turns are appended here so history is preserved but OFF the live board.
pub fn archive_path(board: &Path) -> std::path::PathBuf {
    let dir = board
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let mut base = board
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "board".into());
    if let Some(s) = base.strip_suffix(".md") {
        base = s.to_string();
    }
    if let Some(s) = base.strip_suffix(".board") {
        base = s.to_string();
    }
    dir.join(format!("{base}.archive.md"))
}

/// Roll older turns off the live board into the archive, keeping the most recent
/// `keep_recent` turns live.
///
/// THIS IS THE BOARD-BLOAT FIX. Real boards in the wild grew to 250–557 KB; every
/// full read of one cost an agent that much context. Rollup folds old turns into
/// a sibling `*.archive.md` and rewrites the live board down to its preamble plus
/// the last `keep_recent` turns, so the live board — and thus every agent's
/// working context — stays bounded no matter how long the collaboration runs.
///
/// Safe w.r.t. watchers: shrinking the board triggers each watcher's truncation
/// reset (baseline -> new size), and we keep recent turns precisely so no
/// in-flight context is lost. Rollup is performed by the WRITER (post / explicit
/// `spriff rollup`), never by a watcher — watchers stay read-only to the board.
///
/// Returns the number of turns archived (0 if nothing to do).
pub fn rollup(board: &Path, keep_recent: usize) -> Result<usize> {
    let content = match std::fs::read_to_string(board) {
        Ok(c) => c,
        Err(_) => return Ok(0),
    };
    let offsets = header_offsets(&content);
    if offsets.len() <= keep_recent {
        return Ok(0);
    }
    let preamble = &content[..*offsets.first().unwrap()];
    let split = offsets.len() - keep_recent;
    let cut = offsets[split];
    let archived_part = &content[offsets[0]..cut];
    let kept_part = &content[cut..];
    let archived_count = split;

    // Append the archived turns to the archive file (create with a header).
    let archive = archive_path(board);
    {
        use std::io::Write;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&archive)?;
        if board_size(&archive) == 0 {
            writeln!(
                f,
                "# Archive of {}\n\n> Rolled-up turns. Read SKILL.md; the live board is the working surface.\n",
                board.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
            )?;
        }
        writeln!(
            f,
            "\n<!-- rolled up {} ({archived_count} turns) -->",
            crate::util::utc_now()
        )?;
        f.write_all(archived_part.as_bytes())?;
    }

    // Rewrite the live board to preamble + a pointer + the kept recent turns.
    let mut new_board = String::new();
    new_board.push_str(preamble.trim_end());
    new_board.push_str(&format!(
        "\n\n> {} older turns rolled up to `{}` at {}.\n",
        archived_count,
        archive
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default(),
        crate::util::utc_now()
    ));
    new_board.push_str(kept_part);
    crate::util::atomic_write(board, new_board.as_bytes())?;
    Ok(archived_count)
}

/// Seed a brand-new board with its title line. The protocol preamble itself is
/// NOT inlined here — it lives once in SKILL.md, which agents read on join. This
/// is the single biggest context win over the old boards, which repeated a
/// multi-kilobyte "charter" wall at the top of every file.
pub fn seed_board(path: &Path, name: &str) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let header = format!(
        "# {name}\n\n\
         > Coordination board. Protocol: read SKILL.md once, then post turns in\n\
         > canonical grammar. This file is append-only; never edit prior turns.\n",
    );
    std::fs::write(path, header).with_context(|| format!("seeding board {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# board\n\nintro\n\n\
## 2026-06-28T01:00Z - Peter - hello\nstatus:FYI @Pamela\n\nbody one\n\n-- Peter\n\n\
## 2026-06-28T02:00Z - Pamela - reply\nstatus:NEEDS-REVIEW @Peter\n\nbody two\n\n-- Pamela\n";

    #[test]
    fn parses_headers_and_authors() {
        let turns = parse_turns(SAMPLE);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].author, "Peter");
        assert_eq!(turns[0].subject, "hello");
        assert_eq!(turns[1].author, "Pamela");
        assert!(turns[1].body.contains("body two"));
    }

    #[test]
    fn tolerates_legacy_em_dash_header() {
        let legacy = "## 2026-06-28T01:00Z — Edward — fleet update\n\nx\n";
        let turns = parse_turns(legacy);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].author, "Edward");
        assert_eq!(turns[0].subject, "fleet update");
    }

    #[test]
    fn subject_keeps_internal_separators() {
        let s = "## 2026-06-28T01:00Z - Peter - PR-2 - cross-repo - map\n\nx\n";
        let turns = parse_turns(s);
        assert_eq!(turns[0].subject, "PR-2 - cross-repo - map");
    }

    #[test]
    fn delta_round_trip_filters_own_posts() {
        let dir = std::env::temp_dir().join(format!("spriff-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let board = dir.join("t.board.md");
        seed_board(&board, "t").unwrap();
        let baseline = board_size(&board);

        append_turn(
            &board,
            "2026-01-01T00:00Z",
            "Peter",
            "mine",
            "FYI",
            &[],
            "x",
        )
        .unwrap();
        append_turn(
            &board,
            "2026-01-01T01:00Z",
            "Pamela",
            "theirs",
            "DONE",
            &[],
            "y",
        )
        .unwrap();

        // From Peter's view: only Pamela's turn is the delta.
        let delta = delta_since(&board, baseline, "Peter").unwrap();
        assert_eq!(delta.len(), 1);
        assert_eq!(delta[0].author, "Pamela");

        // Last author is Pamela -> it IS Peter's turn to respond.
        assert_eq!(
            last_turn_header(&board).map(|(_, a, _)| a).as_deref(),
            Some("Pamela")
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn status_validation() {
        assert_eq!(normalize_status("needs-review").unwrap(), "NEEDS-REVIEW");
        assert_eq!(normalize_status("  done ").unwrap(), "DONE");
        assert!(normalize_status("REVEIW").is_err()); // typo rejected
        assert!(normalize_status("").is_err());
    }

    #[test]
    fn rollup_bounds_the_live_board() {
        let dir = std::env::temp_dir().join(format!("spriff-roll-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let board = dir.join("r.board.md");
        seed_board(&board, "r").unwrap();
        for i in 0..10 {
            append_turn(
                &board,
                "2026-01-01T00:00Z",
                "Peter",
                &format!("t{i}"),
                "FYI",
                &[],
                "body",
            )
            .unwrap();
        }
        let before = board_size(&board);
        let archived = rollup(&board, 3).unwrap();
        assert_eq!(archived, 7);
        // Live board shrank; archive exists; only 3 turns remain live.
        assert!(board_size(&board) < before);
        assert_eq!(
            parse_turns(&std::fs::read_to_string(&board).unwrap()).len(),
            3
        );
        assert!(archive_path(&board).exists());
        std::fs::remove_dir_all(&dir).ok();
    }
}
