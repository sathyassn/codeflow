//! A ref git cannot read, reproduced with a mode of 000.
//!
//! A sandbox refused to read a loose ref by its name on 2026-11-03. The Claude
//! Code sandbox cannot be started from a test, but a mode of 000 gave the same
//! git symptoms when both were probed: `bad ref`, a worktree `HEAD` of zeros
//! and every file staged as new. The difference is the errno (EACCES, where
//! the sandbox gives EPERM), and both reach `doctor` and `status` as the same
//! refused read; `git::unreadable`'s unit tests inject EPERM.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

const BRANCH: &str = "task/TSK-1-a-plain-store";
const WORKTREE: &str = ".worktrees/linked";

fn run_git(root: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap()
}

fn git(root: &Path, args: &[&str]) {
    let out = run_git(root, args);
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn cli(root: &Path, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .current_dir(root)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap();
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// A root checkout on `main` and a linked worktree on [`BRANCH`].
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "t"]);
    git(root, &["config", "user.email", "t@example.test"]);
    std::fs::write(root.join("plain.txt"), "x\n").unwrap();
    // Ignored, so the layout is healthy and any warning is this finding's.
    std::fs::write(root.join(".gitignore"), ".worktrees/\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "chore: init"]);
    git(root, &["worktree", "add", "-q", WORKTREE, "-b", BRANCH]);
    dir
}

fn set_mode(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// Refuse `path`, or report that this user reads through a mode of 000.
fn refuse(path: &Path) -> bool {
    set_mode(path, 0o000);
    if std::fs::File::open(path).is_ok() {
        set_mode(path, 0o644);
        eprintln!("skipped: this user reads through mode 000");
        return false;
    }
    true
}

#[test]
fn doctor_and_status_name_a_ref_git_calls_broken_and_never_call_it_damage() {
    let dir = fixture();
    let root = dir.path();
    let linked = root.join(WORKTREE);
    let loose = root.join(".git/refs/heads").join(BRANCH);
    let shown = format!("refs/heads/{BRANCH}");

    let healthy = cli(root, &["doctor", "--check", "repo-integrity"]);
    assert!(!healthy.contains("unreadable"), "{healthy}");
    assert!(!healthy.contains("warn  repo-integrity"), "{healthy}");

    if !refuse(&loose) {
        return;
    }

    // The failure itself: git reports the intact ref as broken.
    let refs = run_git(root, &["show-ref"]);
    assert!(!refs.status.success());
    assert!(
        String::from_utf8_lossy(&refs.stderr).contains(&format!("bad ref {shown}")),
        "{}",
        String::from_utf8_lossy(&refs.stderr)
    );
    let list = String::from_utf8_lossy(&run_git(root, &["worktree", "list", "--porcelain"]).stdout)
        .into_owned();
    assert!(
        list.contains("HEAD 0000000000000000000000000000000000000000"),
        "{list}"
    );

    let doctor = cli(root, &["doctor", "--check", "repo-integrity"]);
    let status = cli(&linked, &["status"]);
    set_mode(&loose, 0o644);

    for (surface, report) in [("doctor", &doctor), ("status", &status)] {
        assert!(
            report.contains(&format!("unreadable ref: {shown}")),
            "{surface}: {report}"
        );
        assert!(
            report.contains("not evidence of repository damage"),
            "{surface}: {report}"
        );
        assert!(report.contains("git branch -m"), "{surface}: {report}");
        assert!(!report.contains("pack-refs"), "{surface}: {report}");
        assert!(!report.contains("corrupt"), "{surface}: {report}");
    }
    assert!(doctor.contains("warn  repo-integrity"), "{doctor}");

    // Restored, nothing is named again.
    let healed = cli(root, &["doctor", "--check", "repo-integrity"]);
    assert!(!healed.contains("unreadable"), "{healed}");
    assert!(!cli(&linked, &["status"]).contains("unreadable"));
    git(root, &["show-ref"]);
}

/// A tracked file stays legitimate: it is named, and the check stays healthy.
#[test]
fn a_refused_tracked_file_is_named_and_asks_nothing() {
    let dir = fixture();
    let root = dir.path();
    let linked = root.join(WORKTREE);
    let tracked = linked.join("plain.txt");
    if !refuse(&tracked) {
        return;
    }
    let doctor = cli(&linked, &["doctor", "--check", "repo-integrity"]);
    let status = cli(&linked, &["status"]);
    set_mode(&tracked, 0o644);
    assert!(
        doctor.contains("unreadable tracked files: plain.txt"),
        "{doctor}"
    );
    assert!(doctor.contains("the file names can stay"), "{doctor}");
    assert!(!doctor.contains("unreadable ref:"), "{doctor}");
    assert!(!doctor.contains("warn  repo-integrity"), "{doctor}");
    // `status` runs often, so it does not open every tracked file.
    assert!(!status.contains("unreadable"), "{status}");
}
