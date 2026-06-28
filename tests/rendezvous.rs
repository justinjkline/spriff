//! End-to-end rendezvous tests — they drive the REAL `spriff` binary (the one
//! cargo built for this test, via `CARGO_BIN_EXE_spriff`) against an isolated
//! `SPRIFF_HOME`, exercising prompt-native rendezvous + the turn-delta contract
//! the way two agents actually use it. No LLM is involved: the behavior under
//! test is the spriff protocol itself, so these run inside `cargo test` and are
//! therefore a real CI gate (not a side script).
//!
//! Locks in the `join --project` behavior shipped in #1, including the operator's
//! hardest mode — two agents launched at the SAME instant from only the shared
//! prompt text must converge on ONE board (the `concurrent_*` test).

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Path to the binary under test (cargo sets this for integration tests).
fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_spriff")
}

/// Monotonic suffix so concurrently-running tests never collide on a temp path.
static SEQ: AtomicUsize = AtomicUsize::new(0);

/// An isolated registry root. Every test gets its own `SPRIFF_HOME`, so the whole
/// suite parallelizes safely; dropping it removes the tree.
struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(tag: &str) -> Sandbox {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let root =
            std::env::temp_dir().join(format!("spriff-e2e-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Sandbox { root }
    }

    /// A fresh, marker-free working directory inside the sandbox.
    fn cwd(&self, name: &str) -> PathBuf {
        let d = self.root.join("cwds").join(name);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A `spriff` Command pinned to this sandbox's SPRIFF_HOME, with the ambient
    /// SPRIFF_* env scrubbed so the dev machine can't leak identity/collab in.
    fn cmd(&self, cwd: &Path, args: &[&str]) -> Command {
        let mut c = Command::new(bin());
        c.env("SPRIFF_HOME", &self.root)
            .env_remove("SPRIFF_COLLAB")
            .env_remove("SPRIFF_AS")
            .current_dir(cwd)
            .args(args);
        c
    }

    fn run(&self, cwd: &Path, args: &[&str]) -> Output {
        self.cmd(cwd, args).output().expect("spawn spriff")
    }

    /// Run a command that consumes a body on stdin (the heredoc-on-stdin contract
    /// that `post` uses), returning its output.
    fn run_stdin(&self, cwd: &Path, args: &[&str], input: &str) -> Output {
        use std::io::Write;
        let mut child = self
            .cmd(cwd, args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn spriff");
        child
            .stdin
            .as_mut()
            .expect("stdin")
            .write_all(input.as_bytes())
            .expect("write stdin");
        drop(child.stdin.take()); // EOF
        child.wait_with_output().expect("wait spriff")
    }

    /// Registered collab slugs = dirs under the root holding a `<name>.toml`.
    fn slugs(&self) -> Vec<String> {
        let mut v = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&self.root) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    let name = e.file_name().to_string_lossy().to_string();
                    if p.join(format!("{name}.toml")).exists() {
                        v.push(name);
                    }
                }
            }
        }
        v.sort();
        v
    }

    fn mission(&self, slug: &str) -> Option<String> {
        let p = self.root.join(slug).join(format!("{slug}.mission.md"));
        std::fs::read_to_string(p)
            .ok()
            .map(|s| s.trim().to_string())
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn marker(cwd: &Path) -> String {
    std::fs::read_to_string(cwd.join(".spriff")).unwrap_or_default()
}
fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}
fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

// ---------------------------------------------------------------------------
// 1. Same --project from two dirs -> ONE board, mission seeded exactly once.
// ---------------------------------------------------------------------------
#[test]
fn same_project_two_dirs_one_board_mission_seeded_once() {
    let sb = Sandbox::new("seq");
    let a = sb.cwd("impl");
    let b = sb.cwd("rev");

    let o1 = sb.run(
        &a,
        &["join", "--role", "implementer", "--project", "fix checkout"],
    );
    assert!(o1.status.success(), "impl join failed: {}", stderr(&o1));
    let o2 = sb.run(
        &b,
        &["join", "--role", "reviewer", "--project", "fix checkout"],
    );
    assert!(o2.status.success(), "reviewer join failed: {}", stderr(&o2));

    assert_eq!(
        sb.slugs(),
        vec!["fix-checkout".to_string()],
        "exactly one board"
    );
    assert_eq!(sb.mission("fix-checkout").as_deref(), Some("fix checkout"));

    // Canonical role personas, both markers pointing at the one board.
    assert!(marker(&a).contains("collab=fix-checkout"));
    assert!(
        marker(&a).contains("as=Abbey"),
        "implementer marker: {}",
        marker(&a)
    );
    assert!(
        marker(&b).contains("as=Alice"),
        "reviewer marker: {}",
        marker(&b)
    );
}

// ---------------------------------------------------------------------------
// 2. A different goal that slugifies the same -> hard error, mission untouched.
// ---------------------------------------------------------------------------
#[test]
fn divergent_project_on_same_slug_is_rejected_mission_unchanged() {
    let sb = Sandbox::new("diverge");
    let a = sb.cwd("a");
    let b = sb.cwd("b");

    let o1 = sb.run(&a, &["join", "--role", "implementer", "--project", "a/b"]);
    assert!(o1.status.success(), "first join: {}", stderr(&o1));
    assert_eq!(sb.mission("a-b").as_deref(), Some("a/b"));

    // "a b" slugifies to the same `a-b` board but names a different goal.
    let o2 = sb.run(&b, &["join", "--role", "reviewer", "--project", "a b"]);
    assert!(!o2.status.success(), "divergent goal must hard-error");
    let err = stderr(&o2);
    assert!(
        err.contains("mission is") && err.contains("a/b"),
        "error should name the existing mission:\n{err}"
    );
    assert_eq!(
        sb.mission("a-b").as_deref(),
        Some("a/b"),
        "mission must be unchanged"
    );
    assert!(
        !b.join(".spriff").exists(),
        "an aborted join must not leave a marker"
    );
}

// ---------------------------------------------------------------------------
// 3. --collab override joins intentionally and hands the peer the REAL key.
// ---------------------------------------------------------------------------
#[test]
fn collab_override_joins_and_peer_command_carries_collab_key() {
    let sb = Sandbox::new("override");
    let a = sb.cwd("a");
    let b = sb.cwd("b");

    assert!(sb
        .run(&a, &["join", "--role", "implementer", "--project", "a/b"])
        .status
        .success());

    let o2 = sb.run(
        &b,
        &[
            "join",
            "--role",
            "reviewer",
            "--collab",
            "a-b",
            "--project",
            "totally different",
        ],
    );
    assert!(o2.status.success(), "override join failed: {}", stderr(&o2));
    let out = stdout(&o2);
    // The peer must be told the explicit slug — NOT a bare --project that would
    // slugify to `totally-different` and strand them on another board.
    assert!(
        out.contains("spriff join --role implementer --collab a-b"),
        "peer command must carry the explicit rendezvous key:\n{out}"
    );
    assert_eq!(
        sb.slugs(),
        vec!["a-b".to_string()],
        "still exactly one board"
    );
}

// ---------------------------------------------------------------------------
// 4. Bare join with several boards + no signal -> refuse, don't guess.
// ---------------------------------------------------------------------------
#[test]
fn bare_join_with_multiple_boards_refuses_to_guess() {
    let sb = Sandbox::new("ambig");
    let a = sb.cwd("a");
    let b = sb.cwd("b");
    let c = sb.cwd("c");

    assert!(sb
        .run(&a, &["join", "--role", "implementer", "--project", "alpha"])
        .status
        .success());
    assert!(sb
        .run(&b, &["join", "--role", "implementer", "--project", "beta"])
        .status
        .success());

    // Fresh marker-free cwd, no --project/--collab, two boards exist.
    let o = sb.run(&c, &["join", "--role", "reviewer"]);
    assert!(
        !o.status.success(),
        "must refuse with multiple boards and no signal"
    );
    let err = stderr(&o);
    assert!(
        err.contains("several collaborations") && err.contains("--project"),
        "refusal must point at the disambiguation flags:\n{err}"
    );
}

// ---------------------------------------------------------------------------
// 5. The turn-delta contract: post -> peer inbox shows it -> ack -> gone;
//    and an author never sees their own post (no self-wake).
// ---------------------------------------------------------------------------
#[test]
fn turn_delta_post_inbox_ack_and_no_self_wake() {
    let sb = Sandbox::new("delta");
    let a = sb.cwd("impl");
    let b = sb.cwd("rev");
    assert!(sb
        .run(&a, &["join", "--role", "implementer", "--project", "demo"])
        .status
        .success());
    assert!(sb
        .run(&b, &["join", "--role", "reviewer", "--project", "demo"])
        .status
        .success());

    let body = "the unmistakable body text 12345\n";
    let p = sb.run_stdin(
        &a,
        &[
            "post",
            "--as",
            "Abbey",
            "-s",
            "hello-subject",
            "--status",
            "FYI",
        ],
        body,
    );
    assert!(p.status.success(), "post failed: {}", stderr(&p));

    // The peer sees exactly that turn.
    let inbox1 = sb.run(&b, &["inbox", "--as", "Alice"]);
    assert!(inbox1.status.success());
    let ib = stdout(&inbox1);
    assert!(
        ib.contains("Abbey") && ib.contains("hello-subject") && ib.contains("unmistakable body"),
        "peer inbox should show the posted turn:\n{ib}"
    );

    // After ack the same turn must not reappear (cursor advanced).
    assert!(sb.run(&b, &["ack", "--as", "Alice"]).status.success());
    let ib2 = stdout(&sb.run(&b, &["inbox", "--as", "Alice"]));
    assert!(
        !ib2.contains("unmistakable body"),
        "acked turn reappeared:\n{ib2}"
    );

    // The author never sees her own post in her own inbox.
    let aib = stdout(&sb.run(&a, &["inbox", "--as", "Abbey"]));
    assert!(
        !aib.contains("unmistakable body"),
        "self-post leaked into own inbox:\n{aib}"
    );
}

// ---------------------------------------------------------------------------
// 6. Alice's case: two agents launched at the SAME instant from the same prompt
//    text must converge on ONE board with consistent identities. This is the
//    create/join race around `created = !config_path.exists()`; it passes only
//    because first-join creation is serialized by the create-lock.
// ---------------------------------------------------------------------------
#[test]
fn concurrent_same_project_joins_converge_on_one_board() {
    use std::thread;
    let sb = Sandbox::new("concurrent");
    let a = sb.cwd("impl");
    let b = sb.cwd("rev");
    let root = sb.root.clone();

    // Spawn both joins as close to simultaneously as the OS allows: each its own
    // thread -> its own process, same --project text, opposite roles.
    let spawn = |cwd: PathBuf, root: PathBuf, role: &'static str| {
        thread::spawn(move || {
            Command::new(bin())
                .env("SPRIFF_HOME", &root)
                .env_remove("SPRIFF_COLLAB")
                .env_remove("SPRIFF_AS")
                .current_dir(&cwd)
                .args(["join", "--role", role, "--project", "race goal"])
                .output()
                .expect("spawn spriff")
        })
    };
    let h1 = spawn(a.clone(), root.clone(), "implementer");
    let h2 = spawn(b.clone(), root.clone(), "reviewer");
    let o1 = h1.join().unwrap();
    let o2 = h2.join().unwrap();

    assert!(
        o1.status.success(),
        "implementer join failed: {}",
        stderr(&o1)
    );
    assert!(o2.status.success(), "reviewer join failed: {}", stderr(&o2));

    // Exactly one board; mission is exactly the shared goal (seeded once).
    assert_eq!(
        sb.slugs(),
        vec!["race-goal".to_string()],
        "must converge on ONE board"
    );
    assert_eq!(sb.mission("race-goal").as_deref(), Some("race goal"));

    // Both markers point at that board, with canonical role personas.
    let (ma, mb) = (marker(&a), marker(&b));
    assert!(ma.contains("collab=race-goal"), "impl marker: {ma}");
    assert!(mb.contains("collab=race-goal"), "rev marker: {mb}");
    assert!(
        ma.contains("as=Abbey"),
        "implementer must resolve to Abbey: {ma}"
    );
    assert!(
        mb.contains("as=Alice"),
        "reviewer must resolve to Alice: {mb}"
    );

    // Neither side points the peer at a different rendezvous key: each hands the
    // peer the SAME --project, exactly.
    assert!(
        stdout(&o1).contains("spriff join --role reviewer --project \"race goal\""),
        "implementer must hand the reviewer the same key:\n{}",
        stdout(&o1)
    );
    assert!(
        stdout(&o2).contains("spriff join --role implementer --project \"race goal\""),
        "reviewer must hand the implementer the same key:\n{}",
        stdout(&o2)
    );
}
