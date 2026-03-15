//! Sentinel command: list pathflow sentinel files.

use std::path::Path;

use anyhow::{Context, Result};

use crate::helpers;

pub fn run() -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_with_dir(&project_dir)
}

fn run_with_dir(project_dir: &Path) -> Result<()> {
    let Ok(sid) = helpers::resolve_session_id(project_dir) else {
        println!("no active session");
        return Ok(());
    };

    let sentinel_dir = codeflow_core::pathflow::sentinel::resolve_dir(project_dir, &sid)
        .context("resolving sentinel directory")?;

    if !sentinel_dir.is_dir() {
        println!("no sentinels");
        return Ok(());
    }

    let entries = std::fs::read_dir(&sentinel_dir).context("reading sentinel directory")?;

    let mut names: Vec<String> = Vec::new();
    for entry in entries {
        let entry = entry?;
        if let Some(name) = entry.file_name().to_str() {
            names.push(name.to_string());
        }
    }
    names.sort();

    if names.is_empty() {
        println!("no sentinels");
    } else {
        for name in &names {
            println!("{name}");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_env_file(state_dir: &std::path::Path, sid: &str) {
        let runtime_dir = state_dir.join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        let env_content =
            format!("export CODEFLOW_SESSION_ID='{sid}'\nexport CF_PROJECT_ROOT='/tmp/test'\n");
        std::fs::write(runtime_dir.join("codeflow-env.sh"), env_content).unwrap();
    }

    #[test]
    fn test_sentinel_no_session() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_sentinel_no_sentinel_dir() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".state");
        write_env_file(&state_dir, "ses-testsentinelnodir1234567");
        let result = run_with_dir(dir.path());
        // Session exists but no sentinel directory => "no sentinels"
        assert!(result.is_ok());
    }

    #[test]
    fn test_sentinel_empty_sentinel_dir() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".state");
        let sid = "ses-testsentinelemptydir1234";
        write_env_file(&state_dir, sid);
        let sentinel_dir = state_dir.join("sentinels").join("pathflow").join(sid);
        std::fs::create_dir_all(&sentinel_dir).unwrap();
        let result = run_with_dir(dir.path());
        // Empty sentinel dir => "no sentinels"
        assert!(result.is_ok());
    }

    #[test]
    fn test_sentinel_with_sentinel_files() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".state");
        let sid = "ses-testsentinelwithfiles123";
        write_env_file(&state_dir, sid);
        let sentinel_dir = state_dir.join("sentinels").join("pathflow").join(sid);
        std::fs::create_dir_all(&sentinel_dir).unwrap();
        std::fs::write(sentinel_dir.join("pathflow-pf-1"), "").unwrap();
        std::fs::write(sentinel_dir.join("pathflow-pf-2"), "").unwrap();
        std::fs::write(sentinel_dir.join("pathflow-pf-3"), "").unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }
}
