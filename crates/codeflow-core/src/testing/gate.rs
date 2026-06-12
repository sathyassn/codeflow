//! The test gate: one entry point shared by `codeflow test` and `integrate`.
//!
//! Resolution order (charter §3.1, AC #7):
//! 1. `.codeflow/test-config.json` present → run its targets as configured.
//! 2. No config → runtime stack detection ([`crate::testing::setup::detect`]);
//!    detected targets run with best-guess commands.
//! 3. Nothing detected → [`GateOutcome::NoTargets`] — the caller MUST surface
//!    this loudly (it is a no-op, not a green run), but it is not an error.

use std::path::Path;

use crate::testing::config::load_test_config;
use crate::testing::error::TestingError;
use crate::testing::runner::run_all_targets;
use crate::testing::setup::detect::detect_stacks;

/// Relative path of the test configuration inside a project.
pub const TEST_CONFIG_PATH: &str = ".codeflow/test-config.json";

/// Result of one target's run through the gate.
#[derive(Debug, Clone)]
pub struct GateTargetResult {
    pub name: String,
    /// Process exit code; `-1` when the command could not run at all.
    pub exit_code: i32,
    pub duration_ms: u64,
    pub stdout: String,
    pub stderr: String,
    /// Set when the target failed before producing an exit code
    /// (spawn failure, missing mode, panic).
    pub error: Option<String>,
}

impl GateTargetResult {
    /// Whether this target passed.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.exit_code == 0 && self.error.is_none()
    }
}

/// Outcome of running the test gate.
#[derive(Debug)]
pub enum GateOutcome {
    /// Nothing to run: no config targets and no detectable stack (or no
    /// target defines the requested mode). Callers must report this loudly.
    NoTargets { reason: String },
    /// Targets ran; `passed` is the gate verdict.
    Completed {
        results: Vec<GateTargetResult>,
        passed: bool,
    },
}

impl GateOutcome {
    /// Whether the gate allows proceeding (no-targets counts as allowed —
    /// AC #7: loud no-op, exit 0).
    #[must_use]
    pub fn allows_proceed(&self) -> bool {
        match self {
            Self::NoTargets { .. } => true,
            Self::Completed { passed, .. } => *passed,
        }
    }
}

/// Run the test gate for a project directory in the given mode
/// (`full` or `quick`).
///
/// # Errors
///
/// Returns `TestingError` when a present config file cannot be loaded —
/// a broken config is a failure, never a silent skip.
pub fn run_gate(project_dir: &Path, mode: &str) -> Result<GateOutcome, TestingError> {
    let config_path = project_dir.join(TEST_CONFIG_PATH);

    let (targets, parallel, fail_fast, effective_mode) = if config_path.exists() {
        let config = load_test_config(&config_path)?;
        if config.targets.is_empty() {
            return Ok(GateOutcome::NoTargets {
                reason: format!("{TEST_CONFIG_PATH} defines no targets"),
            });
        }
        (
            config.targets,
            config.execution.parallel,
            config.execution.fail_fast,
            mode.to_string(),
        )
    } else {
        let detected = detect_stacks(project_dir);
        if detected.is_empty() {
            return Ok(GateOutcome::NoTargets {
                reason: format!("no {TEST_CONFIG_PATH} and no test stack detected"),
            });
        }
        // Detected targets define `essential` and `full` modes; `quick`
        // maps to the lighter `essential` command set.
        let effective = if mode == "quick" { "essential" } else { mode };
        (
            detected.into_iter().map(|d| d.config).collect(),
            false,
            false,
            effective.to_string(),
        )
    };

    let raw = run_all_targets(
        &targets,
        &effective_mode,
        project_dir,
        parallel,
        fail_fast,
        &[],
        &[],
        &[],
        &[],
    );

    if raw.is_empty() {
        return Ok(GateOutcome::NoTargets {
            reason: format!("no enabled target defines mode \"{effective_mode}\""),
        });
    }

    let results: Vec<GateTargetResult> = raw
        .into_iter()
        .map(|r| match r {
            Ok(run) => GateTargetResult {
                name: run.target_name,
                exit_code: run.exit_code,
                duration_ms: run.duration_ms,
                stdout: run.stdout,
                stderr: run.stderr,
                error: None,
            },
            Err(e) => GateTargetResult {
                name: "(unrunnable)".to_string(),
                exit_code: -1,
                duration_ms: 0,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(e.to_string()),
            },
        })
        .collect();

    let passed = results.iter().all(GateTargetResult::passed);
    Ok(GateOutcome::Completed { results, passed })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_config(dir: &Path, targets_json: &str) {
        let codeflow = dir.join(".codeflow");
        std::fs::create_dir_all(&codeflow).unwrap();
        std::fs::write(
            codeflow.join("test-config.json"),
            format!(r#"{{"schema_version": "1.0", "targets": {targets_json}}}"#),
        )
        .unwrap();
    }

    fn echo_target(name: &str, command: &str) -> String {
        format!(
            r#"{{"name": "{name}", "runner": "custom", "modes": {{"full": {{"command": "{command}"}}, "quick": {{"command": "{command}"}}}}}}"#
        )
    }

    #[test]
    fn no_config_no_stack_is_loud_no_op() {
        let dir = tempfile::tempdir().unwrap();
        let outcome = run_gate(dir.path(), "full").unwrap();
        match outcome {
            GateOutcome::NoTargets { reason } => {
                assert!(reason.contains("no test stack detected"), "{reason}");
            }
            GateOutcome::Completed { .. } => panic!("expected NoTargets"),
        }
    }

    #[test]
    fn no_targets_allows_proceed() {
        let outcome = GateOutcome::NoTargets {
            reason: "x".into(),
        };
        assert!(outcome.allows_proceed());
    }

    #[test]
    fn configured_passing_target_runs_green() {
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), &format!("[{}]", echo_target("ok", "exit 0")));

        let outcome = run_gate(dir.path(), "full").unwrap();
        match outcome {
            GateOutcome::Completed { results, passed } => {
                assert!(passed);
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].name, "ok");
                assert!(results[0].passed());
            }
            GateOutcome::NoTargets { reason } => panic!("expected run, got NoTargets: {reason}"),
        }
    }

    #[test]
    fn configured_failing_target_fails_gate() {
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), &format!("[{}]", echo_target("bad", "exit 3")));

        let outcome = run_gate(dir.path(), "full").unwrap();
        match outcome {
            GateOutcome::Completed { results, passed } => {
                assert!(!passed);
                assert_eq!(results[0].exit_code, 3);
                assert!(!outcome_allows(&GateOutcome::Completed { results, passed }));
            }
            GateOutcome::NoTargets { reason } => panic!("expected run, got NoTargets: {reason}"),
        }
    }

    fn outcome_allows(o: &GateOutcome) -> bool {
        o.allows_proceed()
    }

    #[test]
    fn empty_targets_config_is_no_targets() {
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), "[]");

        let outcome = run_gate(dir.path(), "full").unwrap();
        assert!(matches!(outcome, GateOutcome::NoTargets { .. }));
    }

    #[test]
    fn broken_config_is_an_error_not_a_skip() {
        let dir = tempfile::tempdir().unwrap();
        let codeflow = dir.path().join(".codeflow");
        std::fs::create_dir_all(&codeflow).unwrap();
        std::fs::write(codeflow.join("test-config.json"), "{ not json").unwrap();

        let result = run_gate(dir.path(), "full");
        assert!(result.is_err(), "broken config must fail loud");
    }

    #[test]
    fn missing_mode_in_config_is_no_targets() {
        let dir = tempfile::tempdir().unwrap();
        // Target only defines "full"; ask for "quick".
        write_config(
            dir.path(),
            r#"[{"name": "only-full", "runner": "custom", "modes": {"full": {"command": "exit 0"}}}]"#,
        );

        let outcome = run_gate(dir.path(), "quick").unwrap();
        match outcome {
            GateOutcome::NoTargets { reason } => {
                assert!(reason.contains("quick"), "{reason}");
            }
            GateOutcome::Completed { .. } => panic!("expected NoTargets"),
        }
    }

    #[test]
    fn detected_stack_without_config_runs() {
        // A pyproject.toml with pytest makes detection fire; the pytest
        // command itself will fail in this environment, which is fine —
        // the point is that the gate RUNS rather than no-ops.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[tool.pytest.ini_options]\nminversion = \"6.0\"\n",
        )
        .unwrap();

        let outcome = run_gate(dir.path(), "full").unwrap();
        assert!(
            matches!(outcome, GateOutcome::Completed { .. }),
            "detected stack must run, got {outcome:?}"
        );
    }
}
