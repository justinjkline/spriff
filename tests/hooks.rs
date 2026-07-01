//! Integration tests for `spriff hooks` — they drive the REAL built binary
//! (`CARGO_BIN_EXE_spriff`) against throwaway git repos, exercising the two things
//! the pure unit tests cannot: that `install` writes to the dir git ACTUALLY fires
//! hooks from (the linked-worktree trap that a naive `.git/hooks` reconstruction gets
//! wrong), and that a commit made under `SPRIFF_AS` ends up carrying the provenance
//! trailers while a human commit stays clean. No LLM is involved — this is pure git +
//! hook behavior, so it runs inside `cargo test` and is a real CI gate on every OS in
//! the matrix (incl. windows-latest, where git-for-windows executes the sh hook).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The binary under test (cargo sets this for integration tests, building it first).
fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_spriff")
}

/// Monotonic suffix so concurrently-running tests never collide on a temp path.
static SEQ: AtomicUsize = AtomicUsize::new(0);
fn scratch(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "spriff-hooks-it-{}-{}-{}",
        tag,
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn git(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("run git")
}

fn git_ok(dir: &Path, args: &[&str]) {
    let o = git(dir, args);
    assert!(
        o.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&o.stderr)
    );
}

fn init_repo(dir: &Path) {
    git_ok(dir, &["init", "-q"]);
    git_ok(dir, &["config", "user.email", "t@t"]);
    git_ok(dir, &["config", "user.name", "t"]);
    git_ok(dir, &["config", "commit.gpgsign", "false"]);
    git_ok(dir, &["commit", "--allow-empty", "-qm", "init"]);
}

fn install_hooks(repo: &Path) {
    let out = Command::new(bin())
        .args(["hooks", "install", "--repo"])
        .arg(repo)
        .output()
        .expect("run spriff hooks install");
    assert!(
        out.status.success(),
        "spriff hooks install failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Regression for the silent-no-op bug: inside a LINKED worktree, git fires hooks from
/// the COMMON `.git/hooks`, not the per-worktree gitdir. `install` run from the
/// worktree must write there — the exact case the pure `effective_hooks_dir` unit test
/// could not see (it's why the bug shipped in the first draft).
#[test]
fn install_in_linked_worktree_targets_the_common_hooks_dir() {
    let root = scratch("wt");
    let main = root.join("main");
    std::fs::create_dir_all(&main).unwrap();
    init_repo(&main);
    let wt = root.join("wt");
    git_ok(&main, &["worktree", "add", "-q", wt.to_str().unwrap()]);

    install_hooks(&wt);

    // Where git ACTUALLY fires hooks from for the worktree = the common hooks dir.
    let common = String::from_utf8(
        git(
            &wt,
            &["rev-parse", "--path-format=absolute", "--git-path", "hooks"],
        )
        .stdout,
    )
    .unwrap();
    let common_hook = Path::new(common.trim()).join("prepare-commit-msg");
    assert!(
        common_hook.exists(),
        "hook must land in the common hooks dir git fires from: {}",
        common_hook.display()
    );

    // And it must NOT have gone into the per-worktree gitdir (where git never looks).
    let per_worktree_hook = main
        .join(".git")
        .join("worktrees")
        .join("wt")
        .join("hooks")
        .join("prepare-commit-msg");
    assert!(
        !per_worktree_hook.exists(),
        "hook wrongly written to the dead per-worktree dir: {}",
        per_worktree_hook.display()
    );

    std::fs::remove_dir_all(&root).ok();
}

/// End-to-end: after install, a commit made as a spriff agent carries the trailers;
/// a human commit (no SPRIFF_AS) stays clean. Exercises real hook execution on every
/// CI OS — including git-for-windows running the sh hook.
#[test]
fn agent_commit_is_stamped_and_human_commit_is_clean() {
    let dir = scratch("e2e");
    init_repo(&dir);
    install_hooks(&dir);

    // Commit as an agent → both trailers present, sourced from the env vars.
    let out = Command::new("git")
        .arg("-C")
        .arg(&dir)
        .args(["commit", "--allow-empty", "-m", "agent work"])
        .env("SPRIFF_AS", "Pamela")
        .env("SPRIFF_COLLAB", "canonical-agent-fabric")
        .output()
        .expect("agent commit");
    assert!(
        out.status.success(),
        "agent commit failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let agent = String::from_utf8(
        git(
            &dir,
            &[
                "log",
                "-1",
                "--format=%(trailers:key=Spriff-Agent,valueonly)",
            ],
        )
        .stdout,
    )
    .unwrap();
    let mission = String::from_utf8(
        git(
            &dir,
            &[
                "log",
                "-1",
                "--format=%(trailers:key=Spriff-Mission,valueonly)",
            ],
        )
        .stdout,
    )
    .unwrap();
    assert_eq!(agent.trim(), "Pamela");
    assert_eq!(mission.trim(), "canonical-agent-fabric");

    // Commit as a human (SPRIFF_AS explicitly removed) → no provenance trailer.
    let out = Command::new("git")
        .arg("-C")
        .arg(&dir)
        .args(["commit", "--allow-empty", "-m", "human work"])
        .env_remove("SPRIFF_AS")
        .env_remove("SPRIFF_COLLAB")
        .output()
        .expect("human commit");
    assert!(out.status.success());
    let body = String::from_utf8(git(&dir, &["log", "-1", "--format=%B"]).stdout).unwrap();
    assert!(
        !body.contains("Spriff-Agent"),
        "human commit must never be stamped, got:\n{body}"
    );

    std::fs::remove_dir_all(&dir).ok();
}
