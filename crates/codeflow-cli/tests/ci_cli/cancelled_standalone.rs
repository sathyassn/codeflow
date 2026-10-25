//! TSK-414: preserve an unlanded cancellation without starting its task.
use super::{codeflow, combined, git, planned_task, run_in};
use std::io::Write;
use std::path::Path;
use std::process::{Output, Stdio};

const BRANCH: &str = "plan/record-cancellation";
const TASK: &str = "project-management/tasks/TSK-001.md";
const BODY: &str = "Task: TSK-001\n\n## Summary\nPreserve the cancellation.\n\n- Record the reason and scope.\n\n## Changes\n- Add the cancelled record.\n\n## Reviews\nNone: review follows.\n";
const EPIC_BRANCH: &str = "plan/EPC-001-amend";
const EPIC_BODY: &str = "Task: EPC-001\n\n## Summary\nAmend the plan.\n\n- Change the planned records.\n\n## Changes\n- Amend the records.\n\n## Reviews\nNone: review follows.\n";
const POLICY: &str = r#"{"git":{"product_paths":["src/**"],"test_gate_on_push":"warn"}}"#;
const CLOSEOUT: &str =
    "\n## Closeout\n\n- cancelled: The approach was withdrawn.\n- scope: Dropped without replacement.\n";

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn commit(root: &Path) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "docs: record cancellation"]);
}

fn init(root: &Path) {
    git(root, &["init", "-qb", "main"]);
    git(root, &["config", "user.email", "test@example.test"]);
    git(root, &["config", "user.name", "Test"]);
    write(root, ".codeflow/policy.json", POLICY);
}

/// Seed the id registry and give the fixture a remote holding `main`, so
/// the pre-push hook judges the branch against the target.
fn publish(root: &Path) {
    let seeded = run_in(root, &["ids", "seed"]);
    assert!(seeded.status.success(), "{}", combined(&seeded));
    remote(root);
    git(root, &["push", "-q", "origin", "codeflow/registry"]);
}

/// A bare remote holding `main`.
fn remote(root: &Path) {
    let remote = root.join(".git/test-remote.git");
    git(
        root,
        &["init", "--bare", "-qb", "main", remote.to_str().unwrap()],
    );
    git(root, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(root, &["push", "-q", "origin", "main"]);
}

fn fixture(
    status: &str,
    reason: bool,
    scope: bool,
    extra: Option<&str>,
    existing: bool,
) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    init(root);
    if existing {
        write(root, TASK, &planned_task("TSK-001", ""));
    }
    commit(root);
    git(root, &["switch", "-qc", BRANCH]);
    let mut record =
        planned_task("TSK-001", "").replace("status: todo", &format!("status: {status}"));
    record.push_str("\n## Closeout\n\n");
    if reason {
        record.push_str("- cancelled: The approach was withdrawn.\n");
    }
    if scope {
        record.push_str("- scope: Dropped without replacement.\n");
    }
    write(root, TASK, &record);
    if let Some(path) = extra {
        write(root, path, "unwanted change\n");
    }
    commit(root);
    publish(root);
    dir
}

fn ci_on(root: &Path, branch: &str, body: &str) -> Output {
    run_in(
        root,
        &[
            "ci",
            "--base",
            "main",
            "--head",
            "HEAD",
            "--branch",
            branch,
            "--pr-body",
            body,
        ],
    )
}

fn ci(root: &Path) -> Output {
    ci_on(root, BRANCH, BODY)
}

fn pre_push_on(root: &Path, branch: &str) -> Output {
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
        "refs/heads/{branch} {head} refs/heads/{branch} {}",
        "0".repeat(40)
    )
    .unwrap();
    child.wait_with_output().unwrap()
}

fn pre_push(root: &Path) -> Output {
    pre_push_on(root, BRANCH)
}

#[test]
fn cancelled_standalone_record_is_a_planning_amendment_without_start() {
    let dir = fixture("cancelled", true, true, None, false);
    for out in [ci(dir.path()), pre_push(dir.path())] {
        let text = combined(&out);
        assert!(out.status.success(), "{text}");
        assert!(
            text.contains("no epic: TSK-001 added (task record, cancelled)"),
            "{text}"
        );
        assert!(!text.contains("work.stable_planning_anchor"), "{text}");
        assert!(!text.contains("work.classification"), "{text}");
    }
}

/// Each invalid range keeps a refusal from `codeflow ci`, and the refusal
/// names its cause. The pre-push run has no `Task:` line, so it refuses
/// only what no `Task:` line could make valid: a cancelled record without
/// its reason or scope, a product path, and a complete record without its
/// acceptance block. A range CI refuses only for the
/// task its `Task:` line names (a todo or complete record, a docs path, a
/// record already at the merge-base) is also an epic amendment's range, so
/// the push lets it through and the pull request decides.
#[test]
fn cancelled_standalone_route_rejects_invalid_ranges() {
    struct Case {
        status: &'static str,
        reason: bool,
        scope: bool,
        extra: Option<&'static str>,
        existing: bool,
        ci: &'static str,
        push: Option<&'static str>,
    }
    #[rustfmt::skip]
    let cases = [
        Case { status: "todo", reason: true, scope: true, extra: None, existing: false, ci: "task branch", push: None, },
        Case { status: "complete", reason: true, scope: true, extra: None, existing: false, ci: "work.stable_planning_anchor", push: Some("a complete record needs an acceptance block"), },
        Case { status: "cancelled", reason: false, scope: true, extra: None, existing: false, ci: "(missing cancelled)", push: Some("(missing cancelled)"), },
        Case { status: "cancelled", reason: true, scope: false, extra: None, existing: false, ci: "(missing scope)", push: Some("(missing scope)"), },
        Case { status: "cancelled", reason: true, scope: true, extra: Some("src/unwanted.rs"), existing: false, ci: "changes only project-management/ paths", push: Some("src/unwanted.rs"), },
        Case { status: "cancelled", reason: true, scope: true, extra: Some("docs/unwanted.md"), existing: false, ci: "changes only project-management/ paths", push: None, },
        Case { status: "cancelled", reason: true, scope: true, extra: None, existing: true, ci: "already on the target", push: None, },
    ];
    for case in cases {
        let label = format!(
            "{}, reason={}, scope={}, extra={:?}, existing={}",
            case.status, case.reason, case.scope, case.extra, case.existing
        );
        let dir = fixture(
            case.status,
            case.reason,
            case.scope,
            case.extra,
            case.existing,
        );
        let out = ci(dir.path());
        let text = combined(&out);
        assert!(!out.status.success(), "{label}: {text}");
        assert!(
            text.contains(case.ci),
            "{label}: wants {:?}: {text}",
            case.ci
        );
        if case.status == "cancelled" {
            assert!(
                !text.contains("codeflow work start"),
                "{label}: a cancellation is never told to start: {text}"
            );
        }
        let out = pre_push(dir.path());
        let text = combined(&out);
        match case.push {
            Some(cause) => {
                assert!(!out.status.success(), "{label}: {text}");
                assert!(text.contains(cause), "{label}: wants {cause:?}: {text}");
            }
            None => assert!(out.status.success(), "{label}: {text}"),
        }
    }
}

#[test]
fn cancellation_class_uses_the_named_head_and_task() {
    let dir = fixture("cancelled", true, true, None, false);
    let root = dir.path();
    // Hosted CI can run from the target checkout with an explicit PR head.
    git(root, &["switch", "-q", "main"]);
    let check = |branch: &str, body: &str| {
        codeflow()
            .current_dir(root)
            .args([
                "ci",
                "--base",
                "main",
                "--head",
                BRANCH,
                "--branch",
                branch,
                "--pr-body",
                body,
            ])
            .output()
            .unwrap()
    };
    let out = check(BRANCH, BODY);
    assert!(out.status.success(), "{}", combined(&out));
    for (branch, body) in [
        (BRANCH, BODY.replace("Task: TSK-001", "Task: TSK-999")),
        ("fix/record-cancellation", BODY.to_string()),
        (BRANCH, BODY.replace("Task: TSK-001\n", "")),
    ] {
        let out = check(branch, &body);
        assert!(!out.status.success(), "{}", combined(&out));
    }
}

fn epic_record() -> String {
    "---\nid: EPC-001\ntitle: \"outcome\"\nstatus: planning\nwork_type: feat\nspecs: []\ncreated: 2026-10-03\n---\n\n# EPC-001: outcome\n\n## Summary\n\nAn outcome.\n\n## Acceptance Criteria\n\n- AC-1 (journey) On a fresh project, an adopter shall finish the flow.\n".to_string()
}

fn epic_task(id: &str) -> String {
    planned_task(id, "").replace(
        "epic_id: null\nstandalone_reason: bounded work",
        "epic_id: EPC-001\nstandalone_reason: null",
    )
}

fn task_path(id: &str) -> String {
    format!("project-management/tasks/{id}.md")
}

fn cancelled(id: &str) -> String {
    let mut record = planned_task(id, "").replace("status: todo", "status: cancelled");
    record.push_str(CLOSEOUT);
    record
}

type Shape = (&'static str, fn(&Path));

/// Shapes of an ordinary epic planning amendment that `codeflow ci`
/// accepts with `Task: EPC-001`: the pre-push run on the same `plan/`
/// branch must accept them too (TSK-414 review, finding 1).
#[test]
fn epic_amendments_with_standalone_records_pass_ci_and_pre_push() {
    let shapes: [Shape; 4] = [
        ("adds one standalone todo record", |root| {
            write(root, &task_path("TSK-003"), &planned_task("TSK-003", ""));
        }),
        ("cancels an existing standalone record", |root| {
            write(root, &task_path("TSK-002"), &cancelled("TSK-002"));
        }),
        ("adds two standalone todo records", |root| {
            write(root, &task_path("TSK-003"), &planned_task("TSK-003", ""));
            write(root, &task_path("TSK-004"), &planned_task("TSK-004", ""));
        }),
        (
            "adds a cancelled standalone record beside an epic criteria change",
            |root| {
                write(root, &task_path("TSK-003"), &cancelled("TSK-003"));
                write(
                    root,
                    "project-management/epics/EPC-001.md",
                    &epic_record().replace("shall finish the flow", "shall finish the whole flow"),
                );
            },
        ),
    ];
    let mut refused = Vec::new();
    for (shape, change) in shapes {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init(root);
        write(root, "project-management/epics/EPC-001.md", &epic_record());
        write(root, &task_path("TSK-001"), &epic_task("TSK-001"));
        write(root, &task_path("TSK-002"), &planned_task("TSK-002", ""));
        commit(root);
        git(root, &["switch", "-qc", EPIC_BRANCH]);
        change(root);
        commit(root);
        publish(root);
        for (side, out) in [
            ("ci", ci_on(root, EPIC_BRANCH, EPIC_BODY)),
            ("pre-push", pre_push_on(root, EPIC_BRANCH)),
        ] {
            let text = combined(&out);
            if !out.status.success() || text.contains("work.classification") {
                refused.push(format!("{shape} ({side}): {text}"));
            }
        }
    }
    assert!(refused.is_empty(), "{}", refused.join("\n\n"));
}

/// AC-5: the issue's steps end to end. The record lands on a task branch
/// beside a product change, `codeflow task status` cancels it there, and a
/// one-commit `plan/` branch cut from the target carries only that record.
#[test]
fn issue_journey_cancels_on_the_task_branch_and_lands_through_a_plan_branch() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    init(root);
    commit(root);
    remote(root);
    git(root, &["switch", "-qc", "task/TSK-001-work"]);
    let allocated = run_in(
        root,
        &["task", "new", "--standalone-reason", "bounded work", "Work"],
    );
    assert!(allocated.status.success(), "{}", combined(&allocated));
    // Keep the allocated identity; fill the scaffold with a planned body.
    let scaffold = std::fs::read_to_string(root.join(TASK)).unwrap();
    let uid = scaffold
        .lines()
        .find(|line| line.starts_with("uid:"))
        .expect("task new writes a uid");
    write(
        root,
        TASK,
        &planned_task("TSK-001", "").replacen("id: TSK-001\n", &format!("id: TSK-001\n{uid}\n"), 1),
    );
    write(root, "src/unwanted.rs", "unwanted change\n");
    commit(root);
    let seeded = run_in(root, &["ids", "seed"]);
    assert!(seeded.status.success(), "{}", combined(&seeded));
    git(root, &["push", "-q", "origin", "codeflow/registry"]);
    let cancel = run_in(
        root,
        &[
            "task",
            "status",
            "TSK-001",
            "cancelled",
            "--reason",
            "The approach was withdrawn.",
            "--scope",
            "Dropped without replacement.",
        ],
    );
    assert!(cancel.status.success(), "{}", combined(&cancel));
    let record = std::fs::read_to_string(root.join(TASK)).unwrap();
    assert!(record.contains("status: cancelled"), "{record}");
    assert!(
        record.contains("- cancelled: The approach was withdrawn."),
        "{record}"
    );
    assert!(
        record.contains("- scope: Dropped without replacement."),
        "{record}"
    );
    commit(root);
    git(root, &["switch", "-qc", BRANCH, "main"]);
    git(root, &["checkout", "task/TSK-001-work", "--", TASK]);
    commit(root);
    for (side, out) in [("ci", ci(root)), ("pre-push", pre_push(root))] {
        let text = combined(&out);
        assert!(out.status.success(), "{side}: {text}");
        assert!(
            text.contains("no epic: TSK-001 added (task record, cancelled)"),
            "{side}: {text}"
        );
        assert!(
            !text.contains("work.stable_planning_anchor"),
            "{side}: {text}"
        );
    }
}

/// `codeflow ci` on the cancellation branch with the given environment.
fn ci_env(root: &Path, body: &str, env: &[(&str, &str)]) -> Output {
    let mut command = codeflow();
    for (key, value) in env {
        command.env(key, value);
    }
    command
        .args([
            "ci",
            "--base",
            "main",
            "--head",
            "HEAD",
            "--branch",
            BRANCH,
            "--pr-body",
            body,
        ])
        .current_dir(root)
        .output()
        .unwrap()
}

fn head(root: &Path) -> String {
    let repo = git2::Repository::open(root).unwrap();
    let id = repo.head().unwrap().target().unwrap().to_string();
    id
}

/// TSK-1001 AC-11: a record-only cancellation carries no acceptance block,
/// so once it is known to be ready its Reviews section must approve the
/// exact head; a draft, a run that cannot tell and a push pass, and a
/// commit after the review needs a new row.
#[test]
fn a_ready_cancellation_needs_a_review_of_its_head() {
    let dir = fixture("cancelled", true, true, None, false);
    let root = dir.path();
    let ready = [("CODEFLOW_PR_IS_DRAFT", "false")];
    let refused = ci_env(root, BODY, &ready);
    let text = combined(&refused);
    assert!(!refused.status.success(), "{text}");
    assert!(
        text.contains("this ready cancellation has no Reviews row approving its head"),
        "{text}"
    );
    for env in [vec![("CODEFLOW_PR_IS_DRAFT", "true")], vec![]] {
        let out = ci_env(root, BODY, &env);
        assert!(out.status.success(), "{env:?}: {}", combined(&out));
    }
    assert!(pre_push(root).status.success());
    let reviewed = head(root);
    let approved = BODY.replace(
        "None: review follows.\n",
        &format!("| Reviewer | Scope | Verdict |\n|---|---|---|\n| Codex | whole unit at {reviewed} | approved |\n"),
    );
    let out = ci_env(root, &approved, &ready);
    assert!(out.status.success(), "{}", combined(&out));
    // A later commit is not what the review read.
    let record = std::fs::read_to_string(root.join(TASK)).unwrap();
    write(
        root,
        TASK,
        &record.replace("withdrawn.", "withdrawn for good."),
    );
    commit(root);
    let out = ci_env(root, &approved, &ready);
    assert!(!out.status.success(), "{}", combined(&out));
}

/// TSK-1001 AC-12: the cancelled record must pass the full task schema;
/// the lifecycle view does not carry the title or the standalone reason.
#[test]
fn a_cancelled_record_that_fails_the_task_schema_is_refused() {
    for (field, line) in [
        ("title", "title: work\n"),
        ("standalone_reason", "standalone_reason: bounded work\n"),
    ] {
        let dir = fixture("cancelled", true, true, None, false);
        let root = dir.path();
        let record = std::fs::read_to_string(root.join(TASK)).unwrap();
        assert!(record.contains(line), "{record}");
        write(root, TASK, &record.replace(line, ""));
        commit(root);
        let out = ci(root);
        let text = combined(&out);
        assert!(!out.status.success(), "{field}: {text}");
        assert!(
            text.contains("TSK-001 is not a valid task record"),
            "{field}: {text}"
        );
        assert!(text.contains(field), "{field}: {text}");
    }
}

/// TSK-1001 AC-13: product commits that a later commit reverts still land
/// with the range, so every commit is held to the cancellation's paths and
/// to the planning path rules, at the PR check and at push; several
/// record-only commits still pass.
#[test]
fn product_commits_reverted_later_still_refuse_a_planning_range() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    init(root);
    commit(root);
    git(root, &["switch", "-qc", BRANCH]);
    write(root, "src/lib.rs", "pub fn abandoned() {}\n");
    commit(root);
    std::fs::remove_file(root.join("src/lib.rs")).unwrap();
    commit(root);
    let mut record = planned_task("TSK-001", "").replace("status: todo", "status: cancelled");
    record.push_str(CLOSEOUT);
    write(root, TASK, &record);
    commit(root);
    publish(root);
    let out = ci(root);
    let text = combined(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("in every commit as well as the final diff"),
        "{text}"
    );
    let pushed = pre_push(root);
    let text = combined(&pushed);
    assert!(!pushed.status.success(), "{text}");
    assert!(text.contains("src/lib.rs"), "{text}");

    // Two record-only commits pass.
    let dir = fixture("cancelled", true, true, None, false);
    let root = dir.path();
    let record = std::fs::read_to_string(root.join(TASK)).unwrap();
    write(
        root,
        TASK,
        &record.replace("withdrawn.", "withdrawn after review."),
    );
    commit(root);
    let out = ci(root);
    assert!(out.status.success(), "{}", combined(&out));
}

/// TSK-1001 AC-9: a planning amendment runs the one docs validation, and
/// it validates the candidate head even when the checkout is elsewhere (a
/// hosted check runs from the target checkout); the clean candidate passes
/// from either place, and the temporary candidate worktree is removed.
#[test]
fn a_planning_amendment_validates_the_candidate_once() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    init(root);
    write(root, "project-management/epics/EPC-001.md", &epic_record());
    write(root, &task_path("TSK-001"), &epic_task("TSK-001"));
    commit(root);
    git(root, &["switch", "-qc", EPIC_BRANCH]);
    write(root, &task_path("TSK-003"), &epic_task("TSK-003"));
    commit(root);
    publish(root);
    let from = |checkout: &str, head: &str| {
        git(root, &["switch", "-q", checkout]);
        let out = codeflow()
            .args([
                "ci",
                "--base",
                "main",
                "--head",
                head,
                "--branch",
                EPIC_BRANCH,
                "--pr-body",
                EPIC_BODY,
            ])
            .current_dir(root)
            .output()
            .unwrap();
        git(root, &["switch", "-q", EPIC_BRANCH]);
        out
    };
    for checkout in [EPIC_BRANCH, "main"] {
        let out = from(checkout, EPIC_BRANCH);
        let text = combined(&out);
        assert!(out.status.success(), "{checkout}: {text}");
        assert_eq!(
            text.matches("codeflow ci: work.valid_graph:").count(),
            1,
            "{text}"
        );
    }
    // A record the schema refuses (no title) fails from either checkout.
    let record = epic_task("TSK-003").replace("title: work\n", "");
    write(root, &task_path("TSK-003"), &record);
    commit(root);
    for checkout in [EPIC_BRANCH, "main"] {
        let out = from(checkout, EPIC_BRANCH);
        let text = combined(&out);
        assert!(!out.status.success(), "{checkout}: {text}");
        assert!(text.contains("work.valid_graph"), "{checkout}: {text}");
        assert!(text.contains("title"), "{checkout}: {text}");
    }
    let worktrees = std::process::Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(root)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&worktrees.stdout)
            .matches("worktree ")
            .count(),
        1,
        "{}",
        String::from_utf8_lossy(&worktrees.stdout)
    );
}
