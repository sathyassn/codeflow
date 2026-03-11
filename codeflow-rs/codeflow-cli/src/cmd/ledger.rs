//! Ledger command: display ledger file information.

use std::path::Path;

use anyhow::{Context, Result};

use crate::helpers;

pub fn run() -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_with_dir(&project_dir)
}

fn run_with_dir(project_dir: &Path) -> Result<()> {
    let ledger_dir = project_dir.join(".state").join("ledger");

    if !ledger_dir.is_dir() {
        println!("no ledger directory");
        return Ok(());
    }

    let entries = std::fs::read_dir(&ledger_dir).context("reading ledger directory")?;

    let mut count = 0;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            let meta = std::fs::metadata(&path)?;
            println!("{name}: {} bytes", meta.len());
            count += 1;
        }
    }

    if count == 0 {
        println!("no ledger files");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ledger_command_exists() {
        let _: fn() -> Result<()> = run;
    }

    #[test]
    fn test_ledger_no_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_ledger_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_dir = dir.path().join(".state").join("ledger");
        std::fs::create_dir_all(&ledger_dir).unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_ledger_with_jsonl_files() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_dir = dir.path().join(".state").join("ledger");
        std::fs::create_dir_all(&ledger_dir).unwrap();
        std::fs::write(ledger_dir.join("sessions.jsonl"), "{}").unwrap();
        std::fs::write(ledger_dir.join("events.jsonl"), "{}\n{}").unwrap();
        std::fs::write(ledger_dir.join("readme.txt"), "not jsonl").unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }
}
