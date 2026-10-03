//! Journey (TSK-104 AC-6): on a fresh `codeflow init --full` project, a new
//! epic line, a leaf task, a follow-up, an unnamed fix refusal and a
//! new ADR each take one command and at most one ordinary pull request, and
//! `codeflow ci` classifies each pull request from the project's own paths.
//! The binary under test is the one Cargo built; hooks installed by `init`
//! resolve `codeflow` to it through `PATH`.

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
        "## Summary\nA bounded change.\n\n{task_line}\n\n## Changes\n- one change\n\n## Testing\n- journey step\n\n## Reviews\n\n| Reviewer | Scope | Verdict |\n|---|---|---|\n| journey | this range | approve |\n\n## Release impact\n\n- Impact: patch\n- Breaking: no\n- Rationale: journey fixture.\n- Migration: none\n"
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
}

#[test]
#[allow(clippy::too_many_lines)] // One journey keeps the light paths in the order a project lives them.
fn each_light_path_takes_one_command_and_one_pull_request() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    ok(
        &codeflow(&root, &["init", "--yes", "--full"]),
        "init --full",
    );
    let policy = std::fs::read_to_string(root.join(".codeflow/policy.json")).unwrap();
    assert!(
        policy.contains("\"product_paths\": [\"src/**\""),
        "init writes the project-owned product paths: {policy}"
    );

    // An epic and its integration line: one command.
    git(&root, &["switch", "-q", "-c", "plan/outcome"]);
    let epic = ok(
        &codeflow(&root, &["epic", "new", "--integration", "Outcome"]),
        "epic new --integration",
    );
    assert!(
        epic.contains("integration/EPC-001-outcome  from "),
        "{epic}"
    );
    let line = "integration/EPC-001-outcome";
    git(&root, &["rev-parse", "--verify", line]);

    // A leaf task: one command, in the same planning pull request.
    ok(
        &codeflow(
            &root,
            &["task", "new", "--epic", "EPC-001", "--into", line, "Leaf"],
        ),
        "task new",
    );
    edit(
        &root,
        "project-management/epics/EPC-001.md",
        "- AC-1\n",
        "- AC-1 When run, the system shall work.\n",
    );
    // The leaf changes product code, so it names its journey (TSK-105).
    edit(
        &root,
        "project-management/tasks/TSK-001.md",
        "- AC-1\n",
        "- AC-1 (journey) When run, the system shall work.\n",
    );
    commit(&root, "chore: plan the outcome");
    let (code, out) = pull_request(&root, "plan/outcome", line, "Task: EPC-001");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("class: planning-only"), "{out}");
    land(&root, line, "plan/outcome");

    // Build the leaf: the pull request names the task carried by its branch.
    git(&root, &["switch", "-q", "-c", "task/TSK-001-leaf"]);
    ok(
        &codeflow(&root, &["work", "start", "TSK-001"]),
        "work start",
    );
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/leaf.rs"), "pub fn leaf() {}\n").unwrap();
    commit(&root, "feat: add the leaf");
    let (code, out) = pull_request(&root, "task/TSK-001-leaf", line, "Task: TSK-001");
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("class: tracked TSK-001 (from the Task: line)"),
        "{out}"
    );
    land(&root, line, "task/TSK-001-leaf");

    // A follow-up of an epic task: refused on a branch that is not a plan/ branch, one command on a plan/ branch.
    let refused = codeflow(&root, &["task", "new", "--follow-up-of", "TSK-001", "Tidy"]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("plan/ branch"));
    git(&root, &["switch", "-q", "-c", "plan/tidy"]);
    let follow = ok(
        &codeflow(&root, &["task", "new", "--follow-up-of", "TSK-001", "Tidy"]),
        "task new --follow-up-of",
    );
    assert!(follow.contains("TSK-002"), "{follow}");
    let record = std::fs::read_to_string(root.join("project-management/tasks/TSK-002.md")).unwrap();
    assert!(record.contains("\nfollow_up_of: TSK-001 "), "{record}");
    assert!(record.contains("\nepic_id: EPC-001 "), "{record}");
    assert!(
        record.contains(&format!("integration_target: \"{line}\"")),
        "{record}"
    );
    edit(
        &root,
        "project-management/tasks/TSK-002.md",
        "- AC-1\n",
        "- AC-1 When run, the system shall be tidy.\n",
    );
    commit(&root, "chore: file the follow-up");
    let (code, out) = pull_request(&root, "plan/tidy", line, "Task: EPC-001");
    assert_eq!(code, 0, "{out}");
    land(&root, line, "plan/tidy");

    // An unnamed fix is refused on docs and product code alike.
    git(&root, &["switch", "-q", "-c", "docs/typo"]);
    std::fs::write(root.join("README.md"), "# Project\n\nFixed a typo.\n").unwrap();
    commit(&root, "docs: fix a typo");
    let (code, out) = pull_request(&root, "docs/typo", line, "Task: none: fix a typo");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("is neither"), "{out}");
    std::fs::write(root.join("src/leaf.rs"), "pub fn leaf() { }\n").unwrap();
    commit(&root, "fix: tweak the leaf");
    let (code, out) = pull_request(&root, "docs/typo", line, "Task: none: fix a typo");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("is neither"), "{out}");

    // A new ADR supports the named follow-up task in one pull request.
    git(&root, &["switch", "-q", "-c", "docs/adr", line]);
    let adr = ok(
        &codeflow(&root, &["adr", "new", "Adopt a cache: keep it small"]),
        "adr new",
    );
    let path = adr.split_whitespace().nth(1).unwrap().to_string();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\nstatus: proposed "), "{text}");
    assert!(
        text.contains("\ntitle: \"Adopt a cache: keep it small\"\n"),
        "{text}"
    );
    assert!(text.contains(": Adopt a cache: keep it small\n"), "{text}");
    // Numbered by the project's registry and bound to its uid (TSK-101).
    let uid_line = text.lines().find(|line| line.starts_with("uid: ")).unwrap();
    let bound = git(&root, &["show", "codeflow/registry:ids/ADR/0002.toml"]);
    assert!(
        bound.contains(&format!("uid = \"{}\"", &uid_line[5..])),
        "{bound}"
    );
    ok(
        &codeflow(&root, &["validate", "--docs"]),
        "validate --docs after adr new",
    );
    commit(&root, "docs: propose the cache decision");
    let (code, out) = pull_request(&root, "docs/adr", line, "Task: TSK-002");
    assert_eq!(code, 0, "{out}");
}

/// Complete `id` on the current branch: an acceptance block reviewed at
/// HEAD, written by `task status`, then committed.
fn complete(root: &Path, scratch: &Path, id: &str) {
    let reviewed = git(root, &["rev-parse", "HEAD"]);
    let evidence = scratch.join(format!("{id}-acceptance.yaml"));
    std::fs::write(
        &evidence,
        format!(
            "acceptance:\n  reviewed: {reviewed}\n  review: session:journey@sha256:00\n  criteria:\n    AC-1: verified | journey step\n  journey: verified | light paths journey\n  not_verified: none\n  follow_ups: none: journey fixture\n  verdict: approved\n"
        ),
    )
    .unwrap();
    ok(
        &codeflow(
            root,
            &[
                "task",
                "status",
                id,
                "complete",
                "--acceptance",
                evidence.to_str().unwrap(),
            ],
        ),
        "task status complete",
    );
    commit(root, &format!("docs(tasks): complete {id}"));
}

/// Journey (TSK-214 AC-1, sathyassn/codeflow#21): a follow-up of a
/// standalone task is itself a standalone task. Filed on a task branch cut
/// from the target, its record lands with its work in one tracked pull
/// request, as its source did; on a `plan/` branch, where no epic exists to
/// name, the command refuses and names that route.
#[test]
#[allow(clippy::too_many_lines)] // One journey keeps the source and its follow-up in the order a project lives them.
fn a_follow_up_of_a_standalone_task_lands_with_its_work() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    ok(
        &codeflow(&root, &["init", "--yes", "--full"]),
        "init --full",
    );
    let main = git(&root, &["branch", "--show-current"]);

    // The standalone source: its record and code in one pull request.
    git(&root, &["switch", "-q", "-c", "task/source"]);
    ok(
        &codeflow(
            &root,
            &[
                "task",
                "new",
                "--standalone-reason",
                "one bounded outcome",
                "--into",
                &main,
                "Source",
            ],
        ),
        "task new --standalone-reason",
    );
    edit(
        &root,
        "project-management/tasks/TSK-001.md",
        "- AC-1\n",
        "- AC-1 (journey) When run, the system shall work.\n",
    );
    commit(&root, "docs(tasks): plan the source");
    ok(
        &codeflow(&root, &["work", "claim", "TSK-001"]),
        "work claim",
    );
    let source = git(&root, &["branch", "--show-current"]);
    assert!(source.starts_with("task/TSK-001-"), "{source}");
    ok(
        &codeflow(&root, &["work", "start", "TSK-001"]),
        "work start",
    );
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/source.rs"), "pub fn source() {}\n").unwrap();
    commit(&root, "feat: add the source");
    complete(&root, dir.path(), "TSK-001");
    let (code, out) = pull_request(&root, &source, &main, "Task: TSK-001");
    assert_eq!(code, 0, "{out}");
    // The hosted merge of that pull request: the installed hooks refuse a
    // merge commit on a protected branch, so only this step runs without them.
    git(&root, &["switch", "-q", &main]);
    git(
        &root,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "merge",
            "-q",
            "--no-ff",
            &source,
            "-m",
            "chore: land the source",
        ],
    );

    // A standalone source: file the follow-up on a task branch, commit its record, then claim it.
    git(&root, &["switch", "-q", "-c", "task/tidy", &main]);
    let follow = ok(
        &codeflow(&root, &["task", "new", "--follow-up-of", "TSK-001", "Tidy"]),
        "task new --follow-up-of a standalone task",
    );
    assert!(follow.contains("TSK-002"), "{follow}");
    let record = std::fs::read_to_string(root.join("project-management/tasks/TSK-002.md")).unwrap();
    assert!(record.contains("\nfollow_up_of: TSK-001 "), "{record}");
    assert!(record.contains("\nepic_id: null "), "{record}");
    assert!(
        record.contains("standalone_reason: \"follow-up of standalone task TSK-001\""),
        "{record}"
    );
    assert!(
        record.contains(&format!("integration_target: \"{main}\"")),
        "{record}"
    );
    edit(
        &root,
        "project-management/tasks/TSK-002.md",
        "- AC-1\n",
        "- AC-1 (journey) When run, the system shall be tidy.\n",
    );
    commit(&root, "docs(tasks): file the follow-up");
    ok(
        &codeflow(&root, &["work", "claim", "TSK-002"]),
        "work claim",
    );
    let branch = git(&root, &["branch", "--show-current"]);
    assert!(branch.starts_with("task/TSK-002-"), "{branch}");
    ok(
        &codeflow(&root, &["work", "start", "TSK-002"]),
        "work start",
    );
    std::fs::write(root.join("src/source.rs"), "pub fn source() { }\n").unwrap();
    commit(&root, "fix: tidy the source");
    complete(&root, dir.path(), "TSK-002");
    let (code, out) = pull_request(&root, &branch, &main, "Task: TSK-002");
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("class: tracked TSK-002 (from the Task: line)"),
        "{out}"
    );

    // On a planning branch there is no epic to name: refused, with the route.
    git(&root, &["switch", "-q", "-c", "plan/tidy", &main]);
    let refused = codeflow(&root, &["task", "new", "--follow-up-of", "TSK-001", "More"]);
    assert_eq!(refused.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(stderr.contains("its own task branch"), "{stderr}");
    assert!(
        !root.join("project-management/tasks/TSK-003.md").exists(),
        "a refused follow-up writes no record"
    );
}
