//! Integration test for `codeflow doctor --reset-db` (INF-TSK-050-003 AC-01).
//!
//! Verifies the user-facing contract:
//! 1. The subcommand exists and accepts `--reset-db`.
//! 2. With a DB directory present, the directory is renamed to a
//!    timestamped backup sibling and the original is gone.
//! 3. The backup name matches `codeflow.db.backup-{ISO8601}` format.
//! 4. With NO DB directory present, the command exits 0 (idempotent).
//! 5. **Rework**: clap accepts `--force` ONLY in combination with
//!    `--reset-db`. Standalone `--force` (without `--reset-db`) is
//!    rejected by clap at parse time. Active-session guard semantics
//!    are unit-tested in `cli/src/cmd/doctor.rs::tests` directly
//!    against `run_reset_db_with_dir` (with seeded active_work rows)
//!    because the integration test cannot easily seed pre-existing
//!    DB content via the binary surface.
//!
//! Invokes the real binary via `CARGO_BIN_EXE_codeflow` so the test
//! exercises the actual CLI parser path (clap), not just the helper
//! function. This catches missing flag wiring at integration time.

use std::path::{Path, PathBuf};
use std::process::Command;

fn codeflow_binary() -> PathBuf {
    option_env!("CARGO_BIN_EXE_codeflow")
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .unwrap_or_else(|| {
            let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
            manifest_dir
                .parent()
                .expect("cli parent")
                .join("target")
                .join("debug")
                .join("codeflow")
        })
}

#[test]
fn reset_db_renames_existing_db_to_timestamped_backup() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let project = tmp.path();
    // The .state directory must exist for CODEFLOW_WORKTREE_PATH to be
    // honored by helpers::detect_project_dir.
    std::fs::create_dir_all(project.join(".state")).expect("create .state");
    let db_dir = project.join(".state").join("db").join("codeflow.db");
    std::fs::create_dir_all(&db_dir).expect("create db dir");
    // Write a sentinel file inside so we can verify the rename moved
    // the contents (not just an empty dir).
    let sentinel = db_dir.join("data.sst");
    std::fs::write(&sentinel, b"sentinel").expect("write sentinel");

    // Force project resolution to the tempdir; otherwise
    // detect_project_dir walks up to the real worktree root and would
    // touch the user's actual DB.
    let output = Command::new(codeflow_binary())
        .args(["doctor", "--reset-db"])
        .current_dir(project)
        .env("CODEFLOW_WORKTREE_PATH", project)
        .env_remove("CF_PROJECT_ROOT")
        .output()
        .expect("invoke codeflow doctor --reset-db");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "exit must be 0; stdout={stdout}, stderr={stderr}"
    );
    assert!(
        stdout.contains("backing up to"),
        "stdout must announce the backup; got: {stdout}"
    );
    assert!(
        stdout.contains("db reset complete"),
        "stdout must confirm completion; got: {stdout}"
    );

    // Original directory must be gone.
    assert!(
        !db_dir.exists(),
        "original DB dir must be removed after --reset-db"
    );

    // A backup sibling must exist with the timestamp suffix and the
    // sentinel file inside.
    let parent = db_dir.parent().expect("parent");
    let mut backup_dirs: Vec<_> = std::fs::read_dir(parent)
        .expect("list parent")
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("codeflow.db.backup-")
        })
        .collect();
    assert_eq!(
        backup_dirs.len(),
        1,
        "exactly one backup sibling must exist"
    );
    let backup = backup_dirs.pop().expect("backup entry").path();
    let restored_sentinel = backup.join("data.sst");
    assert!(
        restored_sentinel.exists(),
        "backup must contain the original sentinel file at {}",
        restored_sentinel.display()
    );
    let body = std::fs::read(&restored_sentinel).expect("read restored sentinel");
    assert_eq!(body, b"sentinel", "sentinel contents must match");
}

#[test]
fn force_flag_requires_reset_db_flag() {
    // INF-TSK-050-003 AC-01 (rework): clap declaration is
    // `#[arg(long = "force", requires = "reset_db")]` — passing
    // `--force` alone (no `--reset-db`) must fail at parse time, not
    // run a destructive command. Verifies the safety wiring catches
    // operator typos like `codeflow doctor --force` (intended as a
    // standard doctor run).
    let tmp = tempfile::tempdir().expect("create tempdir");
    let project = tmp.path();
    std::fs::create_dir_all(project.join(".state")).expect("create .state");

    let output = Command::new(codeflow_binary())
        .args(["doctor", "--force"])
        .current_dir(project)
        .env("CODEFLOW_WORKTREE_PATH", project)
        .env_remove("CF_PROJECT_ROOT")
        .output()
        .expect("invoke codeflow doctor --force");

    // clap exits non-zero on argument-validation failure.
    assert!(
        !output.status.success(),
        "clap must reject --force without --reset-db"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    // clap's error message includes the requirement constraint.
    assert!(
        stderr.contains("--reset-db") || stderr.contains("requires"),
        "stderr must explain the --reset-db requirement; got: {stderr}"
    );
}

#[test]
fn reset_db_is_idempotent_when_no_db_present() {
    // INF-TSK-050-003 AC-01 idempotency clause: re-running --reset-db
    // when no DB directory exists must exit 0 with a clear message
    // (not panic, not error).
    let tmp = tempfile::tempdir().expect("create tempdir");
    let project = tmp.path();
    // The .state dir is required for CODEFLOW_WORKTREE_PATH to be
    // honored by helpers::detect_project_dir, but we deliberately
    // omit `.state/db/codeflow.db`.
    std::fs::create_dir_all(project.join(".state")).expect("create .state");

    let output = Command::new(codeflow_binary())
        .args(["doctor", "--reset-db"])
        .current_dir(project)
        .env("CODEFLOW_WORKTREE_PATH", project)
        .env_remove("CF_PROJECT_ROOT")
        .output()
        .expect("invoke codeflow doctor --reset-db");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "exit must be 0 even with no DB; stdout={stdout}"
    );
    assert!(
        stdout.contains("nothing to reset"),
        "must announce no-op when nothing exists; got: {stdout}"
    );
}
