//! Settings command: validate and sync settings templates.

use std::path::Path;

use anyhow::{Context, Result};
use clap::Subcommand;

use crate::helpers;

/// Settings subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum SettingsCommand {
    /// Validate settings templates
    Validate,
    /// Sync active template to settings files
    Sync {
        /// Template name to sync (e.g. "strict", "autonomous")
        #[arg(long)]
        template: Option<String>,
    },
}

pub fn run(cmd: Option<SettingsCommand>) -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;

    match cmd {
        Some(SettingsCommand::Validate) | None => run_validate(&project_dir),
        Some(SettingsCommand::Sync { template }) => run_sync(&project_dir, template.as_deref()),
    }
}

fn run_validate(project_dir: &Path) -> Result<()> {
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

fn run_sync(project_dir: &Path, template_name: Option<&str>) -> Result<()> {
    let changes = codeflow_core::settings::sync_settings_template(project_dir, template_name)
        .context("syncing settings template")?;

    for change in &changes {
        let action = match change.action {
            codeflow_core::settings::SyncAction::Created => "created",
            codeflow_core::settings::SyncAction::Updated => "updated",
            codeflow_core::settings::SyncAction::Unchanged => "unchanged",
        };
        println!("[{action}] {} <- {}", change.destination, change.template);
    }

    let modified = changes
        .iter()
        .filter(|c| c.action != codeflow_core::settings::SyncAction::Unchanged)
        .count();

    if modified == 0 {
        println!("settings already up to date");
    } else {
        println!("{modified} file(s) synced");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_validate_command_exists() {
        let _: fn(Option<SettingsCommand>) -> Result<()> = run;
    }

    #[test]
    fn test_settings_run_validate_empty() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_validate(dir.path());
        let _ = result;
    }

    #[test]
    fn test_settings_run_validate_with_templates() {
        let dir = tempfile::tempdir().unwrap();
        let tmpl_dir = dir.path().join(".claude").join("settings-templates");
        std::fs::create_dir_all(&tmpl_dir).unwrap();
        let settings_json = r#"{"permissions":{}}"#;
        std::fs::write(tmpl_dir.join("autonomous.json"), settings_json).unwrap();
        std::fs::write(tmpl_dir.join("interactive.json"), settings_json).unwrap();

        let result = run_validate(dir.path());
        let _ = result;
    }

    #[test]
    fn test_settings_sync_creates_files() {
        let dir = tempfile::tempdir().unwrap();
        let tmpl_dir = dir.path().join(".claude").join("settings-templates");
        std::fs::create_dir_all(&tmpl_dir).unwrap();
        std::fs::write(
            tmpl_dir.join("autonomous.json"),
            r#"{"permissions":{"allow":["Read"]}}"#,
        )
        .unwrap();

        let result = run_sync(dir.path(), None);
        assert!(result.is_ok(), "sync should succeed: {result:?}");

        let settings = dir.path().join(".claude").join("settings.json");
        assert!(settings.exists(), "settings.json should be created");
    }

    #[test]
    fn test_settings_sync_preserves_overrides() {
        let dir = tempfile::tempdir().unwrap();
        let tmpl_dir = dir.path().join(".claude").join("settings-templates");
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&tmpl_dir).unwrap();
        std::fs::create_dir_all(&claude_dir).unwrap();

        // Template has one key.
        std::fs::write(
            tmpl_dir.join("autonomous.json"),
            r#"{"permissions":{"allow":["Read"]}}"#,
        )
        .unwrap();

        // Destination has an extra key (manual override).
        std::fs::write(
            claude_dir.join("settings.json"),
            r#"{"permissions":{"allow":["Read"]},"custom_key":"preserved"}"#,
        )
        .unwrap();

        let result = run_sync(dir.path(), None);
        assert!(result.is_ok());

        let content = std::fs::read_to_string(claude_dir.join("settings.json")).unwrap();
        assert!(
            content.contains("custom_key"),
            "manual override should be preserved"
        );
    }

    #[test]
    fn test_settings_sync_with_template_flag() {
        let dir = tempfile::tempdir().unwrap();
        let tmpl_dir = dir.path().join(".claude").join("settings-templates");
        std::fs::create_dir_all(&tmpl_dir).unwrap();
        std::fs::write(tmpl_dir.join("strict.json"), r#"{"mode":"strict"}"#).unwrap();

        let result = run_sync(dir.path(), Some("strict"));
        assert!(result.is_ok(), "sync with --template should succeed");
    }

    #[test]
    fn test_settings_sync_missing_template() {
        let dir = tempfile::tempdir().unwrap();
        let tmpl_dir = dir.path().join(".claude").join("settings-templates");
        std::fs::create_dir_all(&tmpl_dir).unwrap();

        let result = run_sync(dir.path(), Some("nonexistent"));
        assert!(result.is_err(), "sync with missing template should fail");
    }
}
