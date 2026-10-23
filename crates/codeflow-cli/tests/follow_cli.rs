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
/// A task record complete with an acceptance block that passes the
/// acceptance rules, as a landed task's record is.
fn accepted(id: &str, title: &str, reviewed: &str) -> String {
    format!(
        "{}\n## Closeout\n\n```yaml\nacceptance:\n  reviewed: {}\n  review: \"reviewed by the fixture\"\n  criteria:\n    AC-1: verified | the fixture ran the command\n  journey: verified | the fixture journey\n  not_verified: \"nothing\"\n  follow_ups: \"none: nothing left\"\n  verdict: approved\n```\n",
        task(id, title, "complete"),
        reviewed
    )
}

/// The work of a task, reviewed: a commit on its branch in `dir`, whose id
/// the completion's acceptance block names.
fn reviewed_work(dir: &Path, id: &str) -> String {
    write(dir, &format!("src/{id}.txt"), "the reviewed work\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "feat: the reviewed work"]);
    git(dir, &["rev-parse", "HEAD"])
}

/// Land a task as a pull request does: reviewed work and its bound
/// completion on a task branch, merged into `main` and pushed.
fn land(root: &Path, id: &str) {
    let branch = format!("task/{id}-landing");
    git(root, &["checkout", "-qb", &branch]);
    let reviewed = reviewed_work(root, id);
    // Only the status and the Closeout change after the review.
    let path = root.join(format!("project-management/tasks/{id}.md"));
    let record = std::fs::read_to_string(&path).unwrap();
    let title = record
        .lines()
        .find_map(|line| line.strip_prefix("title: "))
        .unwrap()
        .to_string();
    write(
        root,
        &format!("project-management/tasks/{id}.md"),
        &accepted(id, &title, &reviewed),
    );
    git(root, &["commit", "-qam", "docs(tasks): complete the task"]);
    git(root, &["checkout", "-q", "main"]);
    git(
        root,
        &["merge", "-q", "--no-ff", "-m", "Merge the task", &branch],
    );
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
    // Security F1: unsetting the harness marker is not enough. Without an
    // interactive terminal the operator role fails closed.
    agent.truncate(1);
    refused(
        &cli(
            &f.root,
            &agent,
            &add_args("asked by hand", &["--no-link-reason", "a hallway question"]),
        ),
        "only from an interactive terminal",
    );
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
    assert!(!ledger.items.contains_key("FU-002"));
    let table = ok(&cli(&f.root, &[], &["follow", "list", "--all"]));
    assert!(
        table.contains("0 open") && table.contains("1 dropped"),
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
    let reviewed = reviewed_work(&second, "TSK-001");
    write(
        &second,
        "project-management/tasks/TSK-001.md",
        &accepted("TSK-001", "ship the cache warmer", &reviewed),
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

// ---------------------------------------------------------------------------
// Review round 1 reproductions through the binary; each failed at
// 998dadb1e.
// ---------------------------------------------------------------------------

/// Codex F2: a task complete on its target without a passing acceptance
/// block does not close a fulfils item.
#[test]
fn completion_without_acceptance_does_not_close() {
    let f = fixture();
    bind(&f.root);
    let one = lane("LN-001", "s1");
    ok(&cli(
        &f.root,
        &one,
        &add_args("watch the cache warmer", &["--link", "fulfils:TSK-001"]),
    ));
    write(
        &f.root,
        "project-management/tasks/TSK-001.md",
        &task("TSK-001", "landed", "complete"),
    );
    git(
        &f.root,
        &["commit", "-qam", "docs(tasks): complete without acceptance"],
    );
    git(&f.root, &["push", "-q", "origin", "main"]);
    let out = ok(&cli(&f.root, &one, &["follow", "check", "--offline"]));
    assert!(!out.contains("closed"), "{out}");
}

/// Security F10 (round 2): a branch link is observed only over https or
/// ssh with the stock upload pack. A local-path remote is never contacted,
/// so its configured upload-pack command never runs and nothing closes.
/// (A recreated branch reopening its closure is covered with an observer
/// in `coord_store.rs`.)
#[cfg(unix)]
#[test]
fn a_local_transport_remote_is_never_run_or_trusted() {
    let f = fixture();
    let paths = bind(&f.root);
    let one = lane("LN-001", "s1");
    git(
        &f.root,
        &["push", "-q", "origin", "main:refs/heads/task/old"],
    );
    git(&f.root, &["push", "-q", "origin", "--delete", "task/old"]);
    let marker = f.root.parent().unwrap().join("upload-pack-ran");
    git(
        &f.root,
        &[
            "config",
            "remote.origin.uploadpack",
            &format!(
                "sh -c 'touch {}; exec git-upload-pack \"$@\"' x",
                marker.display()
            ),
        ],
    );
    ok(&cli(
        &f.root,
        &one,
        &add_args(
            "delete the old branch",
            &["--link", "fulfils:branch:task/old"],
        ),
    ));
    let out = ok(&cli(&f.root, &one, &["follow", "check"]));
    assert!(!out.contains("closed"), "{out}");
    assert!(!marker.exists(), "the remote's upload-pack command ran");
    assert_eq!(folded(&paths).items["FU-001"].state, ItemState::Open);
}

/// Codex F6: an update that adds a dependency records its baseline, so a
/// later change makes the item due, and a far schedule is capped daily.
#[test]
fn an_updated_dependency_is_watched_and_its_wait_is_capped() {
    let f = fixture();
    let paths = bind(&f.root);
    let one = lane("LN-001", "s1");
    ok(&cli(
        &f.root,
        &one,
        &add_args("watch the gate task", &["--no-link-reason", "not yet"]),
    ));
    ok(&cli(
        &f.root,
        &one,
        &[
            "follow",
            "check",
            "FU-001",
            "--observed",
            "nothing yet",
            "--offline",
        ],
    ));
    ok(&cli(
        &f.root,
        &one,
        &[
            "follow",
            "update",
            "FU-001",
            "--link",
            "prerequisite:TSK-002",
            "--next-check",
            "30d",
        ],
    ));
    let next = folded(&paths).items["FU-001"].record.next_check.clone();
    let next = codeflow_core::coord::parse_time(&next).unwrap();
    assert!(next <= codeflow_core::coord::now_secs() + codeflow_core::coord::MAX_RECHECK_SECS + 5);
    write(
        &f.root,
        "project-management/tasks/TSK-002.md",
        &task("TSK-002", "gate", "blocked"),
    );
    git(&f.root, &["commit", "-qam", "docs(tasks): block the task"]);
    git(&f.root, &["push", "-q", "origin", "main"]);
    let shown = ok(&cli(&f.root, &[], &["follow", "list", "FU-001"]));
    assert!(
        shown.contains("prerequisite:TSK-002 changed: todo -> blocked"),
        "{shown}"
    );
}

/// Codex F7, security F6: a rejected argument is never echoed when it is
/// secret-shaped.
#[test]
fn a_rejected_secret_argument_is_never_echoed() {
    let f = fixture();
    bind(&f.root);
    let one = lane("LN-001", "s1");
    let token = format!("ghp_{}", "w".repeat(36));
    for args in [
        vec!["--link".to_string(), format!("context:{token}")],
        vec!["--waits-on".to_string(), token.clone()],
        vec![
            "--next-check".to_string(),
            token.clone(),
            "--no-link-reason".into(),
            "x".into(),
        ],
    ] {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = cli(&f.root, &one, &add_args("x", &refs));
        assert!(!out.status.success());
        assert!(!text(&out).contains(&token), "echoed the token");
    }
}

/// Codex F9: `work status` never writes Git's index, even with stale stat
/// data that `git status` would refresh.
#[test]
fn work_status_never_writes_the_index() {
    let f = fixture();
    let file = f.root.join("project-management/tasks/TSK-001.md");
    let content = std::fs::read(&file).unwrap();
    std::fs::write(&file, &content).unwrap();
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    std::fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(later)
        .unwrap();
    let index = f.root.join(".git/index");
    let before = std::fs::read(&index).unwrap();
    ok(&cli(&f.root, &[], &["work", "status"]));
    assert_eq!(
        before,
        std::fs::read(&index).unwrap(),
        "work status wrote the index"
    );
}

/// A bound `follow check` that records no event still refreshes the
/// actor's liveness; a read never does.
#[test]
fn a_check_with_nothing_to_record_refreshes_liveness() {
    let f = fixture();
    let paths = bind(&f.root);
    let status = paths.status.join("s2.json");
    assert!(!status.exists());
    ok(&cli(&f.root, &[], &["follow", "list"]));
    assert!(!status.exists());
    let out = ok(&cli(
        &f.root,
        &lane("LN-002", "s2"),
        &["follow", "check", "--offline"],
    ));
    assert!(out.contains("nothing due"), "{out}");
    assert!(status.is_file(), "the check refreshed no liveness");
}

// ---------------------------------------------------------------------------
// Review round 2 reproductions; each failed at 03bc5a13f.
// ---------------------------------------------------------------------------

/// Codex R1: an acceptance block whose reviewed commit does not exist is
/// not bound, so it never closes as machine-verified.
#[test]
fn an_unbound_acceptance_block_does_not_close() {
    let f = fixture();
    let paths = bind(&f.root);
    let by = coordinator("WS-001", "c1");
    ok(&cli(
        &f.root,
        &by,
        &add_args("verify accepted work", &["--link", "fulfils:TSK-001"]),
    ));
    write(
        &f.root,
        "project-management/tasks/TSK-001.md",
        &accepted("TSK-001", "ship the cache warmer", &"0".repeat(40)),
    );
    git(
        &f.root,
        &["commit", "-qam", "docs: add syntactic acceptance"],
    );
    git(&f.root, &["push", "-q", "origin", "main"]);
    ok(&cli(&f.root, &by, &["follow", "check", "--offline"]));
    assert_eq!(folded(&paths).items["FU-001"].state, ItemState::Open);
}

/// Codex R2, security F3-r: a work branch named `fix/main`, and an
/// `integration/` line the agent made locally, are never landing lines,
/// even with a bound acceptance block.
#[test]
fn a_made_up_landing_line_never_closes() {
    for branch in ["fix/main", "integration/EPC-999-fake"] {
        let f = fixture();
        let paths = bind(&f.root);
        let by = coordinator("WS-001", "c1");
        // A new task whose record exists only on the made-up line, as a
        // task does before its planning pull request lands.
        git(&f.root, &["checkout", "-qb", branch]);
        let todo = task("TSK-003", "a new task", "todo").replace(
            "integration_target: main",
            &format!("integration_target: \"{branch}\""),
        );
        write(&f.root, "project-management/tasks/TSK-003.md", &todo);
        git(&f.root, &["add", "."]);
        git(
            &f.root,
            &["commit", "-qm", "docs(tasks): plan off the line"],
        );
        ok(&cli(
            &f.root,
            &by,
            &add_args("verify the new task", &["--link", "fulfils:TSK-003"]),
        ));
        let reviewed = reviewed_work(&f.root, "TSK-003");
        let record = accepted("TSK-003", "a new task", &reviewed).replace(
            "integration_target: main",
            &format!("integration_target: \"{branch}\""),
        );
        write(&f.root, "project-management/tasks/TSK-003.md", &record);
        git(
            &f.root,
            &["commit", "-qam", "docs(tasks): complete off the line"],
        );
        // Pushed, it is still no landing line: a work branch, or an
        // integration line no epic on the default target names.
        git(&f.root, &["push", "-q", "origin", branch]);
        let out = ok(&cli(&f.root, &by, &["follow", "check", "--offline"]));
        assert!(!out.contains("closed"), "{branch}: {out}");
        assert_eq!(
            folded(&paths).items["FU-001"].state,
            ItemState::Open,
            "{branch}"
        );
    }
}

/// Codex R4, security F11: purging one item removes only its own lines
/// from a repaired damaged suffix; another item's recovery event stays.
#[test]
fn purge_keeps_other_items_quarantined_lines() {
    let f = fixture();
    let paths = bind(&f.root);
    let by = coordinator("WS-001", "c1");
    for title in ["first", "second"] {
        ok(&cli(
            &f.root,
            &by,
            &add_args(title, &["--no-link-reason", "unlinked"]),
        ));
    }
    let prefix = std::fs::read(&paths.events).unwrap();
    ok(&cli(
        &f.root,
        &by,
        &[
            "follow",
            "check",
            "FU-001",
            "--observed",
            "PRIVATE-ONE",
            "--offline",
        ],
    ));
    ok(&cli(
        &f.root,
        &by,
        &[
            "follow",
            "check",
            "FU-002",
            "--observed",
            "KEEP-TWO-RECOVERY",
            "--offline",
        ],
    ));
    let all = std::fs::read(&paths.events).unwrap();
    let mut damaged = prefix.clone();
    damaged.extend_from_slice(b"damaged interior line\n");
    damaged.extend_from_slice(&all[prefix.len()..]);
    std::fs::write(&paths.events, damaged).unwrap();
    let quarantined = store::repair(&paths, store::COMMAND_WAIT)
        .unwrap()
        .quarantined
        .unwrap();
    ok(&cli(&f.root, &by, &["follow", "purge", "FU-001"]));
    let kept = std::fs::read_to_string(&quarantined).unwrap();
    assert!(
        kept.contains("KEEP-TWO-RECOVERY"),
        "the other item's line was lost"
    );
    assert!(
        !kept.contains("PRIVATE-ONE"),
        "the purged item's line stayed"
    );
    assert!(kept.contains("damaged interior line"));
}

/// Codex R5: after a purge, a retry of the same keyed, source-identified
/// add is recognised and never restores the purged text.
#[test]
fn a_retry_after_purge_never_restores_text() {
    let f = fixture();
    let paths = bind(&f.root);
    let by = coordinator("WS-001", "c1");
    let args = add_args(
        "PRIVATE review content",
        &[
            "--no-link-reason",
            "unlinked",
            "--source-reference",
            "review-identity",
            "--idempotency-key",
            "review-retry",
        ],
    );
    ok(&cli(&f.root, &by, &args));
    ok(&cli(&f.root, &by, &["follow", "purge", "FU-001"]));
    let out = ok(&cli(&f.root, &by, &args));
    assert!(out.contains("already recorded"), "{out}");
    assert_eq!(folded(&paths).items.len(), 1);
    assert!(!std::fs::read_to_string(&paths.events)
        .unwrap()
        .contains("PRIVATE"));
    // Without the key, the same source is still an exact duplicate.
    let keyless = add_args(
        "PRIVATE again",
        &[
            "--no-link-reason",
            "unlinked",
            "--source-reference",
            "review-identity",
        ],
    );
    refused(&cli(&f.root, &by, &keyless), "exact duplicate");
}

/// Codex R6: a control character before a token in a damaged line does not
/// hide the token from the damage warning's redaction.
#[test]
fn a_control_character_does_not_hide_a_token_in_a_damage_warning() {
    let f = fixture();
    let paths = bind(&f.root);
    let token = format!("ghp_{}", "z".repeat(36));
    let raw = std::fs::read_to_string(&paths.events).unwrap();
    let raw = raw.replacen(
        "\"event\":\"stream_opened\"",
        &format!("\"event\":\"\\u001b{token}\""),
        1,
    );
    std::fs::write(&paths.events, raw).unwrap();
    let out = cli(&f.root, &[], &["follow", "list"]);
    assert!(!text(&out).contains(&token), "the token was echoed");
}

/// Codex R8: a `gh` that exits at once but leaves a descendant holding its
/// output open is cut off at the observation limit.
#[cfg(unix)]
#[test]
fn a_descendant_holding_the_pipes_cannot_outlast_the_limit() {
    use codeflow_core::coord::due::{Observer, RepoObserver, OBSERVATION_TIMEOUT};
    use std::os::unix::fs::PermissionsExt;
    let f = fixture();
    let bin = f.root.parent().unwrap().join("fake-bin");
    std::fs::create_dir(&bin).unwrap();
    let script = bin.join("gh");
    std::fs::write(
        &script,
        "#!/bin/sh\n/bin/sleep 40 &\nprintf '%s' '{\"state\":\"MERGED\",\"baseRefName\":\"main\",\"mergeCommit\":{\"oid\":\"abc\"},\"statusCheckRollup\":[]}'\nexit 0\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let old = std::env::var_os("PATH").unwrap();
    std::env::set_var(
        "PATH",
        format!("{}:{}", bin.display(), old.to_string_lossy()),
    );
    let started = std::time::Instant::now();
    let out = RepoObserver::new(&f.root, true).pull_request("github.com/example/project", 1);
    let elapsed = started.elapsed();
    std::env::set_var("PATH", old);
    assert!(
        elapsed < OBSERVATION_TIMEOUT + std::time::Duration::from_secs(3),
        "{elapsed:?}"
    );
    assert!(
        out.is_none(),
        "an observation cut off at the limit is unknown"
    );
}

// ---------------------------------------------------------------------------
// Review round 3 reproductions; each failed at 560bb1361.
// ---------------------------------------------------------------------------

/// Codex S1: a merge that brought in only the reviewed work, with the task
/// still todo, is no proof for a completion made afterwards on `main`; a
/// fast-forward of the same stays open too.
#[test]
fn a_merge_head_without_the_completion_is_no_proof() {
    for merged in [false, true] {
        let f = fixture();
        let paths = bind(&f.root);
        let by = coordinator("WS-001", "c1");
        ok(&cli(
            &f.root,
            &by,
            &add_args("bound review needed", &["--link", "fulfils:TSK-001"]),
        ));
        git(&f.root, &["checkout", "-qb", "task/TSK-001-review"]);
        let reviewed = reviewed_work(&f.root, "TSK-001");
        git(&f.root, &["checkout", "-q", "main"]);
        if merged {
            git(
                &f.root,
                &[
                    "merge",
                    "-q",
                    "--no-ff",
                    "-m",
                    "merge work still todo",
                    "task/TSK-001-review",
                ],
            );
        } else {
            git(
                &f.root,
                &["merge", "-q", "--ff-only", "task/TSK-001-review"],
            );
        }
        write(&f.root, "src/TSK-001.txt", "UNREVIEWED replacement work\n");
        write(
            &f.root,
            "project-management/tasks/TSK-001.md",
            &accepted("TSK-001", "ship the cache warmer", &reviewed),
        );
        git(
            &f.root,
            &["commit", "-qam", "feat: unreviewed work and completion"],
        );
        git(&f.root, &["push", "-q", "origin", "main"]);
        ok(&cli(&f.root, &by, &["follow", "check", "--offline"]));
        assert_eq!(
            folded(&paths).items["FU-001"].state,
            ItemState::Open,
            "merged={merged}"
        );
    }
}

/// Codex S2: a task that names `origin/main` is judged on origin's line
/// only; a completion pushed to another remote never stands in, and an
/// absent origin tracking ref is unknown.
#[test]
fn a_named_remote_is_never_replaced_by_another() {
    let f = fixture();
    let record = task("TSK-001", "ship the cache warmer", "todo").replace(
        "integration_target: main",
        "integration_target: origin/main",
    );
    write(&f.root, "project-management/tasks/TSK-001.md", &record);
    git(&f.root, &["commit", "-qam", "docs: pin the origin target"]);
    git(&f.root, &["push", "-q", "origin", "main"]);
    let paths = bind(&f.root);
    let by = coordinator("WS-001", "c1");
    ok(&cli(
        &f.root,
        &by,
        &add_args(
            "land specifically on origin",
            &["--link", "fulfils:TSK-001"],
        ),
    ));
    let other = f.root.parent().unwrap().join("other.git");
    git(
        f.root.parent().unwrap(),
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            other.to_str().unwrap(),
        ],
    );
    git(
        &f.root,
        &["remote", "add", "other", other.to_str().unwrap()],
    );
    git(&f.root, &["checkout", "-qb", "task/TSK-001-other"]);
    let reviewed = reviewed_work(&f.root, "TSK-001");
    write(
        &f.root,
        "project-management/tasks/TSK-001.md",
        &accepted("TSK-001", "ship the cache warmer", &reviewed).replace(
            "integration_target: main",
            "integration_target: origin/main",
        ),
    );
    git(&f.root, &["commit", "-qam", "docs: complete task"]);
    git(&f.root, &["checkout", "-q", "main"]);
    git(
        &f.root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "merge task",
            "task/TSK-001-other",
        ],
    );
    git(&f.root, &["push", "-q", "other", "main"]);
    git(&f.root, &["fetch", "-q", "other"]);
    git(&f.root, &["update-ref", "-d", "refs/remotes/origin/main"]);
    let tasks = codeflow_core::workgraph::readiness::target_tasks(&f.root).unwrap();
    assert!(!tasks.contains_key("TSK-001"), "{tasks:?}");
    ok(&cli(&f.root, &by, &["follow", "check", "--offline"]));
    assert_eq!(folded(&paths).items["FU-001"].state, ItemState::Open);
}

/// Codex S3: user text shaped like a purge digest is still purged, and a
/// second purge changes nothing more.
#[test]
fn purge_never_trusts_user_text_that_looks_like_a_digest() {
    let f = fixture();
    let paths = bind(&f.root);
    let by = coordinator("WS-001", "c1");
    ok(&cli(
        &f.root,
        &by,
        &add_args(
            "privacy",
            &[
                "--no-link-reason",
                "unlinked",
                "--idempotency-key",
                "purged:PRIVATE-KEY",
                "--source-reference",
                "[purged PRIVATE-SOURCE]",
            ],
        ),
    ));
    ok(&cli(&f.root, &by, &["follow", "purge", "FU-001"]));
    let once = std::fs::read_to_string(&paths.events).unwrap();
    assert!(!once.contains("PRIVATE"), "user text stayed");
    ok(&cli(&f.root, &by, &["follow", "purge", "FU-001"]));
    let twice = std::fs::read_to_string(&paths.events).unwrap();
    let body = |log: &str| {
        log.lines()
            .filter(|line| !line.contains("\"item_purged\""))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(
        body(&once),
        body(&twice),
        "a repeated purge changed the item's events"
    );
    // The retry is still recognised after both purges.
    let retry = ok(&cli(
        &f.root,
        &by,
        &add_args(
            "privacy",
            &[
                "--no-link-reason",
                "unlinked",
                "--idempotency-key",
                "purged:PRIVATE-KEY",
                "--source-reference",
                "[purged PRIVATE-SOURCE]",
            ],
        ),
    ));
    assert!(retry.contains("already recorded"), "{retry}");
}

/// Codex S4: a quarantined event with a damaged byte is still recognised by
/// its subject and removed; another subject's bytes stay.
#[test]
fn purge_removes_a_damaged_quarantined_line_of_its_item() {
    let f = fixture();
    let paths = bind(&f.root);
    let by = coordinator("WS-001", "c1");
    for title in ["first", "second"] {
        ok(&cli(
            &f.root,
            &by,
            &add_args(title, &["--no-link-reason", "unlinked"]),
        ));
    }
    let prefix = std::fs::read(&paths.events).unwrap();
    ok(&cli(
        &f.root,
        &by,
        &[
            "follow",
            "check",
            "FU-001",
            "--observed",
            "PRIVATE-DAMAGED",
            "--offline",
        ],
    ));
    ok(&cli(
        &f.root,
        &by,
        &[
            "follow",
            "check",
            "FU-002",
            "--observed",
            "KEEP-TWO",
            "--offline",
        ],
    ));
    let all = std::fs::read(&paths.events).unwrap();
    let mut suffix = all[prefix.len()..].to_vec();
    let first_end = suffix.iter().position(|byte| *byte == b'\n').unwrap();
    suffix.insert(first_end, 255);
    let mut damaged = prefix.clone();
    damaged.extend(suffix);
    std::fs::write(&paths.events, damaged).unwrap();
    let quarantined = store::repair(&paths, store::COMMAND_WAIT)
        .unwrap()
        .quarantined
        .unwrap();
    ok(&cli(&f.root, &by, &["follow", "purge", "FU-001"]));
    let kept = std::fs::read(&quarantined).unwrap();
    let kept = String::from_utf8_lossy(&kept);
    assert!(
        !kept.contains("PRIVATE-DAMAGED"),
        "the damaged line of FU-001 stayed"
    );
    assert!(kept.contains("KEEP-TWO"));
}

/// Codex S5: an explicit schedule is part of an add's identity; a default
/// schedule is not, so a later retry is still the same operation.
#[test]
fn an_add_schedule_is_content_and_a_default_retry_is_the_same() {
    let f = fixture();
    let paths = bind(&f.root);
    let by = coordinator("WS-001", "c1");
    let with = |key: &'static str, schedule: Option<&'static str>| {
        let mut args = vec!["--no-link-reason", "unlinked", "--idempotency-key", key];
        if let Some(schedule) = schedule {
            args.extend(["--next-check", schedule]);
        }
        args
    };
    ok(&cli(
        &f.root,
        &by,
        &add_args("schedule", &with("explicit", Some("10m"))),
    ));
    let out = cli(
        &f.root,
        &by,
        &add_args("schedule", &with("explicit", Some("30d"))),
    );
    assert_eq!(out.status.code(), Some(7), "{}", text(&out));
    ok(&cli(
        &f.root,
        &by,
        &add_args("default", &with("defaulted", None)),
    ));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let again = ok(&cli(
        &f.root,
        &by,
        &add_args("default", &with("defaulted", None)),
    ));
    assert!(again.contains("already recorded"), "{again}");
    assert_eq!(folded(&paths).items.len(), 2);
}

/// Codex S6: without a configured remote a task link never closes on its
/// own, while commit proof against the local default branch does, as the
/// documentation states.
#[test]
fn a_clone_without_a_remote_closes_on_commit_proof_only() {
    let f = fixture();
    git(&f.root, &["remote", "remove", "origin"]);
    let paths = bind(&f.root);
    let by = coordinator("WS-001", "c1");
    let sha = git(&f.root, &["rev-parse", "HEAD"]);
    ok(&cli(
        &f.root,
        &by,
        &add_args(
            "local commit",
            &["--link", &format!("fulfils:commit:{sha}")],
        ),
    ));
    ok(&cli(
        &f.root,
        &by,
        &add_args("local task", &["--link", "fulfils:TSK-001"]),
    ));
    // The task completed and bound on the local default branch.
    git(&f.root, &["checkout", "-qb", "task/TSK-001-local"]);
    let reviewed = reviewed_work(&f.root, "TSK-001");
    write(
        &f.root,
        "project-management/tasks/TSK-001.md",
        &accepted("TSK-001", "ship the cache warmer", &reviewed),
    );
    git(
        &f.root,
        &["commit", "-qam", "docs(tasks): complete the task"],
    );
    git(&f.root, &["checkout", "-q", "main"]);
    git(
        &f.root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "Merge the task",
            "task/TSK-001-local",
        ],
    );
    ok(&cli(&f.root, &by, &["follow", "check", "--offline"]));
    let ledger = folded(&paths);
    assert!(matches!(ledger.items["FU-001"].state, ItemState::Closed(_)));
    assert_eq!(ledger.items["FU-002"].state, ItemState::Open);
}

/// Security F3-rr: an integration line pushed under a real epic number
/// closes nothing until a task record on the default target names it; once
/// one does, a bound completion there closes.
#[test]
fn an_integration_line_counts_only_when_the_default_target_names_it() {
    for named in [false, true] {
        let f = fixture();
        let line = "integration/EPC-001-fixture";
        if named {
            let naming = task("TSK-002", "document the release gate timing", "todo").replace(
                "integration_target: main",
                &format!("integration_target: \"{line}\""),
            );
            write(&f.root, "project-management/tasks/TSK-002.md", &naming);
            git(
                &f.root,
                &["commit", "-qam", "docs(plan): target the epic line"],
            );
            git(&f.root, &["push", "-q", "origin", "main"]);
        }
        let paths = bind(&f.root);
        let by = coordinator("WS-001", "c1");
        git(&f.root, &["checkout", "-qb", "task/TSK-003-x"]);
        let todo = task("TSK-003", "a new task", "todo").replace(
            "integration_target: main",
            &format!("integration_target: \"{line}\""),
        );
        write(&f.root, "project-management/tasks/TSK-003.md", &todo);
        git(&f.root, &["add", "."]);
        git(&f.root, &["commit", "-qm", "docs(tasks): plan on the line"]);
        ok(&cli(
            &f.root,
            &by,
            &add_args("verify the line task", &["--link", "fulfils:TSK-003"]),
        ));
        let reviewed = reviewed_work(&f.root, "TSK-003");
        let record = accepted("TSK-003", "a new task", &reviewed).replace(
            "integration_target: main",
            &format!("integration_target: \"{line}\""),
        );
        write(&f.root, "project-management/tasks/TSK-003.md", &record);
        git(
            &f.root,
            &["commit", "-qam", "docs(tasks): complete on the line"],
        );
        git(
            &f.root,
            &["push", "-q", "origin", &format!("HEAD:refs/heads/{line}")],
        );
        git(&f.root, &["fetch", "-q", "origin"]);
        ok(&cli(&f.root, &by, &["follow", "check", "--offline"]));
        let closed = matches!(folded(&paths).items["FU-001"].state, ItemState::Closed(_));
        assert_eq!(closed, named, "named={named}");
    }
}

/// Security F11-r: the operator can purge an item that survives only in
/// quarantine, matched by its parsed subject in any JSON spacing.
#[test]
fn the_operator_purges_an_item_that_survives_only_in_quarantine() {
    let f = fixture();
    let paths = bind(&f.root);
    std::fs::create_dir_all(&paths.quarantine).unwrap();
    let fragment = paths.quarantine.join("damaged-1-x.bin");
    std::fs::write(
        &fragment,
        "{\"subject_id\": \"FU-009\", \"data\": {\"title\": \"PRIVATE-QUARANTINED\"}}\n{\"subject_id\":\"FU-008\",\"data\":\"KEEP\"}\n",
    )
    .unwrap();
    let lead = fixture::coordinator("WS-001", "c1", 1);
    let error = store::purge(&paths, &lead, "FU-009", store::COMMAND_WAIT).unwrap_err();
    assert!(format!("{error}").contains("only the operator"), "{error}");
    let purged = store::purge(&paths, &fixture::operator(), "FU-009", store::COMMAND_WAIT).unwrap();
    assert_eq!(purged.quarantine_removed, 1);
    let kept = std::fs::read_to_string(&fragment).unwrap();
    assert!(!kept.contains("PRIVATE") && kept.contains("KEEP"), "{kept}");
}

// ---------------------------------------------------------------------------
// Review round 4 reproductions; each failed at 8f5b4c9bb.
// ---------------------------------------------------------------------------

/// Codex T1: the current completion episode is what is bound. A landed
/// completion closes; reopening the task reopens the item; restoring the
/// old completion and its acceptance over changed work stays open; a new
/// reviewed completion closes again.
#[test]
#[allow(clippy::too_many_lines)]
fn only_the_current_completion_episode_is_bound() {
    let f = fixture();
    let paths = bind(&f.root);
    let by = coordinator("WS-001", "c1");
    ok(&cli(
        &f.root,
        &by,
        &add_args("review each completion", &["--link", "fulfils:TSK-001"]),
    ));
    land(&f.root, "TSK-001");
    ok(&cli(&f.root, &by, &["follow", "check", "--offline"]));
    assert!(matches!(
        folded(&paths).items["FU-001"].state,
        ItemState::Closed(_)
    ));
    let rel = "project-management/tasks/TSK-001.md";
    let old = std::fs::read_to_string(f.root.join(rel)).unwrap();
    write(
        &f.root,
        rel,
        &old.replacen("status: complete", "status: todo", 1),
    );
    write(
        &f.root,
        "src/TSK-001.txt",
        "unreviewed replacement implementation\n",
    );
    git(
        &f.root,
        &[
            "commit",
            "-qam",
            "fix: reopen task and change implementation",
        ],
    );
    git(&f.root, &["push", "-q", "origin", "main"]);
    let out = ok(&cli(&f.root, &by, &["follow", "check", "--offline"]));
    assert!(out.contains("FU-001 reopened"), "{out}");
    write(&f.root, rel, &old);
    git(
        &f.root,
        &[
            "commit",
            "-qam",
            "docs: re-complete with obsolete acceptance",
        ],
    );
    git(&f.root, &["push", "-q", "origin", "main"]);
    ok(&cli(&f.root, &by, &["follow", "check", "--offline"]));
    assert_eq!(
        folded(&paths).items["FU-001"].state,
        ItemState::Open,
        "stale re-completion closed"
    );
    // A proper reopen keeps the old block superseded; a new review of the
    // current work, completed and landed, closes again.
    git(&f.root, &["checkout", "-qb", "task/TSK-001-again"]);
    let reopened = old
        .replacen("status: complete", "status: todo", 1)
        .replacen(
            "acceptance:\n",
            "acceptance_superseded:\n  reason: the work changed\n",
            1,
        );
    write(&f.root, rel, &reopened);
    git(
        &f.root,
        &["commit", "-qam", "docs(tasks): reopen for the new review"],
    );
    let reviewed = reviewed_work(&f.root, "TSK-001");
    let block = format!(
        "\n```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: \"reviewed again\"\n  criteria:\n    AC-1: verified | the fixture ran the command\n  journey: verified | the fixture journey\n  not_verified: \"nothing\"\n  follow_ups: \"none: nothing left\"\n  verdict: approved\n```\n"
    );
    write(
        &f.root,
        rel,
        &format!(
            "{}{block}",
            reopened.replacen("status: todo", "status: complete", 1)
        ),
    );
    git(
        &f.root,
        &[
            "commit",
            "-qam",
            "docs(tasks): complete after the new review",
        ],
    );
    git(&f.root, &["checkout", "-q", "main"]);
    git(
        &f.root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "Merge the new review",
            "task/TSK-001-again",
        ],
    );
    git(&f.root, &["push", "-q", "origin", "main"]);
    let out = ok(&cli(&f.root, &by, &["follow", "check", "--offline"]));
    assert!(
        out.contains("FU-001 closed on machine-verified evidence"),
        "{out}"
    );
}

/// Codex T3: a later merge of unrelated work, forked before the completion,
/// leaves a valid closure closed.
#[test]
fn an_unrelated_later_merge_keeps_a_valid_closure() {
    let f = fixture();
    let paths = bind(&f.root);
    let by = coordinator("WS-001", "c1");
    ok(&cli(
        &f.root,
        &by,
        &add_args("keep correct proof", &["--link", "fulfils:TSK-001"]),
    ));
    land(&f.root, "TSK-001");
    ok(&cli(&f.root, &by, &["follow", "check", "--offline"]));
    assert!(matches!(
        folded(&paths).items["FU-001"].state,
        ItemState::Closed(_)
    ));
    let reviewed = git(&f.root, &["rev-parse", "task/TSK-001-landing~1"]);
    git(&f.root, &["checkout", "-qb", "task/unrelated", &reviewed]);
    write(&f.root, "src/unrelated.txt", "unrelated later work\n");
    git(&f.root, &["add", "."]);
    git(&f.root, &["commit", "-qm", "feat: unrelated change"]);
    git(&f.root, &["checkout", "-q", "main"]);
    git(
        &f.root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "merge unrelated work",
            "task/unrelated",
        ],
    );
    git(&f.root, &["push", "-q", "origin", "main"]);
    ok(&cli(&f.root, &by, &["follow", "check", "--offline"]));
    assert!(matches!(
        folded(&paths).items["FU-001"].state,
        ItemState::Closed(_)
    ));
}

/// Codex T2: two events joined by a lost line break are kept whole and
/// reported, whichever of the two subjects is purged, in either order.
#[test]
fn a_joined_quarantined_line_is_kept_for_either_subject() {
    for (first, second, purged) in [
        ("FU-001", "FU-002", "FU-001"),
        ("FU-001", "FU-002", "FU-002"),
        ("FU-002", "FU-001", "FU-001"),
        ("FU-002", "FU-001", "FU-002"),
    ] {
        let f = fixture();
        let paths = bind(&f.root);
        let by = coordinator("WS-001", "c1");
        ok(&cli(
            &f.root,
            &by,
            &add_args("one", &["--no-link-reason", "unlinked"]),
        ));
        ok(&cli(
            &f.root,
            &by,
            &add_args("two", &["--no-link-reason", "unlinked"]),
        ));
        let prefix = std::fs::read(&paths.events).unwrap();
        ok(&cli(
            &f.root,
            &by,
            &[
                "follow",
                "check",
                first,
                "--observed",
                "FIRST-TEXT",
                "--offline",
            ],
        ));
        let middle = std::fs::read(&paths.events).unwrap();
        ok(&cli(
            &f.root,
            &by,
            &[
                "follow",
                "check",
                second,
                "--observed",
                "SECOND-TEXT",
                "--offline",
            ],
        ));
        let all = std::fs::read(&paths.events).unwrap();
        let mut damaged = prefix.clone();
        damaged.extend_from_slice(&middle[prefix.len()..middle.len() - 1]);
        damaged.extend_from_slice(&all[middle.len()..]);
        std::fs::write(&paths.events, damaged).unwrap();
        let quarantined = store::repair(&paths, store::COMMAND_WAIT)
            .unwrap()
            .quarantined
            .unwrap();
        let before = std::fs::read(&quarantined).unwrap();
        let out = cli(&f.root, &by, &["follow", "purge", purged]);
        assert!(out.status.success(), "{}", text(&out));
        assert!(text(&out).contains("kept, not purged"), "{}", text(&out));
        assert_eq!(
            before,
            std::fs::read(&quarantined).unwrap(),
            "{first} {second} purge {purged}"
        );
    }
}

/// Codex N1: purging an item that survives only in quarantine says that no
/// event was recorded.
#[test]
fn a_quarantine_only_purge_says_no_event_was_recorded() {
    let f = fixture();
    let paths = bind(&f.root);
    std::fs::create_dir_all(&paths.quarantine).unwrap();
    std::fs::write(
        paths.quarantine.join("damaged-1-x.bin"),
        "{\"subject_id\":\"FU-009\",\"data\":\"X\"}\n",
    )
    .unwrap();
    let purged = store::purge(&paths, &fixture::operator(), "FU-009", store::COMMAND_WAIT).unwrap();
    assert_eq!(purged.sequence, None);
    assert_eq!(purged.quarantine_removed, 1);
}

/// Security R4-1: a bare `main` target reads `origin`'s line; setting the
/// local branch's upstream to another remote changes nothing.
#[test]
fn a_bare_target_never_follows_a_changed_upstream() {
    let f = fixture();
    let paths = bind(&f.root);
    let by = coordinator("WS-001", "c1");
    ok(&cli(
        &f.root,
        &by,
        &add_args("land on origin", &["--link", "fulfils:TSK-001"]),
    ));
    let evil = f.root.parent().unwrap().join("evil.git");
    git(
        f.root.parent().unwrap(),
        &["init", "-q", "--bare", "-b", "main", evil.to_str().unwrap()],
    );
    git(&f.root, &["remote", "add", "evil", evil.to_str().unwrap()]);
    git(&f.root, &["checkout", "-qb", "task/TSK-001-evil"]);
    let reviewed = reviewed_work(&f.root, "TSK-001");
    write(
        &f.root,
        "project-management/tasks/TSK-001.md",
        &accepted("TSK-001", "ship the cache warmer", &reviewed),
    );
    git(&f.root, &["commit", "-qam", "docs: complete"]);
    git(&f.root, &["push", "-q", "evil", "HEAD:refs/heads/main"]);
    git(&f.root, &["checkout", "-q", "main"]);
    git(&f.root, &["fetch", "-q", "evil"]);
    git(&f.root, &["branch", "-q", "-u", "evil/main", "main"]);
    ok(&cli(&f.root, &by, &["follow", "check", "--offline"]));
    assert_eq!(folded(&paths).items["FU-001"].state, ItemState::Open);
}
