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
    super::fixture_seed::copy("ci-acceptance", build_fixture)
}

fn build_fixture() -> tempfile::TempDir {
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

fn redaction_fixture(superseded: bool) -> (tempfile::TempDir, String, String) {
    let dir = fixture();
    let root = dir.path();
    std::fs::create_dir_all(root.join(".codeflow")).unwrap();
    std::fs::write(root.join(".codeflow/policy.json"), "{}\n").unwrap();
    commit(root, "chore: adopt default policy");
    git(root, &["switch", "-qc", "task/TSK-001-first"]);
    std::fs::write(root.join("work.rs"), "// work\n").unwrap();
    let planned = std::fs::read_to_string(root.join(TASK)).unwrap().replace(
        "the system shall work",
        "the system shall serve customer-name",
    );
    std::fs::write(root.join(TASK), planned).unwrap();
    let reviewed = commit(root, "feat: work");
    task(
        root,
        "complete",
        &block(&reviewed).replace("unit", "customer-name unit"),
    );
    let record = std::fs::read_to_string(root.join(TASK)).unwrap().replace(
        "the system shall work",
        "the system shall serve customer-name",
    );
    std::fs::write(root.join(TASK), record).unwrap();
    commit(root, "docs: complete task");
    merge(root, "task/TSK-001-first");
    std::fs::write(root.join("other.rs"), "// direct commit\n").unwrap();
    let mut source = commit(root, "fix: direct change");
    if superseded {
        let out = run_in(
            root,
            &[
                "task",
                "status",
                "TSK-001",
                "todo",
                "--reason",
                "repair needed",
            ],
        );
        assert!(out.status.success(), "{}", combined(&out));
        source = commit(root, "docs: reopen task");
    }
    git(root, &["switch", "-qc", "plan/redact"]);
    let old = std::fs::read_to_string(root.join(TASK)).unwrap();
    (dir, old, source)
}

fn redacted(old: &str, source: &str) -> String {
    let digest = codeflow_core::scaffold::sha256_hex(old.as_bytes());
    old.replace("customer-name", "customer").replacen(
        "---\n", &format!("---\nredaction: {{source: {source}, sha256: {digest}, review: 'https://example.test/redaction/1'}}\n"), 1)
}

#[test]
fn redaction_after_a_direct_commit_keeps_the_landed_results() {
    let (dir, old, source) = redaction_fixture(false);
    std::fs::write(dir.path().join(TASK), redacted(&old, &source)).unwrap();
    let head = commit(dir.path(), "docs: redact record");
    let out = ci(dir.path(), "plan/redact", true);
    assert!(out.status.success(), "{}", combined(&out));
    let reviewed = ci_reviewed(dir.path(), "plan/redact", true, &reviews(&head));
    assert!(reviewed.status.success(), "{}", combined(&reviewed));
}

#[test]
fn redaction_keeps_a_reopened_records_superseded_results() {
    let (dir, old, source) = redaction_fixture(true);
    std::fs::write(dir.path().join(TASK), redacted(&old, &source)).unwrap();
    commit(dir.path(), "docs: redact superseded record");
    let out = ci(dir.path(), "plan/redact", true);
    assert!(out.status.success(), "{}", combined(&out));
}

#[test]
fn redaction_refuses_changed_results_review_and_unrelated_files() {
    for (field, from, to, diagnostic) in [
        ("result", "AC-1: verified", "AC-1: waived", "AC-1 result"),
        (
            "reviewed",
            "  reviewed: ",
            "  reviewed: a",
            "changes reviewed",
        ),
        (
            "review",
            "https://example.test/review/1",
            "https://example.test/review/2",
            "review provenance",
        ),
        (
            "journey",
            "journey: verified",
            "journey: failed",
            "journey result",
        ),
        (
            "verdict",
            "verdict: approved",
            "verdict: changes_requested",
            "changes verdict",
        ),
        ("digest", "sha256: ", "sha256: a", "sha256 does not match"),
        (
            "outside",
            "unused",
            "unused",
            "outside.rs outside the record",
        ),
        (
            "review missing",
            "https://example.test/redaction/1",
            "",
            "redaction review is empty",
        ),
    ] {
        let (dir, old, source) = redaction_fixture(false);
        let new = redacted(&old, &source).replace(from, to);
        std::fs::write(dir.path().join(TASK), new).unwrap();
        if field == "outside" {
            std::fs::write(dir.path().join("outside.rs"), "// unrelated\n").unwrap();
        }
        commit(dir.path(), "docs: attempt redaction");
        let out = ci(dir.path(), "plan/redact", true);
        let text = combined(&out);
        assert!(
            !out.status.success() && text.contains(diagnostic),
            "{field}: {text}"
        );
    }
}

#[test]
fn redaction_command_and_pre_push_accept_the_record_only_journey() {
    use std::io::Write;
    use std::process::Stdio;
    let install = tempfile::tempdir().unwrap();
    let binary = install
        .path()
        .join(format!("codeflow{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(env!("CARGO_BIN_EXE_codeflow"), &binary).unwrap();
    let installed = || super::codeflow_at(&binary);
    let (dir, old, source) = redaction_fixture(false);
    let replacement = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(replacement.path(), old.replace("customer-name", "customer")).unwrap();
    let out = installed()
        .current_dir(dir.path())
        .args([
            "task",
            "redact",
            "TSK-001",
            "--replacement",
            replacement.path().to_str().unwrap(),
            "--review",
            "https://example.test/redaction/1",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", combined(&out));
    let after = std::fs::read_to_string(dir.path().join(TASK)).unwrap();
    assert!(!after.contains("customer-name"));
    assert!(after.contains(&source));
    let head = commit(dir.path(), "docs: redact record");
    let body = "## Summary\nRedact names.\n\n- Keep results.\n\nTask: TSK-001\n\n## Changes\n- Redact wording.\n";
    let out = installed()
        .current_dir(dir.path())
        .args([
            "ci",
            "--base",
            LINE,
            "--head",
            "HEAD",
            "--branch",
            "plan/redact",
            "--pr-body",
            body,
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", combined(&out));
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "--bare", "-q"]);
    let url = remote.path().to_str().unwrap();
    git(dir.path(), &["remote", "add", "origin", url]);
    git(
        remote.path(),
        &["symbolic-ref", "HEAD", &format!("refs/heads/{LINE}")],
    );
    git(dir.path(), &["push", "-q", "origin", "main", LINE]);
    let mut child = installed()
        .args(["git-hook", "pre-push", "origin", url])
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.take().unwrap(),
        "refs/heads/plan/redact {head} refs/heads/plan/redact {}",
        "0".repeat(40)
    )
    .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", combined(&out));
}

#[test]
fn a_redacted_superseded_record_can_complete_its_later_fix() {
    let (dir, old, source) = redaction_fixture(true);
    let root = dir.path();
    std::fs::write(root.join(TASK), redacted(&old, &source)).unwrap();
    commit(root, "docs: redact superseded record");
    merge(root, "plan/redact");
    git(root, &["switch", "-qc", "task/TSK-001-fix"]);
    std::fs::write(root.join("work.rs"), "// fix\n").unwrap();
    let reviewed = commit(root, "fix: finish repair");
    let acceptance = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        acceptance.path(),
        block(&reviewed)
            .replace("```yaml\n", "")
            .replace("```\n", ""),
    )
    .unwrap();
    let out = run_in(
        root,
        &[
            "task",
            "status",
            "TSK-001",
            "complete",
            "--acceptance",
            acceptance.path().to_str().unwrap(),
        ],
    );
    assert!(out.status.success(), "{}", combined(&out));
    commit(root, "docs: complete repair");
    let out = ci_reviewed(root, "task/TSK-001-fix", true, &reviews(&reviewed));
    assert!(out.status.success(), "{}", combined(&out));
    merge(root, "task/TSK-001-fix");
    let findings = codeflow_core::workgraph::acceptance::pull_request_findings(
        root,
        "main",
        "HEAD",
        &codeflow_core::workgraph::acceptance::Criteria::Amendable,
    )
    .unwrap();
    assert!(findings.iter().all(|finding| finding.note), "{findings:?}");
}

#[test]
fn a_line_carries_the_record_only_redaction_at_its_landing() {
    use codeflow_core::workgraph::acceptance::{pull_request_findings, Criteria};
    let (dir, old, source) = redaction_fixture(false);
    let root = dir.path();
    std::fs::write(root.join(TASK), redacted(&old, &source)).unwrap();
    commit(root, "docs: redact record");
    merge(root, "plan/redact");
    std::fs::write(root.join("later.rs"), "// later line work\n").unwrap();
    commit(root, "feat: later line work");
    let findings = pull_request_findings(root, "main", "HEAD", &Criteria::Amendable).unwrap();
    assert!(findings.iter().all(|finding| finding.note), "{findings:?}");
    assert!(
        findings
            .iter()
            .any(|finding| finding.message.contains("record-only redaction")),
        "{findings:?}"
    );
}
