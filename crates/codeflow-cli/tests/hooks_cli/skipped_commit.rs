//! Skipping commit hooks leaves the installed reference hook active.

use super::{codeflow, git_command, init_repo};

#[test]
fn doctor_reports_unverified_remote_ref_protection_without_a_host() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/doctor");
    let result = codeflow()
        .args(["doctor", "--check", "remote-ref-protection"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    let stdout = String::from_utf8_lossy(&result.stdout);
    for text in [
        "remote-ref-protection",
        "main",
        "master",
        "cannot verify",
        "push hook is skipped",
        "codeflow remote protect",
    ] {
        assert!(stdout.contains(text), "{text}: {stdout}");
    }
}

#[test]
fn installed_reference_hook_refuses_a_commit_with_commit_hooks_skipped() {
    // The skip is test data, used only in this disposable repository.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    let installed = codeflow()
        .args(["init", "--minimal", "--yes"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(installed.status.success(), "{installed:?}");
    let binary_dir = std::path::Path::new(env!("CARGO_BIN_EXE_codeflow"))
        .parent()
        .unwrap();
    let path = std::env::join_paths(
        std::iter::once(binary_dir.to_path_buf()).chain(
            std::env::var_os("PATH")
                .iter()
                .flat_map(std::env::split_paths),
        ),
    )
    .unwrap();
    let run = |args: &[&str]| {
        git_command()
            .args(args)
            .current_dir(dir.path())
            .env("PATH", &path)
            .env_remove("CODEFLOW_HOOK_BINARY")
            .output()
            .unwrap()
    };
    let before = run(&["rev-parse", "HEAD"]);
    assert!(before.status.success(), "{before:?}");
    // Without the skip, the installed pre-commit hook refuses first.
    let normal = run(&["commit", "--allow-empty", "-m", "test: protected commit"]);
    assert!(!normal.status.success(), "{normal:?}");
    assert!(
        String::from_utf8_lossy(&normal.stderr).contains("git.commit_to_protected"),
        "{normal:?}"
    );

    let skipped = run(&[
        "commit",
        "--no-verify",
        "--allow-empty",
        "-m",
        "test: protected commit",
    ]);
    assert!(!skipped.status.success(), "{skipped:?}");
    let stderr = String::from_utf8_lossy(&skipped.stderr);
    assert!(stderr.contains("git.local_ref_protection"), "{stderr}");
    assert!(!stderr.contains("git.commit_to_protected"), "{stderr}");
    let after = run(&["rev-parse", "HEAD"]);
    assert!(after.status.success(), "{after:?}");
    assert_eq!(after.stdout, before.stdout, "protected branch moved");

    // The same skip on a feature branch succeeds, including an otherwise
    // invalid message. This proves the skip took effect in the fixture. The
    // switch moves a ref, so the installed reference hook runs: it needs the
    // built binary first on PATH, as every other git step here does.
    let switched = run(&["switch", "-c", "feat/control"]);
    assert!(switched.status.success(), "{switched:?}");
    let control = run(&[
        "commit",
        "--no-verify",
        "--allow-empty",
        "-m",
        "fixture control",
    ]);
    assert!(control.status.success(), "{control:?}");
}
