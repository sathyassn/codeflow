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

/// Lines that set one of git's configuration-source variables, which can
/// replace the hooks path for every later git call. Each refuses on the line
/// that sets it, whatever the value, with or without a git call after it.
const CONFIG_SOURCES: &[&str] = &[
    "export GIT_CONFIG_GLOBAL=/tmp/g.gitconfig",
    "export GIT_CONFIG_SYSTEM=/tmp/s.gitconfig",
    "export GIT_CONFIG_COUNT=1",
    "export GIT_CONFIG_KEY_0=include.path",
    "export GIT_CONFIG_KEY_0=core.hooksPath",
    "export GIT_CONFIG_KEY_3=user.email",
    "export GIT_CONFIG_PARAMETERS=\"'include.path=/tmp/i'\"",
    "GIT_CONFIG_GLOBAL=/tmp/g.gitconfig",
    "GIT_CONFIG_GLOBAL=/tmp/g.gitconfig; export GIT_CONFIG_GLOBAL",
    "declare -x GIT_CONFIG_GLOBAL=/tmp/g.gitconfig",
    "export GIT_CONFIG_GLOBAL",
    "env GIT_CONFIG_GLOBAL=/tmp/g.gitconfig git commit -m x",
    "GIT_CONFIG_GLOBAL=/tmp/g.gitconfig git commit -m x",
    "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=include.path GIT_CONFIG_VALUE_0=/tmp/i git commit -m x",
    "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=user.email GIT_CONFIG_VALUE_0=x git status",
    "export GIT_CONFIG_GLOBAL=/tmp/g.gitconfig; git commit -m x",
    "sh -c 'export GIT_CONFIG_GLOBAL=/tmp/g.gitconfig'",
    "true && export GIT_CONFIG_SYSTEM=/tmp/s.gitconfig",
    // Wrapped spellings (TSK-1349 delta review finding A and its sweep).
    "builtin export GIT_CONFIG_GLOBAL=/tmp/g.gitconfig",
    "builtin declare -x GIT_CONFIG_KEY_0=core.hooksPath",
    "builtin export GIT_CONFIG_PARAMETERS=\"'core.hooksPath=/dev/null'\"",
    "builtin export GIT_CONFIG_COUNT=1",
    "builtin builtin export GIT_CONFIG_SYSTEM=/tmp/s.gitconfig",
    "command export GIT_CONFIG_GLOBAL=/tmp/g.gitconfig",
    "command builtin export GIT_CONFIG_GLOBAL=/tmp/g.gitconfig",
    "builtin command export GIT_CONFIG_GLOBAL=/tmp/g.gitconfig",
    "command -p declare -x GIT_CONFIG_KEY_1=core.hooksPath",
    "FOO=1 builtin export GIT_CONFIG_GLOBAL=/tmp/g.gitconfig",
    "builtin export GIT_CONFIG_GLOBAL",
    "builtin typeset -x GIT_CONFIG_COUNT=1",
    "exec builtin export GIT_CONFIG_GLOBAL=/tmp/g.gitconfig",
    "nohup builtin export GIT_CONFIG_GLOBAL=/tmp/g.gitconfig",
    "env builtin export GIT_CONFIG_GLOBAL=/tmp/g.gitconfig",
    "eval 'builtin export GIT_CONFIG_GLOBAL=/tmp/g.gitconfig'",
    "eval builtin export GIT_CONFIG_GLOBAL=/tmp/g.gitconfig",
    "eval 'export GIT_CONFIG_KEY_0=core.hooksPath'",
    "sh -c 'builtin export GIT_CONFIG_GLOBAL=/tmp/g.gitconfig'",
    "bash -c 'builtin declare -x GIT_CONFIG_KEY_0=core.hooksPath'",
    "exec env GIT_CONFIG_GLOBAL=/tmp/g.gitconfig git commit -m x",
    "nohup env GIT_CONFIG_GLOBAL=/tmp/g.gitconfig git commit -m x",
    "env -S 'GIT_CONFIG_GLOBAL=/tmp/g.gitconfig git commit -m x'",
    "env -S 'GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.hooksPath GIT_CONFIG_VALUE_0=/dev/null git commit -m x'",
    "nohup env GIT_CONFIG_PARAMETERS=\"'core.hooksPath=/dev/null'\" git commit -m x",
    "command env GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.hooksPath GIT_CONFIG_VALUE_0=/dev/null git commit -m x",
];

/// A hook-path override on a git call's global options, in every spelling
/// and through every wrapper (TSK-1349 delta review finding B and its
/// sweep). Git 2.53 rejects an attached `-c<entry>` as an unknown option;
/// the guards still refuse it, so a git that reads it cannot skip the hooks.
const HOOKS_PATH_FLAGS: &[&str] = &[
    "git -ccore.hooksPath=/dev/null commit -m x",
    "git -cCore.HooksPath=/dev/null commit -m x",
    "git -c CORE.hookspath=/dev/null commit -m x",
    "git -c Core.HooksPath=/dev/null commit -m x",
    "git --config-env=core.hooksPath=HP commit -m x",
    "git --config-env=CORE.HOOKSPATH=HP commit -m x",
    "git --config-env core.hooksPath=HP commit -m x",
    "git -C . -ccore.hooksPath=/dev/null commit -m x",
    "command git -c core.hooksPath=/dev/null commit -m x",
    "command git -ccore.hooksPath=/dev/null commit -m x",
    "exec git -c core.hooksPath=/dev/null commit -m x",
    "exec git -ccore.hooksPath=/dev/null commit -m x",
    "env git -c core.hooksPath=/dev/null commit -m x",
    "env git -ccore.hooksPath=/dev/null commit -m x",
    "env -S 'git -c core.hooksPath=/dev/null commit -m x'",
    "env -S 'git -ccore.hooksPath=/dev/null commit -m x'",
    "nohup git -c core.hooksPath=/dev/null commit -m x",
    "nohup git -ccore.hooksPath=/dev/null commit -m x",
    "eval 'git -c core.hooksPath=/dev/null commit -m x'",
    "eval 'git -ccore.hooksPath=/dev/null commit -m x'",
    "eval git -ccore.hooksPath=/dev/null commit -m x",
    "sh -c 'git -c core.hooksPath=/dev/null commit -m x'",
    "sh -c 'git -ccore.hooksPath=/dev/null commit -m x'",
    "bash -c 'git -cCore.HooksPath=/dev/null commit -m x'",
    "git -c \"alias.c=-ccore.hooksPath=/dev/null commit\" c -m x",
];

/// Include flags on a git call: git reads the named file as configuration,
/// so it can set the hooks path. The file need not exist for the refusal.
const INCLUDES: &[&str] = &[
    "git -c include.path=/tmp/i commit -m x",
    "git -c Include.Path=/nonexistent/unreadable commit -m x",
    "git -c includeIf.gitdir:/.path=/tmp/i commit -m x",
    "git --config-env include.path=INC commit -m x",
    "git --config-env=includeIf.onbranch:feat/*.path=INC commit -m x",
    "sh -c 'git -c include.path=/tmp/i commit -m x'",
    "git -cinclude.path=/tmp/i commit -m x",
    "git -cInclude.Path=/tmp/i commit -m x",
    "git -c INCLUDE.PATH=/tmp/i commit -m x",
    "git -cincludeIf.gitdir:/.path=/tmp/i commit -m x",
    "git --config-env=Include.Path=INC commit -m x",
    "command git -cinclude.path=/tmp/i commit -m x",
    "exec git -cinclude.path=/tmp/i commit -m x",
    "env git -cinclude.path=/tmp/i commit -m x",
    "env -S 'git -cinclude.path=/tmp/i commit -m x'",
    "nohup git -cinclude.path=/tmp/i commit -m x",
    "eval 'git -cinclude.path=/tmp/i commit -m x'",
    "sh -c 'git -cinclude.path=/tmp/i commit -m x'",
];

/// Findings 1 to 3 of the TSK-1349 security review: both guards refuse a
/// configuration-source assignment under `git.hook_integrity`, and exec-guard
/// refuses an include flag under it, so exec-guard admits nothing git-guard
/// refuses. git-guard refuses an include flag under `git.policy_authority`,
/// which protects every include key, as it did before this task.
#[test]
fn both_guards_refuse_configuration_sources_and_includes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("p");
    std::fs::create_dir(&root).unwrap();
    let worktree = fixture(&root);
    let mut admitted = Vec::new();
    let cases = CONFIG_SOURCES
        .iter()
        .chain(HOOKS_PATH_FLAGS)
        .map(|command| (*command, false))
        .chain(INCLUDES.iter().map(|command| (*command, true)));
    for (command, include) in cases {
        for name in ["git-guard", "exec-guard"] {
            let out = hook(name, &worktree, command);
            let said = stderr(&out);
            // The refusal must name the configuration it reads: an attached
            // `-c…/dev/null` also reads as a clustered `-n`, which is not
            // the reason these forms refuse.
            let reason = [
                "another hooks path",
                "overrides the hook path",
                "includes a configuration file",
            ]
            .iter()
            .any(|why| said.contains(why));
            // git-guard may refuse an include as an authority change, or,
            // behind `eval`, because git itself rejects the configuration
            // it was asked to verify.
            let rule = (said.contains("policy rule git.hook_integrity") && reason)
                || (include
                    && name == "git-guard"
                    && (said.contains("policy rule git.policy_authority")
                        || (said.contains("policy rule git.hook_integrity")
                            && said.contains("cannot verify Git configuration"))));
            if out.status.code() != Some(2) || !rule {
                admitted.push(format!("{name}: {command}\n{said}"));
            }
        }
    }
    assert!(
        admitted.is_empty(),
        "configuration sources admitted:\n{}",
        admitted.join("\n")
    );
}

/// The lines around these variables that set nothing: reading or naming
/// them, and the variables git does not read as a configuration source.
#[test]
fn neither_guard_refuses_a_configuration_variable_it_does_not_set() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("p");
    std::fs::create_dir(&root).unwrap();
    let worktree = fixture(&root);
    let mut refused = Vec::new();
    for command in [
        "git commit -m x",
        "echo GIT_CONFIG_GLOBAL",
        "git commit -m GIT_CONFIG_GLOBAL=x",
        "git -c user.email=t@example.com commit -m x",
    ] {
        for name in ["git-guard", "exec-guard"] {
            let said = stderr(&hook(name, &worktree, command));
            if said.contains("can point git at another hooks path")
                || said.contains("includes a configuration file")
            {
                refused.push(format!("{name}: {command}\n{said}"));
            }
        }
    }
    assert!(refused.is_empty(), "{}", refused.join("\n"));
}

/// The threat the refusal closes, driven with the git the tests run: a
/// commit runs the repository's pre-commit hook, and the same commit with
/// `GIT_CONFIG_GLOBAL`, or `GIT_CONFIG_COUNT` with an `include.path`, naming
/// a file that sets `core.hooksPath` to an empty directory runs none. The
/// export that sets the variable is its own tool call, refused by both
/// guards above; the plain commit after it is admitted, which is why the
/// export must refuse.
#[test]
fn a_configuration_source_skips_the_hooks_of_a_later_plain_commit() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("p");
    std::fs::create_dir(&root).unwrap();
    init_repo(&root, "feat/x");
    let marker = dir.path().join("ran");
    let hooks = root.join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let pre_commit = hooks.join("pre-commit");
    std::fs::write(
        &pre_commit,
        format!(
            "#!/bin/sh\ntouch '{}'\n",
            codeflow_core::portable_path::slashed(&marker)
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&pre_commit, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let empty = dir.path().join("no-hooks");
    std::fs::create_dir(&empty).unwrap();
    let redirect = dir.path().join("redirect.gitconfig");
    std::fs::write(
        &redirect,
        format!(
            "[core]\n\thooksPath = {}\n",
            codeflow_core::portable_path::slashed(&empty)
        ),
    )
    .unwrap();
    let commit = |env: &[(&str, &Path)], extra: &[(&str, &str)]| -> bool {
        let _ = std::fs::remove_file(&marker);
        let mut cmd = super::git_command();
        cmd.args(["commit", "--allow-empty", "-q", "-m", "chore: probe"])
            .current_dir(&root);
        for (k, v) in env {
            cmd.env(k, v);
        }
        for (k, v) in extra {
            cmd.env(k, v);
        }
        assert!(cmd.status().unwrap().success());
        marker.exists()
    };
    assert!(commit(&[], &[]), "a plain commit runs the pre-commit hook");
    assert!(
        !commit(&[("GIT_CONFIG_GLOBAL", &redirect)], &[]),
        "GIT_CONFIG_GLOBAL redirects the hooks"
    );
    assert!(
        !commit(
            &[("GIT_CONFIG_VALUE_0", &redirect)],
            &[
                ("GIT_CONFIG_COUNT", "1"),
                ("GIT_CONFIG_KEY_0", "include.path")
            ]
        ),
        "an include through GIT_CONFIG_COUNT redirects the hooks"
    );
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
