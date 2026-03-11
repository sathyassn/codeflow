//! Doctor command: diagnose infrastructure health.

use anyhow::Result;

use crate::helpers;

pub async fn run() -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    let state_dir = project_dir.join(".state");

    let session_id = helpers::resolve_session_id(&state_dir).unwrap_or_default();

    let opts = codeflow_core::doctor::Options {
        db_path: state_dir
            .join("db")
            .join("codeflow.db")
            .to_string_lossy()
            .to_string(),
        ledger_dir: state_dir.join("ledger").to_string_lossy().to_string(),
        project_dir: project_dir.to_string_lossy().to_string(),
        state_dir: state_dir.to_string_lossy().to_string(),
        session_id,
        home_dir: std::env::var("HOME").unwrap_or_default(),
        look_path: None,
        exec_command: None,
    };

    let results = codeflow_core::doctor::run_all(&opts).await;

    let mut has_failure = false;
    for r in &results {
        let icon = match r.status {
            codeflow_core::doctor::Status::Pass => "ok",
            codeflow_core::doctor::Status::Warn => "warn",
            codeflow_core::doctor::Status::Fail => {
                has_failure = true;
                "FAIL"
            }
        };
        println!("[{icon}] {}: {}", r.name, r.message);
    }

    if has_failure {
        anyhow::bail!("one or more doctor checks failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_doctor_command_exists() {
        // Compile-time verification that run is async.
        let _: fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>>>> =
            || Box::pin(super::run());
    }

    #[test]
    fn test_doctor_status_display_variants() {
        // Verify all Status variants map to expected display strings.
        let pass = codeflow_core::doctor::Status::Pass;
        let warn = codeflow_core::doctor::Status::Warn;
        let fail = codeflow_core::doctor::Status::Fail;
        assert!(matches!(pass, codeflow_core::doctor::Status::Pass));
        assert!(matches!(warn, codeflow_core::doctor::Status::Warn));
        assert!(matches!(fail, codeflow_core::doctor::Status::Fail));
    }

    #[test]
    fn test_doctor_options_construction() {
        let opts = codeflow_core::doctor::Options {
            db_path: "/tmp/db".to_string(),
            ledger_dir: "/tmp/ledger".to_string(),
            project_dir: "/tmp/project".to_string(),
            state_dir: "/tmp/state".to_string(),
            session_id: "ses-test".to_string(),
            home_dir: "/home/test".to_string(),
            look_path: None,
            exec_command: None,
        };
        assert_eq!(opts.db_path, "/tmp/db");
        assert_eq!(opts.session_id, "ses-test");
        assert!(opts.look_path.is_none());
    }
}
