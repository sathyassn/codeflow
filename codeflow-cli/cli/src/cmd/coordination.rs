//! Coordination command: coordination operations for multi-agent workflows.

use std::path::Path;

use anyhow::Result;

use crate::helpers;

pub fn run() -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_with_dir(&project_dir)
}

#[allow(clippy::unnecessary_wraps)]
fn run_with_dir(project_dir: &Path) -> Result<()> {
    let coord_dir = project_dir.join(".state").join("coordination");

    if !coord_dir.is_dir() {
        println!("no coordination state");
        return Ok(());
    }

    println!("coordination: {}", coord_dir.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordination_command_exists() {
        let _: fn() -> Result<()> = run;
    }

    #[test]
    fn test_coordination_no_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_coordination_with_dir() {
        let dir = tempfile::tempdir().unwrap();
        let coord_dir = dir.path().join(".state").join("coordination");
        std::fs::create_dir_all(&coord_dir).unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }
}
