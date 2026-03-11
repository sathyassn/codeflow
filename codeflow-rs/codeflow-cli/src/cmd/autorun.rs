//! Autorun command: batch execution of tasks.

use std::path::Path;

use anyhow::{Context, Result};

use crate::helpers;

#[allow(clippy::unused_async)] // Called with .await from main.rs dispatch
pub async fn run() -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_with_dir(&project_dir)
}

fn run_with_dir(project_dir: &Path) -> Result<()> {
    let batch_path = project_dir
        .join(".codeflow")
        .join("config")
        .join("autorun")
        .join("batch.yaml");

    if !batch_path.exists() {
        anyhow::bail!("no autorun batch file found at {}", batch_path.display());
    }

    let parsed = codeflow_core::autorun::batch::parse_batch_file(&batch_path)
        .context("parsing autorun batch")?;

    println!("autorun batch: {} tasks", parsed.tasks.len());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_autorun_no_batch_file() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("no autorun batch file"),
            "expected batch file error, got: {msg}"
        );
    }

    #[test]
    fn test_autorun_with_valid_batch() {
        let dir = tempfile::tempdir().unwrap();
        let batch_dir = dir.path().join(".codeflow").join("config").join("autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        std::fs::write(
            batch_dir.join("batch.yaml"),
            "name: test-batch\ntasks:\n  - id: task-1\n",
        )
        .unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_autorun_with_invalid_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let batch_dir = dir.path().join(".codeflow").join("config").join("autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        std::fs::write(batch_dir.join("batch.yaml"), "{{invalid yaml").unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(
            msg.contains("parsing autorun batch"),
            "expected parse context, got: {msg}"
        );
    }

    #[test]
    fn test_autorun_valid_batch_reports_task_count() {
        let dir = tempfile::tempdir().unwrap();
        let batch_dir = dir.path().join(".codeflow").join("config").join("autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        std::fs::write(
            batch_dir.join("batch.yaml"),
            "name: multi\ntasks:\n  - id: task-1\n  - id: task-2\n  - id: task-3\n",
        )
        .unwrap();
        // Should succeed and reach the println with task count.
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }
}
