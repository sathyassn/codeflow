//! Journey (TSK-105 AC-7): on a fresh `codeflow init --full` project, a task
//! completed with a valid acceptance block passes `task status complete` and
//! `codeflow ci`, and each fault fails for its stated reason: a stale block
//! after a later code change, a waiver without its planning amendment,
//! criteria changed on the task branch, and an adopter-facing range whose
//! task has no journey. The binary under test is the one Cargo built; the
//! installed `codeflow` on `PATH` is never used.

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

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn ok(out: &Output, what: &str) -> String {
    assert!(out.status.success(), "{what} failed:\n{}", text(out));
    text(out)
}

fn fails(out: &Output, what: &str, reason: &str) {
    assert_eq!(out.status.code(), Some(1), "{what}:\n{}", text(out));
    assert!(
        text(out).contains(reason),
        "{what}: missing {reason:?} in\n{}",
        text(out)
    );
}

fn commit(root: &Path, message: &str) -> String {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", message]);
    git(root, &["rev-parse", "HEAD"])
}

fn edit(root: &Path, relative: &str, from: &str, to: &str) {
    let path = root.join(relative);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(from), "{relative} lacks {from:?}");
    std::fs::write(path, text.replacen(from, to, 1)).unwrap();
}

const TASK: &str = "project-management/tasks/TSK-001.md";
const SECOND: &str = "project-management/tasks/TSK-002.md";
const EPIC: &str = "project-management/epics/EPC-001.md";
const LINE: &str = "integration/EPC-001-journey";
const SCOPE: &str = "the acceptance check proves structure and binding only";

/// An acceptance block for TSK-001 reviewed at `reviewed`, with AC-1's
/// outcome given.
fn acceptance(dir: &Path, reviewed: &str, ac1: &str) -> String {
    let body = format!(
        "acceptance:\n  reviewed: {reviewed}\n  review: session:journey@sha256:00\n  criteria:\n    AC-1: {ac1}\n    AC-2: verified | the journey ran\n  journey: verified | crates/codeflow-cli/tests/acceptance_journey.rs\n  not_verified: none\n  follow_ups: none: journey fixture\n  verdict: approved\n"
    );
    let path = dir.join("acceptance.yaml");
    std::fs::write(&path, body).unwrap();
    path.to_string_lossy().to_string()
}

/// The project's installed pull request template, filled for `task`.
fn body(root: &Path, task: &str) -> String {
    std::fs::read_to_string(root.join(".github/pull_request_template.md"))
        .unwrap()
        .replace("Task: `TSK-NNN | none: <reason>`", &format!("Task: {task}"))
        .replace("## Summary\n", "## Summary\n\nBuild the task.\n")
        .replace("\n-\n", "\n- Build the task.\n")
        .replace(
            "- Revision and command:",
            "- Revision and command: HEAD, cargo test",
        )
        .replace(
            "(paste the real test summary output here)",
            "Ran 1 test\nOK",
        )
        .replace("- Coverage:", "- Coverage: not measured for this journey")
        .replace("- New tests:", "- New tests: none")
        .replace("- Not tested:", "- Not tested: Windows")
        .replace("|  |  |  |", "| Reviewer | HEAD | approved |")
        .replace(
            "- Impact: `none | patch | minor | major`",
            "- Impact: minor",
        )
        .replace("- Breaking: `yes | no`", "- Breaking: no")
        .replace("- Rationale:", "- Rationale: a new command.")
        .replace(
            "- Migration: `none`, steps, or \"see Breaking change\"",
            "- Migration: none",
        )
}

/// `codeflow ci` on the range from the line, as the pull request job runs
/// it, with the installed template filled for `task`.
fn ci(root: &Path, branch: &str, task: &str) -> Output {
    let body = body(root, task);
    codeflow(
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
#[allow(clippy::too_many_lines)] // One journey keeps the faults in the order a project meets them.
fn a_completion_is_bound_to_the_reviewed_commit_on_a_fresh_project() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    ok(
        &codeflow(&root, &["init", "--yes", "--full"]),
        "init --full",
    );
    git(&root, &["switch", "-q", "-c", LINE]);

    // Plan: an epic and two tasks; the first has a journey criterion, the
    // second has none. `src/**` is product code.
    git(&root, &["switch", "-q", "-c", "plan/acceptance"]);
    ok(&codeflow(&root, &["epic", "new", "outcome"]), "epic new");
    for title in ["first", "second"] {
        let args = ["task", "new", "--epic", "EPC-001", "--into", LINE, title];
        ok(&codeflow(&root, &args), "task new");
    }
    edit(
        &root,
        EPIC,
        "- AC-1\n",
        "- AC-1 When used, the system shall work.\n",
    );
    edit(
        &root,
        TASK,
        "- AC-1\n",
        "- AC-1 When run, the system shall work.\n- AC-2 (journey) On a fresh project, the command shall succeed.\n",
    );
    edit(
        &root,
        SECOND,
        "- AC-1\n",
        "- AC-1 When run, the system shall work.\n",
    );
    let policy_path = root.join(".codeflow/policy.json");
    let mut policy: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&policy_path).unwrap()).unwrap();
    policy["git"]["product_paths"] = serde_json::json!(["src/**"]);
    std::fs::write(
        &policy_path,
        serde_json::to_string_pretty(&policy).unwrap() + "\n",
    )
    .unwrap();
    commit(&root, "chore: plan the acceptance journey");
    git(&root, &["switch", "-q", LINE]);
    git(
        &root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "plan/acceptance",
            "-m",
            "chore: land the plan",
        ],
    );
    let planned = git(&root, &["rev-parse", "HEAD"]);

    // A planning amendment on the line narrows TSK-001 AC-1.
    git(&root, &["switch", "-q", "-c", "plan/narrow"]);
    edit(&root, TASK, "shall work.\n", "shall work on Linux.\n");
    let amendment = commit(&root, "chore: narrow TSK-001 AC-1");
    git(&root, &["switch", "-q", LINE]);
    git(
        &root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "plan/narrow",
            "-m",
            "chore: land the amendment",
        ],
    );

    // The task: build, review, complete with a valid block. CI passes and
    // states what it proves.
    let branch = "task/TSK-001-first";
    git(&root, &["switch", "-q", "-c", branch]);
    ok(
        &codeflow(&root, &["work", "start", "TSK-001"]),
        "work start",
    );
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn first() {}\n").unwrap();
    let reviewed = commit(&root, "feat: add the first function");
    let valid = acceptance(dir.path(), &reviewed, &format!("waived | {amendment}"));
    ok(
        &codeflow(
            &root,
            &[
                "task",
                "status",
                "TSK-001",
                "complete",
                "--acceptance",
                &valid,
            ],
        ),
        "task status complete with a valid block",
    );
    let completed = commit(&root, "chore: complete TSK-001");
    let passed = ok(&ci(&root, branch, "TSK-001"), "ci on the valid completion");
    assert!(passed.contains(SCOPE), "{passed}");

    // Fault: a later code change makes the block stale.
    std::fs::write(root.join("src/lib.rs"), "pub fn changed() {}\n").unwrap();
    commit(&root, "feat: change after review");
    fails(
        &ci(&root, branch, "TSK-001"),
        "a stale block after a later code change",
        "src/lib.rs changed after the reviewed commit",
    );
    git(&root, &["reset", "-q", "--hard", &completed]);

    // Fault: a waiver naming a commit that is not the amendment on the
    // target; the verb refuses before it writes.
    git(&root, &["reset", "-q", "--hard", &reviewed]);
    // The commit that landed the plan is on the target, but it also changed
    // the policy, so it is not a planning amendment (a target commit that
    // changes records only but not AC-1 is covered in `acceptance_cli`).
    let unamended = acceptance(dir.path(), &reviewed, &format!("waived | {planned}"));
    let refused = codeflow(
        &root,
        &[
            "task",
            "status",
            "TSK-001",
            "complete",
            "--acceptance",
            &unamended,
        ],
    );
    fails(
        &refused,
        "a waiver without its amendment",
        "which also changes .codeflow/policy.json; a planning amendment changes planning records only",
    );
    assert!(text(&refused).contains(SCOPE), "{}", text(&refused));
    let branch_only = acceptance(dir.path(), &reviewed, &format!("waived | {reviewed}"));
    fails(
        &codeflow(
            &root,
            &[
                "task",
                "status",
                "TSK-001",
                "complete",
                "--acceptance",
                &branch_only,
            ],
        ),
        "a waiver naming a branch commit",
        "names the pull request head; a waiver is a planning amendment on the target",
    );
    let record = std::fs::read_to_string(root.join(TASK)).unwrap();
    assert!(!record.contains("\nstatus: complete"), "{record}");

    // Fault: the task branch changes its record's criteria.
    edit(
        &root,
        TASK,
        "the command shall succeed",
        "the command may succeed",
    );
    commit(&root, "chore: loosen AC-2");
    fails(
        &ci(&root, branch, "TSK-001"),
        "criteria changed on the task branch",
        "TSK-001 changes its criteria on this branch",
    );

    // Fault: an adopter-facing range whose task has no journey.
    git(&root, &["switch", "-q", "-c", "task/TSK-002-second", LINE]);
    ok(
        &codeflow(&root, &["work", "start", "TSK-002"]),
        "work start",
    );
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/second.rs"), "pub fn second() {}\n").unwrap();
    commit(&root, "feat: add the second function");
    fails(
        &ci(&root, "task/TSK-002-second", "TSK-002"),
        "an adopter-facing range without a journey",
        "TSK-002 changes the adopter-facing path set but has no `(journey)` criterion",
    );
}
