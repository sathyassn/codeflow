//! Merge mode preserves a reviewed head through the installed hook plane.

use std::path::Path;
use std::process::{Command, Output};

fn run(root: &Path, program: &str, args: &[&str], home: &Path, agent: bool) -> Output {
    Command::new(program)
        .args(args)
        .current_dir(root)
        .env("CODEFLOW_HOME", home)
        .env("CODEFLOW_HOOK_BINARY", env!("CARGO_BIN_EXE_codeflow"))
        .env("CLAUDECODE", if agent { "1" } else { "" })
        .env("CODEX_SESSION_ID", "")
        .env("CODEX_THREAD_ID", "")
        .env("GROK_SESSION_ID", "")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.test")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.test")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE")
        .output()
        .unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

const MERGE_ARGS: [&str; 5] = [
    "integrate",
    "feat/reviewed",
    "--into",
    "integration/landing",
    "--merge",
];

fn assert_root_refusal(root: &Path, home: &Path) {
    let refused = run(
        root,
        env!("CARGO_BIN_EXE_codeflow"),
        &MERGE_ARGS,
        home,
        true,
    );
    let said = text(&refused);
    assert!(!refused.status.success(), "{said}");
    assert!(
        said.contains("preflight:")
            && said.contains("root checkout")
            && said.contains("linked worktree"),
        "{said}"
    );
    assert!(!said.contains("resolve on the source branch"), "{said}");
    assert!(!root.join(".git/MERGE_HEAD").exists());
    // Prove the installed hooks see an agent at block level, rather than
    // inheriting Cargo's intentionally blanked harness marker.
    let switch = run(root, "git", &["checkout", "feat/reviewed"], home, false);
    assert!(switch.status.success(), "{}", text(&switch));
    let blocked = run(
        root,
        "git",
        &["commit", "--allow-empty", "-m", "test: prove agent block"],
        home,
        true,
    );
    assert!(!blocked.status.success());
    assert!(
        text(&blocked).contains("git.root_checkout_commits (block)"),
        "{}",
        text(&blocked)
    );
    let switch = run(
        root,
        "git",
        &["checkout", "integration/landing"],
        home,
        false,
    );
    assert!(switch.status.success(), "{}", text(&switch));
}

#[test]
fn hooked_merge_refuses_root_and_lands_from_a_linked_worktree() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let home = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let output = run(root, "git", args, home.path(), false);
        assert!(output.status.success(), "git {args:?}: {}", text(&output));
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    };
    let init = run(
        root,
        env!("CARGO_BIN_EXE_codeflow"),
        &["init", "--yes", "--full"],
        home.path(),
        false,
    );
    assert!(init.status.success(), "{}", text(&init));
    assert!(root.join(".codeflow/git-hooks/pre-merge-commit").is_file());
    let policy: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join(".codeflow/policy.json")).unwrap())
            .unwrap();
    assert_eq!(policy["git"]["root_checkout_commits"], "block");
    git(&["checkout", "-b", "integration/landing"]);
    git(&["checkout", "-b", "feat/reviewed"]);
    std::fs::write(root.join("feature.txt"), "reviewed\n").unwrap();
    git(&["add", "feature.txt"]);
    git(&["commit", "-m", "feat: add reviewed feature"]);
    let reviewed = git(&["rev-parse", "HEAD"]);
    git(&["checkout", "integration/landing"]);
    std::fs::write(root.join("line.txt"), "line\n").unwrap();
    git(&["add", "line.txt"]);
    git(&["commit", "-m", "fix: advance the line"]);
    let line = git(&["rev-parse", "HEAD"]);
    assert_root_refusal(root, home.path());
    assert_eq!(git(&["rev-parse", "integration/landing"]), line);
    assert_eq!(git(&["rev-parse", "feat/reviewed"]), reviewed);
    assert_eq!(git(&["branch", "--show-current"]), "integration/landing");
    let linked = root.join(".worktrees/reviewed");
    git(&["worktree", "add", linked.to_str().unwrap(), "feat/reviewed"]);
    std::fs::write(linked.join(".codeflow/test-config.json"), r#"{"schema_version":"1.0","targets":[{"name":"merged-files","runner":"custom","modes":{"full":{"command":"git ls-files --error-unmatch feature.txt line.txt"}}}]}"#).unwrap();
    let output = run(
        &linked,
        env!("CARGO_BIN_EXE_codeflow"),
        &MERGE_ARGS,
        home.path(),
        true,
    );
    assert!(output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("test gate: passed"));
    assert_eq!(git(&["rev-parse", "integration/landing^1"]), line);
    assert_eq!(git(&["rev-parse", "integration/landing^2"]), reviewed);
    assert_eq!(
        git(&["log", "-1", "--format=%s", "integration/landing"]),
        "Merge branch 'feat/reviewed' into integration/landing"
    );
    assert_eq!(git(&["rev-parse", "feat/reviewed"]), reviewed);
    assert_eq!(git(&["branch", "--show-current"]), "integration/landing");
    assert!(root.join("feature.txt").exists());
    assert!(git(&["status", "--porcelain", "--untracked-files=no"]).is_empty());
}
