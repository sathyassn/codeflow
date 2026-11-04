//! TSK-1058: the guards refuse a `Monitor` input they do not recognise,
//! the refusal names the field, and `security.monitor_extra_fields` widens
//! the list only from the policy published on the remote's default branch.
use super::{codeflow, git, init_repo, run_with_stdin, write_policy};
use std::path::Path;
use std::process::Output;

const DEFAULT: &str =
    r#"{"git":{"protected_branches":["main","master"],"root_checkout_commits":"off"}}"#;
const WIDENED: &str = r#"{"git":{"protected_branches":["main","master"],"root_checkout_commits":"off"},"security":{"monitor_extra_fields":["script"]}}"#;

fn monitor(guard: &str, root: &Path, input: serde_json::Value) -> Output {
    let mut payload = serde_json::json!({
        "tool_name": "Monitor",
        "cwd": root,
    });
    payload["tool_input"] = input;
    run_with_stdin(
        codeflow()
            .args(["hook", guard, "--contract", "3"])
            .current_dir(root)
            .env_remove("CLAUDE_PROJECT_DIR"),
        &payload.to_string(),
    )
}

/// A checkout on `task/x` whose `origin/main` holds `published`.
fn published(root: &Path, published: &str) {
    init_repo(root, "main");
    write_policy(root, published);
    git(root, &["add", ".codeflow/policy.json"]);
    git(root, &["commit", "-m", "chore: policy"]);
    git(
        root,
        &["remote", "add", "origin", "file:///nonexistent/r.git"],
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

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn refused_as_unrecognised(output: &Output) -> bool {
    output.status.code() == Some(2) && stderr(output).contains("unrecognised Monitor payload")
}

#[test]
fn the_refusal_names_the_unknown_field_and_the_fix() {
    let dir = tempfile::tempdir().unwrap();
    published(dir.path(), DEFAULT);
    for guard in ["exec-guard", "git-guard"] {
        let out = monitor(
            guard,
            dir.path(),
            serde_json::json!({"command": "git status", "script": "echo hi"}),
        );
        assert!(refused_as_unrecognised(&out), "{guard}: {out:?}");
        let text = stderr(&out);
        assert!(text.contains("`script`"), "{guard}: {text}");
        assert!(text.contains("not a bad command"), "{guard}: {text}");
        assert!(
            text.contains("security.monitor_extra_fields"),
            "{guard}: {text}"
        );
    }
}

#[test]
fn the_policy_published_on_the_default_branch_widens_the_list() {
    let dir = tempfile::tempdir().unwrap();
    published(dir.path(), WIDENED);
    for guard in ["exec-guard", "git-guard"] {
        let listed = monitor(
            guard,
            dir.path(),
            serde_json::json!({"command": "git status", "script": "echo hi"}),
        );
        assert!(listed.status.success(), "{guard}: {listed:?}");
        // A field the policy does not list is still refused, by name.
        let other = monitor(
            guard,
            dir.path(),
            serde_json::json!({"command": "git status", "other": 1}),
        );
        assert!(refused_as_unrecognised(&other), "{guard}: {other:?}");
        assert!(stderr(&other).contains("`other`"), "{guard}");
    }
    // The command is still judged, whatever extra field rides with it.
    let sudo = monitor(
        "exec-guard",
        dir.path(),
        serde_json::json!({"command": "timeout 1 sudo true", "script": "x"}),
    );
    assert_eq!(sudo.status.code(), Some(2), "{sudo:?}");
    assert!(stderr(&sudo).contains("security.privilege_escalation"));
    // A widening never makes the input without a command or socket pass.
    let bare = monitor("exec-guard", dir.path(), serde_json::json!({"script": "x"}));
    assert!(refused_as_unrecognised(&bare), "{bare:?}");
}

#[test]
fn a_session_cannot_widen_the_list_itself() {
    let dir = tempfile::tempdir().unwrap();
    published(dir.path(), DEFAULT);
    let input = serde_json::json!({"command": "git status", "script": "echo hi"});
    // In the working copy only.
    write_policy(dir.path(), WIDENED);
    for guard in ["exec-guard", "git-guard"] {
        let out = monitor(guard, dir.path(), input.clone());
        assert!(
            refused_as_unrecognised(&out),
            "working copy, {guard}: {out:?}"
        );
    }
    // Committed on the session's own branch.
    git(dir.path(), &["add", ".codeflow/policy.json"]);
    git(dir.path(), &["commit", "-m", "chore: widen on a branch"]);
    for guard in ["exec-guard", "git-guard"] {
        let out = monitor(guard, dir.path(), input.clone());
        assert!(refused_as_unrecognised(&out), "branch, {guard}: {out:?}");
    }
    // A repository with no remote has no published policy to honour.
    let local = tempfile::tempdir().unwrap();
    init_repo(local.path(), "task/x");
    write_policy(local.path(), WIDENED);
    git(local.path(), &["add", ".codeflow/policy.json"]);
    git(local.path(), &["commit", "-m", "chore: widen at HEAD"]);
    let out = monitor("exec-guard", local.path(), input);
    assert!(refused_as_unrecognised(&out), "no remote: {out:?}");
}
