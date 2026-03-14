//! Report command: generate reports from session data.

use std::path::Path;

use anyhow::Result;

use crate::helpers;

pub fn run() -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_with_dir(&project_dir)
}

fn run_with_dir(project_dir: &Path) -> Result<()> {
    let logs_dir = project_dir.join(".state").join("logs");

    if !logs_dir.is_dir() {
        println!("no logs directory");
        return Ok(());
    }

    let events_path = logs_dir.join("pathflow-events.jsonl");
    if events_path.exists() {
        let meta = std::fs::metadata(&events_path)?;
        println!("pathflow-events.jsonl: {} bytes", meta.len());
    } else {
        println!("no pathflow events");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_report_command_exists() {
        let _: fn() -> Result<()> = run;
    }

    #[test]
    fn test_report_no_logs_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_report_with_events_file() {
        let dir = tempfile::tempdir().unwrap();
        let logs_dir = dir.path().join(".state").join("logs");
        std::fs::create_dir_all(&logs_dir).unwrap();
        std::fs::write(logs_dir.join("pathflow-events.jsonl"), "{}\n{}\n").unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_report_empty_logs_dir() {
        let dir = tempfile::tempdir().unwrap();
        let logs_dir = dir.path().join(".state").join("logs");
        std::fs::create_dir_all(&logs_dir).unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }
}
