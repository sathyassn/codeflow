//! `codeflow report ceremony` on a fixture history with known counts
//! (TSK-149 AC-1, AC-2). The host is a `gh` shim on `PATH`: one that
//! answers, one that fails, and none at all.
#![cfg(unix)]

use std::path::Path;
use std::process::{Command, Output};

fn isolated_home() -> &'static Path {
    static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| tempfile::tempdir().expect("home tempdir"))
        .path()
}

/// A fixed clock for every commit, so merge times and the refusal window
/// are known: minute `n` of 2026-09-10 10:00 UTC.
fn at(minute: u32) -> String {
    format!("2026-09-10T{:02}:{:02}:00Z", 10 + minute / 60, minute % 60)
}

struct Fixture {
    dir: tempfile::TempDir,
    minute: std::cell::Cell<u32>,
}

impl Fixture {
    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn git(&self, args: &[&str]) {
        let minute = self.minute.get();
        self.minute.set(minute + 1);
        let when = at(minute);
        let out = Command::new("git")
            .args(args)
            .current_dir(self.root())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.test")
            .env("GIT_COMMITTER_NAME", "Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.test")
            .env("GIT_AUTHOR_DATE", &when)
            .env("GIT_COMMITTER_DATE", &when)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.root().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// Land `branch` on `main` as GitHub's merge commit for pull request
    /// `number`, after `edit` makes one commit on it.
    fn pull_request(
        &self,
        number: u32,
        branch: &str,
        subject_head: &str,
        edit: impl FnOnce(&Self),
    ) {
        self.git(&["switch", "-q", "-c", branch, "main"]);
        edit(self);
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", &format!("chore: work for {number}")]);
        self.git(&["switch", "-q", "main"]);
        self.git(&[
            "merge",
            "-q",
            "--no-ff",
            branch,
            "-m",
            &format!("Merge pull request #{number} from {subject_head}"),
        ]);
    }
}

const TASK: &str = "---\nid: TSK-001\nstatus: todo\ndepends_on: []\n---\n\n# TSK-001: a task\n\n## Description\n\nWhat it does.\n\n## Acceptance Criteria\n\n- [ ] AC-1 it works\n\n## Closeout\n\nPending implementation.\n";

/// Seven pull requests in the window 1..7, one after it:
///
/// | # | branch | kind | change | record status |
/// |---|---|---|---|---|
/// | 1 | task/TSK-001-first | task | TSK-001 | no |
/// | 2 | task/TSK-001-second | task | TSK-001 | no |
/// | 3 | fix/a-bug | fix | itself | no |
/// | 4 | plan/new-task | plan | itself | no (adds a record) |
/// | 5 | plan/close-tsk-001 | plan | - | yes |
/// | 6 | plan/amend-tsk-001 | plan | itself | no (Description) |
/// | 7 | owner/docs/guide | docs | itself | no |
/// | 8 | fix/later | fix | outside the window | - |
fn fixture() -> Fixture {
    let fixture = Fixture {
        dir: tempfile::tempdir().unwrap(),
        minute: std::cell::Cell::new(0),
    };
    fixture.git(&["init", "-q", "-b", "main"]);
    fixture.write("project-management/tasks/TSK-001.md", TASK);
    fixture.write("src/lib.rs", "// lib\n");
    fixture.git(&["add", "-A"]);
    fixture.git(&["commit", "-q", "-m", "chore: seed"]);
    fixture.pull_request(1, "task/TSK-001-first", "task/TSK-001-first", |f| {
        f.write("src/one.rs", "// one\n");
    });
    fixture.pull_request(2, "task/TSK-001-second", "task/TSK-001-second", |f| {
        f.write("src/two.rs", "// two\n");
    });
    fixture.pull_request(3, "fix/a-bug", "fix/a-bug", |f| {
        f.write("src/lib.rs", "// lib, fixed\n");
    });
    fixture.pull_request(4, "plan/new-task", "plan/new-task", |f| {
        f.write(
            "project-management/tasks/TSK-002.md",
            &TASK.replace("TSK-001", "TSK-002"),
        );
    });
    fixture.pull_request(5, "plan/close-tsk-001", "plan/close-tsk-001", |f| {
        f.write(
            "project-management/tasks/TSK-001.md",
            &TASK
                .replace("status: todo", "status: complete")
                .replace("- [ ] AC-1", "- [x] AC-1")
                .replace("Pending implementation.", "Complete: landed in 1 and 2."),
        );
    });
    fixture.pull_request(6, "plan/amend-tsk-001", "plan/amend-tsk-001", |f| {
        let current =
            std::fs::read_to_string(f.root().join("project-management/tasks/TSK-001.md")).unwrap();
        f.write(
            "project-management/tasks/TSK-001.md",
            &current.replace("What it does.", "What it does, amended."),
        );
    });
    fixture.pull_request(7, "docs/guide", "owner/docs/guide", |f| {
        f.write("docs/guide.md", "# Guide\n");
    });
    fixture.pull_request(8, "fix/later", "fix/later", |f| {
        f.write("src/later.rs", "// later\n");
    });
    fixture
}

/// A directory holding an executable `gh` that runs `script`.
fn gh_shim(script: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gh");
    std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    dir
}

/// `codeflow report ceremony <args>` in `root`, with `bin` as the only
/// directory on `PATH` (an empty one when `None`): the report reads Git in
/// process, so the only program it may start is the host's `gh`.
fn report(root: &Path, bin: Option<&Path>, args: &[&str]) -> Output {
    let empty = tempfile::tempdir().unwrap();
    let path = bin.unwrap_or(empty.path());
    Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .args(["report", "ceremony"])
        .args(args)
        .current_dir(root)
        .env("PATH", path)
        .env("CODEFLOW_HOME", isolated_home())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .expect("codeflow runs")
}

fn stdout(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// The rows every run over 1..7 prints, whatever the host says.
fn assert_counts(text: &str) {
    for row in [
        "  docs                         1                1        1.00",
        "  fix                          1                1        1.00",
        "  plan                         2                2        1.00",
        "  task                         2                1        2.00",
        "  record status                1                -           -",
        "  all                          7                5        1.40",
        "  Record status pull requests: 5",
    ] {
        assert!(text.contains(row), "missing {row:?} in:\n{text}");
    }
    assert!(text.contains("  7 merged, "), "{text}");
}

/// A ledger holding a marker before the fixture's history, two refusals in
/// the window, a warn-level event and a refusal after the window.
fn write_refusals(root: &Path, marker: &str) {
    let dir = root.join(".git/codeflow/ledger/refusals");
    std::fs::create_dir_all(&dir).unwrap();
    let lines: Vec<String> = [
        format!(r#"{{"event":"refusal_recording_started","timestamp":"{marker}"}}"#),
        format!(
            r#"{{"event":"refusal","timestamp":"{}","plane":"pre-push","level":"block","rules":["git.commit_ticket"]}}"#,
            at(20)
        ),
        format!(
            r#"{{"event":"refusal","timestamp":"{}","plane":"git-guard","level":"block","rules":["git.push_to_protected"]}}"#,
            at(30)
        ),
        format!(
            r#"{{"event":"refusal","timestamp":"{}","plane":"pre-push","level":"warn","rules":["git.commit_ticket"]}}"#,
            at(31)
        ),
        r#"{"event":"refusal","timestamp":"2026-09-11T09:00:00Z","plane":"pre-push","level":"block","rules":["git.branch_naming"]}"#.to_string(),
    ]
    .into_iter()
    // Nothing is recorded before the marker.
    .filter(|line| {
        let at = line.split(r#""timestamp":""#).nth(1).unwrap_or_default();
        at[..20] >= *marker
    })
    .collect();
    std::fs::write(dir.join("refusals.jsonl"), lines.join("\n") + "\n").unwrap();
}

const ANSWER: &str = r#"printf '%s' '{"data":{"repository":{"pr1":{"reviews":{"totalCount":2}},"pr2":{"reviews":{"totalCount":1}},"pr3":{"reviews":{"totalCount":0}},"pr4":{"reviews":{"totalCount":0}},"pr5":{"reviews":{"totalCount":0}},"pr6":{"reviews":{"totalCount":0}},"pr7":null}}}'"#;

#[test]
fn the_report_counts_changes_reviews_and_refusals_over_a_window() {
    // AC-1: known counts from a fixture history, review rounds from the
    // host, refusals from the ledger; a warn-level event is not counted.
    let fixture = fixture();
    write_refusals(fixture.root(), "2026-09-01T00:00:00Z");
    let gh = gh_shim(ANSWER);
    let text = stdout(&report(fixture.root(), Some(gh.path()), &["--prs", "1..7"]));
    assert!(
        text.starts_with("Ceremony report: pull requests 1 to 7\n"),
        "{text}"
    );
    assert_counts(&text);
    assert!(
        text.contains(
            "Review rounds per pull request: 1.50 over the 2 pull requests with host review data; unknown for the other 5"
        ),
        "{text}"
    );
    assert!(
        text.contains("Refusals hit by this clone's hooks and guards: 2 (git-guard 1, pre-push 1)"),
        "{text}"
    );
}

#[test]
fn a_failing_host_prints_unknown_and_keeps_the_local_counts() {
    // AC-2: the host call fails or `gh` is absent; review rounds are
    // unknown, never estimated, and the local counts still print.
    let fixture = fixture();
    let failing = gh_shim("echo 'HTTP 502: Bad Gateway' >&2\nexit 1");
    let text = stdout(&report(
        fixture.root(),
        Some(failing.path()),
        &["--prs", "1..7"],
    ));
    assert_counts(&text);
    assert!(
        text.contains(
            "Review rounds per pull request: unknown (the host call failed: `gh` failed: HTTP 502: Bad Gateway)"
        ),
        "{text}"
    );
    let absent = report(fixture.root(), None, &["--prs", "1..7"]);
    let text = stdout(&absent);
    assert_counts(&text);
    assert!(
        text.contains(
            "Review rounds per pull request: unknown (the host call failed: `gh` did not start"
        ),
        "{text}"
    );
    // A host that answers with no submitted review has no review data.
    let silent = gh_shim(
        &ANSWER
            .replace(r#""totalCount":2"#, r#""totalCount":0"#)
            .replace(r#""totalCount":1"#, r#""totalCount":0"#),
    );
    let text = stdout(&report(
        fixture.root(),
        Some(silent.path()),
        &["--prs", "1..7"],
    ));
    assert!(
        text.contains(
            "Review rounds per pull request: unknown (the host holds no submitted review for these 7 pull requests)"
        ),
        "{text}"
    );
}

#[test]
fn refusals_before_recording_began_are_unknown() {
    // A window that ends before this clone began recording cannot count
    // refusals; one that starts before it says so.
    let fixture = fixture();
    let gh = gh_shim(ANSWER);
    let text = stdout(&report(fixture.root(), Some(gh.path()), &["--prs", "1..7"]));
    assert!(
        text.contains(
            "Refusals hit by this clone's hooks and guards: unknown (this clone has recorded none;"
        ),
        "{text}"
    );
    write_refusals(fixture.root(), "2026-09-12T00:00:00Z");
    let text = stdout(&report(fixture.root(), Some(gh.path()), &["--prs", "1..7"]));
    assert!(
        text.contains(
            "unknown (this clone began recording at 2026-09-12T00:00:00Z, after this window)"
        ),
        "{text}"
    );
    write_refusals(fixture.root(), &at(25));
    let text = stdout(&report(fixture.root(), Some(gh.path()), &["--prs", "1..7"]));
    assert!(
        text.contains(&format!(
            "1 since recording began at {}, inside the window; earlier refusals are unknown (git-guard 1)",
            at(25)
        )),
        "{text}"
    );
}

#[test]
fn a_date_window_selects_by_merge_time() {
    let fixture = fixture();
    write_refusals(fixture.root(), "2026-09-01T00:00:00Z");
    let gh = gh_shim(ANSWER);
    let text = stdout(&report(
        fixture.root(),
        Some(gh.path()),
        &["--since", "2026-09-10", "--until", "2026-09-10"],
    ));
    assert!(text.contains("  8 merged, "), "{text}");
    assert!(
        text.contains("Refusals hit by this clone's hooks and guards: 2 (git-guard 1, pre-push 1)"),
        "{text}"
    );
    let text = stdout(&report(
        fixture.root(),
        Some(gh.path()),
        &["--since", "2026-09-11"],
    ));
    assert!(
        text.contains("no merged pull request in this clone's refs is in the window"),
        "{text}"
    );
    assert!(
        text.contains("Refusals hit by this clone's hooks and guards: 1 (pre-push 1)"),
        "{text}"
    );
}

#[test]
fn a_malformed_window_is_refused() {
    let fixture = fixture();
    for args in [
        &["--prs", "7..1"][..],
        &["--since", "10 Sept"][..],
        &["--since", "2026-09-10", "--until", "2026-09-01"][..],
    ] {
        let out = report(fixture.root(), None, args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
    let out = report(fixture.root(), None, &[]);
    assert!(!out.status.success(), "a window is required");
}
