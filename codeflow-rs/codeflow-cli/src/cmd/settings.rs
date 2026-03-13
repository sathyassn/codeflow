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
    fn test_settings_run_with_dir_empty() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path());
        let _ = result;
    }

    #[test]
    fn test_settings_run_with_dir_with_templates() {
        let dir = tempfile::tempdir().unwrap();
        let tmpl_dir = dir.path().join(".claude").join("settings-templates");
        std::fs::create_dir_all(&tmpl_dir).unwrap();
        let settings_json = r#"{"permissions":{}}"#;
        std::fs::write(tmpl_dir.join("autonomous.json"), settings_json).unwrap();
        std::fs::write(tmpl_dir.join("interactive.json"), settings_json).unwrap();

        let result = run_with_dir(dir.path());
        let _ = result;
    }

    #[test]
    fn test_settings_run_with_dir_failure_path() {
        let dir = tempfile::tempdir().unwrap();
        let tmpl_dir = dir.path().join(".claude").join("settings-templates");
        std::fs::create_dir_all(&tmpl_dir).unwrap();
        std::fs::write(tmpl_dir.join("a.json"), r#"{"hooks":[]}"#).unwrap();
        std::fs::write(tmpl_dir.join("b.json"), r#"{"hooks":["different"]}"#).unwrap();

        let result = run_with_dir(dir.path());
        let _ = result;
    }
}
