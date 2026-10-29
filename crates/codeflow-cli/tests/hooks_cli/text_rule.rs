//! TSK-397: the built exec guard applies the strict text rule by default;
//! `security.text_guard: relaxed` is honoured only from the protected
//! remote-tracking refs, and a retry flag keeps a relaxed call strict.
use super::{codeflow, git, init_repo, run_time_word_refusal, run_with_stdin, write_policy};
use std::path::Path;
use std::process::Output;

const RELAXED: &str = r#"{"git":{"protected_branches":["main","master"],"root_checkout_commits":"off"},"security":{"text_guard":"relaxed"}}"#;
const DEFAULT: &str =
    r#"{"git":{"protected_branches":["main","master"],"root_checkout_commits":"off"}}"#;
/// An everyday line the strict rule refuses by its run-time word and the
/// line's rule allows.
const EVERYDAY: &str = r#"echo "$HOME""#;

fn exec_guard(root: &Path, command: &str, retry: bool) -> Output {
    let mut input = serde_json::json!({ "command": command });
    if retry {
        input["dangerouslyDisableSandbox"] = serde_json::json!(true);
    }
    let payload = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": input,
        "cwd": root,
    });
    run_with_stdin(
        codeflow()
            .args(["hook", "exec-guard", "--contract", "3"])
            .current_dir(root)
            .env_remove("CLAUDE_PROJECT_DIR"),
        &payload.to_string(),
    )
}

/// A checkout on `task/x` whose `origin/main` holds `published` and whose
/// own `HEAD` and working copy hold `local`.
fn published(root: &Path, published: &str) {
    init_repo(root, "main");
    write_policy(root, published);
    git(root, &["add", ".codeflow/policy.json"]);
    git(root, &["commit", "-m", "chore: policy"]);
    git(
        root,
        &["remote", "add", "origin", "https://example.invalid/r.git"],
    );
    git(root, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    git(
        root,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    git(root, &["checkout", "-q", "-b", "task/x"]);
}

#[test]
fn the_strict_rule_is_the_default() {
    let dir = tempfile::tempdir().unwrap();
    published(dir.path(), DEFAULT);
    let out = exec_guard(dir.path(), EVERYDAY, false);
    assert!(run_time_word_refusal(&out), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("keeps the default"),
        "{out:?}"
    );
}

#[test]
fn relaxed_on_the_published_default_branch_is_honoured() {
    let dir = tempfile::tempdir().unwrap();
    published(dir.path(), RELAXED);
    let out = exec_guard(dir.path(), EVERYDAY, false);
    assert!(out.status.success(), "{out:?}");
}

#[test]
fn relaxed_in_the_worktree_or_on_a_branch_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    published(dir.path(), DEFAULT);
    write_policy(dir.path(), RELAXED);
    assert!(run_time_word_refusal(&exec_guard(
        dir.path(),
        EVERYDAY,
        false
    )));
    git(dir.path(), &["add", ".codeflow/policy.json"]);
    git(dir.path(), &["commit", "-m", "chore: relax on a branch"]);
    assert!(run_time_word_refusal(&exec_guard(
        dir.path(),
        EVERYDAY,
        false
    )));
}

#[test]
fn relaxed_without_a_remote_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "task/x");
    write_policy(dir.path(), RELAXED);
    git(dir.path(), &["add", ".codeflow/policy.json"]);
    git(dir.path(), &["commit", "-m", "chore: relax at HEAD"]);
    let out = exec_guard(dir.path(), EVERYDAY, false);
    assert!(run_time_word_refusal(&out), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("not honoured"),
        "{out:?}"
    );
}

#[test]
fn relaxed_keeps_a_retry_strict() {
    let dir = tempfile::tempdir().unwrap();
    published(dir.path(), RELAXED);
    let out = exec_guard(dir.path(), EVERYDAY, true);
    assert!(run_time_word_refusal(&out), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("dangerouslyDisableSandbox"),
        "{out:?}"
    );
}
