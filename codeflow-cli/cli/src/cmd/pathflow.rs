//! Pathflow command: display pathflow phase status.

use std::path::Path;

use anyhow::{Context, Result};

use crate::helpers;

pub fn run() -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_with_dir(&project_dir)
}

fn run_with_dir(project_dir: &Path) -> Result<()> {
    let state_dir = project_dir.join(".state");

    let Ok(sid) = helpers::resolve_session_id(&state_dir) else {
        println!("no active session");
        return Ok(());
    };

    // Check pathflow session status.
    let pathflow_dir = state_dir.join("session").join(&sid).join("pathflow");
    let status_path = pathflow_dir.join("pathflow-session-status.json");

    let is_active = if let Ok(data) = std::fs::read_to_string(&status_path) {
        serde_json::from_str::<serde_json::Value>(&data)
            .ok()
            .and_then(|v| v.get("status").and_then(|s| s.as_str()).map(String::from))
            .is_some_and(|s| !s.is_empty() && s != "pf-complete")
    } else {
        false
    };

    if !is_active {
        println!("pathflow: inactive");
        return Ok(());
    }

    println!("pathflow: active (session {sid})");

    // List sentinels to show phase progress.
    let sentinel_dir = codeflow_core::pathflow::sentinel::resolve_dir(project_dir, &sid)
        .context("resolving sentinel directory")?;

    if sentinel_dir.is_dir() {
        let entries = std::fs::read_dir(&sentinel_dir).context("reading sentinel directory")?;

        let mut names: Vec<String> = Vec::new();
        for entry in entries {
            let entry = entry?;
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_string());
            }
        }
        names.sort();

        for name in &names {
            println!("  {name}");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a minimal env file so `resolve_session_id` returns a session ID.
    fn write_env_file(state_dir: &std::path::Path, sid: &str) {
        std::fs::create_dir_all(state_dir).unwrap();
        let env_content =
            format!("export CODEFLOW_SESSION_ID='{sid}'\nexport CF_PROJECT_ROOT='/tmp/test'\n");
        std::fs::write(state_dir.join("codeflow-env.sh"), env_content).unwrap();
    }

    #[test]
    fn test_pathflow_no_session() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_pathflow_inactive() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".state");
        write_env_file(&state_dir, "ses-testpathflowinactive12345");
        let result = run_with_dir(dir.path());
        // Session exists but no pathflow-active flag => "pathflow: inactive"
        assert!(result.is_ok());
    }

    #[test]
    fn test_pathflow_active_no_sentinels() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".state");
        let sid = "ses-testpathflowactive123456";
        write_env_file(&state_dir, sid);
        // Create status file.
        let flag_dir = state_dir.join("session").join(sid).join("pathflow");
        std::fs::create_dir_all(&flag_dir).unwrap();
        std::fs::write(
            flag_dir.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress"}"#,
        )
        .unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_pathflow_active_with_sentinels() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".state");
        let sid = "ses-testpathflowsentinels12";
        write_env_file(&state_dir, sid);
        // Create status file.
        let flag_dir = state_dir.join("session").join(sid).join("pathflow");
        std::fs::create_dir_all(&flag_dir).unwrap();
        std::fs::write(
            flag_dir.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress"}"#,
        )
        .unwrap();
        // Create sentinel directory with some sentinel files.
        let sentinel_dir = state_dir.join("sentinels").join("pathflow").join(sid);
        std::fs::create_dir_all(&sentinel_dir).unwrap();
        std::fs::write(sentinel_dir.join("pathflow-pf-1"), "").unwrap();
        std::fs::write(sentinel_dir.join("pathflow-pf-2"), "").unwrap();
        let result = run_with_dir(dir.path());
        assert!(result.is_ok());
    }
}
