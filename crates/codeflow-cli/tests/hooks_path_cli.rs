//! TSK-1395 (ADR-0099): `CodeFlow`'s git hooks run however a git invocation
//! names its repository and working directory. `codeflow init` writes one
//! absolute `core.hooksPath` to the main working tree's managed hooks,
//! `codeflow update` migrates the earlier relative form and a stale path,
//! and `codeflow doctor` reports both. The binary under test is the one
//! Cargo built; the installed `codeflow` on `PATH` is never used.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// One invocation class: its label, the directory git runs in, the git
/// arguments, and the environment it sets.
type Case<'a> = (&'a str, PathBuf, Vec<String>, Vec<(&'a str, &'a Path)>);

#[path = "support/process_folders.rs"]
mod process_folders;
use process_folders::isolated_home;

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
        .env_remove("CODEFLOW_HOOK_BINARY")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE")
        // A caller's agent session must not judge the fixture's own commits.
        .env_remove("CLAUDECODE")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CODEX_THREAD_ID")
        .env_remove("CODEX_CI")
        .env_remove("GROK_SESSION_ID")
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn codeflow(root: &Path, args: &[&str]) -> String {
    let out = with_env(&mut Command::new(env!("CARGO_BIN_EXE_codeflow")))
        .args(args)
        .current_dir(root)
        .output()
        .expect("codeflow runs");
    text(&out)
}

/// Run git in `cwd` with extra environment; returns the output.
fn git_in(cwd: &Path, args: &[&str], envs: &[(&str, &Path)]) -> Output {
    let mut command = Command::new("git");
    with_env(&mut command).args(args).current_dir(cwd);
    for (key, value) in envs {
        command.env(key, value);
    }
    command
        .output()
        .unwrap_or_else(|error| panic!("git {args:?} in {}: {error}", cwd.display()))
}

fn git(cwd: &Path, args: &[&str]) -> String {
    let out = git_in(cwd, args, &[]);
    assert!(out.status.success(), "git {args:?}: {}", text(&out));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn rev(cwd: &Path, name: &str) -> String {
    git(cwd, &["rev-parse", name])
}

/// A project at the minimal tier in a folder whose name has a space, with
/// `feat/x` one commit ahead of `main` in a linked worktree and a local bare
/// remote. Returns (temp dir, main working tree, linked worktree).
struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    linked: PathBuf,
    elsewhere: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("a project");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    let init = codeflow(&root, &["init", "--minimal", "--yes"]);
    assert!(init.contains("git hooks wired"), "{init}");
    let origin = dir.path().join("origin.git");
    codeflow_fixture::clone(dir.path(), &root, &origin)
        .bare()
        .run();
    git(
        &root,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&root, &["fetch", "-q", "origin"]);
    let linked = dir.path().join("linked tree");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feat/x",
            linked.to_str().unwrap(),
            "main",
        ],
    );
    std::fs::write(linked.join("x.txt"), "x\n").unwrap();
    git(&linked, &["add", "x.txt"]);
    git(&linked, &["commit", "-q", "-m", "feat: add x"]);
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::create_dir_all(linked.join("sub")).unwrap();
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    Fixture {
        _dir: dir,
        root,
        linked,
        elsewhere,
    }
}

fn managed(root: &Path) -> String {
    format!(
        "{}/.codeflow/git-hooks",
        git(root, &["rev-parse", "--show-toplevel"])
    )
}

/// Each invocation class tries to move the protected local `main` to
/// `feat/x`; the reference-transaction hook must refuse every one.
fn assert_every_class_refuses_the_ref_update(fx: &Fixture) {
    let root = &fx.root;
    let git_dir = root.join(".git");
    let linked_git_dir = PathBuf::from(git(&fx.linked, &["rev-parse", "--absolute-git-dir"]));
    let target = rev(root, "feat/x");
    let main = rev(root, "main");
    let r = root.to_str().unwrap();
    let g = git_dir.to_str().unwrap();
    let update = ["update-ref", "refs/heads/main", target.as_str()];
    let with = |prefix: &[&str]| -> Vec<String> {
        prefix
            .iter()
            .chain(update.iter())
            .map(|s| (*s).to_string())
            .collect()
    };
    let cases: Vec<Case> = vec![
        ("project root", root.clone(), with(&[]), vec![]),
        ("subfolder", root.join("sub"), with(&[]), vec![]),
        ("git directory", git_dir.clone(), with(&[]), vec![]),
        (
            "folder in the git directory",
            git_dir.join("refs"),
            with(&[]),
            vec![],
        ),
        ("-C", fx.elsewhere.clone(), with(&["-C", r]), vec![]),
        (
            "--git-dir and --work-tree",
            fx.elsewhere.clone(),
            with(&[&format!("--git-dir={g}"), &format!("--work-tree={r}")]),
            vec![],
        ),
        (
            "--git-dir from a subfolder",
            root.join("sub"),
            with(&[&format!("--git-dir={g}")]),
            vec![],
        ),
        (
            "GIT_DIR and GIT_WORK_TREE",
            fx.elsewhere.clone(),
            with(&[]),
            vec![
                ("GIT_DIR", git_dir.as_path()),
                ("GIT_WORK_TREE", root.as_path()),
            ],
        ),
        (
            "GIT_DIR from a subfolder",
            root.join("sub"),
            with(&[]),
            vec![("GIT_DIR", git_dir.as_path())],
        ),
        ("linked worktree", fx.linked.clone(), with(&[]), vec![]),
        (
            "linked worktree subfolder",
            fx.linked.join("sub"),
            with(&[]),
            vec![],
        ),
        (
            "linked worktree's git directory",
            linked_git_dir.clone(),
            with(&[]),
            vec![],
        ),
    ];
    for (class, cwd, args, envs) in cases {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = git_in(&cwd, &args, &envs);
        assert!(
            !out.status.success(),
            "{class}: the update was not refused: {}",
            text(&out)
        );
        assert_eq!(rev(root, "main"), main, "{class}: main moved");
    }
}

/// AC-1 and AC-6: from every invocation class the reference-transaction
/// hook runs; the commit hooks run for an invocation that names the
/// repository by environment from a subfolder; and pre-push runs from the
/// git directory.
#[test]
fn every_hook_runs_for_every_invocation_class() {
    let fx = fixture();
    assert_eq!(
        git(&fx.root, &["config", "core.hooksPath"]),
        managed(&fx.root)
    );
    assert_every_class_refuses_the_ref_update(&fx);

    // Commit hooks: a subject the policy refuses, in the linked worktree,
    // with the repository named by environment from a subfolder.
    let linked_git_dir = PathBuf::from(git(&fx.linked, &["rev-parse", "--absolute-git-dir"]));
    let head = rev(&fx.linked, "HEAD");
    let out = git_in(
        &fx.linked.join("sub"),
        &["commit", "-q", "--allow-empty", "-m", "Bad subject."],
        &[("GIT_DIR", linked_git_dir.as_path())],
    );
    assert!(
        !out.status.success(),
        "commit was not refused: {}",
        text(&out)
    );
    assert_eq!(rev(&fx.linked, "HEAD"), head);

    // pre-push from the git directory: the protected remote branch.
    let origin_main = git(&fx.root, &["ls-remote", "origin", "refs/heads/main"]);
    let out = git_in(
        &fx.root.join(".git"),
        &["push", "-q", "origin", "refs/heads/feat/x:refs/heads/main"],
        &[],
    );
    assert!(
        !out.status.success(),
        "push was not refused: {}",
        text(&out)
    );
    assert_eq!(
        git(&fx.root, &["ls-remote", "origin", "refs/heads/main"]),
        origin_main
    );

    let doctor = codeflow(&fx.root, &["doctor", "--check", "hooks"]);
    assert!(!doctor.contains("core.hooksPath"), "{doctor}");
    let doctor = codeflow(&fx.linked, &["doctor", "--check", "hooks"]);
    assert!(!doctor.contains("core.hooksPath"), "{doctor}");
}

/// AC-3 and AC-4: a project with the relative form earlier releases wrote,
/// and a worktree created before the migration, is reported by doctor and
/// migrated by `codeflow update` with no other step.
#[test]
fn update_migrates_the_relative_form() {
    let fx = fixture();
    git(
        &fx.root,
        &["config", "core.hooksPath", ".codeflow/git-hooks"],
    );
    let doctor = codeflow(&fx.linked, &["doctor", "--check", "hooks"]);
    assert!(
        doctor.contains("core.hooksPath is the relative") && doctor.contains("codeflow update"),
        "{doctor}"
    );
    let update = codeflow(&fx.root, &["update"]);
    assert!(
        update.contains("core.hooksPath moved from .codeflow/git-hooks"),
        "{update}"
    );
    assert_eq!(
        git(&fx.linked, &["config", "core.hooksPath"]),
        managed(&fx.root)
    );
    assert_every_class_refuses_the_ref_update(&fx);
    let doctor = codeflow(&fx.linked, &["doctor", "--check", "hooks"]);
    assert!(!doctor.contains("core.hooksPath"), "{doctor}");
    // A second update changes nothing.
    let again = codeflow(&fx.root, &["update"]);
    assert!(!again.contains("core.hooksPath moved"), "{again}");
}

/// AC-3 and AC-4: after the repository moves, doctor names the stale path
/// and `codeflow update` rewrites it to the new place.
#[test]
fn a_moved_repository_is_reported_and_repaired() {
    let fx = fixture();
    git(
        &fx.root,
        &["worktree", "remove", "--force", fx.linked.to_str().unwrap()],
    );
    let before = managed(&fx.root);
    let moved = fx.root.with_file_name("moved project");
    std::fs::rename(&fx.root, &moved).unwrap();
    assert_eq!(git(&moved, &["config", "core.hooksPath"]), before);
    let doctor = codeflow(&moved, &["doctor", "--check", "hooks"]);
    assert!(
        doctor.contains("does not reach") && doctor.contains("codeflow update"),
        "{doctor}"
    );
    let update = codeflow(&moved, &["update"]);
    assert!(update.contains("core.hooksPath moved from"), "{update}");
    assert_eq!(git(&moved, &["config", "core.hooksPath"]), managed(&moved));
    let doctor = codeflow(&moved, &["doctor", "--check", "hooks"]);
    assert!(!doctor.contains("core.hooksPath"), "{doctor}");
}

/// AC-4: a path to another hook manager's folder is left as it is.
#[test]
fn update_leaves_another_managers_path() {
    let fx = fixture();
    git(&fx.root, &["config", "core.hooksPath", ".husky"]);
    let update = codeflow(&fx.root, &["update"]);
    assert!(!update.contains("core.hooksPath moved"), "{update}");
    assert_eq!(git(&fx.root, &["config", "core.hooksPath"]), ".husky");
}
