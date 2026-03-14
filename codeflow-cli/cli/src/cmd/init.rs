//! Init command: initialize a `CodeFlow` project.

use std::path::Path;

use anyhow::{Context, Result};

use crate::helpers;

pub fn run() -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_with_dir(&project_dir)
}

fn run_with_dir(project_dir: &Path) -> Result<()> {
    // Create essential directories.
    let dirs = [
        ".state/db",
        ".state/ledger",
        ".state/runtime",
        ".state/logs",
        ".state/sentinels",
        ".state/session",
        ".codeflow/config",
    ];

    for dir in &dirs {
        let path = project_dir.join(dir);
        std::fs::create_dir_all(&path).with_context(|| format!("creating {}", path.display()))?;
    }

    println!("initialized codeflow project at {}", project_dir.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_command_exists() {
        let _: fn() -> Result<()> = run;
    }

    #[test]
    fn test_init_creates_directories() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
        assert!(dir.path().join(".state/db").is_dir());
        assert!(dir.path().join(".state/ledger").is_dir());
        assert!(dir.path().join(".state/runtime").is_dir());
        assert!(dir.path().join(".state/logs").is_dir());
        assert!(dir.path().join(".state/sentinels").is_dir());
        assert!(dir.path().join(".state/session").is_dir());
        assert!(dir.path().join(".codeflow/config").is_dir());
    }

    #[test]
    fn test_init_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let r1 = run_with_dir(dir.path());
        let r2 = run_with_dir(dir.path());
        assert!(r1.is_ok());
        assert!(r2.is_ok());
    }
}
