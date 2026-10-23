//! `codeflow follow` and `codeflow work status` through the built binary
//! (TSK-422): every verb and its refusals, coordinator-held and lane-held
//! items through one sequence, concurrent appenders, read-only listing,
//! the recovery inventory, and the two-worktree journey.
//!
//! Actors are bound through the library fixture, which writes the same
//! stream, claim, lane and seat events the role commands will write.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use codeflow_core::coord::fixture;
use codeflow_core::coord::store::{self, Paths, ReadOutcome};
use codeflow_core::coord::transition::{ItemState, Ledger};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn task(id: &str, title: &str, status: &str) -> String {
    format!("---\nid: {id}\nuid: uid-{id}\nepic_id: null\nstandalone_reason: bounded outcome\nintegration_target: main\ntitle: {title}\nstatus: {status}\nwork_type: feat\nspecs: []\ndepends_on: []\ncreated: 2026-10-01\n---\n\n## Description\nWork.\n\n## Acceptance Criteria\n- AC-1 When run, the command shall succeed. (journey)\n")
}

/// A repository with two tasks on `main`, pushed to a bare `origin`.
struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    origin: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    let origin = dir.path().join("origin.git");
    std::fs::create_dir(&root).unwrap();
    git(
        dir.path(),
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            origin.to_str().unwrap(),
        ],
    );
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.name", "t"]);
    git(&root, &["config", "user.email", "t@example.test"]);
    write(
        &root,
        "project-management/tasks/TSK-001.md",
        &task("TSK-001", "ship the cache warmer", "todo"),
    );
    write(
        &root,
        "project-management/tasks/TSK-002.md",
        &task("TSK-002", "document the release gate timing", "todo"),
    );
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "docs: plan tasks"]);
    git(
        &root,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&root, &["push", "-q", "-u", "origin", "main"]);
    Fixture {
        _dir: dir,
        root,
        origin,
    }
}

fn cli(dir: &Path, env: &[(&str, &str)], args: &[&str]) -> Output {
    let mut cmd = Command::new(std::env::var_os("CODEFLOW_JOURNEY_BIN").map_or_else(
        || PathBuf::from(env!("CARGO_BIN_EXE_codeflow")),
        PathBuf::from,
    ));
    cmd.current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    // Every name is literal, so the fixture-environment contract scan can
    // read what the child inherits.
    cmd.env_remove("CLAUDECODE")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CODEX_THREAD_ID")
        .env_remove("CODEX_CI")
        .env_remove("GROK_SESSION_ID")
        .env_remove("CODEFLOW_COORD_ROLE")
        .env_remove("CODEFLOW_COORD_INSTANCE")
        .env_remove("CODEFLOW_COORD_GENERATION")
        .env_remove("CODEFLOW_COORD_STREAM")
        .env_remove("CODEFLOW_COORD_HARNESS")
        .env_remove("CODEFLOW_COORD_MODEL")
        .env_remove("CODEFLOW_COORD_SESSION");
    for (name, value) in env {
        match *name {
            "CODEFLOW_COORD_ROLE" => cmd.env("CODEFLOW_COORD_ROLE", value),
            "CODEFLOW_COORD_INSTANCE" => cmd.env("CODEFLOW_COORD_INSTANCE", value),
            "CODEFLOW_COORD_GENERATION" => cmd.env("CODEFLOW_COORD_GENERATION", value),
            "CODEFLOW_COORD_STREAM" => cmd.env("CODEFLOW_COORD_STREAM", value),
            "CLAUDECODE" => cmd.env("CLAUDECODE", value),
            other => panic!("the test sets no {other}"),
        };
    }
    cmd.output().unwrap()
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn ok(out: &Output) -> String {
    assert!(out.status.success(), "{}", text(out));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn refused(out: &Output, needle: &str) {
    assert_eq!(out.status.code(), Some(1), "{}", text(out));
    assert!(
        text(out).contains(needle),
        "wanted `{needle}` in {}",
        text(out)
    );
}

fn lane(id: &'static str, seat: &'static str) -> Vec<(&'static str, &'static str)> {
    vec![
        ("CODEFLOW_COORD_ROLE", id),
        ("CODEFLOW_COORD_INSTANCE", seat),
        ("CODEFLOW_COORD_GENERATION", "1"),
    ]
}

fn coordinator(stream: &'static str, instance: &'static str) -> Vec<(&'static str, &'static str)> {
    vec![
        ("CODEFLOW_COORD_ROLE", "coordinator"),
        ("CODEFLOW_COORD_INSTANCE", instance),
        ("CODEFLOW_COORD_GENERATION", "1"),
        ("CODEFLOW_COORD_STREAM", stream),
    ]
}

/// One stream led by `c1`, with lanes LN-001 (seat `s1`) and LN-002
/// (seat `s2`).
fn bind(root: &Path) -> Paths {
    let paths = Paths::discover(root).unwrap().0;
    fixture::open_stream(&paths, "WS-001", "c1").unwrap();
    assert_eq!(
        fixture::open_lane(&paths, "WS-001", "c1", "s1").unwrap(),
        "LN-001"
    );
    assert_eq!(
        fixture::open_lane(&paths, "WS-001", "c1", "s2").unwrap(),
        "LN-002"
    );
    paths
}

fn folded(paths: &Paths) -> Ledger {
    match store::read(paths) {
        ReadOutcome::Ledger(snapshot) => {
            assert!(
                snapshot.folded.damage.is_none(),
                "{:?}",
                snapshot.folded.damage
            );
            snapshot.folded.ledger
        }
        other => panic!("expected a ledger: {other:?}"),
    }
}

fn add_args<'a>(title: &'a str, link: &'a [&'a str]) -> Vec<&'a str> {
    let mut args = vec![
        "follow",
        "add",
        "--title",
        title,
        "--description",
        "Raised in review; a cold reader needs only this record.",
        "--done-when",
        "the linked work is complete on its target",
        "--source",
        "review",
    ];
    args.extend_from_slice(link);
    args
}

/// Land TSK-001 on `main`: mark it complete there and push.
fn land(root: &Path, id: &str) {
    write(
        root,
        &format!("project-management/tasks/{id}.md"),
        &task(id, "landed", "complete"),
    );
    git(root, &["commit", "-qam", "docs(tasks): complete the task"]);
    git(root, &["push", "-q", "origin", "main"]);
}

#[test]
#[allow(clippy::too_many_lines)]
fn every_verb_records_its_event_and_each_refusal_holds() {
    let f = fixture();
    let paths = bind(&f.root);
    let (one, two) = (lane("LN-001", "s1"), lane("LN-002", "s2"));
    let out = ok(&cli(
        &f.root,
        &one,
        &add_args("ask about the cache size", &["--link", "context:TSK-002"]),
    ));
    assert!(out.contains("FU-001 added (sequence"), "{out}");
    ok(&cli(
        &f.root,
        &one,
        &[
            "follow",
            "update",
            "FU-001",
            "--title",
            "ask the operator about the cache size",
        ],
    ));
    refused(
        &cli(
            &f.root,
            &two,
            &["follow", "update", "FU-001", "--title", "x"],
        ),
        "only the holder",
    );
    // `source` is never updatable: update has no such flag.
    let out = cli(
        &f.root,
        &one,
        &["follow", "update", "FU-001", "--source", "operator"],
    );
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    let out = ok(&cli(
        &f.root,
        &one,
        &[
            "follow",
            "check",
            "FU-001",
            "--observed",
            "the operator has not answered",
            "--next-check",
            "1h",
            "--answer-source",
            "operator",
            "--offline",
        ],
    ));
    assert!(out.contains("FU-001 checked"), "{out}");
    ok(&cli(
        &f.root,
        &one,
        &["follow", "hand-over", "FU-001", "--to", "LN-002"],
    ));
    let sha = git(&f.root, &["rev-parse", "HEAD"]);
    let evidence = format!("commit:{sha}");
    refused(
        &cli(
            &f.root,
            &one,
            &["follow", "close", "FU-001", "--evidence", &evidence],
        ),
        "only the holder",
    );
    refused(
        &cli(
            &f.root,
            &two,
            &[
                "follow",
                "close",
                "FU-001",
                "--evidence",
                "commit:0123456789abcdef",
            ],
        ),
        "not in this repository",
    );
    ok(&cli(
        &f.root,
        &two,
        &["follow", "close", "FU-001", "--evidence", &evidence],
    ));
    ok(&cli(
        &f.root,
        &two,
        &[
            "follow",
            "reopen",
            "FU-001",
            "--reason",
            "the commit did not answer it",
        ],
    ));
    refused(
        &cli(
            &f.root,
            &two,
            &[
                "follow",
                "drop",
                "FU-001",
                "--reason",
                " ",
                "--disposition",
                "nothing",
            ],
        ),
        "reason",
    );
    ok(&cli(
        &f.root,
        &two,
        &[
            "follow",
            "drop",
            "FU-001",
            "--reason",
            "superseded by the release notes",
            "--disposition",
            "nothing left",
        ],
    ));
    // An agent never records as the operator; an unbound observer never writes.
    let mut agent = vec![("CODEFLOW_COORD_ROLE", "operator"), ("CLAUDECODE", "1")];
    refused(
        &cli(
            &f.root,
            &agent,
            &add_args("x", &["--no-link-reason", "none"]),
        ),
        "cannot record as the operator",
    );
    agent.truncate(1);
    ok(&cli(
        &f.root,
        &agent,
        &add_args("asked by hand", &["--no-link-reason", "a hallway question"]),
    ));
    refused(
        &cli(&f.root, &[], &add_args("x", &["--no-link-reason", "none"])),
        "no role is bound",
    );
    // A record that links nothing and gives no reason is refused on input.
    refused(
        &cli(&f.root, &one, &add_args("x", &[])),
        "typed link or a no_link_reason",
    );
    // The folded state, and the list as a table and as Markdown.
    let ledger = folded(&paths);
    let item = &ledger.items["FU-001"];
    assert!(matches!(item.state, ItemState::Dropped { .. }));
    assert_eq!(item.proofs.len(), 1, "the reopened proof stays in history");
    assert_eq!(
        item.last_check.as_ref().unwrap().1.answer_source.as_deref(),
        Some("operator")
    );
    assert_eq!(ledger.items["FU-002"].added_by.role, "operator");
    let table = ok(&cli(&f.root, &[], &["follow", "list", "--all"]));
    assert!(
        table.contains("1 open") && table.contains("1 dropped"),
        "{table}"
    );
    let md = ok(&cli(&f.root, &[], &["follow", "list", "--md"]));
    assert!(md.starts_with("# Coordination ledger"), "{md}");
    // A cold reader gets the whole item without its conversation.
    let full = ok(&cli(&f.root, &[], &["follow", "list", "FU-001", "--md"]));
    for needle in [
        "## FU-001: ask the operator about the cache size",
        "**description:** Raised in review",
        "**done when:** the linked work",
        "**source:** review",
        "**links:** context:TSK-002",
        "**proof:** asserted: commit:",
        "**reopened:** the commit did not answer it",
        "**dropped:** superseded by the release notes",
        "(answer from operator)",
    ] {
        assert!(full.contains(needle), "missing `{needle}` in:\n{full}");
    }
}

#[test]
fn a_coordinator_held_item_runs_the_same_sequence_as_a_lane_held_one() {
    let f = fixture();
    let paths = bind(&f.root);
    let (lead, one) = (coordinator("WS-001", "c1"), lane("LN-001", "s1"));
    ok(&cli(
        &f.root,
        &lead,
        &add_args(
            "coordinator follows the cache warmer",
            &["--link", "fulfils:TSK-001"],
        ),
    ));
    ok(&cli(
        &f.root,
        &one,
        &add_args(
            "lane follows the cache warmer",
            &["--link", "fulfils:TSK-001"],
        ),
    ));
    // The same matrix: neither closes the other's item.
    let sha = git(&f.root, &["rev-parse", "HEAD"]);
    let evidence = format!("commit:{sha}");
    refused(
        &cli(
            &f.root,
            &one,
            &["follow", "close", "FU-001", "--evidence", &evidence],
        ),
        "only the holder",
    );
    refused(
        &cli(
            &f.root,
            &lead,
            &["follow", "close", "FU-002", "--evidence", &evidence],
        ),
        "only the holder",
    );
    for actor in [&lead, &one] {
        let out = ok(&cli(&f.root, actor, &["follow", "check", "--offline"]));
        assert!(!out.contains("closed"), "{out}");
    }
    land(&f.root, "TSK-001");
    for (actor, id) in [(&lead, "FU-001"), (&one, "FU-002")] {
        let out = ok(&cli(&f.root, actor, &["follow", "check", "--offline"]));
        assert!(
            out.contains(&format!("{id} closed on machine-verified evidence")),
            "{out}"
        );
    }
    let ledger = folded(&paths);
    let comparable = |id: &str| {
        let item = &ledger.items[id];
        let ItemState::Closed(proof) = &item.state else {
            panic!("{id} is not closed")
        };
        (
            item.revision,
            proof.kind,
            proof.target.clone(),
            proof.revision.clone(),
            proof.source.clone(),
            proof.reference.clone(),
        )
    };
    assert_eq!(comparable("FU-001"), comparable("FU-002"));
    let listed = ok(&cli(&f.root, &[], &["follow", "list", "--all"]));
    let under = listed
        .split("WS-001 / coordinator")
        .nth(1)
        .expect("a coordinator group");
    assert!(
        under.lines().nth(1).unwrap_or_default().contains("FU-001"),
        "{listed}"
    );
    assert!(listed.contains("WS-001 / LN-001"), "{listed}");
}

#[test]
fn three_concurrent_appenders_never_corrupt_the_log_or_repeat_a_sequence() {
    let f = fixture();
    let paths = bind(&f.root);
    let handles: Vec<_> = [("LN-001", "s1"), ("LN-002", "s2"), ("coordinator", "c1")]
        .into_iter()
        .map(|(role, instance)| {
            let root = f.root.clone();
            std::thread::spawn(move || {
                let env = if role == "coordinator" {
                    coordinator("WS-001", instance)
                } else {
                    lane(role, instance)
                };
                for n in 0..8 {
                    let title = format!("{role} item {n}");
                    let out = cli(
                        &root,
                        &env,
                        &add_args(&title, &["--no-link-reason", "load test"]),
                    );
                    assert!(out.status.success(), "{}", text(&out));
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    let ledger = folded(&paths);
    assert_eq!(ledger.items.len(), 24);
    let numbers: Vec<u64> = ledger
        .items
        .keys()
        .map(|id| id.trim_start_matches("FU-").parse().unwrap())
        .collect();
    assert_eq!(numbers, (1..=24).collect::<Vec<_>>());
    let log = std::fs::read_to_string(&paths.events).unwrap();
    for (index, line) in log.lines().enumerate() {
        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(value["sequence"].as_u64(), Some(index as u64 + 1));
    }
}

fn tree_state(dir: &Path) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let meta = entry.metadata().unwrap();
            if meta.is_dir() {
                stack.push(entry.path());
            }
            out.push((entry.path(), meta.len(), meta.modified().unwrap()));
        }
    }
    out.sort();
    out
}

#[test]
fn every_read_writes_nothing_and_reports_empty_unavailable_and_unwritten_apart() {
    let f = fixture();
    let paths = Paths::discover(&f.root).unwrap().0;
    // No ledger: empty, and nothing is created.
    let out = ok(&cli(&f.root, &[], &["follow", "list"]));
    assert!(out.contains("empty"), "{out}");
    assert!(!paths.dir.parent().unwrap().exists());
    bind(&f.root);
    ok(&cli(
        &f.root,
        &lane("LN-001", "s1"),
        &add_args("a", &["--no-link-reason", "none"]),
    ));
    let before = tree_state(&paths.dir);
    for args in [
        &["follow", "list"][..],
        &["follow", "list", "--md", "--all"],
        &["follow", "list", "FU-001"],
        &["follow", "snapshot"],
    ] {
        ok(&cli(&f.root, &[], args));
    }
    assert_eq!(
        before,
        tree_state(&paths.dir),
        "a read changed the ledger's files"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let set = |path: &Path, mode| {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        set(&paths.events, 0o400);
        set(&paths.dir, 0o500);
        let listed = cli(&f.root, &[], &["follow", "list"]);
        let added = cli(
            &f.root,
            &lane("LN-001", "s1"),
            &add_args("b", &["--no-link-reason", "none"]),
        );
        set(&paths.dir, 0o700);
        set(&paths.events, 0o000);
        let denied = cli(&f.root, &[], &["follow", "list"]);
        set(&paths.events, 0o600);
        assert!(listed.status.success(), "{}", text(&listed));
        assert_eq!(added.status.code(), Some(5), "{}", text(&added));
        assert!(text(&added).contains("unwritten"), "{}", text(&added));
        assert_eq!(denied.status.code(), Some(6), "{}", text(&denied));
        assert!(text(&denied).contains("unavailable"), "{}", text(&denied));
    }
    // A damaged ledger only warns on read and refuses a write.
    let mut log = std::fs::read_to_string(&paths.events).unwrap();
    log.push_str("{\"broken\": true}\n");
    std::fs::write(&paths.events, log).unwrap();
    let listed = cli(&f.root, &[], &["follow", "list"]);
    assert!(
        listed.status.success() && text(&listed).contains("read-only"),
        "{}",
        text(&listed)
    );
    let added = cli(
        &f.root,
        &lane("LN-001", "s1"),
        &add_args("c", &["--no-link-reason", "none"]),
    );
    assert_eq!(added.status.code(), Some(4), "{}", text(&added));
}

#[test]
fn suggestions_snapshot_and_purge_work_from_the_command_line() {
    let f = fixture();
    let paths = bind(&f.root);
    let one = lane("LN-001", "s1");
    let out = ok(&cli(
        &f.root,
        &one,
        &add_args(
            "document the release gate timing again",
            &["--no-link-reason", "not decided yet"],
        ),
    ));
    assert!(out.contains("similar: TSK-002"), "{out}");
    let out = ok(&cli(
        &f.root,
        &one,
        &add_args(
            "document release gate timing",
            &[
                "--no-link-reason",
                "none",
                "--reject",
                "TSK-002=a different gate",
            ],
        ),
    ));
    assert!(!out.contains("similar: TSK-002"), "{out}");
    assert!(out.contains("similar: FU-001"), "{out}");
    // An exact duplicate (same source identity) is refused.
    let exact = [
        "--no-link-reason",
        "none",
        "--source-reference",
        "review 9 finding 2",
    ];
    ok(&cli(&f.root, &one, &add_args("first", &exact)));
    refused(
        &cli(&f.root, &one, &add_args("second", &exact)),
        "exact duplicate",
    );
    // A retried add with the same key records once.
    let keyed = ["--no-link-reason", "none", "--idempotency-key", "retry-1"];
    ok(&cli(&f.root, &one, &add_args("keyed", &keyed)));
    let again = ok(&cli(&f.root, &one, &add_args("keyed", &keyed)));
    assert!(again.contains("already recorded"), "{again}");
    // The snapshot is written where asked and never committed.
    let output = f.root.join("ledger-snapshot.md");
    ok(&cli(
        &f.root,
        &[],
        &["follow", "snapshot", "--output", output.to_str().unwrap()],
    ));
    assert!(std::fs::read_to_string(&output)
        .unwrap()
        .contains("## FU-001"));
    assert!(git(&f.root, &["status", "--porcelain"]).contains("?? ledger-snapshot.md"));
    refused_or(
        &cli(
            &f.root,
            &[],
            &["follow", "snapshot", "--output", output.to_str().unwrap()],
        ),
        5,
    );
    // Purge by the holder removes the item's text from the live log.
    ok(&cli(&f.root, &one, &["follow", "purge", "FU-001"]));
    let log = std::fs::read_to_string(&paths.events).unwrap();
    assert!(!log.contains("document the release gate timing again"));
    assert_eq!(folded(&paths).items["FU-001"].record.title, "[purged]");
}

fn refused_or(out: &Output, code: i32) {
    assert_eq!(out.status.code(), Some(code), "{}", text(out));
}

#[test]
fn work_status_lists_each_kind_of_unlanded_work_per_task() {
    let f = fixture();
    let wt = |name: &str| f.root.parent().unwrap().join(name);
    // Uncommitted changes.
    git(
        &f.root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "task/TSK-001-dirty",
            wt("dirty").to_str().unwrap(),
        ],
    );
    write(&wt("dirty"), "notes.txt", "draft\n");
    // An unpushed commit.
    git(
        &f.root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "task/TSK-002-ahead",
            wt("ahead").to_str().unwrap(),
        ],
    );
    write(&wt("ahead"), "a.txt", "a\n");
    git(&wt("ahead"), &["add", "a.txt"]);
    git(&wt("ahead"), &["commit", "-qm", "feat: unpushed work"]);
    // A commit on no branch.
    git(
        &f.root,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            wt("loose").to_str().unwrap(),
        ],
    );
    write(&wt("loose"), "b.txt", "b\n");
    git(&wt("loose"), &["add", "b.txt"]);
    git(
        &wt("loose"),
        &["commit", "-qm", "feat: commit on no branch"],
    );
    // A worktree whose folder is gone.
    git(
        &f.root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "task/TSK-003-gone",
            wt("gone").to_str().unwrap(),
        ],
    );
    std::fs::remove_dir_all(wt("gone")).unwrap();
    let out = ok(&cli(&f.root, &[], &["work", "status"]));
    let section = |task: &str| {
        out.split(&format!("\n{task}\n"))
            .nth(1)
            .unwrap_or_default()
            .to_string()
    };
    assert!(
        section("TSK-001").contains("uncommitted: 1 (?? notes.txt)"),
        "{out}"
    );
    assert!(
        section("TSK-002").contains("unpushed commits: 1 ("),
        "{out}"
    );
    assert!(section("TSK-003").contains("not recoverable"), "{out}");
    assert!(out.contains("commits on no branch: 1 ("), "{out}");
    assert!(out.contains("feat: commit on no branch"), "{out}");
    // The clean main checkout is counted, not listed.
    assert!(out.starts_with("work status: 4 of 5 worktrees"), "{out}");
}

/// The journey (AC-9): two worktrees, two actors bound through the
/// fixture, a fulfils item and a no-link item, a close on a commit, a
/// refused cross close, a feature-branch completion that closes nothing,
/// the landing that closes the fulfilled item with machine-verified
/// evidence, one folded state from either worktree, and CI's verdict with
/// the ledger removed. `CODEFLOW_JOURNEY_BIN` points it at an installed
/// build.
#[test]
#[allow(clippy::too_many_lines)]
fn journey_two_worktrees_two_actors_and_a_landing() {
    let f = fixture();
    let second = f.root.parent().unwrap().join("second");
    git(
        &f.root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "task/TSK-001-cache-warmer",
            second.to_str().unwrap(),
        ],
    );
    let paths = bind(&f.root);
    let (one, two) = (lane("LN-001", "s1"), lane("LN-002", "s2"));
    let out = ok(&cli(
        &f.root,
        &one,
        &add_args(
            "confirm the cache warmer lands",
            &["--link", "fulfils:TSK-001"],
        ),
    ));
    println!("journey: {out}");
    let out = ok(&cli(
        &second,
        &two,
        &add_args(
            "rerun the flaky timing test",
            &["--no-link-reason", "a local observation"],
        ),
    ));
    println!("journey: {out}");
    let sha = git(&second, &["rev-parse", "HEAD"]);
    let evidence = format!("commit:{sha}");
    let out = ok(&cli(
        &second,
        &two,
        &["follow", "close", "FU-002", "--evidence", &evidence],
    ));
    println!("journey: {out}");
    let out = cli(
        &second,
        &two,
        &["follow", "close", "FU-001", "--evidence", &evidence],
    );
    refused(&out, "only the holder");
    println!("journey: refused as expected: {}", text(&out).trim());
    // Completion on the feature branch closes nothing.
    write(
        &second,
        "project-management/tasks/TSK-001.md",
        &task("TSK-001", "ship the cache warmer", "complete"),
    );
    git(
        &second,
        &[
            "commit",
            "-qam",
            "docs(tasks): complete TSK-001 on its branch",
        ],
    );
    let out = ok(&cli(&f.root, &one, &["follow", "check", "--offline"]));
    assert!(!out.contains("closed"), "{out}");
    println!("journey: after the branch completion: {}", out.trim());
    // The landing closes it, machine-verified.
    git(
        &f.root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "Merge TSK-001",
            "task/TSK-001-cache-warmer",
        ],
    );
    git(&f.root, &["push", "-q", "origin", "main"]);
    let out = ok(&cli(&f.root, &one, &["follow", "check", "--offline"]));
    assert!(
        out.contains("FU-001 closed on machine-verified evidence"),
        "{out}"
    );
    println!("journey: after the landing: {}", out.trim());
    let ledger = folded(&paths);
    let ItemState::Closed(proof) = &ledger.items["FU-001"].state else {
        panic!("FU-001 open")
    };
    assert_eq!(
        proof.kind,
        codeflow_core::coord::schema::EvidenceKind::MachineVerified
    );
    assert_eq!(proof.target.as_deref(), Some("main"));
    // One folded state, read from either worktree.
    let from_root = ok(&cli(&f.root, &[], &["follow", "list", "--md", "--all"]));
    let from_second = ok(&cli(&second, &[], &["follow", "list", "--md", "--all"]));
    assert_eq!(from_root, from_second);
    println!("journey: list --md --all\n{from_root}");
    // CI gives the same verdict with the ledger present and removed.
    let present = cli(&second, &[], &["ci"]);
    std::fs::remove_dir_all(&paths.dir).unwrap();
    let removed = cli(&second, &[], &["ci"]);
    assert_eq!(present.status.code(), removed.status.code());
    assert_eq!(present.stdout, removed.stdout);
    println!(
        "journey: ci exit {:?} with and without the ledger",
        present.status.code()
    );
    let _ = &f.origin;
}

/// AC-5: a link resolves through the record's id and UID, never its title,
/// so a renamed task keeps its link; the id reused by another record (a new
/// UID on the target) makes the follow-up due for relinking.
#[test]
fn a_renamed_task_keeps_its_link_and_a_reused_id_is_caught() {
    let f = fixture();
    bind(&f.root);
    let one = lane("LN-001", "s1");
    ok(&cli(
        &f.root,
        &one,
        &add_args(
            "watch the gate timing task",
            &["--link", "prerequisite:TSK-002"],
        ),
    ));
    write(
        &f.root,
        "project-management/tasks/TSK-002.md",
        &task("TSK-002", "time the release gate end to end", "todo"),
    );
    git(&f.root, &["commit", "-qam", "docs(tasks): rename the task"]);
    git(&f.root, &["push", "-q", "origin", "main"]);
    let shown = ok(&cli(&f.root, &[], &["follow", "list", "FU-001"]));
    assert!(shown.contains("prerequisite:TSK-002"), "{shown}");
    assert!(!shown.contains("different record"), "{shown}");
    let reused = task("TSK-002", "another record", "todo").replace("uid-TSK-002", "uid-other");
    write(&f.root, "project-management/tasks/TSK-002.md", &reused);
    git(&f.root, &["commit", "-qam", "docs(tasks): reuse the id"]);
    git(&f.root, &["push", "-q", "origin", "main"]);
    let shown = ok(&cli(&f.root, &[], &["follow", "list", "FU-001"]));
    assert!(shown.contains("now names a different record"), "{shown}");
}
