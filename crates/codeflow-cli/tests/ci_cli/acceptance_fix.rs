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
    let body = format!(
        "## Summary\nRepair work.\n\n{}\n## Changes\n- repair\n\n## Testing\n- fixture\n",
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
        let out = ci(root, "task/TSK-001-fix", true);
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
