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
            // resolve() prefers SPRIFF_CONFIG over the cwd marker for non-join
            // commands (post/inbox/ack), so a dev's ambient config would hijack
            // the isolated board. Scrub it too, or the suite isn't hermetic.
            // (Alice's catch.)
            .env_remove("SPRIFF_CONFIG")
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
// Mid-turn race: a peer turn that lands AFTER the reviewer reads its inbox but
// BEFORE it acks must NOT be swallowed by the ack. This locks the fix for the
// real "there were posts but the supervised agent never reacted" bug: `ack`
// must advance the consume cursor only to the agent's READ FRONTIER, never to
// the live board end.
// ---------------------------------------------------------------------------
#[test]
fn ack_does_not_swallow_a_turn_that_arrived_after_the_read() {
    let sb = Sandbox::new("ackrace");
    let a = sb.cwd("impl");
    let b = sb.cwd("rev");
    assert!(sb
        .run(&a, &["join", "--role", "implementer", "--project", "race"])
        .status
        .success());
    assert!(sb
        .run(&b, &["join", "--role", "reviewer", "--project", "race"])
        .status
        .success());

    // T1: Abbey posts turn A.
    assert!(sb
        .run_stdin(
            &a,
            &["post", "--as", "Abbey", "-s", "turn-A", "--status", "FYI"],
            "alpha body AAA\n",
        )
        .status
        .success());

    // T2: Alice reads her inbox and SEES turn A (this records her read frontier).
    let ib_a = stdout(&sb.run(&b, &["inbox", "--as", "Alice"]));
    assert!(
        ib_a.contains("turn-A") && ib_a.contains("alpha body AAA"),
        "Alice should see turn A:\n{ib_a}"
    );

    // T3: the race — Abbey posts turn B while Alice is "working" on A, BEFORE her ack.
    assert!(sb
        .run_stdin(
            &a,
            &["post", "--as", "Abbey", "-s", "turn-B", "--status", "FYI"],
            "bravo body BBB\n",
        )
        .status
        .success());

    // T4: Alice acks. She only ever saw A, so the ack must consume A but NOT B.
    assert!(sb.run(&b, &["ack", "--as", "Alice"]).status.success());

    // T5: turn B must STILL be unread for Alice (the bug was that it vanished).
    let ib_b = stdout(&sb.run(&b, &["inbox", "--as", "Alice"]));
    assert!(
        ib_b.contains("turn-B") && ib_b.contains("bravo body BBB"),
        "REGRESSION: turn B that arrived after the read but before the ack was swallowed:\n{ib_b}"
    );
    // And turn A must be gone (the ack genuinely consumed what was seen).
    assert!(
        !ib_b.contains("alpha body AAA"),
        "turn A should have been consumed by the ack:\n{ib_b}"
    );

    // T6: after reading B and acking again, the inbox is finally clear.
    assert!(sb.run(&b, &["ack", "--as", "Alice"]).status.success());
    let ib_c = stdout(&sb.run(&b, &["inbox", "--as", "Alice"]));
    assert!(
        !ib_c.contains("bravo body BBB") && ib_c.contains("inbox clear"),
        "after reading + acking B the inbox should be clear:\n{ib_c}"
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
                .env_remove("SPRIFF_CONFIG")
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

// ---------------------------------------------------------------------------
// 7. --lens is reviewer-only: an implementer that passes it is rejected loudly.
// ---------------------------------------------------------------------------
#[test]
fn lens_is_rejected_for_the_implementer() {
    let sb = Sandbox::new("lensimpl");
    let a = sb.cwd("impl");
    let o = sb.run(
        &a,
        &[
            "join",
            "--role",
            "implementer",
            "--project",
            "demo",
            "--lens",
            "security",
        ],
    );
    assert!(!o.status.success(), "implementer --lens must be rejected");
    assert!(
        stderr(&o).contains("--lens is for reviewers only"),
        "error should explain the lens is reviewer-only:\n{}",
        stderr(&o)
    );
}

// ---------------------------------------------------------------------------
// 8. The SECOND reviewer in a 3-agent crew can join via --as and declare a lens
//    (Alice's catch: join used to hardcode any reviewer to roster slot 1).
// ---------------------------------------------------------------------------
#[test]
fn second_reviewer_can_join_and_declare_a_lens() {
    let sb = Sandbox::new("tworev");
    let (a, b, c) = (sb.cwd("impl"), sb.cwd("rev1"), sb.cwd("rev2"));

    // Implementer creates a 3-agent crew: Abbey (exec), Alice + Annie (reviewers).
    assert!(sb
        .run(
            &a,
            &[
                "join",
                "--role",
                "implementer",
                "--as",
                "Abbey",
                "--with",
                "Alice",
                "--project",
                "trio",
                "--agents",
                "3"
            ],
        )
        .status
        .success());

    // First reviewer joins slot 1.
    let r1 = sb.run(
        &b,
        &[
            "join",
            "--role",
            "reviewer",
            "--as",
            "Alice",
            "--with",
            "Abbey",
            "--project",
            "trio",
            "--lens",
            "security",
        ],
    );
    assert!(
        r1.status.success(),
        "first reviewer join failed: {}",
        stderr(&r1)
    );

    // SECOND reviewer (slot 2) must now be able to join and declare its lens.
    let r2 = sb.run(
        &c,
        &[
            "join",
            "--role",
            "reviewer",
            "--as",
            "Annie",
            "--with",
            "Abbey",
            "--project",
            "trio",
            "--lens",
            "correctness",
        ],
    );
    assert!(
        r2.status.success(),
        "second reviewer join failed: {}",
        stderr(&r2)
    );
    assert!(
        marker(&c).contains("as=Annie"),
        "rev2 marker should be Annie: {}",
        marker(&c)
    );

    // Each reviewer's declared lens is visible via status; the two are distinct.
    assert!(stdout(&sb.run(&b, &["status", "--as", "Alice"])).contains("security"));
    assert!(stdout(&sb.run(&c, &["status", "--as", "Annie"])).contains("correctness"));
}

// ---------------------------------------------------------------------------
// 9. Reviewer #2 may CREATE the crew first (win the create race) without
//    corrupting the generated roster. (Alice's catch: the slot fix must apply at
//    creation too, else `--as Annie` overwrote slot 1 → Abbey, Annie, Annie.)
// ---------------------------------------------------------------------------
#[test]
fn second_reviewer_can_create_the_crew_first() {
    let sb = Sandbox::new("revfirst");
    let c = sb.cwd("rev2");
    let o = sb.run(
        &c,
        &[
            "join",
            "--role",
            "reviewer",
            "--as",
            "Annie",
            "--with",
            "Abbey",
            "--project",
            "rev first trio",
            "--agents",
            "3",
            "--lens",
            "correctness",
        ],
    );
    assert!(
        o.status.success(),
        "reviewer-first create failed: {}",
        stderr(&o)
    );
    assert!(
        marker(&c).contains("as=Annie"),
        "rev2 marker: {}",
        marker(&c)
    );

    // The generated roster must stay intact: exactly one each of Abbey/Alice/Annie.
    let cfg = std::fs::read_to_string(sb.root.join("rev-first-trio").join("rev-first-trio.toml"))
        .expect("config written");
    assert_eq!(
        cfg.matches("persona = \"Abbey\"").count(),
        1,
        "roster: {cfg}"
    );
    assert_eq!(
        cfg.matches("persona = \"Alice\"").count(),
        1,
        "Alice dropped: {cfg}"
    );
    assert_eq!(
        cfg.matches("persona = \"Annie\"").count(),
        1,
        "Annie duplicated: {cfg}"
    );
    // Annie's lens still landed.
    assert!(stdout(&sb.run(&c, &["status", "--as", "Annie"])).contains("correctness"));
}

// ---------------------------------------------------------------------------
// 10. A reviewer naming the GENERATED EXECUTOR (no --with) is a role conflict —
//     rejected, with NO corrupt board created. (Alice's catch: it used to fall
//     back to slot 1 and overwrite the first reviewer → Abbey, Abbey, Annie.)
// ---------------------------------------------------------------------------
#[test]
fn reviewer_naming_the_generated_executor_is_rejected() {
    let sb = Sandbox::new("execconflict");
    let a = sb.cwd("rev");
    let o = sb.run(
        &a,
        &[
            "join",
            "--role",
            "reviewer",
            "--as",
            "Abbey",
            "--project",
            "bad exec",
            "--agents",
            "3",
            "--lens",
            "security",
        ],
    );
    assert!(!o.status.success(), "reviewer-as-executor must be rejected");
    assert!(
        stderr(&o).contains("implementer"),
        "error should explain the role conflict:\n{}",
        stderr(&o)
    );
    assert!(
        sb.slugs().is_empty(),
        "a rejected join must not create a board: {:?}",
        sb.slugs()
    );
}

// ---------------------------------------------------------------------------
// 11. A reviewer MAY rename the implementer via --with (Alice's note that this
//     variant is legitimate): `--as Abbey --with Alice` → Alice executor, Abbey
//     reviewer, no duplicates.
// ---------------------------------------------------------------------------
#[test]
fn reviewer_may_rename_the_implementer_via_with() {
    let sb = Sandbox::new("withrename");
    let a = sb.cwd("rev");
    let o = sb.run(
        &a,
        &[
            "join",
            "--role",
            "reviewer",
            "--as",
            "Abbey",
            "--with",
            "Alice",
            "--project",
            "with rename",
            "--agents",
            "3",
            "--lens",
            "security",
        ],
    );
    assert!(
        o.status.success(),
        "valid --with rename failed: {}",
        stderr(&o)
    );
    assert!(marker(&a).contains("as=Abbey"), "marker: {}", marker(&a));
    let cfg = std::fs::read_to_string(sb.root.join("with-rename").join("with-rename.toml"))
        .expect("config written");
    // One each — no duplicate from the rename.
    assert_eq!(cfg.matches("persona = \"Alice\"").count(), 1, "{cfg}");
    assert_eq!(cfg.matches("persona = \"Abbey\"").count(), 1, "{cfg}");
    assert_eq!(cfg.matches("persona = \"Annie\"").count(), 1, "{cfg}");
}

// ---------------------------------------------------------------------------
// Inactivity watchdog: a board silent past the stall threshold is surfaced
// loudly by `doctor` (idle time + a ⚠ STALLED flag), and the ironclad/proactive
// posture is reported. Drives the real binary against a board with an old turn.
// ---------------------------------------------------------------------------
#[test]
fn doctor_surfaces_a_stalled_board_and_ironclad_posture() {
    let sb = Sandbox::new("stall");
    let cwd = sb.cwd("op");
    let o = sb.run(
        &cwd,
        &["init", "stalltest", "--agents", "2", "--letter", "a"],
    );
    assert!(o.status.success(), "init failed: {}", stderr(&o));

    // Rewrite the board so its only turn is years old -> way past the 1h default.
    let board = sb.root.join("stalltest").join("stalltest.board.md");
    std::fs::write(
        &board,
        "# board\n\nintro\n\n## 2020-01-01T00:00:00Z - Abbey - old turn\nstatus:FYI @Alice\n\nstale\n\n-- Abbey\n",
    )
    .unwrap();

    let d = sb.run(&cwd, &["doctor", "--collab", "stalltest"]);
    assert!(d.status.success(), "doctor failed: {}", stderr(&d));
    let out = stdout(&d);
    assert!(
        out.contains("idle:"),
        "doctor should report idle time:\n{out}"
    );
    assert!(
        out.contains("STALLED"),
        "doctor should flag the stall:\n{out}"
    );
    // Ironclad on by default; proactive review at normal.
    assert!(
        out.contains("ironclad:"),
        "doctor should report ironclad:\n{out}"
    );
    assert!(
        out.contains("proactive-review: normal"),
        "doctor should report proactive-review level:\n{out}"
    );
}

// ---------------------------------------------------------------------------
// `spriff supervise` emits a canonical persistent supervisor unit wrapping
// `serve` for this persona (so nobody hand-rolls a plist), without installing
// when --install is absent. Platform-agnostic assertions cover both launchd and
// systemd output.
// ---------------------------------------------------------------------------
#[test]
fn supervise_prints_a_serve_wrapping_unit_without_installing() {
    let sb = Sandbox::new("sup");
    let cwd = sb.cwd("op");
    let o = sb.run(&cwd, &["init", "suptest", "--agents", "2", "--letter", "a"]);
    assert!(o.status.success(), "init failed: {}", stderr(&o));

    let s = sb.run(
        &cwd,
        &[
            "supervise",
            "--collab",
            "suptest",
            "--as",
            "Abbey",
            "--autonomous",
            "--",
            "echo",
            "hi",
        ],
    );
    assert!(s.status.success(), "supervise failed: {}", stderr(&s));
    let out = stdout(&s);
    // The generated service wraps `spriff serve` for this persona+collab.
    assert!(out.contains("serve"), "should wrap serve:\n{out}");
    assert!(out.contains("Abbey"), "should name the persona:\n{out}");
    assert!(out.contains("suptest"), "should name the collab:\n{out}");
    // The wrapped `serve` must carry the explicit autonomous opt-in, or the
    // installed service's own `serve` would refuse to start at boot.
    assert!(
        out.contains("--autonomous"),
        "wrapped serve must carry the autonomous opt-in:\n{out}"
    );
    // Without --install it must NOT claim to have loaded/enabled anything.
    assert!(
        !out.contains("now subscribed"),
        "must not install without --install:\n{out}"
    );

    // An off-roster persona is refused. We pass --autonomous so the command gets
    // PAST the spawn-opt-in guard and actually reaches the off-roster check —
    // otherwise it would bail on the (earlier) autonomous guard and this would no
    // longer exercise the roster validation it claims to.
    let bad = sb.run(
        &cwd,
        &[
            "supervise",
            "--collab",
            "suptest",
            "--as",
            "Zelda",
            "--autonomous",
            "--",
            "echo",
            "hi",
        ],
    );
    assert!(!bad.status.success(), "off-roster supervise should fail");
}

// ---------------------------------------------------------------------------
// 15. The foreground/operator-steered `wait` loop must not silently race a
//     separate supervised child for the same persona. This was the confusing
//     "which Punchyman is actually watching?" failure mode: if `serve` is already
//     subscribed, `wait` now refuses unless the operator explicitly opts into the
//     duplicate-agent risk.
// ---------------------------------------------------------------------------
#[test]
fn wait_refuses_when_same_persona_is_already_supervised() {
    let sb = Sandbox::new("wait-supervised");
    let cwd = sb.cwd("op");
    let o = sb.run(&cwd, &["init", "waitsup", "--agents", "2", "--letter", "a"]);
    assert!(o.status.success(), "init failed: {}", stderr(&o));

    let mut serve = sb
        .cmd(
            &cwd,
            &[
                "serve",
                "--collab",
                "waitsup",
                "--as",
                "Alice",
                "--autonomous",
                "--no-kickoff",
                "--idle-timeout",
                "30",
                "--poll",
                "1",
                "--",
                "echo",
                "unused",
            ],
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn serve");

    let subscribed = (0..60).any(|_| {
        let s = sb.run(&cwd, &["status", "--collab", "waitsup", "--as", "Alice"]);
        if stdout(&s).contains("subscribed: yes") {
            true
        } else {
            std::thread::sleep(std::time::Duration::from_millis(50));
            false
        }
    });
    assert!(subscribed, "serve never acquired the persona lock");

    let blocked = sb.run(
        &cwd,
        &[
            "wait",
            "--collab",
            "waitsup",
            "--as",
            "Alice",
            "--timeout",
            "1",
        ],
    );
    assert!(
        !blocked.status.success(),
        "wait should refuse while serve owns this persona"
    );
    let err = stderr(&blocked);
    assert!(
        err.contains("CURRENT-session") && err.contains("separate `spriff serve` supervisor"),
        "error must explain the foreground-vs-supervised split:\n{err}"
    );

    let allowed = sb.run(
        &cwd,
        &[
            "wait",
            "--collab",
            "waitsup",
            "--as",
            "Alice",
            "--timeout",
            "1",
            "--allow-while-supervised",
        ],
    );
    assert_eq!(
        allowed.status.code(),
        Some(2),
        "override should bypass the duplicate-agent guard and then time out"
    );
    assert!(
        stderr(&allowed).contains("interactive wait-loop armed"),
        "override path should still identify itself as the current-session loop"
    );

    let _ = serve.kill();
    let _ = serve.wait();
}

// ---------------------------------------------------------------------------
// 16. spriff is meant to be composed in shell loops. Piping a verbose command
//     into `grep -q` closes stdout as soon as grep finds a match; that must not
//     print Rust's broken-pipe panic noise to stderr.
// ---------------------------------------------------------------------------
#[cfg(unix)]
#[test]
fn status_pipe_to_grep_q_does_not_emit_broken_pipe_panic() {
    let sb = Sandbox::new("pipe");
    let cwd = sb.cwd("op");
    let o = sb.run(
        &cwd,
        &["init", "pipetest", "--agents", "2", "--letter", "a"],
    );
    assert!(o.status.success(), "init failed: {}", stderr(&o));

    let pipe_err = sb.root.join("pipe.err");
    let status = Command::new("sh")
        .arg("-c")
        .arg(
            "\"$SPRIFF_BIN\" status --collab pipetest --as Alice 2>\"$PIPE_ERR\" | grep -q '^collaboration:'",
        )
        .env("SPRIFF_HOME", &sb.root)
        .env("SPRIFF_BIN", bin())
        .env("PIPE_ERR", &pipe_err)
        .env_remove("SPRIFF_COLLAB")
        .env_remove("SPRIFF_AS")
        .env_remove("SPRIFF_CONFIG")
        .current_dir(&cwd)
        .status()
        .expect("run status|grep");
    assert!(
        status.success(),
        "pipeline should match the first status line"
    );

    let err = std::fs::read_to_string(&pipe_err).unwrap_or_default();
    assert!(
        !err.contains("panicked") && !err.contains("Broken pipe"),
        "closed stdout pipe must be quiet, got stderr:\n{err}"
    );
}

// ---------------------------------------------------------------------------
// 17. `wait --once` is the NON-BLOCKING per-turn poll for a chat-driven agent:
//     it checks the inbox exactly once and exits immediately — 0 with the delta
//     printed when a peer turn is waiting, 2 when nothing is new. It must NOT
//     block, and it must record the read frontier so a later `ack` consumes
//     exactly what was shown (never a turn that lands afterward).
// ---------------------------------------------------------------------------
#[test]
fn wait_once_is_nonblocking_and_exit_coded() {
    let sb = Sandbox::new("wait-once");
    let a = sb.cwd("impl");
    let b = sb.cwd("rev");
    assert!(sb
        .run(
            &a,
            &["join", "--role", "implementer", "--project", "once demo"]
        )
        .status
        .success());
    assert!(sb
        .run(
            &b,
            &["join", "--role", "reviewer", "--project", "once demo"]
        )
        .status
        .success());

    // Nothing posted yet → a single non-blocking poll returns immediately with
    // exit code 2 (and does not hang). We give it a hard cap via the test harness
    // by simply running it: if it blocked, the suite would stall here.
    let empty = sb.run(&b, &["wait", "--once", "--as", "Alice"]);
    assert_eq!(
        empty.status.code(),
        Some(2),
        "an empty non-blocking poll must exit 2, got {:?}\nstderr:\n{}",
        empty.status.code(),
        stderr(&empty)
    );
    assert!(
        !stdout(&empty).contains("new turn(s) since your last ack"),
        "empty poll must not print a delta:\n{}",
        stdout(&empty)
    );

    // Implementer posts → the very next non-blocking poll returns exit 0 and
    // prints the delta.
    assert!(sb
        .run_stdin(
            &a,
            &[
                "post",
                "--as",
                "Abbey",
                "-s",
                "once-turn",
                "--status",
                "FYI"
            ],
            "once body ZZZ\n",
        )
        .status
        .success());

    let hit = sb.run(&b, &["wait", "--once", "--as", "Alice"]);
    assert_eq!(
        hit.status.code(),
        Some(0),
        "a non-blocking poll with a waiting turn must exit 0, got {:?}\nstderr:\n{}",
        hit.status.code(),
        stderr(&hit)
    );
    assert!(
        stdout(&hit).contains("once-turn") && stdout(&hit).contains("once body ZZZ"),
        "the poll must print the waiting delta:\n{}",
        stdout(&hit)
    );

    // The poll recorded the read frontier, so a plain `ack` now consumes exactly
    // what was shown and the next poll is clean (exit 2) — proving the one-shot
    // path shares the mid-turn-skip-safe frontier semantics of inbox/wait.
    assert!(sb.run(&b, &["ack", "--as", "Alice"]).status.success());
    let after = sb.run(&b, &["wait", "--once", "--as", "Alice"]);
    assert_eq!(
        after.status.code(),
        Some(2),
        "after ack the next non-blocking poll must be clean (exit 2), got {:?}",
        after.status.code()
    );
}
