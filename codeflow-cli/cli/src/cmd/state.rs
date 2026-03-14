//! State command: display session state and pathflow status.

use std::path::Path;

use anyhow::{Context, Result};

use crate::helpers;

pub fn run() -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_with_dir(&project_dir)
}

fn run_with_dir(project_dir: &Path) -> Result<()> {
    let state_dir = project_dir.join(".state");

    // Show current session ID.
    match helpers::resolve_session_id(&state_dir) {
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
        let result = run_with_dir(dir.path());
        // Should succeed even with no state files.
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
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_state_with_session_and_active_task() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".state");
        // Create session env file.
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(
            state_dir.join("codeflow-env.sh"),
            "export CODEFLOW_SESSION_ID='ses-teststatewithtask12345'\nexport CF_PROJECT_ROOT='/tmp/test'\n",
        )
        .unwrap();
        // Create active task.
        let runtime_dir = state_dir.join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::write(
            runtime_dir.join("active-task.json"),
            r#"{"task_id":"TSK-002"}"#,
        )
        .unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }
}
