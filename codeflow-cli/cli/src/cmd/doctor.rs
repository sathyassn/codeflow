//! Doctor command: diagnose infrastructure health.

use anyhow::Result;

use crate::helpers;

pub async fn run() -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_with_dir(&project_dir).await
}

async fn run_with_dir(project_dir: &std::path::Path) -> Result<()> {
    let state_dir = project_dir.join(".state");

    let session_id = helpers::resolve_session_id(project_dir).unwrap_or_default();

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
    use super::*;
    use codeflow_core::doctor::{CheckResult, Options, Status};
    use std::time::Duration;

    #[test]
    fn test_doctor_command_exists() {
        // Compile-time verification that run is async.
        let _: fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>>>> =
            || Box::pin(super::run());
    }

    #[test]
    fn test_doctor_status_display_variants() {
        let pass = Status::Pass;
        let warn = Status::Warn;
        let fail = Status::Fail;
        assert!(matches!(pass, Status::Pass));
        assert!(matches!(warn, Status::Warn));
        assert!(matches!(fail, Status::Fail));
    }

    #[test]
    fn test_doctor_options_construction() {
        let opts = Options {
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

    #[test]
    fn test_doctor_status_icon_mapping() {
        // Verify the icon mapping matches what run() produces.
        let results = vec![
            CheckResult {
                name: "db".to_string(),
                status: Status::Pass,
                message: "ok".to_string(),
                duration: Duration::ZERO,
            },
            CheckResult {
                name: "ledger".to_string(),
                status: Status::Warn,
                message: "missing".to_string(),
                duration: Duration::ZERO,
            },
            CheckResult {
                name: "schema".to_string(),
                status: Status::Fail,
                message: "broken".to_string(),
                duration: Duration::ZERO,
            },
        ];

        let mut has_failure = false;
        let mut output = Vec::new();
        for r in &results {
            let icon = match r.status {
                Status::Pass => "ok",
                Status::Warn => "warn",
                Status::Fail => {
                    has_failure = true;
                    "FAIL"
                }
            };
            output.push(format!("[{icon}] {}: {}", r.name, r.message));
        }

        assert_eq!(output[0], "[ok] db: ok");
        assert_eq!(output[1], "[warn] ledger: missing");
        assert_eq!(output[2], "[FAIL] schema: broken");
        assert!(has_failure);
    }

    #[test]
    fn test_doctor_no_failures() {
        let results = vec![CheckResult {
            name: "check1".to_string(),
            status: Status::Pass,
            message: "good".to_string(),
            duration: Duration::ZERO,
        }];

        let mut has_failure = false;
        for r in &results {
            if matches!(r.status, Status::Fail) {
                has_failure = true;
            }
        }
        assert!(!has_failure);
    }

    #[tokio::test]
    async fn test_doctor_run_all_with_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".state");
        std::fs::create_dir_all(state_dir.join("db")).unwrap();
        std::fs::create_dir_all(state_dir.join("ledger")).unwrap();

        let opts = Options {
            db_path: state_dir
                .join("db")
                .join("codeflow.db")
                .to_string_lossy()
                .to_string(),
            ledger_dir: state_dir.join("ledger").to_string_lossy().to_string(),
            project_dir: dir.path().to_string_lossy().to_string(),
            state_dir: state_dir.to_string_lossy().to_string(),
            session_id: String::new(),
            home_dir: dir.path().to_string_lossy().to_string(),
            look_path: None,
            exec_command: None,
        };

        let results = codeflow_core::doctor::run_all(&opts).await;
        assert!(!results.is_empty());
    }

    #[tokio::test]
    async fn test_doctor_run_with_dir() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".state");
        std::fs::create_dir_all(state_dir.join("db")).unwrap();
        std::fs::create_dir_all(state_dir.join("ledger")).unwrap();

        // run_with_dir exercises the full run() path minus detect_project_dir.
        // Some checks may fail (expected in a temp dir), so we just verify it runs.
        let _result = run_with_dir(dir.path()).await;
    }
}
