//! Interactive wizard for `codeflow test setup`.
//!
//! Uses the injectable [`PromptProvider`] trait so tests can drive the entire
//! wizard flow without a live terminal.

use std::collections::BTreeMap;
use std::path::Path;

use crate::testing::config::{
    CoverageConfig, CoverageFormat, CoverageRule, CoverageScope, ExecutionConfig, ModeCommand,
    ReportConfig, ReportFormat, RunnerType, TargetConfig, TestConfig,
};
use crate::testing::setup::detect;
use crate::testing::setup::prompt::{ExistingConfigAction, PromptProvider};

/// Run the interactive wizard, returning the built config.
///
/// The wizard flow:
/// 1. Detect existing config -> ask Edit/Replace/Abort
/// 2. Detect stacks -> confirm each
/// 3. Per-target: walk through mode commands, report config, coverage config
/// 4. Build config
///
/// Doctor validation and write happen in the caller (setup/mod.rs).
///
/// # Errors
///
/// Returns `WizardError` on I/O failure or invalid input.
pub fn run_wizard(
    repo_root: &Path,
    prompts: &dyn PromptProvider,
    existing_config: bool,
) -> Result<Option<TestConfig>, WizardError> {
    // Step 1: Handle existing config
    if existing_config {
        let action = ask_existing_action(prompts)?;
        match action {
            ExistingConfigAction::Abort => return Ok(None),
            ExistingConfigAction::Edit | ExistingConfigAction::Replace => {}
        }
    }

    // Step 2: Detect stacks
    let detected = detect::detect_stacks(repo_root);
    let mut targets: Vec<TargetConfig> = Vec::new();

    if detected.is_empty() {
        println!("No test stacks detected. Starting with empty config.");
    } else {
        println!("Detected {} test stack(s):", detected.len());
        for dt in &detected {
            let runner_str =
                serde_json::to_string(&dt.config.runner).unwrap_or_else(|_| "unknown".to_string());
            println!(
                "  - {} (runner: {}, cwd: {})",
                dt.config.name,
                runner_str.trim_matches('"'),
                dt.config.cwd.as_deref().unwrap_or("."),
            );
        }

        // Step 3: Confirm each and customize
        for dt in detected {
            let add = prompts.confirm(&format!("Add target '{}'?", dt.config.name), true)?;
            if add {
                let customized = customize_target(prompts, dt.config)?;
                targets.push(customized);
            }
        }
    }

    // Step 4: Build config
    let config = TestConfig {
        description: None,
        schema_ref: Some(".codeflow/schemas/test-config.schema.json".to_string()),
        schema_version: "1.0".to_string(),
        execution: ExecutionConfig::default(),
        defaults: crate::testing::config::DefaultsConfig::default(),
        targets,
    };

    Ok(Some(config))
}

/// Walk through per-target customization prompts.
///
/// Detected defaults are shown in brackets — pressing Enter accepts the default.
fn customize_target(
    prompts: &dyn PromptProvider,
    mut target: TargetConfig,
) -> Result<TargetConfig, WizardError> {
    // Mode commands
    let default_essential = target
        .modes
        .get("essential")
        .map_or(String::new(), |m| m.command.clone());
    let default_full = target
        .modes
        .get("full")
        .map_or(String::new(), |m| m.command.clone());

    let essential_input =
        prompts.prompt(&format!("  Essential mode command [{default_essential}]:"))?;
    if !essential_input.is_empty() {
        target.modes.insert(
            "essential".to_string(),
            ModeCommand {
                command: essential_input,
            },
        );
    }

    let full_input = prompts.prompt(&format!("  Full mode command [{default_full}]:"))?;
    if !full_input.is_empty() {
        target.modes.insert(
            "full".to_string(),
            ModeCommand {
                command: full_input,
            },
        );
    }

    // Report config
    let has_report = target.report.is_some();
    let default_report_path = target
        .report
        .as_ref()
        .map_or(String::new(), |r| r.path.clone());
    let report_path_input = prompts.prompt(&format!(
        "  Report path (empty={}): [{default_report_path}]:",
        if has_report { "keep" } else { "none" }
    ))?;
    if !report_path_input.is_empty() {
        let report_fmt = target
            .report
            .as_ref()
            .map_or(ReportFormat::Junit, |r| r.format.clone());
        target.report = Some(ReportConfig {
            format: report_fmt,
            path: report_path_input,
            derive_from: target.report.and_then(|r| r.derive_from),
        });
    }

    // Coverage config
    let has_coverage = target.coverage.is_some();
    let default_cov_path = target
        .coverage
        .as_ref()
        .map_or(String::new(), |c| c.path.clone());
    let cov_path_input = prompts.prompt(&format!(
        "  Coverage path (empty={}): [{default_cov_path}]:",
        if has_coverage { "keep" } else { "none" }
    ))?;
    if !cov_path_input.is_empty() {
        let cov_fmt = target
            .coverage
            .as_ref()
            .map_or(CoverageFormat::Lcov, |c| c.format.clone());
        let existing_rules = target
            .coverage
            .as_ref()
            .map_or_else(Vec::new, |c| c.rules.clone());
        let rules = if existing_rules.is_empty() {
            vec![CoverageRule {
                scope: CoverageScope::PerFile,
                include: vec![],
                exclude: vec![],
                minimum: 85,
            }]
        } else {
            existing_rules
        };
        target.coverage = Some(CoverageConfig {
            format: cov_fmt,
            path: cov_path_input,
            transform: target.coverage.and_then(|c| c.transform),
            rules,
            exceptions: vec![],
        });
    }

    Ok(target)
}

/// Run the add-target wizard (appends a single target).
///
/// # Errors
///
/// Returns `WizardError` on I/O failure or invalid input.
pub fn run_add_target_wizard(
    repo_root: &Path,
    prompts: &dyn PromptProvider,
) -> Result<Option<TargetConfig>, WizardError> {
    let name = prompts.prompt("Target name:")?;
    if name.is_empty() {
        return Err(WizardError::EmptyInput("target name".to_string()));
    }

    let runner_str =
        prompts.prompt("Runner (cargo/pytest/jest/vitest/go/mocha/rspec/phpunit/custom):")?;
    let runner: RunnerType = serde_json::from_str(&format!("\"{runner_str}\""))
        .map_err(|_| WizardError::InvalidRunner(runner_str.clone()))?;

    let cwd = prompts.prompt("Working directory (relative to repo root, or '.' for root):")?;
    let cwd = if cwd.is_empty() || cwd == "." {
        None
    } else {
        if std::path::Path::new(&cwd).is_absolute() || cwd.split('/').any(|c| c == "..") {
            return Err(WizardError::InvalidCwd(cwd));
        }
        Some(cwd)
    };

    let essential_cmd = prompts.prompt("Essential mode command (e.g., 'cargo test'):")?;
    let full_cmd = prompts.prompt("Full mode command (e.g., 'cargo test --all'):")?;

    let mut modes = BTreeMap::new();
    if !essential_cmd.is_empty() {
        modes.insert(
            "essential".to_string(),
            ModeCommand {
                command: essential_cmd,
            },
        );
    }
    if !full_cmd.is_empty() {
        modes.insert("full".to_string(), ModeCommand { command: full_cmd });
    }

    let target = TargetConfig {
        name,
        enabled: true,
        cwd,
        env: BTreeMap::new(),
        runner,
        modes,
        report: None,
        coverage: None,
        ci_skip: None,
        ci_skip_reason: None,
        structural: None,
        tags: Vec::new(),
        test_files: Vec::new(),
    };

    let _ = repo_root;

    Ok(Some(target))
}

fn ask_existing_action(prompts: &dyn PromptProvider) -> Result<ExistingConfigAction, WizardError> {
    let answer = prompts.prompt("Existing config found. [E]dit / [R]eplace / [A]bort:")?;
    match answer.to_lowercase().as_str() {
        "e" | "edit" => Ok(ExistingConfigAction::Edit),
        "r" | "replace" => Ok(ExistingConfigAction::Replace),
        _ => Ok(ExistingConfigAction::Abort),
    }
}

/// Errors from the wizard.
#[derive(Debug, thiserror::Error)]
pub enum WizardError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("empty input for {0}")]
    EmptyInput(String),

    #[error("invalid runner: {0}")]
    InvalidRunner(String),

    #[error("invalid cwd (absolute path or parent traversal not allowed): {0}")]
    InvalidCwd(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::setup::prompt::ScriptedPromptProvider;

    #[test]
    fn wizard_abort_on_existing_config() {
        let dir = tempfile::tempdir().unwrap();
        let prompts = ScriptedPromptProvider::new(vec!["a"]);
        let result = run_wizard(dir.path(), &prompts, true).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn wizard_replace_existing_no_stacks() {
        let dir = tempfile::tempdir().unwrap();
        let prompts = ScriptedPromptProvider::new(vec!["r"]);
        let result = run_wizard(dir.path(), &prompts, true).unwrap();
        let config = result.unwrap();
        assert!(config.targets.is_empty());
    }

    #[test]
    fn wizard_detects_rust_and_confirms_with_defaults() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"test\"\n",
        )
        .unwrap();
        // "y" to confirm, then 3 empty strings to accept defaults for modes/report/coverage
        let prompts = ScriptedPromptProvider::new(vec!["y", "", "", "", ""]);
        let config = run_wizard(dir.path(), &prompts, false).unwrap().unwrap();
        assert_eq!(config.targets.len(), 1);
        assert_eq!(config.targets[0].name, "rust-core");
        // Defaults preserved from detection
        assert!(config.targets[0].modes.contains_key("essential"));
        assert!(config.targets[0].modes.contains_key("full"));
    }

    #[test]
    fn wizard_detects_rust_and_customizes_modes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"test\"\n",
        )
        .unwrap();
        // "y" to confirm, custom essential, custom full, empty report, empty coverage
        let prompts =
            ScriptedPromptProvider::new(vec!["y", "cargo test --lib", "cargo test --all", "", ""]);
        let config = run_wizard(dir.path(), &prompts, false).unwrap().unwrap();
        assert_eq!(
            config.targets[0].modes["essential"].command,
            "cargo test --lib"
        );
        assert_eq!(config.targets[0].modes["full"].command, "cargo test --all");
    }

    #[test]
    fn wizard_detects_rust_and_declines() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"test\"\n",
        )
        .unwrap();
        let prompts = ScriptedPromptProvider::new(vec!["n"]);
        let config = run_wizard(dir.path(), &prompts, false).unwrap().unwrap();
        assert!(config.targets.is_empty());
    }

    #[test]
    fn wizard_no_existing_no_stacks() {
        let dir = tempfile::tempdir().unwrap();
        let prompts = ScriptedPromptProvider::new(vec![]);
        let config = run_wizard(dir.path(), &prompts, false).unwrap().unwrap();
        assert!(config.targets.is_empty());
    }

    #[test]
    fn add_target_wizard_basic() {
        let dir = tempfile::tempdir().unwrap();
        let prompts = ScriptedPromptProvider::new(vec![
            "my-target",
            "cargo",
            ".",
            "cargo test",
            "cargo test --all",
        ]);
        let target = run_add_target_wizard(dir.path(), &prompts)
            .unwrap()
            .unwrap();
        assert_eq!(target.name, "my-target");
        assert_eq!(target.runner, RunnerType::Cargo);
        assert!(target.modes.contains_key("essential"));
        assert!(target.modes.contains_key("full"));
    }

    #[test]
    fn add_target_wizard_empty_name_errors() {
        let dir = tempfile::tempdir().unwrap();
        let prompts = ScriptedPromptProvider::new(vec![""]);
        let result = run_add_target_wizard(dir.path(), &prompts);
        assert!(result.is_err());
    }

    #[test]
    fn add_target_wizard_invalid_runner() {
        let dir = tempfile::tempdir().unwrap();
        let prompts = ScriptedPromptProvider::new(vec!["my-target", "invalid-runner"]);
        let result = run_add_target_wizard(dir.path(), &prompts);
        assert!(result.is_err());
    }

    #[test]
    fn add_target_wizard_rejects_traversal_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let prompts = ScriptedPromptProvider::new(vec!["my-target", "cargo", "../../etc"]);
        let result = run_add_target_wizard(dir.path(), &prompts);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("invalid cwd"), "got: {err}");
    }

    #[test]
    fn add_target_wizard_rejects_absolute_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let prompts = ScriptedPromptProvider::new(vec!["my-target", "cargo", "/etc"]);
        let result = run_add_target_wizard(dir.path(), &prompts);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("invalid cwd"), "got: {err}");
    }

    #[test]
    fn customize_target_accepts_all_defaults() {
        let prompts = ScriptedPromptProvider::new(vec!["", "", "", ""]);
        let target = TargetConfig {
            name: "test".to_string(),
            enabled: true,
            cwd: None,
            env: BTreeMap::new(),
            runner: RunnerType::Cargo,
            modes: BTreeMap::from([
                (
                    "essential".to_string(),
                    ModeCommand {
                        command: "cargo test".to_string(),
                    },
                ),
                (
                    "full".to_string(),
                    ModeCommand {
                        command: "cargo test --all".to_string(),
                    },
                ),
            ]),
            report: None,
            coverage: None,
            ci_skip: None,
            ci_skip_reason: None,
            structural: None,
            tags: Vec::new(),
            test_files: Vec::new(),
        };
        let result = customize_target(&prompts, target).unwrap();
        assert_eq!(result.modes["essential"].command, "cargo test");
        assert_eq!(result.modes["full"].command, "cargo test --all");
        assert!(result.report.is_none());
        assert!(result.coverage.is_none());
    }

    #[test]
    fn customize_target_overrides_modes_and_coverage() {
        let prompts = ScriptedPromptProvider::new(vec![
            "custom-essential",
            "custom-full",
            "",
            "coverage.lcov",
        ]);
        let target = TargetConfig {
            name: "test".to_string(),
            enabled: true,
            cwd: None,
            env: BTreeMap::new(),
            runner: RunnerType::Cargo,
            modes: BTreeMap::new(),
            report: None,
            coverage: None,
            ci_skip: None,
            ci_skip_reason: None,
            structural: None,
            tags: Vec::new(),
            test_files: Vec::new(),
        };
        let result = customize_target(&prompts, target).unwrap();
        assert_eq!(result.modes["essential"].command, "custom-essential");
        assert_eq!(result.modes["full"].command, "custom-full");
        assert!(result.coverage.is_some());
        assert_eq!(result.coverage.unwrap().path, "coverage.lcov");
    }
}
