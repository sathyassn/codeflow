//! Journeys (TSK-103 AC-5, AC-7, AC-9): on a fresh `codeflow init --full`
//! project with a bare `origin`, a backlog holding ready, waiting, blocked,
//! cross-line, awaiting-selection and already-claimed tasks. `work next`
//! then `work claim` pick the ready task and `status` agrees; the new-claim,
//! own-branch and pull request contexts judge one task the same way through
//! claim, start, build, complete and CI; `fix/`, `feat/` and `spike/`
//! branches carrying the id start it; a competing branch and a mismatched
//! `Task:` line are refused; a selection lands only from a `plan/` branch.
//! The binary under test is the one Cargo built.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn isolated_home() -> &'static Path {
    static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| tempfile::tempdir().expect("home tempdir"))
        .path()
}

fn with_env(command: &mut Command) -> &mut Command {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(exe.parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("joinable PATH");
    command
        .env("CODEFLOW_HOME", isolated_home())
        .env("PATH", path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Journey")
        .env("GIT_AUTHOR_EMAIL", "journey@example.test")
        .env("GIT_COMMITTER_NAME", "Journey")
        .env("GIT_COMMITTER_EMAIL", "journey@example.test")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE")
        .env_remove("CODEFLOW_PR_BODY")
        .env_remove("GITHUB_BASE_REF")
        .env_remove("GITHUB_HEAD_REF")
        .env_remove("GITHUB_EVENT_NAME")
}

fn codeflow(root: &Path, args: &[&str]) -> Output {
    with_env(&mut Command::new(env!("CARGO_BIN_EXE_codeflow")))
        .args(args)
        .current_dir(root)
        .output()
        .expect("codeflow runs")
}

fn git(root: &Path, args: &[&str]) -> String {
    let out = with_env(&mut Command::new("git"))
        .args(args)
        .current_dir(root)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn ok(out: &Output, what: &str) -> String {
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "{what} failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
}

fn refused(out: &Output, what: &str) -> String {
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert_eq!(
        out.status.code(),
        Some(1),
        "{what} should refuse:\n{}\n{stderr}",
        String::from_utf8_lossy(&out.stdout)
    );
    stderr
}

fn commit(root: &Path, message: &str) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", message]);
}

fn edit(root: &Path, relative: &str, from: &str, to: &str) {
    let path = root.join(relative);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(from), "{relative} lacks {from:?}");
    std::fs::write(path, text.replacen(from, to, 1)).unwrap();
}

fn body(task_line: &str) -> String {
    format!(
        "## Summary\nA bounded change.\n\n- one change\n\n{task_line}\n\n## Changes\n- one change\n\n## Testing\n- journey step\n\n## Reviews\n\n| Reviewer | Scope | Verdict |\n|---|---|---|\n| journey | this range | approve |\n\n## Release impact\n\n- Impact: patch\n- Breaking: no\n- Rationale: journey fixture.\n- Migration: none\n"
    )
}

/// `codeflow ci` for the pull request `branch` into `target`.
fn pull_request(root: &Path, branch: &str, target: &str, task_line: &str) -> (i32, String) {
    let out = codeflow(
        root,
        &[
            "ci",
            "--base",
            target,
            "--head",
            "HEAD",
            "--branch",
            branch,
            "--pr-body",
            &body(task_line),
        ],
    );
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

fn land(root: &Path, target: &str, branch: &str) {
    git(root, &["switch", "-q", target]);
    git(
        root,
        &[
            "merge",
            "-q",
            "--no-ff",
            branch,
            "-m",
            &format!("chore: land {branch}"),
        ],
    );
    git(root, &["push", "-q", "origin", target]);
}

fn task_file(id: &str) -> String {
    format!("project-management/tasks/{id}.md")
}

const LINE: &str = "integration/EPC-001-backlog";
const OTHER: &str = "integration/EPC-002-other";

/// A fresh full-tier project with a bare origin and a planned backlog landed
/// on two epic lines:
///
/// | Task | Line | State |
/// |---|---|---|
/// | TSK-001 Root | backlog | ready |
/// | TSK-002 Next | backlog | waiting on TSK-001 |
/// | TSK-003 Held | backlog | blocked |
/// | TSK-004 Join | backlog | blocked, awaiting selection |
/// | TSK-005 Taken | backlog | ready, then claimed elsewhere |
/// | TSK-006 Across | other | ready |
/// | TSK-007 Cross | backlog | waiting on TSK-006 on the other line |
#[allow(clippy::too_many_lines)] // One fixture plans the whole backlog in order.
fn planned_project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    let bare = dir.path().join("origin.git");
    std::fs::create_dir(&root).unwrap();
    ok(
        &codeflow(&root, &["init", "--yes", "--full"]),
        "init --full",
    );
    // The origin starts as a bare copy: the hooks refuse a push to the
    // protected default branch, as they should.
    let default = git(&root, &["branch", "--show-current"]);
    for line in [LINE, OTHER] {
        git(&root, &["branch", line, &default]);
    }
    git(
        dir.path(),
        &[
            "clone",
            "-q",
            "--bare",
            root.to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
    );
    git(&root, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&root, &["fetch", "-q", "origin"]);
    // The scaffold holds a record (ADR-0001), so the shared id registry is
    // seeded once before the first new record (TSK-101).
    ok(&codeflow(&root, &["ids", "seed"]), "ids seed");

    git(&root, &["switch", "-q", "-c", "plan/backlog", LINE]);
    ok(&codeflow(&root, &["epic", "new", "Backlog"]), "epic new");
    ok(&codeflow(&root, &["epic", "new", "Other"]), "epic new");
    for (epic, line, title) in [
        ("EPC-001", LINE, "Root"),
        ("EPC-001", LINE, "Next"),
        ("EPC-001", LINE, "Held"),
        ("EPC-001", LINE, "Join"),
        ("EPC-001", LINE, "Taken"),
        ("EPC-002", OTHER, "Across"),
        ("EPC-001", LINE, "Cross"),
    ] {
        ok(
            &codeflow(
                &root,
                &["task", "new", "--epic", epic, "--into", line, title],
            ),
            "task new",
        );
    }
    let criterion = "- AC-1 When run, the system shall work.\n";
    for epic in ["EPC-001", "EPC-002"] {
        edit(
            &root,
            &format!("project-management/epics/{epic}.md"),
            "- AC-1\n",
            criterion,
        );
    }
    // A task changing product code carries a journey criterion (TSK-105).
    let task_criteria =
        format!("{criterion}- AC-2 (journey) On a fresh project, the flow shall pass.\n");
    for n in 1..=7 {
        edit(
            &root,
            &task_file(&format!("TSK-00{n}")),
            "- AC-1\n",
            &task_criteria,
        );
    }
    edit(
        &root,
        &task_file("TSK-002"),
        "depends_on: []",
        "depends_on: [TSK-001]",
    );
    edit(
        &root,
        &task_file("TSK-007"),
        "depends_on: []",
        "depends_on: [TSK-006]",
    );
    commit(&root, "chore: plan the backlog");
    ok(
        &codeflow(
            &root,
            &[
                "task",
                "status",
                "TSK-003",
                "blocked",
                "--reason",
                "wait",
                "--owner",
                "primary",
                "--revisit",
                "vendor reply",
            ],
        ),
        "block TSK-003",
    );
    std::fs::create_dir_all(root.join("docs/plan")).unwrap();
    std::fs::write(
        root.join("docs/plan/choice.md"),
        "# Choice\n\nOption A or B.\n",
    )
    .unwrap();
    ok(
        &codeflow(
            &root,
            &[
                "task",
                "status",
                "TSK-004",
                "blocked",
                "--reason",
                "awaiting selection",
                "--owner",
                "primary",
                "--revisit",
                "docs/plan/choice.md",
            ],
        ),
        "block TSK-004",
    );
    edit(
        &root,
        &task_file("TSK-004"),
        "\ncreated:",
        "\nawaiting_selection: docs/plan/choice.md\ncreated:",
    );
    commit(&root, "chore: hold two tasks");
    ok(
        &codeflow(&root, &["validate", "--docs"]),
        "validate the plan",
    );
    let (code, out) = pull_request(&root, "plan/backlog", LINE, "Task: EPC-001");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("class: planning-only"), "{out}");
    land(&root, LINE, "plan/backlog");
    land(&root, OTHER, "plan/backlog");
    git(&root, &["switch", "-q", LINE]);
    (dir, root)
}

#[test]
#[allow(clippy::too_many_lines)] // One journey keeps the backlog in the order a project lives it.
fn work_next_and_claim_pick_the_ready_task_and_status_agrees() {
    let (_dir, root) = planned_project();
    // Another agent claims TSK-005 from its own clone.
    git(
        &root,
        &[
            "push",
            "-q",
            "origin",
            "HEAD:refs/heads/feat/TSK-005-elsewhere",
        ],
    );
    git(&root, &["fetch", "-q", "--prune", "origin"]);

    let next = ok(&codeflow(&root, &["work", "next"]), "work next");
    let lines: Vec<&str> = next.lines().collect();
    // The line resolves as `work start` resolves it: the local branch,
    // which has no configured upstream and holds every landing.
    assert!(
        lines[0].starts_with(&format!("snapshot: {LINE}@")),
        "{next}"
    );
    assert!(!lines[0].contains("fetched never"), "{next}");
    assert!(
        lines[1].starts_with("ready    TSK-001 Root (EPC-001"),
        "{next}"
    );
    assert!(next.contains("ready    TSK-006 Across (EPC-002"), "{next}");
    assert!(!next.contains("TSK-005 Taken"), "claimed elsewhere: {next}");
    assert!(next.contains("waiting  TSK-002"), "{next}");
    assert!(next.contains("waiting  TSK-007"), "{next}");
    assert!(next.contains("TSK-006"), "{next}");
    assert!(next.contains("blocked  TSK-003"), "{next}");
    assert!(next.contains("blocked  TSK-004"), "{next}");
    assert!(next.contains("awaiting selection"), "{next}");
    assert!(next.contains("1 active"), "{next}");
    let ready_at = next.find("ready    TSK-001").unwrap();
    assert!(ready_at < next.find("waiting  TSK-002").unwrap());
    assert!(next.find("waiting  TSK-002").unwrap() < next.find("blocked  TSK-003").unwrap());

    let json: serde_json::Value = serde_json::from_str(&ok(
        &codeflow(&root, &["work", "next", "--json"]),
        "work next --json",
    ))
    .unwrap();
    let state = |id: &str| {
        json["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["id"] == id)
            .map(|task| task["state"].as_str().unwrap().to_string())
    };
    assert_eq!(state("TSK-001").as_deref(), Some("ready"));
    assert_eq!(state("TSK-005").as_deref(), Some("active"));
    assert_eq!(state("TSK-004").as_deref(), Some("blocked"));
    assert!(json["fetched_at"].is_string());

    let other = ok(
        &codeflow(&root, &["work", "next", "--epic", "EPC-002"]),
        "work next --epic",
    );
    assert!(
        other.contains("TSK-006") && !other.contains("TSK-001"),
        "{other}"
    );

    let claimed = ok(&codeflow(&root, &["work", "claim", "TSK-001"]), "claim");
    assert!(
        claimed.contains(&format!("task/TSK-001-root from {LINE}@")),
        "{claimed}"
    );
    assert!(!claimed.to_lowercase().contains("lock"), "{claimed}");
    git(
        &root,
        &["rev-parse", "--verify", "origin/task/TSK-001-root"],
    );
    let taken = refused(
        &codeflow(&root, &["work", "claim", "TSK-005"]),
        "claim taken",
    );
    assert!(taken.contains("feat/TSK-005-elsewhere"), "{taken}");
    let waiting = refused(
        &codeflow(&root, &["work", "claim", "TSK-002"]),
        "claim waiting",
    );
    assert!(waiting.contains("TSK-001"), "{waiting}");
    let held = refused(
        &codeflow(&root, &["work", "claim", "TSK-004"]),
        "claim join",
    );
    assert!(
        held.contains("awaiting selection") || held.contains("blocked"),
        "{held}"
    );

    let status = ok(&codeflow(&root, &["status"]), "status");
    assert!(
        status.contains("tasks: ready 1 · active 2 · waiting 2 · blocked 2 · invalid 0"),
        "{status}"
    );
    assert!(
        status.contains("active: TSK-001 Root (task/TSK-001-root)"),
        "{status}"
    );
    assert!(status.contains("ready: TSK-006 Across"), "{status}");
    assert!(
        status.contains(&format!("retain-live branch {OTHER}")),
        "a live line is never removable: {status}"
    );
    let orient = ok(&codeflow(&root, &["orient"]), "orient");
    assert!(
        orient.contains(
            "tasks: ready 1 · active 2 · waiting 2 · blocked 2 · invalid 0; next: TSK-006"
        ),
        "{orient}"
    );
    let claim_help = ok(&codeflow(&root, &["work", "claim", "--help"]), "claim help");
    assert!(!claim_help.to_lowercase().contains("lock"), "{claim_help}");
}

#[test]
#[allow(clippy::too_many_lines)] // One journey keeps the three contexts in order.
fn the_three_contexts_judge_one_task_the_same_way() {
    let (dir, root) = planned_project();

    // New claim, then the own-branch context on the claimed branch.
    ok(&codeflow(&root, &["work", "claim", "TSK-001"]), "claim");
    git(&root, &["switch", "-q", "task/TSK-001-root"]);
    git(&root, &["branch", "fix/TSK-001-other", LINE]);
    let started = ok(
        &codeflow(&root, &["work", "start", "TSK-001"]),
        "work start",
    );
    assert!(started.contains("anchored"), "{started}");
    assert!(
        started.contains("conflict: other visible branches carry TSK-001: fix/TSK-001-other (reported, not refused)"),
        "{started}"
    );
    let status = ok(&codeflow(&root, &["status"]), "status");
    assert!(
        status.contains("conflict: TSK-001 is carried by fix/TSK-001-other, task/TSK-001-root"),
        "{status}"
    );
    git(&root, &["branch", "-D", "fix/TSK-001-other"]);

    // Build, complete, and the pull request context passes.
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/root.rs"), "pub fn root() {}\n").unwrap();
    commit(&root, "feat: add the root");
    let acceptance = dir.path().join("acceptance.yaml");
    std::fs::write(
        &acceptance,
        format!(
            "acceptance:\n  reviewed: {}\n  review: session:journey@sha256:00\n  criteria:\n    AC-1: verified | journey step\n    AC-2: verified | readiness journey\n  journey: verified | crates/codeflow-cli/tests/readiness_journey.rs\n  not_verified: none\n  follow_ups: none: journey fixture\n  verdict: approved\n",
            git(&root, &["rev-parse", "HEAD"])
        ),
    )
    .unwrap();
    ok(
        &codeflow(
            &root,
            &[
                "task",
                "status",
                "TSK-001",
                "complete",
                "--acceptance",
                acceptance.to_str().unwrap(),
            ],
        ),
        "complete TSK-001",
    );
    commit(&root, "chore: complete the root");
    let (code, out) = pull_request(&root, "task/TSK-001-root", LINE, "Task: TSK-001");
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("class: tracked TSK-001 (from the Task: line)"),
        "{out}"
    );
    git(&root, &["push", "-q", "origin", "task/TSK-001-root"]);
    land(&root, LINE, "task/TSK-001-root");

    // A complete task fails every context.
    let late = refused(
        &codeflow(&root, &["work", "claim", "TSK-001"]),
        "claim complete",
    );
    assert!(late.contains("complete"), "{late}");
    git(&root, &["switch", "-q", "-c", "fix/TSK-001-late", LINE]);
    let late = refused(
        &codeflow(&root, &["work", "start", "TSK-001"]),
        "start complete",
    );
    assert!(late.contains("'complete'"), "{late}");
    std::fs::write(root.join("src/root.rs"), "pub fn late() {}\n").unwrap();
    commit(&root, "fix: a late change");
    let (code, out) = pull_request(&root, "fix/late", LINE, "Task: TSK-001");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("work.stable_planning_anchor"), "{out}");
    let landed = ok(&codeflow(&root, &["status"]), "status");
    assert!(landed.contains("landed: task/TSK-001-root"), "{landed}");

    // TSK-002 is ready now; the own-branch context holds on every prefix.
    for branch in ["fix/TSK-002-a", "feat/TSK-002-b", "spike/TSK-002-c"] {
        git(&root, &["switch", "-q", "-c", branch, LINE]);
        let started = ok(&codeflow(&root, &["work", "start", "TSK-002"]), branch);
        assert!(
            started.contains(&format!("for {branch} -> {LINE}")),
            "{started}"
        );
    }
    let last = ok(
        &codeflow(&root, &["work", "start", "TSK-002"]),
        "spike start",
    );
    assert!(
        last.contains("fix/TSK-002-a, feat/TSK-002-b")
            || last.contains("feat/TSK-002-b, fix/TSK-002-a"),
        "{last}"
    );
    // Competing-branch negative control.
    let competing = refused(
        &codeflow(&root, &["work", "claim", "TSK-002"]),
        "competing claim",
    );
    assert!(competing.contains("already claimed"), "{competing}");
    // Mismatched-association negative control.
    git(&root, &["switch", "-q", "fix/TSK-002-a"]);
    std::fs::write(root.join("src/next.rs"), "pub fn next() {}\n").unwrap();
    commit(&root, "fix: next");
    let (code, out) = pull_request(&root, "fix/TSK-002-a", LINE, "Task: TSK-005");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("names another task"), "{out}");

    // A selection lands only from a plan/ branch.
    for branch in ["plan/select", "task/TSK-002-select"] {
        git(&root, &["switch", "-q", "-c", branch, LINE]);
        edit(
            &root,
            &task_file("TSK-004"),
            "awaiting_selection: docs/plan/choice.md\n",
            "",
        );
        ok(
            &codeflow(&root, &["task", "status", "TSK-004", "todo"]),
            "unblock the join",
        );
        edit(
            &root,
            &task_file("TSK-004"),
            "depends_on: []",
            "depends_on: [TSK-001]",
        );
        // The unselected alternative is cancelled.
        ok(
            &codeflow(
                &root,
                &[
                    "task",
                    "status",
                    "TSK-003",
                    "cancelled",
                    "--reason",
                    "option B not selected",
                    "--scope",
                    "dropped with option B",
                ],
            ),
            "cancel the alternative",
        );
        commit(&root, "chore: select option A");
        let task_line = if branch.starts_with("plan/") {
            "Task: EPC-001"
        } else {
            "Task: TSK-002"
        };
        let (code, out) = pull_request(&root, branch, LINE, task_line);
        if branch.starts_with("plan/") {
            assert_eq!(code, 0, "{out}");
        } else {
            assert_eq!(code, 1, "{out}");
            assert!(out.contains("work.selection"), "{out}");
        }
    }
    // Landed, the selection unblocks the join; the cancelled alternative
    // does not hold it.
    land(&root, LINE, "plan/select");
    let next = ok(
        &codeflow(&root, &["work", "next"]),
        "work next after selection",
    );
    assert!(next.contains("ready    TSK-004 Join"), "{next}");
    assert!(!next.contains("TSK-003"), "{next}");
}

/// Journey (TSK-156 AC-4): on a fresh `init --full` project, `task new`
/// writes the pin guidance quoted, a `depends_on` pin YAML reads as a number
/// (`70283613`) is refused by `validate --docs` with the quote remedy, and
/// the same pin quoted passes.
#[test]
fn a_number_shaped_pin_is_refused_with_the_quote_remedy_on_a_fresh_project() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    ok(
        &codeflow(&root, &["init", "--yes", "--full"]),
        "init --full",
    );
    ok(&codeflow(&root, &["epic", "new", "outcome"]), "epic new");
    for title in ["findings", "consumer"] {
        ok(
            &codeflow(&root, &["task", "new", "--epic", "EPC-001", title]),
            "task new",
        );
    }
    let consumer = "project-management/tasks/TSK-002.md";
    let written = std::fs::read_to_string(root.join(consumer)).unwrap();
    assert!(
        written.contains("{id: TSK-NNN, kind: research, pin: \"<commit sha>\"}, the pin quoted"),
        "{written}"
    );
    edit(
        &root,
        "project-management/epics/EPC-001.md",
        "- AC-1\n",
        "- AC-1 When used, the system shall work.\n",
    );
    for task in ["project-management/tasks/TSK-001.md", consumer] {
        edit(
            &root,
            task,
            "- AC-1\n",
            "- AC-1 When run, the system shall work.\n",
        );
    }
    ok(
        &codeflow(&root, &["validate", "--docs"]),
        "validate before the pin",
    );

    edit(
        &root,
        consumer,
        "depends_on: []",
        "depends_on: [{id: TSK-001, kind: research, pin: 70283613}]",
    );
    let out = codeflow(&root, &["validate", "--docs"]);
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_ne!(out.status.code(), Some(0), "{said}");
    assert!(
        said.contains("TSK-001 pin reads as a YAML number")
            && said.contains("the pin must be quoted: pin: \"<commit sha>\"")
            && !said.contains("not a commit id"),
        "{said}"
    );

    edit(&root, consumer, "pin: 70283613}", "pin: \"70283613\"}");
    ok(
        &codeflow(&root, &["validate", "--docs"]),
        "validate with the pin quoted",
    );

    // Review T156-1: a written null keeps no text and is refused with the
    // same remedy; leaving `pin` out is the not-yet-known state.
    let mut previous = "pin: \"70283613\"}".to_string();
    for null in ["pin: null}", "pin: ~}"] {
        edit(&root, consumer, &previous, null);
        let out = codeflow(&root, &["validate", "--docs"]);
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert_ne!(out.status.code(), Some(0), "{null}: {said}");
        assert!(
            said.contains("TSK-001 pin reads as YAML null")
                && said.contains("quote it: pin: \"<commit sha>\""),
            "{null}: {said}"
        );
        previous = null.to_string();
    }
    edit(&root, consumer, ", pin: ~}", "}");
    ok(
        &codeflow(&root, &["validate", "--docs"]),
        "validate with the pin left out",
    );
}

#[test]
fn a_join_awaiting_selection_validates_and_cannot_start() {
    // TSK-110 (SPC-013 R-104, R-43): the join held by its selection is a
    // valid plan, and no branch can start it until the selection lands.
    let (_dir, root) = planned_project();
    // Control: the plan holding the join validates, and a ready task on the
    // same line starts.
    ok(
        &codeflow(&root, &["validate", "--docs"]),
        "validate with a held join",
    );
    git(&root, &["switch", "-q", "-c", "task/TSK-001-root", LINE]);
    ok(
        &codeflow(&root, &["work", "start", "TSK-001"]),
        "work start TSK-001",
    );

    // Fault: the join refuses to start and names why.
    git(&root, &["switch", "-q", "-c", "task/TSK-004-join", LINE]);
    let out = codeflow(&root, &["work", "start", "TSK-004"]);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!out.status.success(), "the join started:\n{text}");
    assert!(text.contains("TSK-004"), "{text}");
    assert!(text.contains("awaiting selection"), "{text}");
}
