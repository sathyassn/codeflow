//! Shadow test command: run shadow test suite.

use std::path::Path;

use anyhow::Result;

use crate::helpers;

pub fn run() -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_with_dir(&project_dir)
}

#[allow(clippy::unnecessary_wraps)]
fn run_with_dir(_project_dir: &Path) -> Result<()> {
    println!("shadow-test: no shadow tests configured");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shadow_test_run_succeeds() {
        // run() calls detect_project_dir() which falls back to cwd, then run_with_dir.
        let result = run();
        assert!(result.is_ok());
    }

    #[test]
    fn test_shadow_test_with_dir_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }
}
