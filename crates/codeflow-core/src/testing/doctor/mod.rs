//! Test-config validator used by `codeflow doctor --check test-config`.
//!
//! Runs 9 checks against `test-config.json` and reports PASS/FAIL/WARN
//! for each. Designed to catch config drift and misconfigurations.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::testing::config::{self, RunnerType, TestConfig};
use crate::testing::runner::check_balanced_quotes;

/// Status of a single check.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

/// Result of a single doctor check.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DoctorCheck {
    pub name: String,
    pub status: CheckStatus,
    pub message: String,
}

/// Run all doctor checks against a config file on disk.
#[must_use]
pub fn run_all_checks(project_dir: &Path) -> Vec<DoctorCheck> {
    let config_path = project_dir.join(".codeflow").join("test-config.json");

    // Check 1: Config exists
    if !config_path.exists() {
        return vec![DoctorCheck {
            name: "config-exists".to_string(),
            status: CheckStatus::Fail,
            message: format!("test-config.json not found at {}", config_path.display()),
        }];
    }

    // Check 2: Schema validation (load the config)
    let config = match config::load_test_config(&config_path) {
        Ok(c) => c,
        Err(e) => {
            return vec![
                DoctorCheck {
                    name: "config-exists".to_string(),
                    status: CheckStatus::Pass,
                    message: "config file exists".to_string(),
                },
                DoctorCheck {
                    name: "schema-valid".to_string(),
                    status: CheckStatus::Fail,
                    message: format!("{e}"),
                },
            ];
        }
    };

    let mut checks = vec![
        DoctorCheck {
            name: "config-exists".to_string(),
            status: CheckStatus::Pass,
            message: "config file exists".to_string(),
        },
        DoctorCheck {
            name: "schema-valid".to_string(),
            status: CheckStatus::Pass,
            message: "config validates against schema".to_string(),
        },
    ];

    // Run per-target checks
    checks.extend(run_target_checks(&config, project_dir));

    checks
}

/// Run all checks against an in-memory config (used by setup wizard).
#[must_use]
pub fn run_all_checks_on_config(config: &TestConfig, project_dir: &Path) -> Vec<DoctorCheck> {
    let mut checks = vec![
        DoctorCheck {
            name: "config-exists".to_string(),
            status: CheckStatus::Pass,
            message: "config provided in-memory".to_string(),
        },
        DoctorCheck {
            name: "schema-valid".to_string(),
            status: CheckStatus::Pass,
            message: "config validates against schema".to_string(),
        },
    ];
    checks.extend(run_target_checks(config, project_dir));
    checks
}

// Imported v1 check sequence kept intact; splitting it adds indirection
// without clarifying the per-target check flow.
#[allow(clippy::too_many_lines)]
fn run_target_checks(config: &TestConfig, project_dir: &Path) -> Vec<DoctorCheck> {
    let mut checks = Vec::new();

    for target in &config.targets {
        if !target.enabled {
            continue;
        }

        // Check 3a: cwd exists
        if let Some(ref cwd) = target.cwd {
            let cwd_path = project_dir.join(cwd);
            if cwd_path.is_dir() {
                checks.push(DoctorCheck {
                    name: format!("{}.cwd-exists", target.name),
                    status: CheckStatus::Pass,
                    message: format!("cwd '{cwd}' exists"),
                });
            } else {
                checks.push(DoctorCheck {
                    name: format!("{}.cwd-exists", target.name),
                    status: CheckStatus::Fail,
                    message: format!("cwd '{}' does not exist at {}", cwd, cwd_path.display()),
                });
            }
        }

        // Check 3b: commands parse as shell
        for (mode_name, mode_cmd) in &target.modes {
            if check_balanced_quotes(&mode_cmd.command).is_ok() {
                checks.push(DoctorCheck {
                    name: format!("{}.{mode_name}.command-parses", target.name),
                    status: CheckStatus::Pass,
                    message: format!("command parses: {}", mode_cmd.command),
                });
            } else {
                checks.push(DoctorCheck {
                    name: format!("{}.{mode_name}.command-parses", target.name),
                    status: CheckStatus::Fail,
                    message: format!("command has unbalanced quotes: {}", mode_cmd.command),
                });
            }
        }

        // Check 3c: report.path safe
        if let Some(ref report) = target.report {
            checks.push(check_path_safety(
                &format!("{}.report-path", target.name),
                &report.path,
            ));
        }

        // Check 3d: coverage.path safe
        if let Some(ref coverage) = target.coverage {
            checks.push(check_path_safety(
                &format!("{}.coverage-path", target.name),
                &coverage.path,
            ));

            // Check 3e: coverage.transform parses
            if let Some(ref transform) = coverage.transform {
                if check_balanced_quotes(transform).is_ok() {
                    checks.push(DoctorCheck {
                        name: format!("{}.coverage-transform-parses", target.name),
                        status: CheckStatus::Pass,
                        message: format!("transform parses: {transform}"),
                    });
                } else {
                    checks.push(DoctorCheck {
                        name: format!("{}.coverage-transform-parses", target.name),
                        status: CheckStatus::Fail,
                        message: format!("transform has unbalanced quotes: {transform}"),
                    });
                }
            }

            // Check 3f: globs valid
            for rule in &coverage.rules {
                for glob_str in rule.include.iter().chain(rule.exclude.iter()) {
                    let valid = glob::Pattern::new(glob_str).is_ok();
                    if valid {
                        checks.push(DoctorCheck {
                            name: format!("{}.glob-valid", target.name),
                            status: CheckStatus::Pass,
                            message: format!("glob compiles: {glob_str}"),
                        });
                    } else {
                        checks.push(DoctorCheck {
                            name: format!("{}.glob-valid", target.name),
                            status: CheckStatus::Fail,
                            message: format!("invalid glob pattern: {glob_str}"),
                        });
                    }
                }
            }

            // Check 3g: exception files exist
            for exc in &coverage.exceptions {
                let exc_path = project_dir.join(&exc.file);
                if exc_path.exists() {
                    checks.push(DoctorCheck {
                        name: format!("{}.exception-file-exists", target.name),
                        status: CheckStatus::Pass,
                        message: format!("exception file exists: {}", exc.file),
                    });
                } else {
                    checks.push(DoctorCheck {
                        name: format!("{}.exception-file-exists", target.name),
                        status: CheckStatus::Warn,
                        message: format!(
                            "exception file not found: {} (may not exist yet)",
                            exc.file
                        ),
                    });
                }
            }
        }

        // The schema still accepts legacy structural blocks, but the
        // structural validator is deliberately not wired into the public gate.
        // Surface that boundary instead of implying these rules are enforced.
        if target.structural.is_some() {
            checks.push(DoctorCheck {
                name: format!("{}.structural-unenforced", target.name),
                status: CheckStatus::Warn,
                message: "structural rules are stored but not enforced by `codeflow test`; remove the block or treat it as documentation only".to_string(),
            });
        }

        // Check 4: dry-run probe
        checks.push(run_probe(&target.runner, &target.name));
    }

    checks
}

fn check_path_safety(name: &str, path: &str) -> DoctorCheck {
    // Config paths are portable repository-relative paths. A leading root,
    // drive prefix (including drive-relative `C:foo`), or UNC prefix can
    // escape the configured target directory on Windows even when the host
    // running this check uses another path grammar.
    let bytes = path.as_bytes();
    let root_qualified = path.starts_with(['/', '\\'])
        || bytes.get(1) == Some(&b':')
        || Path::new(path).is_absolute();
    if root_qualified {
        return DoctorCheck {
            name: name.to_string(),
            status: CheckStatus::Fail,
            message: format!("absolute path not allowed: {path}"),
        };
    }
    // Reject parent traversal
    if path.split(['/', '\\']).any(|c| c == "..") {
        return DoctorCheck {
            name: name.to_string(),
            status: CheckStatus::Fail,
            message: format!("parent traversal (..) not allowed in path: {path}"),
        };
    }
    if path.contains('\\') {
        return DoctorCheck {
            name: name.to_string(),
            status: CheckStatus::Fail,
            message: format!("portable path must use '/' separators: {path}"),
        };
    }
    DoctorCheck {
        name: name.to_string(),
        status: CheckStatus::Pass,
        message: format!("path is safe: {path}"),
    }
}

/// Probe timeout in seconds.
const PROBE_TIMEOUT_SECS: u64 = 5;

fn run_probe(runner: &RunnerType, target_name: &str) -> DoctorCheck {
    let (cmd, args) = match runner {
        RunnerType::Cargo => ("cargo", vec!["--version"]),
        RunnerType::Pytest => ("pytest", vec!["--version"]),
        RunnerType::Jest => ("jest", vec!["--version"]),
        RunnerType::Vitest => ("vitest", vec!["--version"]),
        RunnerType::Go => ("go", vec!["version"]),
        RunnerType::Mocha => ("mocha", vec!["--version"]),
        RunnerType::Rspec => ("rspec", vec!["--version"]),
        RunnerType::Phpunit => ("phpunit", vec!["--version"]),
        RunnerType::Custom => {
            return DoctorCheck {
                name: format!("{target_name}.probe"),
                status: CheckStatus::Pass,
                message: "custom runner: no probe".to_string(),
            };
        }
    };

    probe_command(cmd, &args, target_name)
}

/// Probes a specific `cmd` with `args` and maps the result to a non-blocking
/// [`DoctorCheck`]. Split out from [`run_probe`] so it can be exercised
/// hermetically with a guaranteed-absent command name — the runner-name path
/// depends on which tools happen to be installed on the host.
fn probe_command(cmd: &str, args: &[&str], target_name: &str) -> DoctorCheck {
    match run_with_timeout(cmd, args, Duration::from_secs(PROBE_TIMEOUT_SECS)) {
        ProbeResult::Success => DoctorCheck {
            name: format!("{target_name}.probe"),
            status: CheckStatus::Pass,
            message: format!("runner probe passed: {cmd}"),
        },
        ProbeResult::NonZeroExit(code) => DoctorCheck {
            name: format!("{target_name}.probe"),
            status: CheckStatus::Warn,
            message: format!("probe for runner={cmd} exited with code {code}; not blocking"),
        },
        ProbeResult::Timeout => DoctorCheck {
            name: format!("{target_name}.probe"),
            status: CheckStatus::Warn,
            message: format!(
                "probe for runner={cmd} exceeded {PROBE_TIMEOUT_SECS}s timeout; not blocking"
            ),
        },
        ProbeResult::SpawnError(msg) => DoctorCheck {
            name: format!("{target_name}.probe"),
            status: CheckStatus::Warn,
            message: format!("probe for runner={cmd} could not spawn: {msg}; not blocking"),
        },
    }
}

enum ProbeResult {
    Success,
    NonZeroExit(i32),
    Timeout,
    SpawnError(String),
}

fn run_with_timeout(cmd: &str, args: &[&str], timeout: Duration) -> ProbeResult {
    let mut child = match Command::new(cmd)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return ProbeResult::SpawnError(e.to_string()),
    };

    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    return ProbeResult::Success;
                }
                return ProbeResult::NonZeroExit(status.code().unwrap_or(-1));
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return ProbeResult::Timeout;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return ProbeResult::SpawnError(e.to_string()),
        }
    }
}

/// Format checks as human-readable output.
#[must_use]
pub fn format_human(checks: &[DoctorCheck]) -> String {
    use std::fmt::Write;

    let mut out = String::new();
    for (i, check) in checks.iter().enumerate() {
        let label = match check.status {
            CheckStatus::Pass => "PASS",
            CheckStatus::Warn => "WARN",
            CheckStatus::Fail => "FAIL",
        };
        let _ = writeln!(
            out,
            "  {}. [{label}] {}: {}",
            i + 1,
            check.name,
            check.message
        );
    }
    out
}

/// Format checks as JSON.
#[must_use]
pub fn format_json(checks: &[DoctorCheck]) -> String {
    serde_json::to_string_pretty(checks).unwrap_or_else(|_| "[]".to_string())
}

/// Compute exit code from checks.
///
/// 0 = all pass or only warnings, 1 = one or more failures.
#[must_use]
pub fn exit_code(checks: &[DoctorCheck]) -> i32 {
    i32::from(checks.iter().any(|c| c.status == CheckStatus::Fail))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::config::*;
    use std::collections::BTreeMap;

    fn make_minimal_config() -> TestConfig {
        TestConfig {
            description: None,
            schema_ref: None,
            schema_version: "1.0".to_string(),
            execution: ExecutionConfig::default(),
            defaults: DefaultsConfig::default(),
            targets: vec![],
        }
    }

    fn make_valid_target() -> TargetConfig {
        TargetConfig {
            name: "test".to_string(),
            enabled: true,
            cwd: None,
            env: BTreeMap::new(),
            shell: crate::testing::config::CommandShell::Auto,
            requires: Vec::new(),
            outputs: Vec::new(),
            narrow: Vec::new(),
            exclusive: false,
            runner: RunnerType::Custom,
            modes: BTreeMap::from([(
                "full".to_string(),
                ModeCommand {
                    command: "echo test".to_string(),
                },
            )]),
            report: None,
            coverage: None,
            ci_skip: None,
            ci_skip_reason: None,
            timeout_seconds: None,
            structural: None,
            tags: Vec::new(),
            test_files: Vec::new(),
        }
    }

    // Check 1: config exists — positive
    #[test]
    fn check_config_exists_pass() {
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join(".codeflow");
        std::fs::create_dir_all(&cfg_path).unwrap();
        std::fs::write(
            cfg_path.join("test-config.json"),
            r#"{"schema_version":"1.0","targets":[]}"#,
        )
        .unwrap();
        let checks = run_all_checks(dir.path());
        assert_eq!(checks[0].status, CheckStatus::Pass);
        assert_eq!(checks[0].name, "config-exists");
    }

    // Check 1: config exists — negative
    #[test]
    fn check_config_missing_fail() {
        let dir = tempfile::tempdir().unwrap();
        let checks = run_all_checks(dir.path());
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].status, CheckStatus::Fail);
        assert_eq!(checks[0].name, "config-exists");
    }

    // Check 2: schema validation — positive
    #[test]
    fn check_schema_valid_pass() {
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join(".codeflow");
        std::fs::create_dir_all(&cfg_path).unwrap();
        std::fs::write(
            cfg_path.join("test-config.json"),
            r#"{"schema_version":"1.0","targets":[]}"#,
        )
        .unwrap();
        let checks = run_all_checks(dir.path());
        assert!(checks.len() >= 2);
        assert_eq!(checks[1].status, CheckStatus::Pass);
        assert_eq!(checks[1].name, "schema-valid");
    }

    // Check 2: schema validation — negative
    #[test]
    fn check_schema_invalid_fail() {
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join(".codeflow");
        std::fs::create_dir_all(&cfg_path).unwrap();
        std::fs::write(cfg_path.join("test-config.json"), r#"{"targets":[]}"#).unwrap();
        let checks = run_all_checks(dir.path());
        assert_eq!(checks[1].status, CheckStatus::Fail);
        assert_eq!(checks[1].name, "schema-valid");
    }

    // Check 3a: cwd exists — positive
    #[test]
    fn check_cwd_exists_pass() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let mut config = make_minimal_config();
        let mut target = make_valid_target();
        target.cwd = Some("src".to_string());
        config.targets.push(target);
        let checks = run_target_checks(&config, dir.path());
        let cwd_check = checks
            .iter()
            .find(|c| c.name.contains("cwd-exists"))
            .unwrap();
        assert_eq!(cwd_check.status, CheckStatus::Pass);
    }

    // Check 3a: cwd exists — negative
    #[test]
    fn check_cwd_missing_fail() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = make_minimal_config();
        let mut target = make_valid_target();
        target.cwd = Some("nonexistent".to_string());
        config.targets.push(target);
        let checks = run_target_checks(&config, dir.path());
        let cwd_check = checks
            .iter()
            .find(|c| c.name.contains("cwd-exists"))
            .unwrap();
        assert_eq!(cwd_check.status, CheckStatus::Fail);
    }

    // Check 3b: command parses — positive
    #[test]
    fn check_command_parses_pass() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = make_minimal_config();
        config.targets.push(make_valid_target());
        let checks = run_target_checks(&config, dir.path());
        let cmd_check = checks
            .iter()
            .find(|c| c.name.contains("command-parses"))
            .unwrap();
        assert_eq!(cmd_check.status, CheckStatus::Pass);
    }

    // Check 3b: command parses — negative
    #[test]
    fn check_command_unbalanced_quotes_fail() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = make_minimal_config();
        let mut target = make_valid_target();
        target.modes.insert(
            "full".to_string(),
            ModeCommand {
                command: "echo 'hello".to_string(),
            },
        );
        config.targets.push(target);
        let checks = run_target_checks(&config, dir.path());
        let cmd_check = checks
            .iter()
            .find(|c| c.name.contains("command-parses"))
            .unwrap();
        assert_eq!(cmd_check.status, CheckStatus::Fail);
    }

    #[test]
    fn configured_structural_rules_warn_that_gate_does_not_enforce_them() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = make_minimal_config();
        let mut target = make_valid_target();
        target.structural = Some(StructuralConfig {
            source_glob: vec!["src/**/*.rs".to_string()],
            ..StructuralConfig::default()
        });
        config.targets.push(target);

        let checks = run_target_checks(&config, dir.path());
        let structural = checks
            .iter()
            .find(|check| check.name == "test.structural-unenforced")
            .expect("structural warning present");
        assert_eq!(structural.status, CheckStatus::Warn);
        assert!(structural.message.contains("not enforced"));
    }

    // Check 3c: report.path safe — positive
    #[test]
    fn check_report_path_safe_pass() {
        let check = check_path_safety("test", "target/report.xml");
        assert_eq!(check.status, CheckStatus::Pass);
    }

    // Check 3c: report.path safe — negative (absolute)
    #[test]
    fn check_report_path_absolute_fail() {
        let check = check_path_safety("test", "/tmp/report.xml");
        assert_eq!(check.status, CheckStatus::Fail);
        assert!(check.message.contains("absolute"));
    }

    // Check 3c: report.path safe — negative (parent traversal)
    #[test]
    fn check_report_path_traversal_fail() {
        let check = check_path_safety("test", "../etc/report.xml");
        assert_eq!(check.status, CheckStatus::Fail);
        assert!(check.message.contains("parent traversal"));
    }

    #[test]
    fn check_report_path_windows_forms_fail_on_every_host() {
        for path in [
            r"C:\reports\result.xml",
            r"C:reports\result.xml",
            r"\\server\share\result.xml",
            r"reports\..\result.xml",
            r"reports\result.xml",
        ] {
            let check = check_path_safety("test", path);
            assert_eq!(check.status, CheckStatus::Fail, "accepted {path}");
        }
    }

    // Check 3d: coverage.path safe — positive
    #[test]
    fn check_coverage_path_safe_pass() {
        let check = check_path_safety("test", "coverage/lcov.info");
        assert_eq!(check.status, CheckStatus::Pass);
    }

    // Check 3d: coverage.path safe — negative
    #[test]
    fn check_coverage_path_absolute_fail() {
        let check = check_path_safety("test", "/absolute/path.lcov");
        assert_eq!(check.status, CheckStatus::Fail);
    }

    // Check 3e: coverage.transform parses — positive
    #[test]
    fn check_transform_parses_pass() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = make_minimal_config();
        let mut target = make_valid_target();
        target.coverage = Some(CoverageConfig {
            format: CoverageFormat::Lcov,
            path: "coverage.lcov".to_string(),
            transform: Some("genhtml coverage.lcov".to_string()),
            rules: vec![],
            exceptions: vec![],
        });
        config.targets.push(target);
        let checks = run_target_checks(&config, dir.path());
        let tc = checks
            .iter()
            .find(|c| c.name.contains("coverage-transform-parses"))
            .unwrap();
        assert_eq!(tc.status, CheckStatus::Pass);
    }

    // Check 3e: coverage.transform parses — negative
    #[test]
    fn check_transform_unbalanced_fail() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = make_minimal_config();
        let mut target = make_valid_target();
        target.coverage = Some(CoverageConfig {
            format: CoverageFormat::Lcov,
            path: "coverage.lcov".to_string(),
            transform: Some("genhtml 'unclosed".to_string()),
            rules: vec![],
            exceptions: vec![],
        });
        config.targets.push(target);
        let checks = run_target_checks(&config, dir.path());
        let tc = checks
            .iter()
            .find(|c| c.name.contains("coverage-transform-parses"))
            .unwrap();
        assert_eq!(tc.status, CheckStatus::Fail);
    }

    // Check 3f: globs valid — positive
    #[test]
    fn check_glob_valid_pass() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = make_minimal_config();
        let mut target = make_valid_target();
        target.coverage = Some(CoverageConfig {
            format: CoverageFormat::Lcov,
            path: "cov.lcov".to_string(),
            transform: None,
            rules: vec![CoverageRule {
                scope: CoverageScope::PerFile,
                include: vec!["**/*.rs".to_string()],
                exclude: vec![],
                minimum: 85,
            }],
            exceptions: vec![],
        });
        config.targets.push(target);
        let checks = run_target_checks(&config, dir.path());
        let gc = checks
            .iter()
            .find(|c| c.name.contains("glob-valid"))
            .unwrap();
        assert_eq!(gc.status, CheckStatus::Pass);
    }

    // Check 3f: globs valid — negative
    #[test]
    fn check_glob_invalid_fail() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = make_minimal_config();
        let mut target = make_valid_target();
        target.coverage = Some(CoverageConfig {
            format: CoverageFormat::Lcov,
            path: "cov.lcov".to_string(),
            transform: None,
            rules: vec![CoverageRule {
                scope: CoverageScope::PerFile,
                include: vec!["[invalid".to_string()],
                exclude: vec![],
                minimum: 85,
            }],
            exceptions: vec![],
        });
        config.targets.push(target);
        let checks = run_target_checks(&config, dir.path());
        let gc = checks
            .iter()
            .find(|c| c.name.contains("glob-valid"))
            .unwrap();
        assert_eq!(gc.status, CheckStatus::Fail);
    }

    // Check 3g: exception file exists — positive
    #[test]
    fn check_exception_file_exists_pass() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("src.rs"), "").unwrap();
        let mut config = make_minimal_config();
        let mut target = make_valid_target();
        target.coverage = Some(CoverageConfig {
            format: CoverageFormat::Lcov,
            path: "cov.lcov".to_string(),
            transform: None,
            rules: vec![],
            exceptions: vec![CoverageException {
                file: "src.rs".to_string(),
                threshold: 50,
                reason: "test".to_string(),
                remove_when: "later".to_string(),
            }],
        });
        config.targets.push(target);
        let checks = run_target_checks(&config, dir.path());
        let ec = checks
            .iter()
            .find(|c| c.name.contains("exception-file-exists"))
            .unwrap();
        assert_eq!(ec.status, CheckStatus::Pass);
    }

    // Check 3g: exception file missing — warning
    #[test]
    fn check_exception_file_missing_warn() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = make_minimal_config();
        let mut target = make_valid_target();
        target.coverage = Some(CoverageConfig {
            format: CoverageFormat::Lcov,
            path: "cov.lcov".to_string(),
            transform: None,
            rules: vec![],
            exceptions: vec![CoverageException {
                file: "nonexistent.rs".to_string(),
                threshold: 50,
                reason: "test".to_string(),
                remove_when: "later".to_string(),
            }],
        });
        config.targets.push(target);
        let checks = run_target_checks(&config, dir.path());
        let ec = checks
            .iter()
            .find(|c| c.name.contains("exception-file-exists"))
            .unwrap();
        assert_eq!(ec.status, CheckStatus::Warn);
    }

    // Check 4: probe — custom runner passes with no probe
    #[test]
    fn check_probe_custom_pass() {
        let check = run_probe(&RunnerType::Custom, "test");
        assert_eq!(check.status, CheckStatus::Pass);
        assert!(check.message.contains("no probe"));
    }

    // Check 4: probe — non-existent runner warns
    #[test]
    fn check_probe_missing_runner_warn() {
        // Hermetic: probe a guaranteed-absent binary rather than assuming a
        // real runner (e.g. phpunit) is missing — it may be installed on CI
        // runners like ubuntu-latest, which would flip this test.
        let check = probe_command("codeflow-nonexistent-runner-xyzzy", &["--version"], "test");
        assert_eq!(check.status, CheckStatus::Warn);
        assert!(
            check.message.contains("could not spawn"),
            "{}",
            check.message
        );
    }

    // Exit code tests
    #[test]
    fn exit_code_all_pass() {
        let checks = vec![DoctorCheck {
            name: "test".to_string(),
            status: CheckStatus::Pass,
            message: "ok".to_string(),
        }];
        assert_eq!(exit_code(&checks), 0);
    }

    #[test]
    fn exit_code_with_warnings() {
        let checks = vec![DoctorCheck {
            name: "test".to_string(),
            status: CheckStatus::Warn,
            message: "warning".to_string(),
        }];
        assert_eq!(exit_code(&checks), 0);
    }

    #[test]
    fn exit_code_with_failures() {
        let checks = vec![DoctorCheck {
            name: "test".to_string(),
            status: CheckStatus::Fail,
            message: "fail".to_string(),
        }];
        assert_eq!(exit_code(&checks), 1);
    }

    // Format tests
    #[test]
    fn format_human_output() {
        let checks = vec![DoctorCheck {
            name: "config-exists".to_string(),
            status: CheckStatus::Pass,
            message: "ok".to_string(),
        }];
        let out = format_human(&checks);
        assert!(out.contains("[PASS]"));
        assert!(out.contains("config-exists"));
    }

    #[test]
    fn format_json_output() {
        let checks = vec![DoctorCheck {
            name: "test".to_string(),
            status: CheckStatus::Fail,
            message: "bad".to_string(),
        }];
        let out = format_json(&checks);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed[0]["status"], "fail");
    }
}
