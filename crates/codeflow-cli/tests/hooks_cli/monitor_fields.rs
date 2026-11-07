//! TSK-1058: the guards refuse a `Monitor` input they do not recognise,
//! the refusal names the field, and `security.monitor_extra_fields` widens
//! the list only from the policy published on the remote's default branch.
use super::{codeflow, enforced_repo, git, init_repo, run_with_stdin, shell_path, write_policy};
use std::path::Path;
use std::process::Output;

const DEFAULT: &str =
    r#"{"git":{"protected_branches":["main","master"],"root_checkout_commits":"off"}}"#;
const WIDENED: &str = r#"{"git":{"protected_branches":["main","master"],"root_checkout_commits":"off"},"security":{"monitor_extra_fields":["script"]}}"#;

fn monitor(guard: &str, root: &Path, input: serde_json::Value) -> Output {
    call(guard, root, "Monitor", input)
}

fn call(guard: &str, cwd: &Path, tool: &str, input: serde_json::Value) -> Output {
    let mut payload = serde_json::json!({
        "tool_name": tool,
        "cwd": cwd,
    });
    payload["tool_input"] = input;
    run_with_stdin(
        codeflow()
            .args(["hook", guard, "--contract", "3"])
            .current_dir(cwd)
            .env_remove("CLAUDE_PROJECT_DIR"),
        &payload.to_string(),
    )
}

/// Each command passes as `Bash` through an allowance a guard grants before
/// it reads the policy, and the same command in a `Monitor` input with an
/// unlisted field is refused as unrecognised. That holds only while the
/// `Monitor` check runs before the allowance, so this pins the order. Each
/// case judged otherwise is added to `wrong`.
fn allowance_precedes(cwd: &Path, commands: &[&str], context: &str, wrong: &mut Vec<String>) {
    for command in commands {
        for guard in ["exec-guard", "git-guard"] {
            let bash = call(guard, cwd, "Bash", serde_json::json!({"command": command}));
            if !bash.status.success() {
                wrong.push(format!(
                    "{context}, {guard}, Bash `{command}` must reach its allowance: {bash:?}"
                ));
            }
            let out = monitor(
                guard,
                cwd,
                serde_json::json!({"command": command, "script": "echo hi"}),
            );
            if !refused_as_unrecognised(&out) {
                wrong.push(format!("{context}, {guard}, Monitor `{command}`: {out:?}"));
            }
        }
    }
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

/// The `Monitor` check comes before every allowance either guard grants
/// while the checkout or the policy authority cannot be read: the recovery
/// fetch and read-only git under an unresolved authority, and the worktree
/// recovery under an unreadable checkout identity.
#[test]
fn the_check_runs_before_every_recovery_allowance() {
    // Unresolved authority: a remote with no tracking refs that cannot be
    // asked whether it holds any.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "task/x");
    git(
        dir.path(),
        &["remote", "add", "origin", "file:///nonexistent/r.git"],
    );
    let mut wrong = Vec::new();
    allowance_precedes(
        dir.path(),
        &["git fetch origin", "git status"],
        "unresolved authority",
        &mut wrong,
    );

    // Unreadable checkout identity: a linked worktree's registration names
    // another repository, so the project root cannot be read.
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    enforced_repo(&root);
    let linked = root.join(".worktrees/w");
    git(
        &root,
        &["worktree", "add", "-b", "task/w", &shell_path(&linked)],
    );
    let other = temp.path().join("other");
    std::fs::create_dir(&other).unwrap();
    git(&other, &["init", "-b", "main"]);
    std::fs::write(
        root.join(".git/worktrees/w/gitdir"),
        format!("{}\n", other.join(".git").display()),
    )
    .unwrap();
    for cwd in [&root, &linked] {
        allowance_precedes(
            cwd,
            &["git worktree prune"],
            "unreadable checkout",
            &mut wrong,
        );
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
