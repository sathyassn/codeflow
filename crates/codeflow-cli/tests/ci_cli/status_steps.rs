//! TSK-1036 AC-7 (issue #400): a range is judged one status step per
//! commit and per merge parent, through `codeflow ci` and the pre-push
//! hook. A terminal status entered and left inside one range, a merge
//! record with an illegal step from one of its parents and a merge that
//! revives a task cancelled on the other side are refused although their
//! endpoints look legal; history that cannot be read refuses with a remedy
//! that names its cause.
use super::{codeflow, combined, git, planned_task, run_in};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const BRANCH: &str = "task/TSK-001-steps";
const TASK: &str = "project-management/tasks/TSK-001.md";
const BODY: &str = "Task: TSK-001\n\n## Summary\nJudge the steps.\n\n- one change\n\n## Reviews\nNone: pending review.\n## Release impact\n- Impact: patch\n- Breaking: no\n- Rationale: Preserve public behavior.\n- Migration: none\n## Changes\n- change the record\n\n## Testing\n- focused test\n";
const POLICY: &str = r#"{"git":{"product_paths":["src/**"],"test_gate_on_push":"warn"}}"#;
const CANCELLED: &str = "\n## Closeout\n\n- cancelled: The approach was withdrawn.\n- scope: Dropped without replacement.\n";
const BLOCKED: &str =
    "\n## Blocker\n\n- reason: waits on the host\n- owner: ops\n- revisit: the host is ready\n";
const TERMINAL: &str = "status cancelled -> todo is refused at commit";

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn commit(root: &Path, message: &str) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", message]);
}

fn rev(root: &Path, revision: &str) -> String {
    let out = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", revision])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// TSK-001 in `status`, with the section that status needs.
fn record(status: &str) -> String {
    let mut text =
        planned_task("TSK-001", "").replace("status: todo", &format!("status: {status}"));
    match status {
        "cancelled" => text.push_str(CANCELLED),
        "blocked" => text.push_str(BLOCKED),
        _ => {}
    }
    text
}

/// `main` holds the policy and TSK-001 at `todo`; `BRANCH` starts there
/// with one product commit. A bare remote holds `main`, so the pre-push
/// hook judges the branch against it.
fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-qb", "main"]);
    git(root, &["config", "user.email", "test@example.test"]);
    git(root, &["config", "user.name", "Test"]);
    write(root, ".codeflow/policy.json", POLICY);
    write(root, TASK, &record("todo"));
    commit(root, "docs: plan the task");
    let seeded = run_in(root, &["ids", "seed"]);
    assert!(seeded.status.success(), "{}", combined(&seeded));
    let remote = root.join(".git/test-remote.git");
    git(
        root,
        &["init", "--bare", "-qb", "main", remote.to_str().unwrap()],
    );
    git(root, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(root, &["push", "-q", "origin", "main", "codeflow/registry"]);
    git(root, &["switch", "-qc", BRANCH]);
    write(root, "src/lib.rs", "pub fn work() {}\n");
    commit(root, "feat: build the work");
    dir
}

fn ci(root: &Path) -> Output {
    run_in(
        root,
        &[
            "ci",
            "--base",
            "main",
            "--head",
            "HEAD",
            "--branch",
            BRANCH,
            "--pr-body",
            BODY,
        ],
    )
}

fn pre_push(root: &Path) -> Output {
    let repo = git2::Repository::open(root).unwrap();
    let head = repo.head().unwrap().target().unwrap();
    let mut child = codeflow()
        .current_dir(root)
        .args([
            "git-hook",
            "pre-push",
            "origin",
            repo.find_remote("origin").unwrap().url().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.take().unwrap(),
        "refs/heads/{BRANCH} {head} refs/heads/{BRANCH} {}",
        "0".repeat(40)
    )
    .unwrap();
    child.wait_with_output().unwrap()
}

/// Both paths refuse with `reason`, and print `remedy` when given.
fn refused(root: &Path, what: &str, reason: &str, remedy: Option<&str>) {
    for (path, out) in [("ci", ci(root)), ("pre-push", pre_push(root))] {
        let text = combined(&out);
        assert!(!out.status.success(), "{what} ({path}) passed:\n{text}");
        assert!(
            text.contains(reason),
            "{what} ({path}) lacks {reason:?}:\n{text}"
        );
        if let Some(remedy) = remedy {
            assert!(
                text.contains(remedy),
                "{what} ({path}) lacks {remedy:?}:\n{text}"
            );
        }
    }
}

/// Neither path reports a status step of TSK-001.
fn no_step_refused(root: &Path, what: &str) {
    for (path, out) in [("ci", ci(root)), ("pre-push", pre_push(root))] {
        let text = combined(&out);
        assert!(
            !text.contains("is refused at commit"),
            "{what} ({path}):\n{text}"
        );
        assert!(
            !text.contains("status history cannot be read"),
            "{what} ({path}):\n{text}"
        );
    }
}

/// AC-7 (b): `todo -> cancelled -> todo` in one range nets to `todo -> todo`
/// and passed on its endpoints; its second step alone is refused, so the
/// range is refused with the terminal-status message.
#[test]
fn a_cancellation_revived_inside_one_range_is_refused() {
    let dir = repo();
    let root = dir.path();
    write(root, TASK, &record("cancelled"));
    commit(root, "docs: cancel the task");
    write(root, TASK, &record("todo"));
    commit(root, "docs: revive the task");
    refused(root, "todo -> cancelled -> todo", TERMINAL, None);
    let text = combined(&ci(root));
    assert!(text.contains("a cancelled task is terminal"), "{text}");
}

/// A cancellation revived across a gap is refused like the direct edit:
/// the record deleted then recreated, moved to its epic's task path, or
/// parked off the record paths and restored (TSK-1036 review 01), or
/// cancelled only in another task file that states its id (review 02).
#[test]
fn a_cancellation_revived_across_a_gap_is_refused() {
    const EPIC_TASK: &str = "project-management/epics/EPC-001/tasks/TSK-001.md";
    let gaps: [(&str, &str, Option<&str>); 4] = [
        ("deleted then recreated", TASK, None),
        ("moved to the epic path", EPIC_TASK, Some(EPIC_TASK)),
        (
            "parked off the record paths",
            TASK,
            Some("notes/TSK-001.md"),
        ),
        (
            "held under another task file name",
            TASK,
            Some("project-management/tasks/TSK-002.md"),
        ),
    ];
    for (what, back, gap) in gaps {
        let dir = repo();
        let root = dir.path();
        write(root, TASK, &record("cancelled"));
        commit(root, "docs: cancel the task");
        std::fs::remove_file(root.join(TASK)).unwrap();
        if let Some(gap) = gap.filter(|gap| *gap != back) {
            write(root, gap, &record("cancelled"));
        }
        if gap != Some(back) {
            commit(root, "docs: move the task away");
        }
        if let Some(gap) = gap.filter(|gap| *gap != back) {
            std::fs::remove_file(root.join(gap)).unwrap();
        }
        write(root, back, &record("todo"));
        commit(root, "docs: bring the task back");
        refused(root, what, TERMINAL, None);
    }
}

/// The record is the file that states its id: TSK-001 replaced by a file
/// named TSK-002 that states `id: TSK-001` and is cancelled, then restored
/// under its own name as `todo`, is refused (TSK-1036 review 02).
#[test]
fn a_cancellation_held_only_under_another_file_name_is_refused() {
    const OTHER: &str = "project-management/tasks/TSK-002.md";
    let dir = repo();
    let root = dir.path();
    std::fs::remove_file(root.join(TASK)).unwrap();
    write(root, OTHER, &record("cancelled"));
    commit(root, "docs: hold the task under another name");
    std::fs::remove_file(root.join(OTHER)).unwrap();
    write(root, TASK, &record("todo"));
    commit(root, "docs: restore the task");
    refused(root, "held only under another file name", TERMINAL, None);
}

/// A side branch from `main` that cancels TSK-001; returns its name.
fn cancelled_side(root: &Path) -> &'static str {
    git(root, &["switch", "-qc", "side", "main"]);
    write(root, TASK, &record("cancelled"));
    commit(root, "docs: cancel the task");
    git(root, &["switch", "-q", BRANCH]);
    "side"
}

/// Merge `side` into the branch with TSK-001 resolved to `resolved`.
fn merge_resolved(root: &Path, side: &str, resolved: &str) {
    git(root, &["merge", "-q", "--no-ff", "--no-commit", side]);
    write(root, TASK, resolved);
    commit(root, "chore: merge the side branch");
}

/// AC-7: a hand-edited merge record is judged against every parent. From
/// the cancelled parent, `blocked` is an illegal step that no parent's
/// chain brings, so it is refused, although the first parent's `todo ->
/// blocked` is legal and the endpoints were accepted before. Taking the
/// cancelled side is the legal control.
#[test]
fn a_merge_record_with_an_illegal_step_from_one_parent_is_refused() {
    let dir = repo();
    let root = dir.path();
    let side = cancelled_side(root);
    merge_resolved(root, side, &record("blocked"));
    refused(
        root,
        "a merge resolved to blocked",
        "status cancelled -> blocked is refused at commit",
        None,
    );

    let control = repo();
    let root = control.path();
    let side = cancelled_side(root);
    merge_resolved(root, side, &record("cancelled"));
    no_step_refused(root, "a merge that takes the cancelled side");
}

/// AC-7: a task cancelled on one side cannot be revived by a merge that
/// takes the other side's `todo`: the other side never changed it, so no
/// chain of legal steps brings `todo` over the cancellation.
#[test]
fn a_merge_that_revives_a_cancelled_task_is_refused() {
    let dir = repo();
    let root = dir.path();
    let side = cancelled_side(root);
    merge_resolved(root, side, &record("todo"));
    refused(
        root,
        "a merge taking todo over a cancellation",
        TERMINAL,
        None,
    );
}

/// A range whose middle commit holds a version of TSK-001 the history
/// judgment must read: `todo -> blocked -> todo`. Returns the middle
/// commit.
fn three_versions(root: &Path, middle: &str) -> String {
    write(root, TASK, middle);
    commit(root, "docs: change the task");
    let at = rev(root, "HEAD");
    write(root, TASK, &record("todo"));
    commit(root, "docs: restore the task");
    at
}

/// AC-7: history that cannot be read refuses, never falling back to the
/// endpoints (which match here), and the remedy names the cause: a shallow
/// boundary, a missing object and an unparsable record each get their own.
#[test]
fn unreadable_status_history_refuses_with_its_cause() {
    // A shallow boundary inside the range.
    let dir = repo();
    let root = dir.path();
    let middle = three_versions(root, &record("blocked"));
    std::fs::write(root.join(".git/shallow"), format!("{middle}\n")).unwrap();
    // `codeflow ci` and the hook refuse a shallow clone before any record
    // is read; the status judgment's own refusal is unit-tested beside it.
    refused(
        root,
        "a shallow range",
        "shallow",
        Some("git fetch --unshallow"),
    );

    // A missing object: the middle version's blob is gone.
    let dir = repo();
    let root = dir.path();
    let middle = three_versions(root, &record("blocked"));
    let blob = rev(root, &format!("{middle}:{TASK}"));
    let loose = root.join(format!(".git/objects/{}/{}", &blob[..2], &blob[2..]));
    std::fs::remove_file(&loose).unwrap();
    let missing = "status history cannot be read (missing object)";
    let ci_text = combined(&ci(root));
    assert!(!ci(root).status.success(), "{ci_text}");
    assert!(
        ci_text.contains(missing) && ci_text.contains("repair the object store"),
        "{ci_text}"
    );

    // An unparsable record version.
    let dir = repo();
    let root = dir.path();
    three_versions(root, "not a record\n");
    refused(
        root,
        "an unparsable version",
        "status history cannot be read (unparsable record)",
        Some("a fetch does not repair it"),
    );
}
