//! The CI entry point judges a fix range and both task-landing provenances.
use super::{combined, git, planned_task, run_in};
use std::path::Path;

const TASK: &str = "project-management/tasks/TSK-001.md";
const LINE: &str = "integration/EPC-001-line";

fn commit(root: &Path, message: &str) -> String {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", message]);
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().into()
}

fn block(reviewed: &str) -> String {
    format!("```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: https://example.test/review/1\n  criteria:\n    AC-1: verified | unit\n    AC-2: verified | journey\n  journey: verified | journey ran\n  not_verified: none\n  follow_ups: none: fixture\n  verdict: approved\n```\n")
}

fn task(root: &Path, status: &str, closeout: &str) {
    let record = planned_task("TSK-001", "")
        .replace("status: todo", &format!("status: {status}"))
        .replace(
            "integration_target: main",
            &format!("integration_target: {LINE}"),
        );
    std::fs::write(
        root.join(TASK),
        format!("{record}\n## Closeout\n\n{closeout}"),
    )
    .unwrap();
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-qb", "main"]);
    git(root, &["config", "user.email", "test@example.test"]);
    git(root, &["config", "user.name", "Test"]);
    std::fs::create_dir_all(root.join("project-management/tasks")).unwrap();
    task(root, "todo", "Pending.\n");
    commit(root, "chore: plan work");
    git(root, &["switch", "-qc", LINE]);
    dir
}

fn merge(root: &Path, branch: &str) {
    git(root, &["switch", "-q", LINE]);
    git(
        root,
        &["merge", "-q", "--no-ff", "-m", "chore: land work", branch],
    );
}

fn ci(root: &Path, branch: &str, named: bool) -> std::process::Output {
    ci_reviewed(root, branch, named, "")
}

/// `reviews` is the body's Reviews section, or empty for none.
fn ci_reviewed(root: &Path, branch: &str, named: bool, reviews: &str) -> std::process::Output {
    let body = format!(
        "## Summary\nRepair work.\n\n- repair\n\n{}\n## Changes\n- repair\n\n## Testing\n- fixture\n{reviews}",
        if named { "Task: TSK-001" } else { "" }
    );
    run_in(
        root,
        &[
            "ci",
            "--base",
            LINE,
            "--head",
            "HEAD",
            "--branch",
            branch,
            "--pr-body",
            &body,
        ],
    )
}

/// A Reviews section with one approving row for the whole unit at `sha`.
fn reviews(sha: &str) -> String {
    format!("\n## Reviews\n\n| Reviewer | Scope | Verdict |\n|---|---|---|\n| Grok 4.7 | whole unit at {sha} | approved |\n")
}

/// Issue 121: a completed task's body approves the commit its acceptance
/// block binds. `light` changes only prose; `row` names a commit or none.
fn completed(light: bool, row: Option<bool>) -> (tempfile::TempDir, String, String) {
    let dir = fixture();
    let root = dir.path();
    git(root, &["switch", "-qc", "task/TSK-001-work"]);
    let file = if light { "docs/notes.md" } else { "work.rs" };
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join(file), "first\n").unwrap();
    let earlier = commit(root, "docs: first pass");
    std::fs::write(root.join(file), "second\n").unwrap();
    let reviewed = commit(root, "docs: second pass");
    task(root, "complete", &block(&reviewed));
    commit(root, "docs: complete task");
    let section = match row {
        Some(bound) => reviews(if bound { &reviewed } else { &earlier }),
        None => String::new(),
    };
    (dir, reviewed, section)
}

#[test]
fn a_completion_needs_a_review_row_for_its_reviewed_commit() {
    // The PR 114 shape: the row names the round-1 head, the block a later one.
    let (dir, reviewed, stale) = completed(false, Some(false));
    let out = ci_reviewed(dir.path(), "task/TSK-001-work", true, &stale);
    let text = combined(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("work.acceptance_binding"), "{text}");
    assert!(
        text.contains(&format!(
            "binds the reviewed commit {reviewed}, and the Reviews section has no row approving it"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!("`whole unit at {reviewed}`")),
        "{text}"
    );

    let (dir, _, bound) = completed(false, Some(true));
    let out = ci_reviewed(dir.path(), "task/TSK-001-work", true, &bound);
    assert!(out.status.success(), "{}", combined(&out));
}

#[test]
fn a_light_completion_may_leave_reviews_out_but_never_name_an_earlier_head() {
    let (dir, _, none) = completed(true, None);
    let out = ci_reviewed(dir.path(), "task/TSK-001-work", true, &none);
    assert!(out.status.success(), "{}", combined(&out));
    // A light range that records no review is not asked to invent one.
    let (dir, _, _) = completed(true, None);
    let out = ci_reviewed(
        dir.path(),
        "task/TSK-001-work",
        true,
        "\n## Reviews\n\nNone: documentation only\n",
    );
    assert!(out.status.success(), "{}", combined(&out));

    let (dir, reviewed, stale) = completed(true, Some(false));
    let out = ci_reviewed(dir.path(), "task/TSK-001-work", true, &stale);
    let text = combined(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains(&format!("`whole unit at {reviewed}`")),
        "{text}"
    );
}

#[test]
fn work_in_progress_with_a_pending_review_passes() {
    let dir = fixture();
    let root = dir.path();
    git(root, &["switch", "-qc", "task/TSK-001-work"]);
    std::fs::write(root.join("work.rs"), "// work\n").unwrap();
    commit(root, "feat: work");
    let out = ci_reviewed(
        root,
        "task/TSK-001-work",
        true,
        "\n## Reviews\n\nNone: pending\n",
    );
    assert!(out.status.success(), "{}", combined(&out));
}

#[test]
fn one_pr_fix_ci_validates_the_new_and_superseded_reviews() {
    for fault in ["valid", "copied", "reason"] {
        let dir = fixture();
        let root = dir.path();
        git(root, &["switch", "-qc", "task/TSK-001-first"]);
        std::fs::write(root.join("work.rs"), "// initial\n").unwrap();
        let reviewed = commit(root, "feat: initial work");
        let old = block(&reviewed);
        task(root, "complete", &old);
        commit(root, "docs: complete task");
        merge(root, "task/TSK-001-first");
        git(root, &["switch", "-qc", "task/TSK-001-fix"]);
        let archived = old.replace(
            "acceptance:\n",
            "acceptance_superseded:\n  reason: regression\n",
        );
        task(root, "todo", &archived);
        commit(root, "docs: reopen task");
        std::fs::write(root.join("work.rs"), "// fixed\n").unwrap();
        let fixed = commit(root, "fix: repair work");
        let archive = if fault == "reason" {
            archived.replace("  reason: regression\n", "")
        } else {
            archived
        };
        task(
            root,
            "complete",
            &format!(
                "{archive}{}",
                block(if fault == "copied" { &reviewed } else { &fixed })
            ),
        );
        commit(root, "docs: re-complete task");
        let out = ci_reviewed(root, "task/TSK-001-fix", true, &reviews(&fixed));
        let text = combined(&out);
        if fault == "valid" {
            assert!(out.status.success(), "{text}");
        } else {
            assert!(!out.status.success(), "{fault}: {text}");
            assert!(
                text.contains(if fault == "copied" {
                    "inside the fix range"
                } else {
                    "reason"
                }),
                "{fault}: {text}"
            );
        }
    }
}

#[test]
fn task_landing_carries_ancestor_review_but_not_a_reopened_fix() {
    let dir = fixture();
    let root = dir.path();
    git(root, &["switch", "-qc", "task/TSK-001-first"]);
    std::fs::write(root.join("work.rs"), "// work\n").unwrap();
    let reviewed = commit(root, "feat: work");
    task(root, "todo", "Review recorded; close after landing.\n");
    commit(root, "docs: record review");
    git(root, &["switch", "-q", LINE]);
    std::fs::write(root.join("other.rs"), "// unrelated\n").unwrap();
    commit(root, "feat: unrelated work");
    merge(root, "task/TSK-001-first");
    git(root, &["switch", "-qc", "plan/complete"]);
    task(root, "complete", &block(&reviewed));
    commit(root, "docs: complete task late");
    let out = ci(root, "plan/complete", false);
    assert!(out.status.success(), "{}", combined(&out));
    merge(root, "plan/complete");
    git(root, &["switch", "-qc", "task/TSK-001-fix"]);
    let archived = block(&reviewed).replace(
        "acceptance:\n",
        "acceptance_superseded:\n  reason: regression\n",
    );
    task(root, "todo", &archived);
    commit(root, "docs: reopen task");
    git(root, &["switch", "-qc", "fix/implementation"]);
    std::fs::write(root.join("work.rs"), "// fixed\n").unwrap();
    commit(root, "fix: repair work");
    git(root, &["switch", "-q", "task/TSK-001-fix"]);
    git(
        root,
        &[
            "merge",
            "--no-ff",
            "-m",
            "chore: merge fix",
            "fix/implementation",
        ],
    );
    task(root, "complete", &format!("{archived}{}", block(&reviewed)));
    commit(root, "docs: re-complete task");
    let out = ci(root, "task/TSK-001-fix", true);
    assert!(
        !out.status.success() && combined(&out).contains("inside the fix range"),
        "{}",
        combined(&out)
    );
}
