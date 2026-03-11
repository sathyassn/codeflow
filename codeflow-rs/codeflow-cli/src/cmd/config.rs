//! Config command: display configuration status.

use std::path::Path;

use anyhow::{Context, Result};

use crate::helpers;

pub fn run() -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_with_dir(&project_dir)
}

fn run_with_dir(project_dir: &Path) -> Result<()> {
    let config_dir = project_dir.join(".codeflow").join("config");

    if !config_dir.is_dir() {
        println!("no config directory at {}", config_dir.display());
        return Ok(());
    }

    // List config files.
    let entries = std::fs::read_dir(&config_dir).context("reading config directory")?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let dir_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            println!("{dir_name}/");
        } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            println!("{name}");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_command_exists() {
        let _: fn() -> Result<()> = run;
    }

    #[test]
    fn test_config_no_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_config_with_entries() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow").join("config");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(config_dir.join("enforcement")).unwrap();
        std::fs::write(config_dir.join("settings.json"), "{}").unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }
}
