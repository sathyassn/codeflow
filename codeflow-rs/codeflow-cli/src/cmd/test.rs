//! Test command: run the codeflow test suite.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

use crate::helpers;

pub fn run() -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_with_dir(&project_dir)
}

fn run_with_dir(project_dir: &Path) -> Result<()> {
    let test_script = project_dir
        .join(".codeflow")
        .join("testing")
        .join("run-all-tests.sh");

    if !test_script.exists() {
        anyhow::bail!("test runner not found at {}", test_script.display());
    }

    let status = Command::new("bash")
        .arg(&test_script)
        .current_dir(project_dir)
        .status()
        .context("executing test runner")?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_test_no_script() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("test runner not found"), "got: {msg}");
    }

    #[test]
    fn test_test_with_passing_script() {
        let dir = tempfile::tempdir().unwrap();
        let script_dir = dir.path().join(".codeflow").join("testing");
        std::fs::create_dir_all(&script_dir).unwrap();
        std::fs::write(script_dir.join("run-all-tests.sh"), "#!/bin/bash\nexit 0\n").unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_test_no_script_error_message() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path());
        let msg = result.unwrap_err().to_string();
        // Error should mention the expected script location.
        assert!(
            msg.contains("test runner not found"),
            "expected 'test runner not found', got: {msg}"
        );
        assert!(
            msg.contains("run-all-tests.sh"),
            "expected path to contain run-all-tests.sh, got: {msg}"
        );
    }

    #[test]
    fn test_test_with_passing_script_returns_ok() {
        // Verify the passing path produces Ok(()) not just not-Err.
        let dir = tempfile::tempdir().unwrap();
        let script_dir = dir.path().join(".codeflow").join("testing");
        std::fs::create_dir_all(&script_dir).unwrap();
        let script_path = script_dir.join("run-all-tests.sh");
        std::fs::write(&script_path, "#!/bin/bash\nexit 0\n").unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok(), "passing script should return Ok(())");
    }

    #[test]
    fn test_test_script_path_includes_codeflow_testing() {
        // Verify the error message includes the full expected path structure.
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains(".codeflow"),
            "error should mention .codeflow directory: {msg}"
        );
        assert!(
            msg.contains("testing"),
            "error should mention testing directory: {msg}"
        );
    }
}
