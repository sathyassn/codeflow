//! TSK-1349: both guards refuse every git option that skips a client hook,
//! on a protected and an unprotected branch, in a linked worktree and with
//! `git -C`, through every wrapper they unwrap. Commands are judged only:
//! each is fed to the hook as a payload and never run.
use super::{codeflow, git, init_repo, run_with_stdin, write_policy};
use std::path::Path;
use std::process::Output;

const POLICY: &str =
    r#"{"git":{"protected_branches":["main","master"],"root_checkout_commits":"off"}}"#;

/// The skip forms, each written as the words after `git`.
const SKIPS: &[&str] = &[
    "commit --no-verify -m x",
    "commit -n -m x",
    "commit --no-verif -m x",
    "commit -qn -m x",
    "commit -anm msg",
    "commit -m x -n",
    "commit -m -- -n",
    "commit --no-verify --verify -m x",
    "commit --verify -n -m x",
    "commit --amend --no-post-rewrite --no-edit",
    "merge --no-verify topic",
    "merge --no-veri topic",
    "pull --no-verify",
    "push --no-verify origin HEAD",
    "push --no-v origin HEAD",
    "rebase --no-verify main",
    "am --no-verify a.patch",
    "am -n a.patch",
    "am -3n a.patch",
    "p4 submit --no-verify",
    "-c core.hooksPath=/dev/null commit -m x",
    "-c \"alias.c=commit --no-verify\" c -m x",
];

/// Forms that skip nothing: `-n` where it means another option, an option
/// value or operand that reads like the flag, and the options that only look
/// alike.
const NOT_SKIPS: &[&str] = &[
    "push -n origin HEAD",
    "merge -n topic",
    "pull -n",
    "rebase -n main",
    "commit -m --no-verify",
    "commit -m x -- --no-verify",
    "commit --verify -m x",
    "commit --n -m x",
    "merge --no-verify-signatures topic",
    "am -m a.patch",
];

/// The wrappers the guards unwrap, `{}` standing for the git call.
const WRAPPERS: &[&str] = &[
    "sh -c '{}'",
    "bash -c '{}'",
    "env FOO=1 {}",
    "true && {}",
    "true; {}",
    "({})",
    "echo ok; {} && echo done",
];

/// The skip forms judged in every place, one per kind of reading.
const IN_EVERY_PLACE: &[&str] = &[
    "commit --no-verify -m x",
    "commit -qn -m x",
    "merge --no-verif topic",
    "push --no-verify origin HEAD",
    "am -n a.patch",
];

fn hook(hook: &str, root: &Path, command: &str) -> Output {
    let payload = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": { "command": command },
        "cwd": root,
    });
    run_with_stdin(
        codeflow()
            .args(["hook", hook, "--contract", "3"])
            .current_dir(root)
            .env_remove("CLAUDE_PROJECT_DIR"),
        &payload.to_string(),
    )
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// A project whose `origin/main` holds the policy, checked out on `main`,
/// with a linked worktree on `feat/w`.
fn fixture(root: &Path) -> std::path::PathBuf {
    init_repo(root, "main");
    write_policy(root, POLICY);
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
    let worktree = root.join(".worktrees/w");
    git(
        root,
        &["worktree", "add", "-q", ".worktrees/w", "-b", "feat/w"],
    );
    worktree
}

/// The places a skip form is judged: the protected root checkout, the
/// unprotected linked worktree, and each of them named with `git -C` from
/// the other.
fn places(root: &Path, worktree: &Path) -> Vec<(std::path::PathBuf, String)> {
    let slashed = |path: &Path| codeflow_core::portable_path::slashed(path);
    vec![
        (root.to_path_buf(), "git".to_string()),
        (worktree.to_path_buf(), "git".to_string()),
        (root.to_path_buf(), format!("git -C {}", slashed(worktree))),
        (worktree.to_path_buf(), format!("git -C {}", slashed(root))),
    ]
}

#[test]
fn both_guards_refuse_every_hook_skip_form_everywhere() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("p");
    std::fs::create_dir(&root).unwrap();
    let worktree = fixture(&root);
    // Every form in the unprotected linked worktree; a form of each kind in
    // every place; every wrapper around a plain skip.
    let mut cases: Vec<(std::path::PathBuf, String)> = SKIPS
        .iter()
        .map(|skip| (worktree.clone(), format!("git {skip}")))
        .collect();
    for (session, git) in places(&root, &worktree) {
        for skip in IN_EVERY_PLACE {
            cases.push((session.clone(), format!("{git} {skip}")));
        }
    }
    for wrapper in WRAPPERS {
        cases.push((
            worktree.clone(),
            wrapper.replace("{}", "git commit --no-verify -m x"),
        ));
    }
    let mut admitted = Vec::new();
    for (session, command) in &cases {
        for name in ["git-guard", "exec-guard"] {
            let out = hook(name, session, command);
            if out.status.code() != Some(2)
                || !stderr(&out).contains("policy rule git.hook_integrity")
            {
                admitted.push(format!("{name} in {}: {command}", session.display()));
            }
        }
    }
    assert!(
        admitted.is_empty(),
        "hook-skip forms admitted:\n{}",
        admitted.join("\n")
    );
}

#[test]
fn neither_guard_reads_a_lookalike_as_a_hook_skip() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("p");
    std::fs::create_dir(&root).unwrap();
    let worktree = fixture(&root);
    let mut refused = Vec::new();
    for (session, git) in places(&root, &worktree).into_iter().skip(1).take(2) {
        for form in NOT_SKIPS {
            let command = format!("{git} {form}");
            for name in ["git-guard", "exec-guard"] {
                let said = stderr(&hook(name, &session, &command));
                if said.contains("skips the") || said.contains("hook path for this command") {
                    refused.push(format!("{name}: {command}\n{said}"));
                }
            }
        }
    }
    assert!(refused.is_empty(), "{}", refused.join("\n"));
}

/// AC-5: the refusal names the rule, the flag as git documents it and the
/// sanctioned path, and carries nothing of the project: no branch, path or
/// value the command wrote.
#[test]
fn the_refusal_names_rule_flag_and_sanctioned_path_only() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("p");
    std::fs::create_dir(&root).unwrap();
    let worktree = fixture(&root);
    for name in ["git-guard", "exec-guard"] {
        let out = hook(name, &worktree, "git commit -qn -m secret-message");
        assert_eq!(out.status.code(), Some(2), "{name}");
        let said = stderr(&out);
        let at = said
            .find("policy rule git.hook_integrity")
            .unwrap_or_else(|| panic!("{name}: {said}"));
        // The rule line and its indented lines, up to the policy file line.
        let mut lines = said[at..].lines();
        let finding: String = lines
            .next()
            .into_iter()
            .chain(lines.take_while(|line| {
                line.starts_with("  ") && !line.trim_start().starts_with("policy file")
            }))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            finding.contains(
                "`git commit -n` (`--no-verify`) skips the pre-commit and commit-msg hooks"
            ),
            "{name}: {finding}"
        );
        assert!(
            finding.contains("fix the cause a hook reports; never skip a hook"),
            "{name}: {finding}"
        );
        for value in ["secret-message", "feat/w", ".worktrees", "-qn"] {
            assert!(
                !finding.contains(value),
                "{name}: the finding carries {value:?}: {finding}"
            );
        }
    }
    let out = hook("git-guard", &worktree, "git push --no-verify origin HEAD");
    assert!(
        stderr(&out).contains("`git push --no-verify` skips the pre-push hook"),
        "{}",
        stderr(&out)
    );
}
