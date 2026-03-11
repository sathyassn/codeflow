//! Session command: display current session information.

use std::path::Path;

use anyhow::{Context, Result};

use crate::helpers;

pub fn run() -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_with_dir(&project_dir)
}

fn run_with_dir(project_dir: &Path) -> Result<()> {
    let state_dir = project_dir.join(".state");

    let sid = helpers::resolve_session_id(&state_dir).context("resolving current session")?;

    println!("{sid}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_env_file(state_dir: &std::path::Path, sid: &str) {
        std::fs::create_dir_all(state_dir).unwrap();
        let env_content =
            format!("export CODEFLOW_SESSION_ID='{sid}'\nexport CF_PROJECT_ROOT='/tmp/test'\n");
        std::fs::write(state_dir.join("codeflow-env.sh"), env_content).unwrap();
    }

    #[test]
    fn test_session_no_state_dir_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(
            msg.contains("resolving current session"),
            "expected context 'resolving current session', got: {msg}"
        );
    }

    #[test]
    fn test_session_with_env_file() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".state");
        write_env_file(&state_dir, "ses-testsessionwithenvfile12");
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }
}
