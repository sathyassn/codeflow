//! Data fetching layer for TUI dashboards.
//!
//! Provides view-model structs (`BatchView`, `TaskView`, `SessionView`) and
//! functions that assemble them from the `DataStore` and filesystem state.

use std::path::Path;

use serde::Serialize;

use crate::error::DbError;
use crate::models::AutorunSession;
use crate::store::DataStore;
use crate::types::{AutorunSessionStatus, AutorunTaskRunStatus};

// ---------------------------------------------------------------------------
// View-model structs
// ---------------------------------------------------------------------------

/// Aggregated view of an autorun batch for TUI rendering.
#[derive(Debug, Clone, Serialize)]
pub struct BatchView {
    pub session_id: String,
    pub batch_name: String,
    pub status: AutorunSessionStatus,
    pub total_tasks: i32,
    pub completed_tasks: i32,
    pub failed_tasks: i32,
    pub skipped_tasks: i32,
    pub running_count: usize,
    pub elapsed_secs: i64,
    pub target_branch: Option<String>,
    pub tasks: Vec<TaskView>,
}

/// Per-task view for the TUI table.
#[derive(Debug, Clone, Serialize)]
pub struct TaskView {
    pub task_id: String,
    pub status: AutorunTaskRunStatus,
    /// Display status (e.g., "Waiting" for pending tasks in a running session).
    pub display_status: String,
    pub phase: Option<String>,
    pub branch: Option<String>,
    pub pr_number: Option<i64>,
    pub tmux_session: Option<String>,
    pub duration_secs: Option<i64>,
    pub exit_code: Option<i64>,
    pub worktree_path: Option<String>,
    pub error_message: Option<String>,
    pub stages: Vec<StageInfo>,
}

/// Stage pipeline entry for the detail pane.
#[derive(Debug, Clone, Serialize)]
pub struct StageInfo {
    pub name: String,
    pub completed: bool,
}

/// View of an interactive session (for future INF-TSK-044-005).
#[derive(Debug, Clone, Serialize)]
pub struct SessionView {
    pub session_id: String,
    pub status: String,
    pub branch: Option<String>,
    pub phase: Option<String>,
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// Data fetching
// ---------------------------------------------------------------------------

/// Fetch a `BatchView` for the given session, or the most recent active session.
///
/// # Errors
///
/// Returns an error if the store cannot be queried or no matching session is found.
pub async fn fetch_batch_view<S: DataStore>(
    store: &S,
    project_dir: &Path,
    session_id: Option<&str>,
) -> Result<Option<BatchView>, DbError> {
    let session = match session_id {
        Some(sid) => store.get_autorun_session(sid).await?,
        None => {
            let sessions = store
                .list_autorun_sessions(crate::models::AutorunSessionFilter::default())
                .await?;
            sessions.into_iter().find(|s| {
                matches!(
                    s.status,
                    AutorunSessionStatus::Running | AutorunSessionStatus::Aborting
                )
            })
        }
    };

    let Some(session) = session else {
        return Ok(None);
    };

    build_batch_view(store, project_dir, &session)
        .await
        .map(Some)
}

/// Build a complete `BatchView` from a session record.
async fn build_batch_view<S: DataStore>(
    store: &S,
    project_dir: &Path,
    session: &AutorunSession,
) -> Result<BatchView, DbError> {
    let workers = store.list_autorun_workers(&session.id).await?;

    let task_runs = store.list_autorun_task_runs(&session.id).await?;

    let running_count = workers
        .iter()
        .filter(|w| w.status == crate::types::AutorunWorkerStatus::Running)
        .count();

    let elapsed_secs = compute_elapsed_secs(&session.created_at);

    let mut tasks = Vec::with_capacity(task_runs.len());
    for run in &task_runs {
        let tmux = workers
            .iter()
            .find(|w| w.task_id == run.task_id)
            .and_then(|w| w.tmux_session.clone());

        let display_status = compute_display_status(run.status, session.status);

        let phase = read_latest_phase(project_dir, run.worktree_path.as_deref());

        let stages = read_stage_pipeline(project_dir, run.worktree_path.as_deref());

        tasks.push(TaskView {
            task_id: run.task_id.clone(),
            status: run.status,
            display_status,
            phase,
            branch: run.branch_name.clone(),
            pr_number: run.pr_number,
            tmux_session: tmux,
            duration_secs: run.duration_seconds.or_else(|| {
                if matches!(run.status, AutorunTaskRunStatus::Running) {
                    run.started_at.as_deref().map(compute_elapsed_secs)
                } else {
                    None
                }
            }),
            exit_code: run.exit_code,
            worktree_path: run.worktree_path.clone(),
            error_message: run.error_message.clone(),
            stages,
        });
    }

    Ok(BatchView {
        session_id: session.id.clone(),
        batch_name: session
            .batch_name
            .clone()
            .unwrap_or_else(|| session.id[..session.id.len().min(20)].to_string()),
        status: session.status,
        total_tasks: session.total_tasks,
        completed_tasks: session.completed_tasks,
        failed_tasks: session.failed_tasks,
        skipped_tasks: session.skipped_tasks,
        running_count,
        elapsed_secs,
        target_branch: session.target_branch.clone(),
        tasks,
    })
}

/// Fetch interactive session views (stub for INF-TSK-044-005).
///
/// Returns an empty list. Will be implemented when the interactive session
/// dashboard task is active.
#[must_use]
pub fn fetch_session_views() -> Vec<SessionView> {
    Vec::new()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn compute_elapsed_secs(started_at: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(started_at)
        .map(|start| {
            chrono::Utc::now()
                .signed_duration_since(start)
                .num_seconds()
                .max(0)
        })
        .unwrap_or(0)
}

fn compute_display_status(
    status: AutorunTaskRunStatus,
    session_status: AutorunSessionStatus,
) -> String {
    if matches!(status, AutorunTaskRunStatus::Pending)
        && matches!(session_status, AutorunSessionStatus::Running)
    {
        "Waiting".to_string()
    } else {
        status.to_string()
    }
}

/// Read the latest PathFlow phase from sentinel files.
pub fn read_latest_phase(project_dir: &Path, worktree_path: Option<&str>) -> Option<String> {
    let base = worktree_path.map_or_else(|| project_dir.to_path_buf(), std::path::PathBuf::from);

    let sentinel_dir = base.join(".state/sentinels/pathflow");
    let Ok(entries) = std::fs::read_dir(&sentinel_dir) else {
        return None;
    };

    // Look for session directories, then find the latest pf-N sentinel.
    let mut latest_phase: Option<u8> = None;
    for entry in entries.flatten() {
        let session_dir = entry.path();
        if !session_dir.is_dir() {
            continue;
        }
        if let Ok(sentinels) = std::fs::read_dir(&session_dir) {
            for sentinel in sentinels.flatten() {
                let name = sentinel.file_name();
                let name = name.to_string_lossy();
                if let Some(rest) = name.strip_prefix("pathflow-pf-") {
                    if let Ok(n) = rest.parse::<u8>() {
                        latest_phase = Some(latest_phase.map_or(n, |cur| cur.max(n)));
                    }
                }
            }
        }
    }

    latest_phase.map(|n| format!("PF{n}"))
}

/// Read stage pipeline status from sentinel files.
fn read_stage_pipeline(project_dir: &Path, worktree_path: Option<&str>) -> Vec<StageInfo> {
    let base = worktree_path.map_or_else(|| project_dir.to_path_buf(), std::path::PathBuf::from);

    let sentinel_dir = base.join(".state/sentinels/pathflow");

    let stage_names = ["ws-dev", "ws-sec", "ws-rev", "ws-qa"];
    let mut completed_stages = std::collections::HashSet::new();

    if let Ok(entries) = std::fs::read_dir(&sentinel_dir) {
        for entry in entries.flatten() {
            let session_dir = entry.path();
            if !session_dir.is_dir() {
                continue;
            }
            if let Ok(sentinels) = std::fs::read_dir(&session_dir) {
                for sentinel in sentinels.flatten() {
                    let name = sentinel.file_name();
                    let name = name.to_string_lossy();
                    for stage in &stage_names {
                        if name.contains(stage) {
                            completed_stages.insert((*stage).to_string());
                        }
                    }
                }
            }
        }
    }

    stage_names
        .iter()
        .map(|name| StageInfo {
            name: name.to_uppercase(),
            completed: completed_stages.contains(*name),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_elapsed_secs_valid_timestamp() {
        let now = chrono::Utc::now();
        let ten_secs_ago = (now - chrono::Duration::seconds(10)).to_rfc3339();
        let elapsed = compute_elapsed_secs(&ten_secs_ago);
        assert!(elapsed >= 9, "elapsed {elapsed} should be >= 9");
        assert!(elapsed <= 12, "elapsed {elapsed} should be <= 12");
    }

    #[test]
    fn test_compute_elapsed_secs_invalid_timestamp() {
        assert_eq!(compute_elapsed_secs("not-a-date"), 0);
    }

    #[test]
    fn test_compute_display_status_pending_running() {
        let status =
            compute_display_status(AutorunTaskRunStatus::Pending, AutorunSessionStatus::Running);
        assert_eq!(status, "Waiting");
    }

    #[test]
    fn test_compute_display_status_pending_completed() {
        let status = compute_display_status(
            AutorunTaskRunStatus::Pending,
            AutorunSessionStatus::Completed,
        );
        assert_eq!(status, "pending");
    }

    #[test]
    fn test_compute_display_status_running() {
        let status =
            compute_display_status(AutorunTaskRunStatus::Running, AutorunSessionStatus::Running);
        assert_eq!(status, "running");
    }

    #[test]
    fn test_compute_display_status_completed() {
        let status = compute_display_status(
            AutorunTaskRunStatus::Completed,
            AutorunSessionStatus::Completed,
        );
        assert_eq!(status, "completed");
    }

    #[test]
    fn test_compute_display_status_failed() {
        let status =
            compute_display_status(AutorunTaskRunStatus::Failed, AutorunSessionStatus::Running);
        assert_eq!(status, "failed");
    }

    #[test]
    fn test_read_latest_phase_no_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let result = read_latest_phase(tmp.path(), None);
        assert!(result.is_none());
    }

    #[test]
    fn test_read_latest_phase_with_sentinels() {
        let tmp = tempfile::tempdir().unwrap();
        let session_dir = tmp.path().join(".state/sentinels/pathflow/ses-test-123");
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(session_dir.join("pathflow-pf-1"), "").unwrap();
        std::fs::write(session_dir.join("pathflow-pf-3"), "").unwrap();

        let result = read_latest_phase(tmp.path(), None);
        assert_eq!(result, Some("PF3".to_string()));
    }

    #[test]
    fn test_read_stage_pipeline_no_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let stages = read_stage_pipeline(tmp.path(), None);
        assert_eq!(stages.len(), 4);
        assert!(stages.iter().all(|s| !s.completed));
    }

    #[test]
    fn test_read_stage_pipeline_with_sentinels() {
        let tmp = tempfile::tempdir().unwrap();
        let session_dir = tmp.path().join(".state/sentinels/pathflow/ses-test-456");
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(session_dir.join("pathflow-ws-dev"), "").unwrap();
        std::fs::write(session_dir.join("pathflow-ws-rev"), "").unwrap();

        let stages = read_stage_pipeline(tmp.path(), None);
        assert_eq!(stages.len(), 4);

        let dev = stages.iter().find(|s| s.name.contains("DEV")).unwrap();
        assert!(dev.completed);

        let rev = stages.iter().find(|s| s.name.contains("REV")).unwrap();
        assert!(rev.completed);

        let qa = stages.iter().find(|s| s.name.contains("QA")).unwrap();
        assert!(!qa.completed);
    }

    #[test]
    fn test_batch_view_serializes_to_json() {
        let view = BatchView {
            session_id: "ar-001".to_string(),
            batch_name: "test-batch".to_string(),
            status: AutorunSessionStatus::Running,
            total_tasks: 5,
            completed_tasks: 2,
            failed_tasks: 1,
            skipped_tasks: 0,
            running_count: 2,
            elapsed_secs: 120,
            target_branch: Some("main".to_string()),
            tasks: vec![TaskView {
                task_id: "task-a".to_string(),
                status: AutorunTaskRunStatus::Running,
                display_status: "running".to_string(),
                phase: Some("PF4".to_string()),
                branch: Some("feat/x".to_string()),
                pr_number: None,
                tmux_session: Some("tmux-1".to_string()),
                duration_secs: Some(60),
                exit_code: None,
                worktree_path: None,
                error_message: None,
                stages: vec![
                    StageInfo {
                        name: "WS-DEV".to_string(),
                        completed: true,
                    },
                    StageInfo {
                        name: "WS-REV".to_string(),
                        completed: false,
                    },
                ],
            }],
        };

        let json = serde_json::to_string(&view).unwrap();
        assert!(json.contains("\"session_id\":\"ar-001\""));
        assert!(json.contains("\"batch_name\":\"test-batch\""));
        assert!(json.contains("\"task_id\":\"task-a\""));
        assert!(json.contains("\"running_count\":2"));
    }

    #[test]
    fn test_session_view_serializes() {
        let view = SessionView {
            session_id: "ses-001".to_string(),
            status: "active".to_string(),
            branch: Some("feat/test".to_string()),
            phase: Some("PF4".to_string()),
            created_at: "2026-04-01T00:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&view).unwrap();
        assert!(json.contains("\"session_id\":\"ses-001\""));
        assert!(json.contains("\"branch\":\"feat/test\""));
    }

    #[test]
    fn test_stage_info_serialization() {
        let info = StageInfo {
            name: "WS-DEV".to_string(),
            completed: true,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"name\":\"WS-DEV\""));
        assert!(json.contains("\"completed\":true"));
    }

    #[test]
    fn test_task_view_defaults() {
        let view = TaskView {
            task_id: "t-1".to_string(),
            status: AutorunTaskRunStatus::Pending,
            display_status: "pending".to_string(),
            phase: None,
            branch: None,
            pr_number: None,
            tmux_session: None,
            duration_secs: None,
            exit_code: None,
            worktree_path: None,
            error_message: None,
            stages: vec![],
        };
        assert!(view.phase.is_none());
        assert!(view.branch.is_none());
        assert!(view.pr_number.is_none());
        assert!(view.stages.is_empty());
    }

    #[test]
    fn test_fetch_session_views_returns_empty() {
        let views = fetch_session_views();
        assert!(views.is_empty());
    }

    #[test]
    fn test_read_latest_phase_with_worktree_path() {
        let tmp = tempfile::tempdir().unwrap();
        let wt = tmp.path().join("worktree");
        let session_dir = wt.join(".state/sentinels/pathflow/ses-wt-001");
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(session_dir.join("pathflow-pf-1"), "").unwrap();
        std::fs::write(session_dir.join("pathflow-pf-4"), "").unwrap();

        let result = read_latest_phase(tmp.path(), Some(wt.to_str().unwrap()));
        assert_eq!(result, Some("PF4".to_string()));
    }

    #[test]
    fn test_read_stage_pipeline_with_worktree_path() {
        let tmp = tempfile::tempdir().unwrap();
        let wt = tmp.path().join("worktree");
        let session_dir = wt.join(".state/sentinels/pathflow/ses-wt-002");
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(session_dir.join("pathflow-ws-dev"), "").unwrap();

        let stages = read_stage_pipeline(tmp.path(), Some(wt.to_str().unwrap()));
        let dev = stages.iter().find(|s| s.name.contains("DEV")).unwrap();
        assert!(dev.completed);
        let rev = stages.iter().find(|s| s.name.contains("REV")).unwrap();
        assert!(!rev.completed);
    }

    // -----------------------------------------------------------------------
    // Async tests using MockStore for fetch_batch_view / build_batch_view
    // -----------------------------------------------------------------------

    fn make_mock_session(id: &str, status: AutorunSessionStatus) -> crate::models::AutorunSession {
        crate::models::AutorunSession {
            id: id.to_string(),
            batch_file: "batch.yaml".to_string(),
            batch_name: Some("test-batch".to_string()),
            status,
            max_session_workers: 2,
            total_tasks: 3,
            completed_tasks: 1,
            failed_tasks: 0,
            pid: Some(999),
            skipped_tasks: 0,
            tmux_session: None,
            stale_reason: None,
            target_branch: Some("main".to_string()),
            final_pr_url: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            completed_at: None,
        }
    }

    fn make_mock_worker(
        session_id: &str,
        task_id: &str,
        status: crate::types::AutorunWorkerStatus,
    ) -> crate::models::AutorunWorker {
        crate::models::AutorunWorker {
            id: format!("aw-{task_id}"),
            session_id: session_id.to_string(),
            worker_num: 1,
            task_id: task_id.to_string(),
            status,
            tmux_session: Some(format!("tmux-{task_id}")),
            worktree_path: None,
            file_scope: vec![],
            scope_policy: "soft".to_string(),
            worker_session_id: None,
            pr_number: None,
            started_at: Some(chrono::Utc::now().to_rfc3339()),
            completed_at: None,
        }
    }

    fn make_mock_task_run(
        session_id: &str,
        task_id: &str,
        status: AutorunTaskRunStatus,
    ) -> crate::models::AutorunTaskRun {
        crate::models::AutorunTaskRun {
            id: format!("atr-{task_id}"),
            worker_id: format!("aw-{task_id}"),
            task_id: task_id.to_string(),
            session_id: session_id.to_string(),
            status,
            branch_name: Some(format!("feat/{task_id}")),
            worktree_path: None,
            pr_number: Some(42),
            pr_url: None,
            blocked_reason: None,
            claim_conflicts: None,
            merge_conflicts: None,
            started_at: Some(chrono::Utc::now().to_rfc3339()),
            completed_at: None,
            duration_seconds: Some(120),
            exit_code: None,
            error_message: None,
            verification_result: None,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    #[tokio::test]
    async fn test_fetch_batch_view_no_sessions() {
        let store = crate::store::mock::MockStore::new();
        let tmp = tempfile::tempdir().unwrap();
        let result = fetch_batch_view(&store, tmp.path(), None).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_fetch_batch_view_by_id() {
        let store = crate::store::mock::MockStore::new();
        let session = make_mock_session("ar-test-1", AutorunSessionStatus::Running);
        store
            .autorun_sessions
            .lock()
            .unwrap()
            .insert("ar-test-1".to_string(), session);

        let worker = make_mock_worker(
            "ar-test-1",
            "task-a",
            crate::types::AutorunWorkerStatus::Running,
        );
        store
            .autorun_workers
            .lock()
            .unwrap()
            .insert("aw-task-a".to_string(), worker);

        let run = make_mock_task_run("ar-test-1", "task-a", AutorunTaskRunStatus::Running);
        store
            .autorun_task_runs
            .lock()
            .unwrap()
            .insert("atr-task-a".to_string(), run);

        let tmp = tempfile::tempdir().unwrap();
        let result = fetch_batch_view(&store, tmp.path(), Some("ar-test-1"))
            .await
            .unwrap();

        assert!(result.is_some());
        let view = result.unwrap();
        assert_eq!(view.session_id, "ar-test-1");
        assert_eq!(view.batch_name, "test-batch");
        assert_eq!(view.total_tasks, 3);
        assert_eq!(view.running_count, 1);
        assert_eq!(view.tasks.len(), 1);
        assert_eq!(view.tasks[0].task_id, "task-a");
        assert_eq!(view.tasks[0].branch.as_deref(), Some("feat/task-a"));
        assert_eq!(view.tasks[0].pr_number, Some(42));
        assert_eq!(view.tasks[0].tmux_session.as_deref(), Some("tmux-task-a"));
        assert_eq!(view.tasks[0].duration_secs, Some(120));
        assert_eq!(view.target_branch.as_deref(), Some("main"));
    }

    #[tokio::test]
    async fn test_fetch_batch_view_finds_running_session() {
        let store = crate::store::mock::MockStore::new();
        let session = make_mock_session("ar-auto", AutorunSessionStatus::Running);
        store
            .autorun_sessions
            .lock()
            .unwrap()
            .insert("ar-auto".to_string(), session);

        let tmp = tempfile::tempdir().unwrap();
        let result = fetch_batch_view(&store, tmp.path(), None).await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().session_id, "ar-auto");
    }

    #[tokio::test]
    async fn test_fetch_batch_view_skips_completed_sessions() {
        let store = crate::store::mock::MockStore::new();
        let session = make_mock_session("ar-done", AutorunSessionStatus::Completed);
        store
            .autorun_sessions
            .lock()
            .unwrap()
            .insert("ar-done".to_string(), session);

        let tmp = tempfile::tempdir().unwrap();
        let result = fetch_batch_view(&store, tmp.path(), None).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_fetch_batch_view_nonexistent_id() {
        let store = crate::store::mock::MockStore::new();
        let tmp = tempfile::tempdir().unwrap();
        let result = fetch_batch_view(&store, tmp.path(), Some("no-such-id"))
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_fetch_batch_view_display_status_waiting() {
        let store = crate::store::mock::MockStore::new();
        let session = make_mock_session("ar-wait", AutorunSessionStatus::Running);
        store
            .autorun_sessions
            .lock()
            .unwrap()
            .insert("ar-wait".to_string(), session);

        let run = make_mock_task_run("ar-wait", "task-p", AutorunTaskRunStatus::Pending);
        store
            .autorun_task_runs
            .lock()
            .unwrap()
            .insert("atr-task-p".to_string(), run);

        let tmp = tempfile::tempdir().unwrap();
        let view = fetch_batch_view(&store, tmp.path(), Some("ar-wait"))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(view.tasks[0].display_status, "Waiting");
    }

    #[tokio::test]
    async fn test_fetch_batch_view_running_task_computes_elapsed() {
        let store = crate::store::mock::MockStore::new();
        let session = make_mock_session("ar-elapsed", AutorunSessionStatus::Running);
        store
            .autorun_sessions
            .lock()
            .unwrap()
            .insert("ar-elapsed".to_string(), session);

        let mut run = make_mock_task_run("ar-elapsed", "task-e", AutorunTaskRunStatus::Running);
        run.duration_seconds = None; // no pre-computed duration
        run.started_at = Some((chrono::Utc::now() - chrono::Duration::seconds(30)).to_rfc3339());
        store
            .autorun_task_runs
            .lock()
            .unwrap()
            .insert("atr-task-e".to_string(), run);

        let tmp = tempfile::tempdir().unwrap();
        let view = fetch_batch_view(&store, tmp.path(), Some("ar-elapsed"))
            .await
            .unwrap()
            .unwrap();

        // Duration should be computed from started_at (~30 seconds).
        let dur = view.tasks[0].duration_secs.unwrap();
        assert!(dur >= 28, "duration {dur} should be >= 28");
        assert!(dur <= 35, "duration {dur} should be <= 35");
    }

    #[tokio::test]
    async fn test_build_batch_view_batch_name_fallback() {
        let store = crate::store::mock::MockStore::new();
        let mut session = make_mock_session("ar-noname-session", AutorunSessionStatus::Running);
        session.batch_name = None;
        store
            .autorun_sessions
            .lock()
            .unwrap()
            .insert("ar-noname-session".to_string(), session);

        let tmp = tempfile::tempdir().unwrap();
        let view = fetch_batch_view(&store, tmp.path(), Some("ar-noname-session"))
            .await
            .unwrap()
            .unwrap();

        // Fallback: first 20 chars of session_id.
        assert_eq!(view.batch_name, "ar-noname-session");
    }
}
