//! Workspace mode through the real binary (TSK-165): `codeflow init
//! --workspace` on an umbrella repository with nested projects, a rerun
//! that changes nothing, a refusal over uncommitted changes, and the hint
//! plain `init` and `update` print without switching anything.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use codeflow_core::root_checkout::WORKSPACE_ROOT_BRANCH;

fn isolated_home() -> &'static Path {
    static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| tempfile::tempdir().expect("home tempdir"))
        .path()
}

fn codeflow(dir: &Path, args: &[&str]) -> Output {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(exe.parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("joinable PATH");
    Command::new(&exe)
        .args(args)
        .current_dir(dir)
        .env("CODEFLOW_HOME", isolated_home())
        .env("PATH", path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE")
        .output()
        .expect("codeflow binary runs")
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn repo_with_commit(dir: &Path) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "--quiet", "-b", "main"]);
    std::fs::write(dir.join("README.md"), "x\n").unwrap();
    git(dir, &["add", "README.md"]);
    git(dir, &["commit", "--quiet", "-m", "init"]);
    dir.canonicalize().unwrap()
}

/// An umbrella on `main` holding a plain repository, a `CodeFlow` project and
/// a repository two levels down.
fn umbrella(dir: &Path) -> PathBuf {
    let root = repo_with_commit(&dir.join("workspace"));
    repo_with_commit(&root.join("plain"));
    let proj = repo_with_commit(&root.join("proj"));
    std::fs::create_dir_all(proj.join(".codeflow")).unwrap();
    repo_with_commit(&root.join("group/deep"));
    root
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn root_branch_key(root: &Path) -> String {
    let text = std::fs::read_to_string(root.join(".codeflow/policy.json")).unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    value["git"]["root_branch"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn init_workspace_sets_up_an_umbrella_and_a_rerun_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let root = umbrella(dir.path());

    let first = codeflow(&root, &["init", "--yes", "--minimal", "--workspace"]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let text = stdout(&first);
    assert!(
        text.contains(&format!(
            "created '{WORKSPACE_ROOT_BRANCH}' from 'main' and switched the root checkout to it"
        )),
        "{text}"
    );
    assert!(
        text.contains("ignored the nested git repository /plain/ in .gitignore"),
        "{text}"
    );
    assert!(
        text.contains("ignored the nested CodeFlow project /proj/ in .gitignore"),
        "{text}"
    );
    assert!(
        text.contains("ignored the nested git repository /group/deep/ in .gitignore"),
        "{text}"
    );
    assert!(text.contains("next steps:"), "{text}");
    assert_eq!(
        git(&root, &["branch", "--show-current"]),
        WORKSPACE_ROOT_BRANCH
    );
    assert_eq!(root_branch_key(&root), WORKSPACE_ROOT_BRANCH);
    let ignore = std::fs::read_to_string(root.join(".gitignore")).unwrap();
    for line in ["/plain/", "/proj/", "/group/deep/"] {
        assert!(
            ignore.lines().any(|l| l == line),
            "{line} missing from {ignore}"
        );
    }

    let status_before = git(&root, &["status", "--porcelain", "--untracked-files=all"]);
    let ignore_before = ignore.clone();
    let policy_before = std::fs::read_to_string(root.join(".codeflow/policy.json")).unwrap();

    let again = codeflow(&root, &["init", "--yes", "--minimal", "--workspace"]);
    assert!(
        again.status.success(),
        "{}",
        String::from_utf8_lossy(&again.stderr)
    );
    let text = stdout(&again);
    assert!(
        text.contains(&format!(
            "root checkout already on '{WORKSPACE_ROOT_BRANCH}'"
        )),
        "{text}"
    );
    assert!(text.contains("git.root_branch already set"), "{text}");
    assert!(!text.contains("ignored the nested"), "{text}");
    assert_eq!(
        git(&root, &["status", "--porcelain", "--untracked-files=all"]),
        status_before
    );
    assert_eq!(
        std::fs::read_to_string(root.join(".gitignore")).unwrap(),
        ignore_before
    );
    assert_eq!(
        std::fs::read_to_string(root.join(".codeflow/policy.json")).unwrap(),
        policy_before
    );
}

#[test]
fn init_workspace_refuses_over_uncommitted_changes_and_touches_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let root = umbrella(dir.path());
    std::fs::write(root.join("README.md"), "edited\n").unwrap();
    let out = codeflow(&root, &["init", "--yes", "--minimal", "--workspace"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains(&format!(
            "would switch the root checkout of {} from 'main' to '{WORKSPACE_ROOT_BRANCH}', but \
             these tracked files have uncommitted changes: README.md. Next: commit or stash \
             them, then rerun codeflow init --workspace",
            root.display()
        )),
        "{err}"
    );
    assert_eq!(git(&root, &["branch", "--show-current"]), "main");
    assert!(!root.join(".codeflow").exists());
}

#[test]
fn plain_init_and_update_in_an_umbrella_print_the_hint_and_switch_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let root = umbrella(dir.path());
    let init = codeflow(&root, &["init", "--yes", "--minimal"]);
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    let hint = format!(
        "so it looks like a workspace; nothing was switched. Next: for an umbrella workspace, \
         run codeflow init --workspace (root branch {WORKSPACE_ROOT_BRANCH}); a single project \
         can ignore this"
    );
    let hint = hint.as_str();
    assert!(stdout(&init).contains(hint), "{}", stdout(&init));
    assert_eq!(git(&root, &["branch", "--show-current"]), "main");
    assert_eq!(root_branch_key(&root), "");

    let update = codeflow(&root, &["update"]);
    assert!(stdout(&update).contains(hint), "{}", stdout(&update));
    assert_eq!(git(&root, &["branch", "--show-current"]), "main");
}

#[test]
fn a_single_repository_gets_no_workspace_hint() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("single"));
    let init = codeflow(&root, &["init", "--yes", "--minimal"]);
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    assert!(
        !stdout(&init).contains("looks like a workspace"),
        "{}",
        stdout(&init)
    );
    assert_eq!(root_branch_key(&root), "");
}

/// git through the installed hooks, with the binary under test first on
/// `PATH` and only the harness `marker` given (or none), so the hooks judge
/// the actor the same way in an agent session and in CI.
fn git_as(dir: &Path, marker: Option<&str>, args: &[&str]) -> Output {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(exe.parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("joinable PATH");
    let mut cmd = Command::new("git");
    cmd.args(args)
        .current_dir(dir)
        .env("PATH", path)
        .env("CODEFLOW_HOME", isolated_home())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE");
    for name in codeflow_core::root_checkout::AGENT_MARKERS {
        cmd.env_remove(name);
    }
    if let Some(name) = marker {
        cmd.env(name, "1");
    }
    cmd.output().expect("git runs")
}

fn both(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn journey_the_root_checkout_rule_through_the_real_hooks_and_doctor() {
    // AC-15: a fresh project, its installed hooks and the built binary.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("single");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "--quiet", "-b", "main"]);
    let root = root.canonicalize().unwrap();
    let init = codeflow(&root, &["init", "--yes", "--minimal"]);
    assert!(init.status.success(), "{}", both(&init));
    // In a repository without commits, init commits the scaffold on main.
    git(&root, &["rev-parse", "--verify", "HEAD"]);

    // A commit in a linked worktree on a feature branch passes.
    git(
        &root,
        &["worktree", "add", "--quiet", ".worktrees/t", "-b", "feat/t"],
    );
    let wt = root.join(".worktrees/t");
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    git(&wt, &["add", "a.txt"]);
    let ok = git_as(&wt, Some("CLAUDECODE"), &["commit", "-m", "feat: add a"]);
    assert!(ok.status.success(), "{}", both(&ok));
    assert!(
        !both(&ok).contains("git.root_checkout_commits"),
        "{}",
        both(&ok)
    );

    // At the root on a feature branch, an agent-marked commit is refused.
    git(&root, &["switch", "--quiet", "-c", "feat/x"]);
    std::fs::write(root.join("b.txt"), "b\n").unwrap();
    git(&root, &["add", "b.txt"]);
    let refused = git_as(
        &root,
        Some("CODEX_THREAD_ID"),
        &["commit", "-m", "feat: add b"],
    );
    let said = both(&refused);
    assert!(!refused.status.success(), "{said}");
    assert!(
        said.contains("BLOCKED \u{2014} policy rule git.root_checkout_commits (block)"),
        "{said}"
    );
    assert!(
        said.contains(&format!(
            "a commit at the root checkout of {} on 'feat/x'; its root branch is 'main'",
            root.display()
        )),
        "{said}"
    );
    assert!(
        said.contains("CODEX_THREAD_ID is set, so this commit comes from an agent session"),
        "{said}"
    );
    assert!(said.contains("`git switch main`"), "{said}");

    // Unmarked, the same commit proceeds with a warning.
    let warned = git_as(&root, None, &["commit", "-m", "feat: add b"]);
    let said = both(&warned);
    assert!(warned.status.success(), "{said}");
    assert!(
        said.contains("warning \u{2014} policy rule git.root_checkout_commits (warn)"),
        "{said}"
    );
    assert!(
        said.contains("no harness marker is set, so this commit is treated as a human's"),
        "{said}"
    );

    // doctor reports the root checkout off its root branch.
    let doctor = codeflow(&root, &["doctor", "--check", "repo-integrity"]);
    let said = both(&doctor);
    assert!(
        said.contains(&format!(
            "git.root_branch: the root checkout of {} is on 'feat/x'",
            root.display()
        )),
        "{said}"
    );
    assert!(
        said.contains("clear it: take the next step each finding above names"),
        "{said}"
    );

    // An umbrella set up with init --workspace reports workspace mode.
    let umbrella_root = umbrella(dir.path());
    let setup = codeflow(
        &umbrella_root,
        &["init", "--yes", "--minimal", "--workspace"],
    );
    assert!(setup.status.success(), "{}", both(&setup));
    let doctor = codeflow(&umbrella_root, &["doctor", "--check", "repo-integrity"]);
    let said = both(&doctor);
    assert!(
        said.contains(&format!(
            "workspace mode: the root checkout of {} stays on '{WORKSPACE_ROOT_BRANCH}' (set by \
             git.root_branch); it holds 3 nested repositories (1 with CodeFlow)",
            umbrella_root.display()
        )),
        "{said}"
    );
}

/// Whether git ignores `path` in `root`, by every rule it reads.
fn git_ignores(root: &Path, path: &str) -> bool {
    Command::new("git")
        .args(["check-ignore", "-q", path])
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .expect("git runs")
        .success()
}

#[test]
fn init_workspace_sets_the_root_branch_in_a_policy_without_a_git_object() {
    // A sparse policy is valid; init --workspace must add the key, not
    // report it as already set.
    let dir = tempfile::tempdir().unwrap();
    let root = umbrella(dir.path());
    std::fs::create_dir_all(root.join(".codeflow")).unwrap();
    std::fs::write(root.join(".codeflow/policy.json"), "{}").unwrap();
    git(&root, &["add", ".codeflow/policy.json"]);
    git(&root, &["commit", "--quiet", "-m", "sparse policy"]);

    let out = codeflow(&root, &["init", "--yes", "--minimal", "--workspace"]);
    let said = both(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        said.contains("set git.root_branch in .codeflow/policy.json"),
        "{said}"
    );
    assert!(!said.contains("git.root_branch already set"), "{said}");
    assert_eq!(root_branch_key(&root), WORKSPACE_ROOT_BRANCH);

    // The root rule now reads the workspace branch as the root branch.
    let doctor = codeflow(&root, &["doctor", "--check", "repo-integrity"]);
    let said = both(&doctor);
    assert!(
        said.contains(&format!(
            "stays on '{WORKSPACE_ROOT_BRANCH}' (set by git.root_branch)"
        )),
        "{said}"
    );
    assert!(
        !said.contains(&format!("is on '{WORKSPACE_ROOT_BRANCH}'")),
        "{said}"
    );
}

#[test]
fn init_workspace_makes_a_negated_ignore_line_effective_and_a_rerun_keeps_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("workspace"));
    repo_with_commit(&root.join("nested"));
    std::fs::write(root.join(".gitignore"), "/nested/\n!/nested/\n").unwrap();
    git(&root, &["add", ".gitignore"]);
    git(&root, &["commit", "--quiet", "-m", "ignore"]);
    assert!(!git_ignores(&root, "nested"));

    let first = codeflow(&root, &["init", "--yes", "--minimal", "--workspace"]);
    let said = both(&first);
    assert!(first.status.success(), "{said}");
    assert!(
        said.contains("ignored the nested git repository /nested/ in .gitignore"),
        "{said}"
    );
    assert!(git_ignores(&root, "nested"));
    let written = std::fs::read_to_string(root.join(".gitignore")).unwrap();

    let again = codeflow(&root, &["init", "--yes", "--minimal", "--workspace"]);
    let said = both(&again);
    assert!(again.status.success(), "{said}");
    assert!(
        said.contains("the nested git repository 'nested' is already in .gitignore"),
        "{said}"
    );
    assert!(git_ignores(&root, "nested"));
    assert_eq!(
        std::fs::read_to_string(root.join(".gitignore")).unwrap(),
        written
    );
}

#[test]
fn init_workspace_ignores_a_nested_repository_under_a_locally_excluded_parent() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("workspace"));
    repo_with_commit(&root.join("group/nested"));
    std::fs::write(root.join(".git/info/exclude"), "/group/\n").unwrap();

    let doctor = codeflow(&root, &["doctor", "--check", "repo-integrity"]);
    let said = both(&doctor);
    assert!(said.contains("group/nested"), "{said}");

    let out = codeflow(&root, &["init", "--yes", "--minimal", "--workspace"]);
    let said = both(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        said.contains("ignored the nested git repository /group/nested/ in .gitignore"),
        "{said}"
    );
    let ignore = std::fs::read_to_string(root.join(".gitignore")).unwrap();
    assert!(ignore.lines().any(|l| l == "/group/nested/"), "{ignore}");
}

#[test]
fn a_case_sensitive_umbrella_gets_the_warning_and_the_literal_rule() {
    // Codex round 2: `/NESTED/` does not ignore `nested` when the
    // repository matches case, whatever the file system's default.
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("workspace"));
    git(&root, &["config", "core.ignoreCase", "false"]);
    std::fs::write(root.join(".gitignore"), "/NESTED/\n").unwrap();
    git(&root, &["add", ".gitignore"]);
    git(&root, &["commit", "--quiet", "-m", "ignore"]);
    repo_with_commit(&root.join("nested"));
    assert!(!git_ignores(&root, "nested"));

    let doctor = codeflow(&root, &["doctor", "--check", "repo-integrity"]);
    let said = both(&doctor);
    assert!(
        said.contains("the nested git repository 'nested'"),
        "{said}"
    );

    for run in ["first", "rerun"] {
        let out = codeflow(&root, &["init", "--yes", "--minimal", "--workspace"]);
        let said = both(&out);
        assert!(out.status.success(), "{run}: {said}");
        assert!(git_ignores(&root, "nested"), "{run}: {said}");
        if run == "first" {
            assert!(
                said.contains("ignored the nested git repository /nested/ in .gitignore"),
                "{said}"
            );
        }
    }
    let ignore = std::fs::read_to_string(root.join(".gitignore")).unwrap();
    assert_eq!(
        ignore.lines().filter(|l| *l == "/nested/").count(),
        1,
        "{ignore}"
    );
}

/// The same run as [`codeflow`], with git's runtime config overrides set:
/// every git process codeflow starts sees `key = value` on top of the files.
fn codeflow_with_override(dir: &Path, args: &[&str], key: &str, value: &str) -> Output {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(exe.parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("joinable PATH");
    Command::new(&exe)
        .args(args)
        .current_dir(dir)
        .env("CODEFLOW_HOME", isolated_home())
        .env("PATH", path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", key)
        .env("GIT_CONFIG_VALUE_0", value)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE")
        .output()
        .expect("codeflow binary runs")
}

/// Whether git ignores `path` in `root` under the runtime override.
fn git_ignores_with_override(root: &Path, path: &str, key: &str, value: &str) -> bool {
    Command::new("git")
        .args(["check-ignore", "-q", path])
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", key)
        .env("GIT_CONFIG_VALUE_0", value)
        .status()
        .expect("git runs")
        .success()
}

#[test]
fn a_runtime_case_override_decides_the_shared_rule_check() {
    // Codex round 3: the stored core.ignoreCase is true, but git runs with
    // an override to false, so `/NESTED/` does not ignore `nested`.
    let (key, value) = ("core.ignoreCase", "false");
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("workspace"));
    git(&root, &["config", "core.ignoreCase", "true"]);
    std::fs::write(root.join(".gitignore"), "/NESTED/\n").unwrap();
    git(&root, &["add", ".gitignore"]);
    git(&root, &["commit", "--quiet", "-m", "ignore"]);
    repo_with_commit(&root.join("nested"));
    assert!(!git_ignores_with_override(&root, "nested", key, value));

    let doctor =
        codeflow_with_override(&root, &["doctor", "--check", "repo-integrity"], key, value);
    let said = both(&doctor);
    assert!(
        said.contains("the nested git repository 'nested'"),
        "{said}"
    );

    for run in ["first", "rerun"] {
        let out = codeflow_with_override(
            &root,
            &["init", "--yes", "--minimal", "--workspace"],
            key,
            value,
        );
        let said = both(&out);
        assert!(out.status.success(), "{run}: {said}");
        assert!(
            git_ignores_with_override(&root, "nested", key, value),
            "{run}: {said}"
        );
    }
    let ignore = std::fs::read_to_string(root.join(".gitignore")).unwrap();
    assert_eq!(
        ignore.lines().filter(|l| *l == "/nested/").count(),
        1,
        "{ignore}"
    );
}
