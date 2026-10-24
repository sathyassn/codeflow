//! TSK-414: preserve an unlanded cancellation without starting its task.
use super::{codeflow, combined, git, planned_task, run_in};
use std::io::Write;
use std::path::Path;
use std::process::{Output, Stdio};

const BRANCH: &str = "plan/record-cancellation";
const TASK: &str = "project-management/tasks/TSK-001.md";
const BODY: &str = "Task: TSK-001\n\n## Summary\nPreserve the cancellation.\n\n- Record the reason and scope.\n\n## Changes\n- Add the cancelled record.\n\n## Reviews\nNone: review follows.\n";

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn commit(root: &Path) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "docs: record cancellation"]);
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
    git(root, &["init", "-qb", "main"]);
    git(root, &["config", "user.email", "test@example.test"]);
    git(root, &["config", "user.name", "Test"]);
    write(
        root,
        ".codeflow/policy.json",
        r#"{"git":{"product_paths":["src/**"],"test_gate_on_push":"warn"}}"#,
    );
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
    let seeded = run_in(root, &["ids", "seed"]);
    assert!(seeded.status.success(), "{}", combined(&seeded));
    let remote = root.join(".git/test-remote.git");
    git(
        root,
        &["init", "--bare", "-qb", "main", remote.to_str().unwrap()],
    );
    git(root, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(root, &["push", "-q", "origin", "main", "codeflow/registry"]);
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

#[test]
fn cancelled_standalone_route_rejects_invalid_ranges() {
    for (status, reason, scope, extra, existing) in [
        ("todo", true, true, None, false),
        ("complete", true, true, None, false),
        ("cancelled", false, true, None, false),
        ("cancelled", true, false, None, false),
        ("cancelled", true, true, Some("src/unwanted.rs"), false),
        ("cancelled", true, true, Some("docs/unwanted.md"), false),
        ("cancelled", true, true, None, true),
    ] {
        let dir = fixture(status, reason, scope, extra, existing);
        for out in [ci(dir.path()), pre_push(dir.path())] {
            let text = combined(&out);
            assert!(!out.status.success(), "{status}, reason={reason}, scope={scope}, extra={extra:?}, existing={existing}: {text}");
            if status == "todo" {
                assert!(text.contains("task branch"), "{text}");
            }
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
