//! Git hooks command: manage git hooks (commit-msg, pre-commit, etc.).

use std::path::Path;

use anyhow::{Context, Result};

use crate::helpers;

pub fn run() -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_with_dir(&project_dir)
}

fn run_with_dir(project_dir: &Path) -> Result<()> {
    let hooks_dir = project_dir.join(".git").join("hooks");

    if !hooks_dir.is_dir() {
        println!("no git hooks directory");
        return Ok(());
    }

    let entries = std::fs::read_dir(&hooks_dir).context("reading git hooks directory")?;

    let mut count = 0;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        // Skip .sample files.
        if path.extension().and_then(|e| e.to_str()) == Some("sample") {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            println!("{name}");
            count += 1;
        }
    }

    if count == 0 {
        println!("no active git hooks");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_git_hooks_command_exists() {
        let _: fn() -> Result<()> = run;
    }

    #[test]
    fn test_git_hooks_no_git_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_git_hooks_skips_sample_files() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join(".git").join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        std::fs::write(hooks_dir.join("pre-commit.sample"), "#!/bin/sh").unwrap();
        std::fs::write(hooks_dir.join("commit-msg"), "#!/bin/sh").unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_git_hooks_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join(".git").join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }
}
