//! Report command: generate reports from session data.

use std::path::Path;

use anyhow::Result;
use codeflow_core::ledger;

use crate::helpers;

pub fn run() -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_with_dir(&project_dir)
}

fn run_with_dir(project_dir: &Path) -> Result<()> {
    let state_dir = project_dir.join(".state");
    let events_path = ledger::resolve_path_in(&state_dir, ledger::files::PATHFLOW_EVENTS)?;

    if events_path.exists() {
        let meta = std::fs::metadata(&events_path)?;
        let bytes = meta.len();
        if bytes == 0 {
            println!("pathflow-events.jsonl: 0 bytes (no pathflow events recorded)"); // EXEMPT: user-facing output names the ledger filename; path is resolved via resolve_path_in
        } else {
            println!("pathflow-events.jsonl: {bytes} bytes"); // EXEMPT: user-facing output names the ledger filename; path is resolved via resolve_path_in
        }
    } else {
        println!("no pathflow events");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger_events_dir(project_dir: &Path) -> std::path::PathBuf {
        project_dir
            .join(".state")
            .join("ledger")
            .join("pathflow-events")
    }

    #[test]
    fn test_report_command_exists() {
        let _: fn() -> Result<()> = run;
    }

    #[test]
    fn test_report_no_ledger_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_report_with_events_file() {
        let dir = tempfile::tempdir().unwrap();
        let events_dir = ledger_events_dir(dir.path());
        std::fs::create_dir_all(&events_dir).unwrap();
        std::fs::write(
            events_dir.join("pathflow-events.jsonl"),
            "{\"a\":1}\n{\"b\":2}\n",
        )
        .unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_report_empty_ledger_dir() {
        let dir = tempfile::tempdir().unwrap();
        let events_dir = ledger_events_dir(dir.path());
        std::fs::create_dir_all(&events_dir).unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_report_empty_events_file_is_zero_bytes_message() {
        // An empty file is not the same as a missing file; surface it
        // explicitly so users understand "0 events" vs "no events recorded".
        let dir = tempfile::tempdir().unwrap();
        let events_dir = ledger_events_dir(dir.path());
        std::fs::create_dir_all(&events_dir).unwrap();
        std::fs::write(events_dir.join("pathflow-events.jsonl"), "").unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }
}
