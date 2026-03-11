//! Settings command: validate settings templates.

use std::path::Path;

use anyhow::{Context, Result};

use crate::helpers;

pub fn run() -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_with_dir(&project_dir)
}

fn run_with_dir(project_dir: &Path) -> Result<()> {
    let results = codeflow_core::settings::validate_settings_templates(project_dir)
        .context("validating settings templates")?;

    let mut has_failure = false;
    for r in &results {
        let status = if r.passed { "ok" } else { "FAIL" };
        if !r.passed {
            has_failure = true;
        }
        println!("[{status}] {:?}: {}", r.check, r.message);
    }

    if has_failure {
        anyhow::bail!("settings validation failed");
    }

    println!("settings ok");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_command_exists() {
        let _: fn() -> Result<()> = run;
    }

    #[test]
    fn test_settings_no_project_dir() {
        let dir = tempfile::tempdir().unwrap();
        // Will either succeed (no templates to validate) or fail gracefully.
        let _result = run_with_dir(dir.path());
    }
}
