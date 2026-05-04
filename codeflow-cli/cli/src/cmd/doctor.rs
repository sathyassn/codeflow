//! Doctor command: diagnose infrastructure health.

use anyhow::{Context, Result};

use crate::helpers;

pub async fn run() -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_with_dir(&project_dir).await
}

/// INF-TSK-050-003 AC-01: `codeflow doctor --reset-db`.
///
/// Backs up `.state/db/codeflow.db/` to a timestamped sibling and then
/// deletes the original directory so the next codeflow command opens a
/// fresh embedded SurrealDB instance with the current schema. The
/// JSONL ledger under `.state/ledger/` is NEVER touched — it is the
/// rebuild authority and remains intact for replay.
///
/// **Active-session safety guard (rework after 2026-05-03 live-DB-wipe
/// incident):** the command refuses to proceed when the on-disk DB has
/// an `active_work` record in `in_progress` status, unless `--force` is
/// passed. The operator-visible message names `--force` so they can
/// override knowingly. When the DB cannot be opened (e.g. the schema
/// deserialization failure that motivated this command in the first
/// place), the guard is bypassed automatically — recovery from
/// corruption must still work.
///
/// User-facing flow (stdout, happy path):
/// ```text
/// codeflow doctor --reset-db
/// backing up to .state/db/codeflow.db.backup-2026-05-03T19-55-01Z
/// db reset complete
/// ```
///
/// User-facing flow (active-session abort):
/// ```text
/// codeflow doctor --reset-db
/// Error: 1 active work session(s) detected in active_work table.
/// Reset aborted to prevent data loss.
/// Pass --force to proceed anyway (will lose active session state).
/// ```
///
/// Idempotent: when no DB directory exists, prints a notice and exits 0.
pub async fn run_reset_db(force: bool) -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_reset_db_with_dir(&project_dir, force).await
}

async fn run_reset_db_with_dir(project_dir: &std::path::Path, force: bool) -> Result<()> {
    let db_dir = project_dir.join(".state").join("db").join("codeflow.db");
    if !db_dir.exists() {
        // Idempotent: nothing to back up. Operator can safely re-run.
        println!("no db at {} — nothing to reset", db_dir.display());
        return Ok(());
    }

    // Active-session safety guard. Open the on-disk DB read-only and
    // query active_work; if there's an in-progress row and --force
    // wasn't passed, abort with the message named by the lead. The
    // open is best-effort: a failed open (deserialization corruption,
    // permission issue, lock held) means we can't read state to gate
    // on, so we proceed — that case is exactly what --reset-db was
    // built to recover from. cf-knowledge-layer's 2026-05-03 incident
    // showed the absence of this guard turns a routine recovery into
    // a destructive wipe of live session state.
    if force {
        eprintln!(
            "warn: --force passed; skipping active_work safety guard. \
             Live session state in active_work will be moved to backup."
        );
    } else {
        match check_active_work(&db_dir).await {
            Ok(Some(work)) => {
                anyhow::bail!(
                    "1 active work session(s) detected in active_work table.\n\
                     Reset aborted to prevent data loss.\n\
                     Pass --force to proceed anyway (will lose active session state).\n\
                     Active work: id={} topic={:?} status={}",
                    work.id,
                    work.topic,
                    work.status
                );
            }
            Ok(None) => {
                // No active work — safe to proceed.
            }
            Err(e) => {
                // DB unreadable. Likely the deserialization-failure
                // recovery path that motivated --reset-db. Log and
                // proceed: the operator explicitly invoked the recovery
                // command, and the unreadable DB cannot be holding
                // valid live state by definition.
                eprintln!(
                    "warn: could not read active_work for safety guard ({e}); \
                     proceeding (treat as recovery from corrupt DB)"
                );
            }
        }
    }

    // Build a sortable RFC 3339 timestamp suffix. Replace ':' with '-'
    // so the resulting filename is portable across filesystems that
    // reject ':' (Windows VFS, some FUSE mounts).
    let stamp = chrono::Utc::now()
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        .replace(':', "-");
    let parent = db_dir
        .parent()
        .with_context(|| format!("computing parent of {}", db_dir.display()))?;
    let backup_dir = parent.join(format!("codeflow.db.backup-{stamp}"));

    println!("backing up to {}", backup_dir.display());
    // Recursive rename: SurrealDB's embedded backend is a directory of
    // RocksDB sst files. `fs::rename` is atomic on a single filesystem;
    // when source and target share a parent that holds true here.
    std::fs::rename(&db_dir, &backup_dir).with_context(|| {
        format!(
            "renaming {} -> {} (filesystem must be the same partition)",
            db_dir.display(),
            backup_dir.display()
        )
    })?;
    println!("db reset complete");
    println!(
        "next codeflow command will recreate the schema; replay JSONL with `codeflow ledger rebuild` if you need history"
    );
    Ok(())
}

/// Open the on-disk store at `db_dir` and query `get_active_work()`.
///
/// Returns `Ok(Some(work))` when an active work record is present and
/// `in_progress` (non-terminal); `Ok(None)` when absent or terminal;
/// `Err` on any DB-layer error (caller decides whether to proceed).
///
/// Internal helper for `run_reset_db_with_dir`. Pure function over the
/// store — no global state — so unit tests can inject the same store
/// with synthetic active_work rows. The store is dropped before this
/// function returns so the subsequent `fs::rename` does not race a
/// held file lock.
async fn check_active_work(
    db_dir: &std::path::Path,
) -> Result<Option<codeflow_core::models::ActiveWork>> {
    use codeflow_core::store::DataStore;
    let store = codeflow_core::store::SurrealStore::open(db_dir)
        .await
        .context("opening database for active_work safety check")?;
    let work = store
        .get_active_work()
        .await
        .context("querying active_work table")?;
    // Filter out terminal statuses. Only `in_progress` blocks reset.
    Ok(work.filter(|w| matches!(w.status, codeflow_core::types::ActiveWorkStatus::InProgress)))
}

async fn run_with_dir(project_dir: &std::path::Path) -> Result<()> {
    let state_dir = project_dir.join(".state");

    let session_id = helpers::resolve_session_id(project_dir).unwrap_or_default();

    let opts = codeflow_core::doctor::Options {
        db_path: state_dir
            .join("db")
            .join("codeflow.db")
            .to_string_lossy()
            .to_string(),
        ledger_dir: state_dir.join("ledger").to_string_lossy().to_string(),
        project_dir: project_dir.to_string_lossy().to_string(),
        state_dir: state_dir.to_string_lossy().to_string(),
        session_id,
        home_dir: std::env::var("HOME").unwrap_or_default(),
        look_path: None,
        exec_command: None,
    };

    let results = codeflow_core::doctor::run_all(&opts).await;

    let mut has_failure = false;
    for r in &results {
        let icon = match r.status {
            codeflow_core::doctor::Status::Pass => "ok",
            codeflow_core::doctor::Status::Warn => "warn",
            codeflow_core::doctor::Status::Fail => {
                has_failure = true;
                "FAIL"
            }
        };
        println!("[{icon}] {}: {}", r.name, r.message);
    }

    if has_failure {
        anyhow::bail!("one or more doctor checks failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeflow_core::doctor::{CheckResult, Options, Status};
    use std::time::Duration;

    #[test]
    fn test_doctor_command_exists() {
        // Compile-time verification that run is async.
        #[allow(clippy::type_complexity)]
        let _: fn() -> std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<()>>>,
        > = || Box::pin(super::run());
    }

    #[test]
    fn test_doctor_status_display_variants() {
        let pass = Status::Pass;
        let warn = Status::Warn;
        let fail = Status::Fail;
        assert!(matches!(pass, Status::Pass));
        assert!(matches!(warn, Status::Warn));
        assert!(matches!(fail, Status::Fail));
    }

    #[test]
    fn test_doctor_options_construction() {
        let opts = Options {
            db_path: "/tmp/db".to_string(),
            ledger_dir: "/tmp/ledger".to_string(),
            project_dir: "/tmp/project".to_string(),
            state_dir: "/tmp/state".to_string(),
            session_id: "ses-test".to_string(),
            home_dir: "/home/test".to_string(),
            look_path: None,
            exec_command: None,
        };
        assert_eq!(opts.db_path, "/tmp/db");
        assert_eq!(opts.session_id, "ses-test");
        assert!(opts.look_path.is_none());
    }

    #[test]
    fn test_doctor_status_icon_mapping() {
        // Verify the icon mapping matches what run() produces.
        let results = vec![
            CheckResult {
                name: "db".to_string(),
                status: Status::Pass,
                message: "ok".to_string(),
                duration: Duration::ZERO,
            },
            CheckResult {
                name: "ledger".to_string(),
                status: Status::Warn,
                message: "missing".to_string(),
                duration: Duration::ZERO,
            },
            CheckResult {
                name: "schema".to_string(),
                status: Status::Fail,
                message: "broken".to_string(),
                duration: Duration::ZERO,
            },
        ];

        let mut has_failure = false;
        let mut output = Vec::new();
        for r in &results {
            let icon = match r.status {
                Status::Pass => "ok",
                Status::Warn => "warn",
                Status::Fail => {
                    has_failure = true;
                    "FAIL"
                }
            };
            output.push(format!("[{icon}] {}: {}", r.name, r.message));
        }

        assert_eq!(output[0], "[ok] db: ok");
        assert_eq!(output[1], "[warn] ledger: missing");
        assert_eq!(output[2], "[FAIL] schema: broken");
        assert!(has_failure);
    }

    #[test]
    fn test_doctor_no_failures() {
        let results = vec![CheckResult {
            name: "check1".to_string(),
            status: Status::Pass,
            message: "good".to_string(),
            duration: Duration::ZERO,
        }];

        let mut has_failure = false;
        for r in &results {
            if matches!(r.status, Status::Fail) {
                has_failure = true;
            }
        }
        assert!(!has_failure);
    }

    #[tokio::test]
    async fn test_doctor_run_all_with_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".state");
        std::fs::create_dir_all(state_dir.join("db")).unwrap();
        std::fs::create_dir_all(state_dir.join("ledger")).unwrap();

        let opts = Options {
            db_path: state_dir
                .join("db")
                .join("codeflow.db")
                .to_string_lossy()
                .to_string(),
            ledger_dir: state_dir.join("ledger").to_string_lossy().to_string(),
            project_dir: dir.path().to_string_lossy().to_string(),
            state_dir: state_dir.to_string_lossy().to_string(),
            session_id: String::new(),
            home_dir: dir.path().to_string_lossy().to_string(),
            look_path: None,
            exec_command: None,
        };

        let results = codeflow_core::doctor::run_all(&opts).await;
        assert!(!results.is_empty());
    }

    #[tokio::test]
    async fn test_doctor_run_with_dir() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".state");
        std::fs::create_dir_all(state_dir.join("db")).unwrap();
        std::fs::create_dir_all(state_dir.join("ledger")).unwrap();

        // run_with_dir exercises the full run() path minus detect_project_dir.
        // Some checks may fail (expected in a temp dir), so we just verify it runs.
        let _result = run_with_dir(dir.path()).await;
    }

    // ─── AC-01 rework: active-session safety guard ────────────────────────
    //
    // INF-TSK-050-003 AC-01 rework after the 2026-05-03 live-DB-wipe
    // incident. These tests cover all three required branches:
    //   1. active_work has in_progress row + no --force → abort
    //   2. active_work has in_progress row + --force=true → proceeds
    //   3. no active_work → proceeds
    // Plus a negative case: terminal status (Complete) does not trigger
    // the guard.

    use codeflow_core::store::DataStore;

    /// Helper: create an isolated tempdir with an empty `.state/db/codeflow.db`
    /// directory (SurrealStore::open creates it on demand). Returns the
    /// tempdir handle so the caller can keep the directory alive AND the
    /// project_dir path used for `run_reset_db_with_dir`.
    fn make_isolated_project() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().to_path_buf();
        std::fs::create_dir_all(project.join(".state").join("db")).expect("mkdir state/db");
        (dir, project)
    }

    /// Helper: open a SurrealStore at the project's `.state/db/codeflow.db`,
    /// apply schema, and seed an in-progress active_work row.
    async fn seed_active_work_in_progress(project_dir: &std::path::Path) {
        let db_dir = project_dir.join(".state").join("db").join("codeflow.db");
        let store = codeflow_core::store::SurrealStore::open(&db_dir)
            .await
            .expect("open store");
        store.apply_schema().await.expect("apply schema");
        let now = chrono::Utc::now().to_rfc3339();
        let work = codeflow_core::models::ActiveWork {
            id: "work-rework-test".to_string(),
            task_id: Some("INF-TSK-050-003".to_string()),
            topic: "rework safety guard test".to_string(),
            status: codeflow_core::types::ActiveWorkStatus::InProgress,
            branch: Some("fix/inf-tsk-050-003-tier-a-correctness-fixes".to_string()),
            scope: vec![],
            deliverables: vec![],
            agent: None,
            session_id: None,
            current_stage: None,
            team_name: None,
            created_at: now.clone(),
            updated_at: now,
        };
        store.set_active_work(&work).await.expect("set active work");
        // Drop store explicitly so the file lock releases before
        // run_reset_db_with_dir tries to rename the directory.
        drop(store);
    }

    #[tokio::test]
    async fn test_reset_db_aborts_when_active_work_in_progress_and_no_force() {
        let (_dir_guard, project) = make_isolated_project();
        seed_active_work_in_progress(&project).await;

        // No --force; guard must fire.
        let result = run_reset_db_with_dir(&project, false).await;
        let err = result.expect_err("guard must abort with active in_progress work");
        let msg = format!("{err}");
        assert!(
            msg.contains("active work session(s) detected"),
            "error must name the active_work cause; got: {msg}"
        );
        assert!(
            msg.contains("--force"),
            "error must name --force as the override; got: {msg}"
        );
        assert!(
            msg.contains("Reset aborted"),
            "error must announce the abort; got: {msg}"
        );

        // Critical post-condition: the DB directory MUST still exist
        // (the guard fired BEFORE rename). This is the actual
        // protection against the wipe scenario.
        assert!(
            project
                .join(".state")
                .join("db")
                .join("codeflow.db")
                .exists(),
            "DB directory must remain intact when the guard aborts"
        );
        // And no backup directory must have been created.
        let backups: Vec<_> = std::fs::read_dir(project.join(".state").join("db"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("codeflow.db.backup-")
            })
            .collect();
        assert_eq!(backups.len(), 0, "no backup must be created when aborted");
    }

    #[tokio::test]
    async fn test_reset_db_proceeds_with_force_even_when_active_work_present() {
        let (_dir_guard, project) = make_isolated_project();
        seed_active_work_in_progress(&project).await;

        // --force=true; guard must be skipped.
        let result = run_reset_db_with_dir(&project, true).await;
        result.expect("--force must override the active_work guard");

        // DB directory is gone, backup sibling exists.
        assert!(
            !project
                .join(".state")
                .join("db")
                .join("codeflow.db")
                .exists(),
            "original DB must be removed after --force --reset-db"
        );
        let backups: Vec<_> = std::fs::read_dir(project.join(".state").join("db"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("codeflow.db.backup-")
            })
            .collect();
        assert_eq!(
            backups.len(),
            1,
            "exactly one backup sibling must exist when --force proceeds"
        );
    }

    #[tokio::test]
    async fn test_reset_db_proceeds_when_no_active_work() {
        let (_dir_guard, project) = make_isolated_project();
        // Open + close the store WITHOUT seeding any active_work.
        let db_dir = project.join(".state").join("db").join("codeflow.db");
        let store = codeflow_core::store::SurrealStore::open(&db_dir)
            .await
            .expect("open");
        store.apply_schema().await.expect("schema");
        drop(store);

        // No --force needed; guard observes no active_work and proceeds.
        let result = run_reset_db_with_dir(&project, false).await;
        result.expect("must proceed when active_work is empty");

        assert!(
            !db_dir.exists(),
            "original DB must be removed when no active_work present"
        );
    }

    #[tokio::test]
    async fn test_reset_db_proceeds_when_active_work_terminal_status() {
        // Terminal statuses (Complete, Blocked) do NOT block the guard.
        // Only `in_progress` is the load-bearing condition.
        let (_dir_guard, project) = make_isolated_project();
        let db_dir = project.join(".state").join("db").join("codeflow.db");
        let store = codeflow_core::store::SurrealStore::open(&db_dir)
            .await
            .expect("open");
        store.apply_schema().await.expect("schema");
        let now = chrono::Utc::now().to_rfc3339();
        let work = codeflow_core::models::ActiveWork {
            id: "work-complete".to_string(),
            task_id: None,
            topic: "completed work".to_string(),
            status: codeflow_core::types::ActiveWorkStatus::Complete,
            branch: None,
            scope: vec![],
            deliverables: vec![],
            agent: None,
            session_id: None,
            current_stage: None,
            team_name: None,
            created_at: now.clone(),
            updated_at: now,
        };
        store.set_active_work(&work).await.expect("set work");
        drop(store);

        // No --force; terminal status must not trigger the guard.
        let result = run_reset_db_with_dir(&project, false).await;
        result.expect("Complete status must not block reset");
        assert!(!db_dir.exists(), "DB removed despite terminal active_work");
    }

    #[tokio::test]
    async fn test_check_active_work_returns_none_for_empty_db() {
        let (_dir_guard, project) = make_isolated_project();
        let db_dir = project.join(".state").join("db").join("codeflow.db");
        let store = codeflow_core::store::SurrealStore::open(&db_dir)
            .await
            .expect("open");
        store.apply_schema().await.expect("schema");
        drop(store);

        let result = check_active_work(&db_dir).await.expect("query ok");
        assert!(result.is_none(), "empty active_work table must return None");
    }
}
