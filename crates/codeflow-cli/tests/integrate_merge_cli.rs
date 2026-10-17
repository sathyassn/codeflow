//! Merge mode preserves the reviewed head on a diverged line.

use std::process::Command;

#[test]
fn merge_lands_the_unchanged_head_as_second_parent() {
    let dir = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.test")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.test")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    };
    git(&["init", "-b", "main"]);
    git(&["commit", "--allow-empty", "-m", "chore: initial commit"]);
    git(&["checkout", "-b", "feat/reviewed"]);
    std::fs::write(dir.path().join("feature.txt"), "reviewed\n").unwrap();
    git(&["add", "feature.txt"]);
    git(&["commit", "-m", "feat: add reviewed feature"]);
    let reviewed = git(&["rev-parse", "HEAD"]);
    git(&["checkout", "main"]);
    std::fs::write(dir.path().join("line.txt"), "line\n").unwrap();
    git(&["add", "line.txt"]);
    git(&["commit", "-m", "fix: advance the line"]);
    let line = git(&["rev-parse", "HEAD"]);
    std::fs::create_dir(dir.path().join(".codeflow")).unwrap();
    std::fs::write(
        dir.path().join(".codeflow/test-config.json"),
        r#"{"schema_version":"1.0","targets":[{"name":"merged-files","runner":"custom","modes":{"full":{"command":"git ls-files --error-unmatch feature.txt line.txt"}}}]}"#,
    )
    .unwrap();
    let home = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .args(["integrate", "feat/reviewed", "--into", "main", "--merge"])
        .current_dir(dir.path())
        .env("CODEFLOW_HOME", home.path())
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.test")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.test")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("test gate: passed"));
    assert_eq!(git(&["rev-parse", "main^1"]), line);
    assert_eq!(git(&["rev-parse", "main^2"]), reviewed);
    assert_eq!(git(&["rev-parse", "feat/reviewed"]), reviewed);
    assert_eq!(git(&["branch", "--show-current"]), "main");
    assert!(git(&["status", "--porcelain", "--untracked-files=no"]).is_empty());
}
