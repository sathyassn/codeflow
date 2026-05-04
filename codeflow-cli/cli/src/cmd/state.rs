//! State command: display session state, set/clear active task.

use std::path::Path;

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use codeflow_core::session::active_task::{
    ActiveTask, active_task_path_resolved, clear_active_task_worktree_aware,
    set_active_task_worktree_aware,
};
#[cfg(not(test))]
use codeflow_core::store::{DataStore, SurrealStore};
#[cfg(not(test))]
use codeflow_core::types::FormatId as FormatIdType;
use codeflow_core::{EpicId, FormatId, SessionId, TaskId};

use crate::helpers;

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum StateCommand {
    /// Show current session state (default)
    Show,
    /// Set the active task in .state/runtime/active-task.json
    SetActiveTask {
        /// Task ID (ULID, e.g., task-01KXYZ...)
        #[arg(long)]
        task_id: String,
        /// Epic ID (ULID, e.g., epic-01KXYZ...)
        #[arg(long)]
        epic_id: String,
        /// Task format ID (e.g., INF-TSK-044-008)
        #[arg(long)]
        task_format_id: String,
        /// Epic format ID (e.g., INF-EPC-044)
        #[arg(long)]
        epic_format_id: String,
        /// Task title
        #[arg(long)]
        title: String,
        /// Task status (default: in_progress)
        #[arg(long, default_value = "in_progress")]
        status: String,
        /// Branch name
        #[arg(long)]
        branch: String,
        /// Session ID
        #[arg(long)]
        session_id: String,
        /// Work type (e.g., FEAT, FIX, PLAN)
        #[arg(long)]
        work_type: Option<String>,
        /// Scope policy: soft, hard, or permissive (overrides DB value when set)
        #[arg(long)]
        scope_policy: Option<String>,
        /// File scope as a JSON array (overrides DB value when set).
        ///
        /// Example: `--file-scope '["src/foo.rs","src/bar/"]'`
        #[arg(long)]
        file_scope: Option<String>,
        /// Target branch for PRs
        #[arg(long)]
        target_branch: Option<String>,
        /// Enable auto-merge for this task's PR
        #[arg(long)]
        auto_merge: Option<bool>,
        /// Epic update strategy (orchestrator or none)
        #[arg(long)]
        epic_update: Option<String>,
    },
    /// Remove the active task file
    ClearActiveTask,
}

pub async fn run(command: Option<StateCommand>) -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_with_dir(&project_dir, command).await
}

async fn run_with_dir(project_dir: &Path, command: Option<StateCommand>) -> Result<()> {
    match command {
        None | Some(StateCommand::Show) => run_show(project_dir),
        Some(StateCommand::SetActiveTask {
            task_id,
            epic_id,
            task_format_id,
            epic_format_id,
            title,
            status,
            branch,
            session_id,
            work_type,
            scope_policy,
            file_scope,
            target_branch,
            auto_merge,
            epic_update,
        }) => {
            run_set_active_task(
                project_dir,
                SetActiveTaskArgs {
                    task_id,
                    epic_id,
                    task_format_id,
                    epic_format_id,
                    title,
                    status,
                    branch,
                    session_id,
                    work_type,
                    scope_policy_override: scope_policy,
                    file_scope_override: file_scope,
                    target_branch,
                    auto_merge,
                    epic_update,
                },
            )
            .await
        }
        Some(StateCommand::ClearActiveTask) => {
            clear_active_task_worktree_aware(project_dir).context("clearing active task")?;
            println!("active-task cleared");
            Ok(())
        }
    }
}

struct SetActiveTaskArgs {
    task_id: String,
    epic_id: String,
    task_format_id: String,
    epic_format_id: String,
    title: String,
    status: String,
    branch: String,
    session_id: String,
    work_type: Option<String>,
    scope_policy_override: Option<String>,
    file_scope_override: Option<String>,
    target_branch: Option<String>,
    auto_merge: Option<bool>,
    epic_update: Option<String>,
}

async fn run_set_active_task(project_dir: &Path, args: SetActiveTaskArgs) -> Result<()> {
    // Look up the task from SurrealDB to populate scope_policy and file_scope.
    // CLI flag values, if present, override the DB-derived values (preserves
    // test/override path per AC-05b).
    //
    // REV-NOTE-004: DB-missing case returns Ok((None, None)) silently (handled
    // in `lookup_task_scope`); only real query errors reach this Err arm. The
    // warning differentiates by surfacing the offending task ID and pointing
    // the operator at the explicit CLI overrides as the recovery path,
    // including the case where the operator may need --file-scope to satisfy
    // the validate_scope_policy hard+empty guard below.
    let (db_scope_policy, db_file_scope) = lookup_task_scope(project_dir, &args.task_format_id)
        .await
        .unwrap_or_else(|e| {
            let task_format_id = &args.task_format_id;
            let has_policy = args.scope_policy_override.is_some();
            let has_scope = args.file_scope_override.is_some();
            let recovery = if has_policy && has_scope {
                "proceeding with the explicit CLI overrides".to_string()
            } else if has_policy || has_scope {
                "proceeding with partial CLI overrides; if this is a hard-policy task you may need to add --file-scope or --scope-policy explicitly to satisfy the write-time validation".to_string()
            } else {
                "proceeding with no overrides; if validation fails below, re-run with --scope-policy <soft|hard|permissive> and --file-scope '[\"...\"]' explicitly".to_string()
            };
            eprintln!(
                "warning: DB query failed for task {task_format_id}: {e}\n         {recovery}",
            );
            (None, None)
        });

    let scope_policy = args.scope_policy_override.or(db_scope_policy);
    let file_scope = parse_file_scope_override(args.file_scope_override.as_deref())
        .context("parsing --file-scope JSON")?
        .or(db_file_scope);

    // Option alpha: hard scope_policy with empty/missing file_scope is
    // rejected at WRITE time. The hook never observes this configuration.
    validate_scope_policy(
        &args.task_format_id,
        scope_policy.as_deref(),
        file_scope.as_ref(),
    )?;

    let task = ActiveTask {
        task_id: TaskId::new_unchecked(args.task_id),
        epic_id: Some(EpicId::new_unchecked(args.epic_id)),
        task_format_id: Some(FormatId::new_unchecked(args.task_format_id.clone())),
        epic_format_id: Some(FormatId::new_unchecked(args.epic_format_id)),
        title: Some(args.title),
        status: Some(args.status),
        branch: Some(args.branch),
        session_id: Some(SessionId::new_unchecked(args.session_id)),
        created_at: Some(chrono::Utc::now().to_rfc3339()),
        updated_at: Some(chrono::Utc::now().to_rfc3339()),
        work_type: args.work_type,
        scope_policy,
        file_scope,
        target_branch: args.target_branch,
        auto_merge: args.auto_merge,
        epic_update: args.epic_update,
        current_stage: None,
        team_name: None,
    };
    set_active_task_worktree_aware(project_dir, &task).context("setting active task")?;
    println!("active-task set: {}", args.task_format_id);
    Ok(())
}

/// Parse the --file-scope JSON literal into `Option<Vec<String>>`.
/// `None` means no override; `Some(vec![])` means explicitly cleared.
fn parse_file_scope_override(raw: Option<&str>) -> Result<Option<Vec<String>>> {
    let Some(s) = raw else {
        return Ok(None);
    };
    let parsed: Vec<String> = serde_json::from_str(s)
        .with_context(|| format!("--file-scope must be a JSON array of strings, got: {s:?}"))?;
    Ok(Some(parsed))
}

/// Canonical set of valid `scope_policy` values.
///
/// Values are matched case-sensitively. The hook (`try_acquire_claim`) and
/// this validator share the same vocabulary; any drift would silently route
/// unknown values to the soft fallback (WS-SEC iter-1 finding #2: A05
/// Security Misconfiguration).
const VALID_SCOPE_POLICIES: &[&str] = &["soft", "hard", "permissive"];

/// Reject malformed `scope_policy` values and `hard` with empty `file_scope`.
///
/// Two checks, in order:
/// 1. **Vocabulary check (WS-SEC iter-1 finding #2):** the value must be one
///    of `VALID_SCOPE_POLICIES`. A typo like `"Hard"` (wrong case) or
///    `"strict"` is rejected at write time so the operator sees the error
///    immediately. Without this, the hook silently routes unknown values to
///    soft via the catch-all arm in `try_acquire_claim`, defeating
///    `scope_policy=hard` enforcement.
/// 2. **Option-α guard:** `hard` with empty/missing `file_scope` is rejected.
///    Hard mode without a declared scope is meaningless; without this guard
///    the hook would fall through to the in-scope branch on
///    `is_in_scope([])` returning true.
fn validate_scope_policy(
    task_format_id: &str,
    scope_policy: Option<&str>,
    file_scope: Option<&Vec<String>>,
) -> Result<()> {
    if let Some(policy) = scope_policy {
        if !VALID_SCOPE_POLICIES.contains(&policy) {
            bail!(
                "invalid scope_policy '{policy}' for task {task_format_id}; \
                 expected one of: {} (case-sensitive). \
                 Fix task definition or pass --scope-policy on the CLI.",
                VALID_SCOPE_POLICIES.join(", "),
            );
        }
        if policy == "hard" {
            let is_empty = file_scope.is_none_or(Vec::is_empty);
            if is_empty {
                bail!(
                    "scope_policy=hard requires non-empty file_scope; \
                     task {task_format_id} has empty file_scope -- \
                     fix task definition (populate the `file_scope` array \
                     in the task markdown) or pass --file-scope on the CLI",
                );
            }
        }
    }
    Ok(())
}

#[cfg(not(test))]
async fn lookup_task_scope(
    project_dir: &Path,
    task_format_id: &str,
) -> Result<(Option<String>, Option<Vec<String>>)> {
    let db_path = project_dir.join(".state").join("db").join("codeflow.db");
    if !db_path.exists() {
        // Database not yet initialized; treat as missing data.
        return Ok((None, None));
    }
    let store = SurrealStore::open(&db_path)
        .await
        .context("opening database for task lookup")?;
    let fmt_id = FormatIdType::new_unchecked(task_format_id);
    match store.get_task_by_format_id(&fmt_id).await {
        Ok(Some(task)) => Ok((Some(task.scope_policy), Some(task.file_scope))),
        Ok(None) => Ok((None, None)),
        Err(e) => Err(anyhow::anyhow!("task lookup: {e}")),
    }
}

#[cfg(test)]
#[allow(clippy::unused_async)]
async fn lookup_task_scope(
    _project_dir: &Path,
    _task_format_id: &str,
) -> Result<(Option<String>, Option<Vec<String>>)> {
    // Tests bypass the DB to avoid SurrealDB initialization under coverage.
    // The CLI override path (--scope-policy, --file-scope) is exercised via
    // SetActiveTaskArgs directly. `async` is preserved so the production
    // and test signatures match exactly (the caller `.await`s either way).
    Ok((None, None))
}

fn run_show(project_dir: &Path) -> Result<()> {
    // Show current session ID.
    match helpers::resolve_session_id(project_dir) {
        Ok(sid) => println!("session: {sid}"),
        Err(_) => println!("session: none"),
    }

    // Show active task. Resolve path through the canonical resolver so the
    // worktree-local file is preferred when CODEFLOW_WORKTREE_PATH is set,
    // with fallback to the project-level path.
    let worktree_path = std::env::var("CODEFLOW_WORKTREE_PATH").ok();
    let active_task_path = active_task_path_resolved(project_dir, worktree_path.as_deref());
    if active_task_path.exists() {
        let content = std::fs::read_to_string(&active_task_path).context("reading active task")?;
        println!("active-task: {content}");
    } else {
        println!("active-task: none");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_state_no_session_no_active_task() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path(), None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_state_with_active_task_file() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::write(
            runtime_dir.join("active-task.json"),
            r#"{"task_id":"TSK-001"}"#,
        )
        .unwrap();
        let result = run_with_dir(dir.path(), None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_state_with_session_and_active_task() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".state");
        let runtime_dir = state_dir.join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::write(
            runtime_dir.join("codeflow-env.sh"),
            "export CODEFLOW_SESSION_ID='ses-teststatewithtask12345'\nexport CF_PROJECT_ROOT='/tmp/test'\n",
        )
        .unwrap();
        std::fs::write(
            runtime_dir.join("active-task.json"),
            r#"{"task_id":"TSK-002"}"#,
        )
        .unwrap();
        let result = run_with_dir(dir.path(), None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_state_set_active_task_creates_file_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(
            dir.path(),
            Some(StateCommand::SetActiveTask {
                task_id: "task-abc123".into(),
                epic_id: "epic-def456".into(),
                task_format_id: "INF-TSK-044-008".into(),
                epic_format_id: "INF-EPC-044".into(),
                title: "State Write Protection".into(),
                status: "in_progress".into(),
                branch: "fix/state-write-protection".into(),
                session_id: "ses-test123".into(),
                work_type: Some("FIX".into()),
                scope_policy: Some("soft".into()),
                file_scope: None,
                target_branch: None,
                auto_merge: None,
                epic_update: None,
            }),
        )
        .await;
        assert!(result.is_ok(), "set-active-task should succeed: {result:?}");

        // Verify the file was written with correct fields.
        let path = dir.path().join(".state/runtime/active-task.json");
        assert!(path.exists(), "active-task.json should exist");

        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(content["task_id"], "task-abc123");
        assert_eq!(content["epic_id"], "epic-def456");
        assert_eq!(content["task_format_id"], "INF-TSK-044-008");
        assert_eq!(content["epic_format_id"], "INF-EPC-044");
        assert_eq!(content["title"], "State Write Protection");
        assert_eq!(content["status"], "in_progress");
        assert_eq!(content["branch"], "fix/state-write-protection");
        assert_eq!(content["session_id"], "ses-test123");
        assert_eq!(content["work_type"], "FIX");
        assert_eq!(content["scope_policy"], "soft");
        // created_at and updated_at should be present.
        assert!(content["created_at"].is_string());
        assert!(content["updated_at"].is_string());
        // No temp file should remain.
        assert!(
            !dir.path()
                .join(".state/runtime/.active-task.json.tmp")
                .exists(),
            "temp file should not remain"
        );
    }

    #[tokio::test]
    async fn test_state_clear_active_task_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::write(
            runtime_dir.join("active-task.json"),
            r#"{"task_id":"TSK-001"}"#,
        )
        .unwrap();

        let result = run_with_dir(dir.path(), Some(StateCommand::ClearActiveTask)).await;
        assert!(result.is_ok(), "clear should succeed: {result:?}");
        assert!(
            !runtime_dir.join("active-task.json").exists(),
            "file should be removed"
        );
    }

    #[tokio::test]
    async fn test_state_clear_active_task_absent_file() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path(), Some(StateCommand::ClearActiveTask)).await;
        assert!(
            result.is_ok(),
            "clear absent file should succeed: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_state_show_subcommand() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path(), Some(StateCommand::Show)).await;
        assert!(result.is_ok(), "show subcommand should succeed: {result:?}");
    }

    #[tokio::test]
    async fn test_set_active_task_hard_with_explicit_file_scope_via_cli() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(
            dir.path(),
            Some(StateCommand::SetActiveTask {
                task_id: "task-hard1".into(),
                epic_id: "epic-h1".into(),
                task_format_id: "INF-TSK-050-010".into(),
                epic_format_id: "INF-EPC-050".into(),
                title: "Hard scope task".into(),
                status: "in_progress".into(),
                branch: "fix/hard".into(),
                session_id: "ses-hard1".into(),
                work_type: Some("FIX".into()),
                scope_policy: Some("hard".into()),
                file_scope: Some(r#"["src/in.rs","src/sub/"]"#.into()),
                target_branch: None,
                auto_merge: None,
                epic_update: None,
            }),
        )
        .await;
        assert!(
            result.is_ok(),
            "should succeed with valid file_scope: {result:?}"
        );

        let path = dir.path().join(".state/runtime/active-task.json");
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(content["scope_policy"], "hard");
        assert!(content["file_scope"].is_array());
        let scope: Vec<String> = serde_json::from_value(content["file_scope"].clone()).unwrap();
        assert_eq!(scope, vec!["src/in.rs".to_string(), "src/sub/".to_string()]);
    }

    #[tokio::test]
    async fn test_set_active_task_hard_without_file_scope_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(
            dir.path(),
            Some(StateCommand::SetActiveTask {
                task_id: "task-hard2".into(),
                epic_id: "epic-h2".into(),
                task_format_id: "INF-TSK-BAD-001".into(),
                epic_format_id: "INF-EPC-BAD".into(),
                title: "Hard scope task missing file_scope".into(),
                status: "in_progress".into(),
                branch: "fix/bad".into(),
                session_id: "ses-bad2".into(),
                work_type: Some("FIX".into()),
                scope_policy: Some("hard".into()),
                file_scope: None,
                target_branch: None,
                auto_merge: None,
                epic_update: None,
            }),
        )
        .await;
        assert!(result.is_err(), "should reject hard + empty file_scope");
        let err_msg = result.err().unwrap().to_string();
        assert!(
            err_msg.contains("scope_policy=hard"),
            "error message must mention scope_policy=hard, got: {err_msg}"
        );
        assert!(
            err_msg.contains("INF-TSK-BAD-001"),
            "error message must name the task, got: {err_msg}"
        );
        // active-task.json must NOT have been written.
        assert!(
            !dir.path().join(".state/runtime/active-task.json").exists(),
            "must not write active-task.json on rejection"
        );
    }

    #[tokio::test]
    async fn test_set_active_task_hard_with_empty_file_scope_array_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(
            dir.path(),
            Some(StateCommand::SetActiveTask {
                task_id: "task-hard3".into(),
                epic_id: "epic-h3".into(),
                task_format_id: "INF-TSK-EMPTY-002".into(),
                epic_format_id: "INF-EPC-EMPTY".into(),
                title: "Hard scope task with empty array".into(),
                status: "in_progress".into(),
                branch: "fix/empty".into(),
                session_id: "ses-empty3".into(),
                work_type: Some("FIX".into()),
                scope_policy: Some("hard".into()),
                file_scope: Some("[]".into()),
                target_branch: None,
                auto_merge: None,
                epic_update: None,
            }),
        )
        .await;
        assert!(result.is_err(), "should reject hard + empty array");
    }

    #[tokio::test]
    async fn test_set_active_task_soft_without_file_scope_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(
            dir.path(),
            Some(StateCommand::SetActiveTask {
                task_id: "task-soft1".into(),
                epic_id: "epic-s1".into(),
                task_format_id: "INF-TSK-SOFT-001".into(),
                epic_format_id: "INF-EPC-SOFT".into(),
                title: "Soft scope".into(),
                status: "in_progress".into(),
                branch: "fix/soft".into(),
                session_id: "ses-soft1".into(),
                work_type: Some("FIX".into()),
                scope_policy: Some("soft".into()),
                file_scope: None,
                target_branch: None,
                auto_merge: None,
                epic_update: None,
            }),
        )
        .await;
        assert!(result.is_ok(), "soft + empty file_scope must be allowed");
    }

    #[test]
    fn test_parse_file_scope_override_none() {
        assert_eq!(parse_file_scope_override(None).unwrap(), None);
    }

    #[test]
    fn test_parse_file_scope_override_valid_array() {
        assert_eq!(
            parse_file_scope_override(Some(r#"["a.rs","b/"]"#)).unwrap(),
            Some(vec!["a.rs".to_string(), "b/".to_string()])
        );
    }

    #[test]
    fn test_parse_file_scope_override_empty_array() {
        assert_eq!(parse_file_scope_override(Some("[]")).unwrap(), Some(vec![]));
    }

    #[test]
    fn test_parse_file_scope_override_invalid_json() {
        assert!(parse_file_scope_override(Some("not json")).is_err());
    }

    #[test]
    fn test_validate_scope_policy_hard_with_scope_ok() {
        let scope = vec!["src/foo.rs".to_string()];
        assert!(validate_scope_policy("INF-TSK-001", Some("hard"), Some(&scope)).is_ok());
    }

    #[test]
    fn test_validate_scope_policy_hard_with_empty_scope_err() {
        let scope: Vec<String> = vec![];
        let err = validate_scope_policy("INF-TSK-001", Some("hard"), Some(&scope)).unwrap_err();
        assert!(err.to_string().contains("INF-TSK-001"));
        assert!(err.to_string().contains("scope_policy=hard"));
    }

    #[test]
    fn test_validate_scope_policy_hard_with_none_scope_err() {
        assert!(validate_scope_policy("INF-TSK-001", Some("hard"), None).is_err());
    }

    #[test]
    fn test_validate_scope_policy_soft_with_empty_scope_ok() {
        assert!(validate_scope_policy("INF-TSK-001", Some("soft"), None).is_ok());
    }

    #[test]
    fn test_validate_scope_policy_permissive_with_empty_scope_ok() {
        assert!(validate_scope_policy("INF-TSK-001", Some("permissive"), None).is_ok());
    }

    #[test]
    fn test_validate_scope_policy_no_policy_ok() {
        assert!(validate_scope_policy("INF-TSK-001", None, None).is_ok());
    }

    // -- WS-SEC iter-1 finding #2 (A05 Security Misconfiguration) --
    // Vocabulary check: unknown scope_policy values must be rejected at write
    // time so they cannot silently route to the hook's soft fallback.

    #[test]
    fn test_validate_scope_policy_wrong_case_hard_rejected() {
        let scope = vec!["src/foo.rs".to_string()];
        let err = validate_scope_policy("INF-TSK-002", Some("Hard"), Some(&scope)).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("invalid scope_policy 'Hard'"),
            "error must name the offending value, got: {msg}"
        );
        assert!(
            msg.contains("INF-TSK-002"),
            "error must name the task, got: {msg}"
        );
        assert!(
            msg.contains("soft, hard, permissive"),
            "error must list the valid values, got: {msg}"
        );
    }

    #[test]
    fn test_validate_scope_policy_wrong_case_soft_rejected() {
        let err = validate_scope_policy("INF-TSK-003", Some("Soft"), None).unwrap_err();
        assert!(err.to_string().contains("invalid scope_policy 'Soft'"));
    }

    #[test]
    fn test_validate_scope_policy_strict_rejected() {
        let err = validate_scope_policy("INF-TSK-004", Some("strict"), None).unwrap_err();
        assert!(err.to_string().contains("invalid scope_policy 'strict'"));
    }

    #[test]
    fn test_validate_scope_policy_typo_rejected() {
        let err = validate_scope_policy("INF-TSK-005", Some("permisive"), None).unwrap_err();
        assert!(err.to_string().contains("invalid scope_policy 'permisive'"));
    }

    #[test]
    fn test_validate_scope_policy_empty_string_rejected() {
        let err = validate_scope_policy("INF-TSK-006", Some(""), None).unwrap_err();
        assert!(err.to_string().contains("invalid scope_policy ''"));
    }

    #[test]
    fn test_validate_scope_policy_known_values_accepted() {
        // soft + permissive accept any file_scope (including None).
        assert!(validate_scope_policy("INF-TSK-007", Some("soft"), None).is_ok());
        assert!(validate_scope_policy("INF-TSK-008", Some("permissive"), None).is_ok());
        // hard requires non-empty scope.
        let scope = vec!["a.rs".to_string()];
        assert!(validate_scope_policy("INF-TSK-009", Some("hard"), Some(&scope)).is_ok());
    }
}
