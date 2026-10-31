//! `work next` and `work claim` never offer a task whose work has landed
//! while its record is still open (TSK-1038, issue 370).
use std::path::Path;
use std::process::{Command, Output};

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().into()
}

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn cli(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .current_dir(root)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap()
}

fn out(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn succeeds(output: &Output) -> String {
    assert!(output.status.success(), "{}", out(output));
    out(output)
}

fn record(id: &str) -> String {
    format!("---\nid: {id}\nepic_id: null\nstandalone_reason: bounded outcome\nintegration_target: main\ntitle: work {id}\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: []\ncreated: 2026-09-29\n---\n\n## Description\nWork.\n\n## Acceptance Criteria\n- AC-1 When run, the command shall succeed. (journey)\n")
}

/// Three open standalone tasks on `main`: TSK-001 and TSK-003 are the ones
/// a test lands, TSK-002 is a control nothing touches.
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "t"]);
    git(root, &["config", "user.email", "t@example.test"]);
    for id in ["TSK-001", "TSK-002", "TSK-003"] {
        write(
            root,
            &format!("project-management/tasks/{id}.md"),
            &record(id),
        );
    }
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "docs: plan tasks"]);
    dir
}

/// Land `branch` on `main` with a merge commit, as a reviewed pull request
/// does, leaving no work on `main` itself.
fn merge(root: &Path, branch: &str) {
    git(
        root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            &format!("land {branch}"),
            branch,
        ],
    );
}

/// TSK-001: a branch with product work, merged, record still `todo`.
fn land_work(root: &Path) {
    git(root, &["switch", "-qc", "task/TSK-001-work"]);
    write(root, "src/fix.rs", "pub fn fixed() {}\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "fix: build the outcome"]);
    git(root, &["switch", "-q", "main"]);
    merge(root, "task/TSK-001-work");
}

fn task_line<'a>(text: &'a str, id: &str) -> Option<&'a str> {
    text.lines()
        .find(|line| line.contains(id) && !line.starts_with(' '))
}

#[test]
fn a_task_whose_work_landed_is_listed_as_landed_with_the_command() {
    let dir = fixture();
    let root = dir.path();
    land_work(root);

    let text = succeeds(&cli(root, &["work", "next"]));
    let line = task_line(&text, "TSK-001").expect(&text);
    assert!(line.starts_with("landed"), "{text}");
    assert!(text.contains("landed branch: task/TSK-001-work"), "{text}");
    assert!(
        text.contains("next: codeflow task status TSK-001 complete"),
        "{text}"
    );
    assert!(
        task_line(&text, "TSK-002").is_some_and(|line| line.starts_with("ready")),
        "the control stays ready: {text}"
    );

    let status = succeeds(&cli(root, &["status"]));
    assert!(
        status.contains("landed task: TSK-001")
            && status.contains("codeflow task status TSK-001 complete"),
        "{status}"
    );
    assert!(status.contains("landed 1"), "{status}");

    let json = succeeds(&cli(root, &["work", "next", "--json"]));
    let json: serde_json::Value = serde_json::from_str(&json).unwrap();
    let tasks = json["tasks"].as_array().unwrap();
    let find = |id: &str| tasks.iter().find(|task| task["id"] == id).unwrap();
    let landed = find("TSK-001");
    assert_eq!(landed["state"], "landed");
    assert_eq!(landed["landed_branches"][0], "task/TSK-001-work");
    assert_eq!(
        landed["suggested_command"],
        "codeflow task status TSK-001 complete"
    );
    assert_eq!(find("TSK-002")["state"], "ready");
    let control = find("TSK-002").as_object().unwrap();
    assert!(control.contains_key("suggested_command"), "{control:?}");
    assert!(control["suggested_command"].is_null(), "{control:?}");
    assert_eq!(
        control["landed_branches"],
        serde_json::json!([]),
        "{control:?}"
    );
}

#[test]
fn claiming_a_task_whose_work_landed_is_refused_with_the_branch_and_command() {
    let dir = fixture();
    let root = dir.path();
    land_work(root);

    let claim = cli(root, &["work", "claim", "TSK-001"]);
    assert!(!claim.status.success(), "{}", out(&claim));
    let text = out(&claim);
    assert!(text.contains("already landed"), "{text}");
    assert!(text.contains("task/TSK-001-work"), "{text}");
    assert!(
        text.contains("codeflow task status TSK-001 complete"),
        "{text}"
    );
    // The refusal made no branch.
    let branches = git(root, &["branch", "--list", "task/TSK-001-*"]);
    assert_eq!(branches.lines().count(), 1, "{branches}");

    // The control is still claimable.
    succeeds(&cli(root, &["work", "claim", "TSK-002"]));
}

#[test]
fn a_branch_that_only_filed_the_record_stays_ready() {
    let dir = fixture();
    let root = dir.path();
    git(root, &["switch", "-qc", "task/TSK-003-record"]);
    write(
        root,
        "project-management/tasks/TSK-003.md",
        &record("TSK-003").replace("Work.", "Work, amended."),
    );
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "docs: amend the record"]);
    git(root, &["switch", "-q", "main"]);
    merge(root, "task/TSK-003-record");

    let text = succeeds(&cli(root, &["work", "next"]));
    assert!(
        task_line(&text, "TSK-003").is_some_and(|line| line.starts_with("ready")),
        "{text}"
    );
    assert!(!text.contains("landed branch"), "{text}");
    succeeds(&cli(root, &["work", "claim", "TSK-003"]));
}

#[test]
fn a_record_branch_with_one_other_file_counts_as_work() {
    let dir = fixture();
    let root = dir.path();
    git(root, &["switch", "-qc", "task/TSK-003-both"]);
    write(
        root,
        "project-management/tasks/TSK-003.md",
        &record("TSK-003").replace("Work.", "Work, amended."),
    );
    write(root, "src/part.rs", "pub fn part() {}\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "feat: record and code"]);
    git(root, &["switch", "-q", "main"]);
    merge(root, "task/TSK-003-both");

    let text = succeeds(&cli(root, &["work", "next"]));
    assert!(
        task_line(&text, "TSK-003").is_some_and(|line| line.starts_with("landed")),
        "{text}"
    );
}

#[test]
fn a_squash_landing_is_listed_as_landed() {
    let dir = fixture();
    let root = dir.path();
    git(root, &["switch", "-qc", "fix/TSK-001-squash"]);
    write(root, "src/squash.rs", "pub fn squash() {}\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "fix: squash"]);
    let squashed = git(root, &["rev-parse", "HEAD"]);
    git(root, &["switch", "-q", "main"]);
    // The line moves first, so the landing is a new commit, not a merge.
    write(root, "src/other.rs", "pub fn other() {}\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "feat: other"]);
    git(root, &["cherry-pick", &squashed]);

    let text = succeeds(&cli(root, &["work", "next"]));
    assert!(
        task_line(&text, "TSK-001").is_some_and(|line| line.starts_with("landed")),
        "{text}"
    );
}

/// AC-5: the journey. The merged branch leaves the record `todo`; `work
/// next` shows it as landed with the command; completing the record with
/// that command makes it disappear.
#[test]
fn completing_the_record_clears_the_landed_task() {
    let dir = fixture();
    let root = dir.path();
    land_work(root);
    let reviewed = git(root, &["rev-parse", "task/TSK-001-work"]);

    let text = succeeds(&cli(root, &["work", "next"]));
    assert!(
        task_line(&text, "TSK-001").is_some_and(|line| line.starts_with("landed")),
        "{text}"
    );

    git(root, &["switch", "-qc", "docs/complete-tsk-001"]);
    let evidence = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(evidence.path(), format!("acceptance:\n  reviewed: {reviewed}\n  review: session:fixture@sha256:00\n  criteria:\n    AC-1: verified | landed journey\n  journey: verified | landed journey\n  not_verified: none\n  follow_ups: none: fixture\n  verdict: approved\n")).unwrap();
    succeeds(&cli(
        root,
        &[
            "task",
            "status",
            "TSK-001",
            "complete",
            "--acceptance",
            evidence.path().to_str().unwrap(),
        ],
    ));
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "docs: complete the outcome"]);
    git(root, &["switch", "-q", "main"]);
    merge(root, "docs/complete-tsk-001");

    let text = succeeds(&cli(root, &["work", "next"]));
    assert!(task_line(&text, "TSK-001").is_none(), "{text}");
    assert!(!text.contains("landed branch"), "{text}");
    assert!(
        task_line(&text, "TSK-002").is_some_and(|line| line.starts_with("ready")),
        "{text}"
    );
}
