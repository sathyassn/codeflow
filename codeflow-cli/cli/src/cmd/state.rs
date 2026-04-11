//! State command: display session state, set/clear active task.

use std::path::Path;

use anyhow::{Context, Result};
use clap::Subcommand;
use codeflow_core::session::active_task::{
    ActiveTask, clear_active_task_worktree_aware, set_active_task_worktree_aware,
};
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
        /// Scope policy: soft, hard, or permissive
        #[arg(long)]
        scope_policy: Option<String>,
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

pub fn run(command: Option<StateCommand>) -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_with_dir(&project_dir, command)
}

fn run_with_dir(project_dir: &Path, command: Option<StateCommand>) -> Result<()> {
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
            target_branch,
            auto_merge,
            epic_update,
        }) => {
            let task = ActiveTask {
                task_id: TaskId::new_unchecked(task_id),
                epic_id: Some(EpicId::new_unchecked(epic_id)),
                task_format_id: Some(FormatId::new_unchecked(task_format_id.clone())),
                epic_format_id: Some(FormatId::new_unchecked(epic_format_id)),
                title: Some(title),
                status: Some(status),
                branch: Some(branch),
                session_id: Some(SessionId::new_unchecked(session_id)),
                created_at: Some(chrono::Utc::now().to_rfc3339()),
                updated_at: Some(chrono::Utc::now().to_rfc3339()),
                work_type,
                scope_policy,
                target_branch,
                auto_merge,
                epic_update,
                current_stage: None,
                team_name: None,
                file_scope: None,
            };
            set_active_task_worktree_aware(project_dir, &task).context("setting active task")?;
            println!("active-task set: {task_format_id}");
            Ok(())
        }
        Some(StateCommand::ClearActiveTask) => {
            clear_active_task_worktree_aware(project_dir).context("clearing active task")?;
            println!("active-task cleared");
            Ok(())
        }
    }
}

fn run_show(project_dir: &Path) -> Result<()> {
    let state_dir = project_dir.join(".state");

    // Show current session ID.
    match helpers::resolve_session_id(project_dir) {
        Ok(sid) => println!("session: {sid}"),
        Err(_) => println!("session: none"),
    }

    // Show active task.
    let active_task_path = state_dir.join("runtime").join("active-task.json");
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

    #[test]
    fn test_state_no_session_no_active_task() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path(), None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_state_with_active_task_file() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::write(
            runtime_dir.join("active-task.json"),
            r#"{"task_id":"TSK-001"}"#,
        )
        .unwrap();
        let result = run_with_dir(dir.path(), None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_state_with_session_and_active_task() {
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
        let result = run_with_dir(dir.path(), None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_state_set_active_task_creates_file_atomically() {
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
                target_branch: None,
                auto_merge: None,
                epic_update: None,
            }),
        );
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

    #[test]
    fn test_state_clear_active_task_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::write(
            runtime_dir.join("active-task.json"),
            r#"{"task_id":"TSK-001"}"#,
        )
        .unwrap();

        let result = run_with_dir(dir.path(), Some(StateCommand::ClearActiveTask));
        assert!(result.is_ok(), "clear should succeed: {result:?}");
        assert!(
            !runtime_dir.join("active-task.json").exists(),
            "file should be removed"
        );
    }

    #[test]
    fn test_state_clear_active_task_absent_file() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path(), Some(StateCommand::ClearActiveTask));
        assert!(
            result.is_ok(),
            "clear absent file should succeed: {result:?}"
        );
    }

    #[test]
    fn test_state_show_subcommand() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path(), Some(StateCommand::Show));
        assert!(result.is_ok(), "show subcommand should succeed: {result:?}");
    }
}
