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
