//! Journey (TSK-104 AC-6): on a fresh `codeflow init --full` project, a new
//! epic line, a leaf task, a follow-up, a small fix with `Task: none` and a
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
    for file in [
        "project-management/epics/EPC-001.md",
        "project-management/tasks/TSK-001.md",
    ] {
        edit(
            &root,
            file,
            "- AC-1\n",
            "- AC-1 When run, the system shall work.\n",
        );
    }
    commit(&root, "chore: plan the outcome");
    let (code, out) = pull_request(&root, "plan/outcome", line, "");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("class: planning-only"), "{out}");
    land(&root, line, "plan/outcome");

    // Build the leaf: the branch carries the task, so the pull request needs
    // no Task line.
    git(&root, &["switch", "-q", "-c", "task/TSK-001-leaf"]);
    ok(
        &codeflow(&root, &["work", "start", "TSK-001"]),
        "work start",
    );
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/leaf.rs"), "pub fn leaf() {}\n").unwrap();
    commit(&root, "feat: add the leaf");
    let (code, out) = pull_request(&root, "task/TSK-001-leaf", line, "");
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("class: tracked TSK-001 (from the branch)"),
        "{out}"
    );
    land(&root, line, "task/TSK-001-leaf");

    // A follow-up: refused off a planning branch, one command on one.
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
    let (code, out) = pull_request(&root, "plan/tidy", line, "");
    assert_eq!(code, 0, "{out}");
    land(&root, line, "plan/tidy");

    // A small fix: `Task: none` passes on docs, and blocks on product code.
    git(&root, &["switch", "-q", "-c", "docs/typo"]);
    std::fs::write(root.join("README.md"), "# Project\n\nFixed a typo.\n").unwrap();
    commit(&root, "docs: fix a typo");
    let (code, out) = pull_request(&root, "docs/typo", line, "Task: none: fix a typo");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("class: direct change"), "{out}");
    std::fs::write(root.join("src/leaf.rs"), "pub fn leaf() { }\n").unwrap();
    commit(&root, "fix: tweak the leaf");
    let (code, out) = pull_request(&root, "docs/typo", line, "Task: none: fix a typo");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("src/leaf.rs (product_paths)"), "{out}");

    // A new ADR: one command, proposed, one direct pull request.
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
    let (code, out) = pull_request(&root, "docs/adr", line, "Task: none: propose a decision");
    assert_eq!(code, 0, "{out}");
}
