//! Test command: run the codeflow test suite.
//!
//! Supports the new generic testing engine via `test-config.json` configuration,
//! with subcommands for report viewing, config mutation, exception management,
//! and artifact cleanup.

use std::path::Path;

use anyhow::Result;
use clap::{Args, Subcommand, ValueEnum};

use crate::helpers;
use codeflow_core::testing::config::{
    self as test_config, CoverageFormat, ReportFormat, RunnerType,
};

/// Test mode controlling which suites run.
#[derive(Debug, Clone, ValueEnum)]
pub enum TestMode {
    /// Quick sanity tests.
    Quick,
    /// Essential tests only (default).
    Essential,
    /// Full suite including integration and property tests.
    Full,
}

impl TestMode {
    fn as_str(&self) -> &str {
        match self {
            Self::Quick => "quick",
            Self::Essential => "essential",
            Self::Full => "full",
        }
    }
}

/// Arguments for the `test` subcommand.
#[derive(Debug, Clone, Args)]
pub struct TestArgs {
    /// Test mode: quick, essential, or full.
    #[arg(long, value_enum)]
    pub mode: Option<TestMode>,

    /// Output format.
    #[arg(long, value_enum, default_value = "human")]
    pub format: OutputFormat,

    /// Run only the named target(s), comma-separated.
    #[arg(long)]
    pub only: Option<String>,

    /// Skip the named target(s), comma-separated.
    #[arg(long)]
    pub skip: Option<String>,

    /// Stop on first target failure.
    #[arg(long, default_value_t = false)]
    pub fail_fast: bool,

    /// Subcommand (report, config, exceptions, clean).
    #[command(subcommand)]
    pub subcommand: Option<TestSubcommand>,
}

/// Output format for CLI commands.
#[derive(Debug, Clone, Default, ValueEnum)]
pub enum OutputFormat {
    /// Human-readable output.
    #[default]
    Human,
    /// Stable JSON output.
    Json,
}

/// Test subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum TestSubcommand {
    /// Test report operations.
    Report {
        #[command(subcommand)]
        command: ReportCommand,
    },
    /// Test configuration operations.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Coverage exception management.
    Exceptions {
        #[command(subcommand)]
        command: ExceptionsCommand,
    },
    /// Clean test artifacts.
    Clean {
        /// Remove test artifacts (default scope).
        #[arg(long, default_value_t = true)]
        artifacts: bool,
        /// Skip confirmation prompt.
        #[arg(long, default_value_t = false)]
        yes: bool,
    },
    /// Interactive setup wizard for test configuration.
    Setup {
        /// Non-interactive auto-detection mode.
        #[arg(long, default_value_t = false)]
        auto: bool,
        /// Apply a named template.
        #[arg(long)]
        template: Option<String>,
        /// List available templates.
        #[arg(long, default_value_t = false)]
        list: bool,
        /// Add a target to existing config (interactive).
        #[arg(long, default_value_t = false)]
        add_target: bool,
        /// Force overwrite existing config (with --template).
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    /// Validate test configuration.
    Doctor {
        /// Output format.
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
}

/// Report subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum ReportCommand {
    /// Show the last test run report.
    Show {
        /// Specific run ID to show.
        #[arg(long)]
        run_id: Option<String>,
        /// Output format.
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
    /// Convert between report formats.
    Convert {
        /// Source format.
        #[arg(long)]
        from: String,
        /// Target format.
        #[arg(long)]
        to: String,
        /// Input file path.
        #[arg(long)]
        input: String,
        /// Output file path.
        #[arg(long)]
        output: String,
    },
    /// Compare two test runs.
    Diff {
        /// First run ID.
        #[arg(long)]
        from: String,
        /// Second run ID.
        #[arg(long)]
        to: String,
    },
}

/// Config subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum ConfigCommand {
    /// Show current test configuration.
    Show,
    /// Add a new test target.
    AddTarget {
        /// Target name.
        #[arg(long)]
        name: String,
        /// Runner type.
        #[arg(long)]
        runner: String,
        /// Working directory.
        #[arg(long)]
        cwd: Option<String>,
        /// Mode command pairs (e.g., full="make test").
        #[arg(long = "mode", value_name = "MODE=CMD")]
        modes: Vec<String>,
    },
    /// Remove a test target.
    RemoveTarget {
        /// Target name.
        #[arg(long)]
        name: String,
    },
    /// Enable a test target.
    Enable {
        /// Target name.
        #[arg(long)]
        target: String,
    },
    /// Disable a test target.
    Disable {
        /// Target name.
        #[arg(long)]
        target: String,
    },
    /// Set a mode command for a target.
    SetCommand {
        /// Target name.
        #[arg(long)]
        target: String,
        /// Mode name.
        #[arg(long)]
        mode: String,
        /// Shell command.
        #[arg(long)]
        command: String,
    },
    /// Set a coverage threshold for a target.
    SetThreshold {
        /// Target name.
        #[arg(long)]
        target: String,
        /// Coverage scope.
        #[arg(long)]
        scope: String,
        /// Minimum threshold (0-100).
        #[arg(long)]
        minimum: u32,
        /// Rule index to modify (if multiple rules exist).
        #[arg(long)]
        rule_index: Option<usize>,
    },
    /// Set report configuration for a target.
    SetReport {
        /// Target name.
        #[arg(long)]
        target: String,
        /// Report format (junit or ctrf).
        #[arg(long)]
        format: String,
        /// Path to report file.
        #[arg(long)]
        path: String,
        /// Derive from format (e.g., junit).
        #[arg(long)]
        derive_from: Option<String>,
    },
    /// Set coverage configuration for a target.
    SetCoverage {
        /// Target name.
        #[arg(long)]
        target: String,
        /// Coverage format.
        #[arg(long)]
        format: String,
        /// Path to coverage artifact.
        #[arg(long)]
        path: String,
    },
}

/// Exceptions subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum ExceptionsCommand {
    /// Add a coverage exception.
    Add {
        /// Target name.
        #[arg(long)]
        target: String,
        /// File path.
        #[arg(long)]
        file: String,
        /// Lowered threshold.
        #[arg(long)]
        threshold: u32,
        /// Justification (required).
        #[arg(long)]
        reason: String,
        /// Removal condition (required).
        #[arg(long)]
        remove_when: String,
    },
    /// Remove a coverage exception.
    Remove {
        /// Target name.
        #[arg(long)]
        target: String,
        /// File path.
        #[arg(long)]
        file: String,
    },
    /// List coverage exceptions.
    List {
        /// Filter by target name.
        #[arg(long)]
        target: Option<String>,
    },
}

pub fn run(args: Option<TestArgs>) -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    let args = args.unwrap_or(TestArgs {
        mode: None,
        format: OutputFormat::Human,
        only: None,
        skip: None,
        fail_fast: false,
        subcommand: None,
    });
    run_with_dir(&project_dir, &args)
}

fn run_with_dir(project_dir: &Path, args: &TestArgs) -> Result<()> {
    // Route subcommands
    if let Some(ref sub) = args.subcommand {
        return match sub {
            TestSubcommand::Report { command } => run_report_subcommand(project_dir, command),
            TestSubcommand::Config { command } => run_config_subcommand(project_dir, command),
            TestSubcommand::Exceptions { command } => {
                run_exceptions_subcommand(project_dir, command)
            }
            TestSubcommand::Clean { yes, .. } => run_clean(project_dir, *yes),
            TestSubcommand::Setup {
                auto,
                template,
                list,
                add_target,
                force,
            } => run_setup(
                project_dir,
                *auto,
                template.as_deref(),
                *list,
                *add_target,
                *force,
            ),
            TestSubcommand::Doctor { format } => run_doctor(project_dir, format),
        };
    }

    // Load config from canonical location
    let config_path = project_dir
        .join(".codeflow")
        .join("config")
        .join("testing")
        .join("test-config.json");

    let config = test_config::load_test_config(&config_path).map_err(|e| anyhow::anyhow!("{e}"))?;
    run_new_engine(project_dir, args, &config)
}

/// Run tests using the new generic engine, then parse reports, evaluate
/// coverage thresholds, render PR body, and write test report.
fn run_new_engine(
    project_dir: &Path,
    args: &TestArgs,
    config: &test_config::TestConfig,
) -> Result<()> {
    use codeflow_core::testing::runner;
    use codeflow_core::testing::validation;

    let mode = args.mode.as_ref().map_or("essential", TestMode::as_str);

    // Check for fresh-project path
    let active_targets: Vec<_> = config.targets.iter().filter(|t| t.enabled).collect();
    if active_targets.is_empty() {
        println!("No test targets configured. Run `codeflow test setup` to add targets.");
        return Ok(());
    }

    let only: Vec<String> = args
        .only
        .as_ref()
        .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
        .unwrap_or_default();
    let skip: Vec<String> = args
        .skip
        .as_ref()
        .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
        .unwrap_or_default();

    for target in &config.targets {
        if target.enabled && !target.modes.contains_key(mode) {
            eprintln!(
                "skipping target {}: mode {mode} not configured",
                target.name
            );
        }
    }

    // Detect changed files relative to the configured base ref.
    // Base ref resolution (first non-empty wins):
    //   1. CODEFLOW_COVERAGE_BASE_REF env var (operator override)
    //   2. `git merge-base HEAD origin/main` (branch-wide diff from default-branch fork point)
    //   3. Literal "HEAD~1" (last-commit fallback when no origin/main or git unavailable)
    // On any git error during the `diff --name-only` call itself,
    // `detect_changed_files` returns an empty vec — Section 3 (Modified File
    // Coverage) then renders as "no modified files detected", which is the
    // correct degraded behaviour for fresh repos or detached HEAD.
    let base_ref = std::env::var("CODEFLOW_COVERAGE_BASE_REF")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| validation::default_base_ref(project_dir));
    let changed_files = validation::detect_changed_files(project_dir, &base_ref);

    // Step 1: Run all targets
    let results = runner::run_all_targets(
        &config.targets,
        mode,
        project_dir,
        config.execution.parallel,
        args.fail_fast,
        &only,
        &skip,
    );

    // Collect successful run results paired with their target config
    let mut any_test_failure = false;
    let mut successful_runs: Vec<(runner::TargetRunResult, &test_config::TargetConfig)> =
        Vec::new();

    for result in &results {
        match result {
            Ok(r) => {
                if r.exit_code != 0 {
                    any_test_failure = true;
                    eprintln!(
                        "FAIL: target {} exited with code {}",
                        r.target_name, r.exit_code
                    );
                } else {
                    eprintln!(
                        "PASS: target {} ({:.1}s)",
                        r.target_name,
                        r.duration_ms as f64 / 1000.0
                    );
                }
                // Find matching target config
                if let Some(tc) = config.targets.iter().find(|t| t.name == r.target_name) {
                    successful_runs.push((r.clone(), tc));
                }
            }
            Err(e) => {
                any_test_failure = true;
                eprintln!("ERROR: {e}");
            }
        }
    }

    // Steps 2-3: Parse reports + coverage, evaluate thresholds.
    // `parse_target_coverage` returns an empty vec when the coverage artifact
    // is missing (e.g., `essential` mode, fresh setup, coverage disabled),
    // producing graceful degradation in the rendered PR body ("no coverage
    // data collected") rather than a hard error.
    let mut post_data: Vec<validation::TargetPostData> = Vec::new();
    let mut any_threshold_failure = false;

    for (run_result, target_config) in &successful_runs {
        // Parse test report (JUnit -> CTRF or native CTRF)
        let report = validation::parse_target_report(target_config, run_result)
            .map_err(|e| anyhow::anyhow!("report parse error for {}: {e}", target_config.name))?;

        // Parse coverage
        let file_coverages = validation::parse_target_coverage(target_config, run_result)
            .map_err(|e| anyhow::anyhow!("coverage parse error for {}: {e}", target_config.name))?;

        // Evaluate thresholds — `changed_files` feeds `ChangedFiles`-scoped
        // rules and Section 3 of the rendered PR body.
        let threshold_results =
            validation::evaluate_target_thresholds(target_config, &file_coverages, &changed_files);

        if threshold_results
            .iter()
            .any(|r| !r.pass && !r.exception_applied)
        {
            any_threshold_failure = true;
        }

        post_data.push(validation::TargetPostData {
            name: target_config.name.clone(),
            mode: mode.to_string(),
            run_result: run_result.clone(),
            report,
            file_coverages,
            threshold_results,
            target_config: (*target_config).clone(),
        });
    }

    // Step 4: Render PR body (Section 3 picks up per-file rows for changed files)
    let pr_body = validation::render_full_pr_body(&post_data, &changed_files, 1);
    println!("{pr_body}");

    // Step 5: Write test report
    let run_id = format!(
        "run-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
    if let Err(e) = validation::write_test_report(project_dir, &run_id, &post_data) {
        eprintln!("warning: failed to write test report: {e}");
    }

    // Step 6: Emit ledger event (includes per-target coverage audit fields)
    validation::emit_ledger_event(project_dir, &run_id, &post_data, &changed_files);

    // Step 7: Exit code — fail if tests or thresholds failed
    if any_test_failure {
        anyhow::bail!("one or more test targets failed");
    }
    if any_threshold_failure {
        let failing: Vec<String> = post_data
            .iter()
            .flat_map(|t| {
                t.threshold_results.iter().filter(|r| !r.pass).map(|r| {
                    format!(
                        "  {} ({:.0}% < {}%)",
                        r.file, r.coverage_percent, r.threshold
                    )
                })
            })
            .collect();
        eprintln!("FAIL: coverage thresholds not met:");
        for f in &failing {
            eprintln!("{f}");
        }
        anyhow::bail!("coverage threshold violations");
    }

    Ok(())
}

/// Report subcommand dispatch.
fn run_report_subcommand(project_dir: &Path, command: &ReportCommand) -> Result<()> {
    match command {
        ReportCommand::Show { run_id, format } => {
            run_report_show(project_dir, run_id.as_deref(), format)
        }
        ReportCommand::Convert {
            from,
            to,
            input,
            output,
        } => run_report_convert(from, to, input, output),
        ReportCommand::Diff { from, to } => run_report_diff(project_dir, from, to),
    }
}

/// Diff two test runs by run-id, showing regressions.
fn run_report_diff(project_dir: &Path, from_id: &str, to_id: &str) -> Result<()> {
    use codeflow_core::testing::report::diff;

    let ledger_dir = project_dir.join(".state").join("ledger");

    let from_event = diff::read_run_from_ledger(&ledger_dir, from_id)
        .map_err(|_| anyhow::anyhow!("run {from_id} not found in ledger"))?;
    let to_event = diff::read_run_from_ledger(&ledger_dir, to_id)
        .map_err(|_| anyhow::anyhow!("run {to_id} not found in ledger"))?;

    // Extract reports from events if available, otherwise build minimal reports from summaries
    let from_report = extract_report_from_event(&from_event, from_id);
    let to_report = extract_report_from_event(&to_event, to_id);

    let result = diff::diff_reports(&from_report, &to_report, from_id, to_id);
    let output = diff::format_diff_human(&result);
    print!("{output}");

    if result.total_regressions > 0 {
        std::process::exit(1);
    }
    Ok(())
}

/// Extract a minimal `CanonicalTestReport` from a ledger event JSON value.
fn extract_report_from_event(
    event: &serde_json::Value,
    run_id: &str,
) -> codeflow_core::testing::report::CanonicalTestReport {
    use codeflow_core::testing::report::{CanonicalTestReport, CtrfStatus, CtrfTest};

    // Build a minimal report from the summary fields
    let total_passed = event
        .get("total_passed")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let total_failed = event
        .get("total_failed")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);

    let mut tests = Vec::new();
    // We don't have individual test names from the summary event,
    // so the diff will be limited to aggregate counts
    for i in 0..total_passed {
        tests.push(CtrfTest {
            name: format!("{run_id}::passed_{i}"),
            status: CtrfStatus::Passed,
            duration: 0.0,
            suite: None,
            message: None,
            trace: None,
            tags: vec![],
            flaky: false,
        });
    }
    for i in 0..total_failed {
        tests.push(CtrfTest {
            name: format!("{run_id}::failed_{i}"),
            status: CtrfStatus::Failed,
            duration: 0.0,
            suite: None,
            message: None,
            trace: None,
            tags: vec![],
            flaky: false,
        });
    }

    CanonicalTestReport::new("ledger", tests)
}

/// Config subcommand dispatch.
fn run_config_subcommand(project_dir: &Path, command: &ConfigCommand) -> Result<()> {
    let config_path = project_dir
        .join(".codeflow")
        .join("config")
        .join("testing")
        .join("test-config.json");

    match command {
        ConfigCommand::Show => {
            let config =
                test_config::load_test_config(&config_path).map_err(|e| anyhow::anyhow!("{e}"))?;
            let json = serde_json::to_string_pretty(&config)?;
            println!("{json}");
            Ok(())
        }
        ConfigCommand::AddTarget {
            name,
            runner,
            cwd,
            modes,
        } => {
            let mut config = load_or_create_config(&config_path)?;

            if config.targets.iter().any(|t| t.name == *name) {
                anyhow::bail!("target '{name}' already exists");
            }

            let runner_type: RunnerType = serde_json::from_str(&format!("\"{runner}\""))
                .map_err(|_| anyhow::anyhow!("unknown runner: {runner}"))?;

            let mut mode_map = std::collections::BTreeMap::new();
            for mode_str in modes {
                if let Some((k, v)) = mode_str.split_once('=') {
                    mode_map.insert(
                        k.to_string(),
                        test_config::ModeCommand {
                            command: v.to_string(),
                        },
                    );
                }
            }

            config.targets.push(test_config::TargetConfig {
                name: name.clone(),
                enabled: true,
                cwd: cwd.clone(),
                env: std::collections::BTreeMap::new(),
                runner: runner_type,
                modes: mode_map,
                report: None,
                coverage: None,
            });

            test_config::write_test_config(&config_path, &config)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("added target '{name}'");
            Ok(())
        }
        ConfigCommand::RemoveTarget { name } => {
            let mut config = load_or_create_config(&config_path)?;
            let before = config.targets.len();
            config.targets.retain(|t| t.name != *name);
            if config.targets.len() == before {
                anyhow::bail!("target '{name}' not found");
            }
            test_config::write_test_config(&config_path, &config)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("removed target '{name}'");
            Ok(())
        }
        ConfigCommand::Enable { target } => toggle_target(&config_path, target, true),
        ConfigCommand::Disable { target } => toggle_target(&config_path, target, false),
        ConfigCommand::SetCommand {
            target,
            mode,
            command,
        } => {
            codeflow_core::testing::runner::check_balanced_quotes(command)
                .map_err(|e| anyhow::anyhow!("{e}"))?;

            let valid_modes = ["quick", "essential", "full"];
            if !valid_modes.contains(&mode.as_str()) {
                anyhow::bail!(
                    "unknown mode '{mode}'; valid modes: {}",
                    valid_modes.join(", ")
                );
            }

            let mut config = load_or_create_config(&config_path)?;
            let t = find_target_mut(&mut config, target)?;
            t.modes.insert(
                mode.clone(),
                test_config::ModeCommand {
                    command: command.clone(),
                },
            );
            test_config::write_test_config(&config_path, &config)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("set {target}.modes.{mode}");
            Ok(())
        }
        ConfigCommand::SetThreshold {
            target,
            scope,
            minimum,
            rule_index,
        } => {
            let scope_type: test_config::CoverageScope =
                serde_json::from_str(&format!("\"{scope}\""))
                    .map_err(|_| anyhow::anyhow!("unknown scope: {scope}"))?;

            let mut config = load_or_create_config(&config_path)?;
            let t = find_target_mut(&mut config, target)?;

            let cov = t
                .coverage
                .get_or_insert_with(|| test_config::CoverageConfig {
                    format: CoverageFormat::Lcov,
                    path: String::new(),
                    transform: None,
                    rules: Vec::new(),
                    exceptions: Vec::new(),
                });

            if let Some(idx) = rule_index {
                if let Some(rule) = cov.rules.get_mut(*idx) {
                    rule.minimum = *minimum;
                } else {
                    anyhow::bail!("rule index {idx} out of range");
                }
            } else {
                // Find existing rule with matching scope or add new
                let found = cov.rules.iter_mut().find(|r| r.scope == scope_type);
                if let Some(rule) = found {
                    rule.minimum = *minimum;
                } else {
                    eprintln!("warning: no existing rule with scope '{scope}'; adding new rule");
                    cov.rules.push(test_config::CoverageRule {
                        scope: scope_type,
                        include: Vec::new(),
                        exclude: Vec::new(),
                        minimum: *minimum,
                    });
                }
            }

            test_config::write_test_config(&config_path, &config)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("set threshold for {target}");
            Ok(())
        }
        ConfigCommand::SetReport {
            target,
            format,
            path,
            derive_from,
        } => {
            let fmt: ReportFormat = serde_json::from_str(&format!("\"{format}\""))
                .map_err(|_| anyhow::anyhow!("unknown report format: {format}"))?;

            let mut config = load_or_create_config(&config_path)?;
            let t = find_target_mut(&mut config, target)?;
            t.report = Some(test_config::ReportConfig {
                format: fmt,
                path: path.clone(),
                derive_from: derive_from.clone(),
            });
            test_config::write_test_config(&config_path, &config)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("set report for {target}");
            Ok(())
        }
        ConfigCommand::SetCoverage {
            target,
            format,
            path,
        } => {
            let fmt: CoverageFormat = serde_json::from_str(&format!("\"{format}\""))
                .map_err(|_| anyhow::anyhow!("unknown coverage format: {format}"))?;

            let mut config = load_or_create_config(&config_path)?;
            let t = find_target_mut(&mut config, target)?;

            let cov = t
                .coverage
                .get_or_insert_with(|| test_config::CoverageConfig {
                    format: fmt.clone(),
                    path: String::new(),
                    transform: None,
                    rules: Vec::new(),
                    exceptions: Vec::new(),
                });
            cov.format = fmt;
            path.clone_into(&mut cov.path);

            test_config::write_test_config(&config_path, &config)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("set coverage for {target}");
            Ok(())
        }
    }
}

/// Exceptions subcommand dispatch.
fn run_exceptions_subcommand(project_dir: &Path, command: &ExceptionsCommand) -> Result<()> {
    let config_path = project_dir
        .join(".codeflow")
        .join("config")
        .join("testing")
        .join("test-config.json");

    match command {
        ExceptionsCommand::Add {
            target,
            file,
            threshold,
            reason,
            remove_when,
        } => {
            let mut config = load_or_create_config(&config_path)?;
            let t = find_target_mut(&mut config, target)?;

            let cov = t
                .coverage
                .get_or_insert_with(|| test_config::CoverageConfig {
                    format: CoverageFormat::Lcov,
                    path: String::new(),
                    transform: None,
                    rules: Vec::new(),
                    exceptions: Vec::new(),
                });

            cov.exceptions.push(test_config::CoverageException {
                file: file.clone(),
                threshold: *threshold,
                reason: reason.clone(),
                remove_when: remove_when.clone(),
            });

            test_config::write_test_config(&config_path, &config)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("added exception for {file} in target {target}");
            Ok(())
        }
        ExceptionsCommand::Remove { target, file } => {
            let mut config = load_or_create_config(&config_path)?;
            let t = find_target_mut(&mut config, target)?;

            if let Some(ref mut cov) = t.coverage {
                let before = cov.exceptions.len();
                cov.exceptions.retain(|e| e.file != *file);
                if cov.exceptions.len() == before {
                    anyhow::bail!("exception for '{file}' not found in target '{target}'");
                }
            } else {
                anyhow::bail!("target '{target}' has no coverage configuration");
            }

            test_config::write_test_config(&config_path, &config)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("removed exception for {file} from target {target}");
            Ok(())
        }
        ExceptionsCommand::List { target } => {
            let config =
                test_config::load_test_config(&config_path).map_err(|e| anyhow::anyhow!("{e}"))?;

            let header = format!(
                "{:<20} {:<40} {:<10} {}",
                "Target", "File", "Threshold", "Reason"
            );
            println!("{header}");
            println!("{}", "-".repeat(90));

            for t in &config.targets {
                if let Some(filter) = target {
                    if t.name != *filter {
                        continue;
                    }
                }
                if let Some(ref cov) = t.coverage {
                    for exc in &cov.exceptions {
                        println!(
                            "{:<20} {:<40} {:<10} {}",
                            t.name, exc.file, exc.threshold, exc.reason
                        );
                    }
                }
            }
            Ok(())
        }
    }
}

/// Clean test artifacts.
fn run_clean(project_dir: &Path, yes: bool) -> Result<()> {
    let test_reports = project_dir.join(".state").join("test-reports");
    let coverage = project_dir.join(".state").join("coverage");

    if !yes {
        eprintln!("This will remove:");
        if test_reports.exists() {
            eprintln!("  {}", test_reports.display());
        }
        if coverage.exists() {
            eprintln!("  {}", coverage.display());
        }
        eprintln!("Use --yes to confirm.");
        return Ok(());
    }

    if test_reports.exists() {
        std::fs::remove_dir_all(&test_reports)?;
        println!("removed {}", test_reports.display());
    }
    if coverage.exists() {
        std::fs::remove_dir_all(&coverage)?;
        println!("removed {}", coverage.display());
    }

    Ok(())
}

// ── Setup & Doctor ────────────────────────────────────────────────────────

#[allow(clippy::fn_params_excessive_bools)]
fn run_setup(
    project_dir: &Path,
    auto: bool,
    template: Option<&str>,
    list: bool,
    add_target: bool,
    force: bool,
) -> Result<()> {
    use codeflow_core::testing::setup;
    use codeflow_core::testing::setup::prompt::TerminalPromptProvider;

    if list {
        return setup::list_templates(project_dir).map_err(|e| anyhow::anyhow!("{e}"));
    }

    if let Some(name) = template {
        return match setup::run_template(project_dir, name, force) {
            Ok(_) => Ok(()),
            Err(setup::SetupError::ConfigExists(p)) => {
                eprintln!(
                    "Config already exists at {}. Use --force to overwrite.",
                    p.display()
                );
                std::process::exit(2);
            }
            Err(setup::SetupError::TemplateNotFound(_)) => {
                eprintln!(
                    "Template not found. Run `codeflow test setup --list` to see available templates."
                );
                std::process::exit(1);
            }
            Err(e) => Err(anyhow::anyhow!("{e}")),
        };
    }

    if auto {
        return setup::run_auto(project_dir)
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("{e}"));
    }

    if add_target {
        let prompts = TerminalPromptProvider;
        return setup::run_add_target(project_dir, &prompts)
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("{e}"));
    }

    // Default: interactive wizard
    let prompts = TerminalPromptProvider;
    setup::run_interactive(project_dir, &prompts)
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("{e}"))
}

#[allow(clippy::unnecessary_wraps)]
fn run_doctor(project_dir: &Path, format: &OutputFormat) -> Result<()> {
    use codeflow_core::testing::doctor;

    let checks = doctor::run_all_checks(project_dir);
    let code = doctor::exit_code(&checks);

    match format {
        OutputFormat::Human => print!("{}", doctor::format_human(&checks)),
        OutputFormat::Json => println!("{}", doctor::format_json(&checks)),
    }

    if code != 0 {
        std::process::exit(code);
    }

    Ok(())
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn load_or_create_config(path: &Path) -> Result<test_config::TestConfig> {
    if path.exists() {
        test_config::load_test_config(path).map_err(|e| anyhow::anyhow!("{e}"))
    } else {
        Ok(test_config::TestConfig {
            description: None,
            schema_ref: Some(".codeflow/schemas/test-config.schema.json".to_string()),
            schema_version: "1.0".to_string(),
            execution: test_config::ExecutionConfig::default(),
            defaults: test_config::DefaultsConfig::default(),
            targets: Vec::new(),
        })
    }
}

fn find_target_mut<'a>(
    config: &'a mut test_config::TestConfig,
    name: &str,
) -> Result<&'a mut test_config::TargetConfig> {
    config
        .targets
        .iter_mut()
        .find(|t| t.name == name)
        .ok_or_else(|| anyhow::anyhow!("target '{name}' not found"))
}

fn toggle_target(config_path: &Path, target: &str, enabled: bool) -> Result<()> {
    let mut config = load_or_create_config(config_path)?;
    let t = find_target_mut(&mut config, target)?;
    t.enabled = enabled;
    test_config::write_test_config(config_path, &config).map_err(|e| anyhow::anyhow!("{e}"))?;
    let state = if enabled { "enabled" } else { "disabled" };
    println!("{state} target '{target}'");
    Ok(())
}

/// Print a saved test report.
///
/// When `run_id` is `Some`, the matching `{run_id}.json` is loaded; otherwise
/// the most recently modified `.json` report is used. The `format` argument
/// controls presentation: `Json` emits the raw report as indented JSON,
/// `Human` emits a readable per-target table.
fn run_report_show(project_dir: &Path, run_id: Option<&str>, format: &OutputFormat) -> Result<()> {
    let report_dir = project_dir.join(".state").join("test-reports");
    if !report_dir.exists() {
        anyhow::bail!("no test reports found at {}", report_dir.display());
    }

    let report_path = if let Some(id) = run_id {
        let path = report_dir.join(format!("{id}.json"));
        if !path.exists() {
            anyhow::bail!(
                "no test report found for run-id '{id}' at {}",
                path.display()
            );
        }
        path
    } else {
        let mut entries: Vec<_> = std::fs::read_dir(&report_dir)?
            .filter_map(std::result::Result::ok)
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
            .collect();
        entries
            .sort_by_key(|e| std::cmp::Reverse(e.metadata().ok().and_then(|m| m.modified().ok())));
        entries
            .first()
            .ok_or_else(|| anyhow::anyhow!("no test report files found"))?
            .path()
    };

    let content = std::fs::read_to_string(&report_path)?;
    let parsed: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| anyhow::anyhow!("failed to parse report at {}: {e}", report_path.display()))?;

    match format {
        OutputFormat::Json => {
            let pretty = serde_json::to_string_pretty(&parsed)?;
            println!("{pretty}");
        }
        OutputFormat::Human => {
            render_report_human(&parsed);
        }
    }

    Ok(())
}

/// Render a test report summary as a human-readable table.
fn render_report_human(report: &serde_json::Value) {
    let run_id = report
        .get("run_id")
        .and_then(|v| v.as_str())
        .unwrap_or("<unknown>");
    let timestamp = report
        .get("timestamp")
        .and_then(|v| v.as_str())
        .unwrap_or("<unknown>");

    println!("Test report");
    println!("  run_id:    {run_id}");
    println!("  timestamp: {timestamp}");
    println!();

    let targets = report
        .get("targets")
        .and_then(|v| v.as_array())
        .map_or(&[][..], Vec::as_slice);

    if targets.is_empty() {
        println!("  (no target data)");
        return;
    }

    println!(
        "{:<20} {:>8} {:>8} {:>10} {:>10} {:>10}",
        "Target", "Exit", "Duration", "Passed", "Failed", "Coverage"
    );
    println!("{}", "-".repeat(70));

    for t in targets {
        let name = t.get("name").and_then(|v| v.as_str()).unwrap_or("<?>");
        let exit = t
            .get("exit_code")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(-1);
        let duration_ms = t
            .get("duration_ms")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let passed = t
            .get("summary")
            .and_then(|s| s.get("passed"))
            .and_then(serde_json::Value::as_u64)
            .map_or_else(|| "--".to_string(), |n| n.to_string());
        let failed = t
            .get("summary")
            .and_then(|s| s.get("failed"))
            .and_then(serde_json::Value::as_u64)
            .map_or_else(|| "--".to_string(), |n| n.to_string());
        let coverage = t
            .get("coverage_percent")
            .and_then(serde_json::Value::as_f64)
            .map_or_else(|| "N/A".to_string(), |p| format!("{p:.1}%"));

        println!(
            "{name:<20} {exit:>8} {:>7.1}s {passed:>10} {failed:>10} {coverage:>10}",
            duration_ms as f64 / 1000.0
        );
    }
}

/// Convert between report formats.
fn run_report_convert(from: &str, to: &str, input: &str, output: &str) -> Result<()> {
    use codeflow_core::testing::report::{CanonicalTestReport, junit};

    let input_path = std::path::Path::new(input);
    let output_path = std::path::Path::new(output);

    match (from, to) {
        ("junit", "ctrf") => {
            let junit_report =
                junit::parse_junit(input_path).map_err(|e| anyhow::anyhow!("{e}"))?;
            let ctrf: CanonicalTestReport = junit_report.into();
            let json = serde_json::to_string_pretty(&ctrf)?;
            std::fs::write(output_path, format!("{json}\n"))?;
            println!("converted {input} (JUnit) -> {output} (CTRF)");
            Ok(())
        }
        _ => anyhow::bail!("unsupported conversion: {from} -> {to}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_config_fails() {
        let dir = tempfile::tempdir().unwrap();
        let args = TestArgs {
            mode: None,
            format: OutputFormat::Human,
            only: None,
            skip: None,
            fail_fast: false,
            subcommand: None,
        };
        let result = run_with_dir(dir.path(), &args);
        assert!(result.is_err(), "missing config should fail");
    }

    #[test]
    fn test_fresh_project_no_targets() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow").join("config").join("testing");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("test-config.json"),
            r#"{"schema_version": "1.0", "targets": []}"#,
        )
        .unwrap();

        let args = TestArgs {
            mode: None,
            format: OutputFormat::Human,
            only: None,
            skip: None,
            fail_fast: false,
            subcommand: None,
        };
        let result = run_with_dir(dir.path(), &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_new_engine_with_passing_target() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow").join("config").join("testing");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("test-config.json"),
            r#"{
                "schema_version": "1.0",
                "targets": [{
                    "name": "echo-test",
                    "runner": "custom",
                    "modes": {"essential": {"command": "echo pass"}}
                }]
            }"#,
        )
        .unwrap();

        let args = TestArgs {
            mode: Some(TestMode::Essential),
            format: OutputFormat::Human,
            only: None,
            skip: None,
            fail_fast: false,
            subcommand: None,
        };
        let result = run_with_dir(dir.path(), &args);
        assert!(result.is_ok(), "echo-test should pass: {result:?}");
    }

    #[test]
    fn test_clean_without_yes_does_not_delete() {
        let dir = tempfile::tempdir().unwrap();
        let reports = dir.path().join(".state").join("test-reports");
        std::fs::create_dir_all(&reports).unwrap();
        std::fs::write(reports.join("test.json"), "{}").unwrap();

        let result = run_clean(dir.path(), false);
        assert!(result.is_ok());
        assert!(reports.exists(), "should not delete without --yes");
    }

    #[test]
    fn test_clean_with_yes_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let reports = dir.path().join(".state").join("test-reports");
        let coverage = dir.path().join(".state").join("coverage");
        std::fs::create_dir_all(&reports).unwrap();
        std::fs::create_dir_all(&coverage).unwrap();

        let result = run_clean(dir.path(), true);
        assert!(result.is_ok());
        assert!(!reports.exists(), "should delete test-reports");
        assert!(!coverage.exists(), "should delete coverage");
    }

    #[test]
    fn test_test_mode_as_str() {
        assert_eq!(TestMode::Quick.as_str(), "quick");
        assert_eq!(TestMode::Essential.as_str(), "essential");
        assert_eq!(TestMode::Full.as_str(), "full");
    }

    #[test]
    fn test_config_add_and_show_target() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow").join("config").join("testing");
        std::fs::create_dir_all(&config_dir).unwrap();

        let cmd = ConfigCommand::AddTarget {
            name: "rust".to_string(),
            runner: "cargo".to_string(),
            cwd: Some("codeflow-cli".to_string()),
            modes: vec!["full=cargo nextest run".to_string()],
        };
        let result = run_config_subcommand(dir.path(), &cmd);
        assert!(result.is_ok(), "add-target should succeed: {result:?}");

        // Verify it was written
        let config_path = config_dir.join("test-config.json");
        let config = test_config::load_test_config(&config_path).unwrap();
        assert_eq!(config.targets.len(), 1);
        assert_eq!(config.targets[0].name, "rust");
    }

    #[test]
    fn test_config_add_duplicate_target_fails() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow").join("config").join("testing");
        std::fs::create_dir_all(&config_dir).unwrap();

        let cmd = ConfigCommand::AddTarget {
            name: "t".to_string(),
            runner: "custom".to_string(),
            cwd: None,
            modes: vec![],
        };
        run_config_subcommand(dir.path(), &cmd).unwrap();
        let result = run_config_subcommand(dir.path(), &cmd);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[test]
    fn test_config_remove_target() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow").join("config").join("testing");
        std::fs::create_dir_all(&config_dir).unwrap();

        // Add then remove
        let add = ConfigCommand::AddTarget {
            name: "t".to_string(),
            runner: "custom".to_string(),
            cwd: None,
            modes: vec![],
        };
        run_config_subcommand(dir.path(), &add).unwrap();

        let remove = ConfigCommand::RemoveTarget {
            name: "t".to_string(),
        };
        let result = run_config_subcommand(dir.path(), &remove);
        assert!(result.is_ok());

        let config_path = config_dir.join("test-config.json");
        let config = test_config::load_test_config(&config_path).unwrap();
        assert!(config.targets.is_empty());
    }

    #[test]
    fn test_exceptions_add_and_list() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow").join("config").join("testing");
        std::fs::create_dir_all(&config_dir).unwrap();

        // Create a target first
        let add_target = ConfigCommand::AddTarget {
            name: "rust".to_string(),
            runner: "cargo".to_string(),
            cwd: None,
            modes: vec![],
        };
        run_config_subcommand(dir.path(), &add_target).unwrap();

        // Add exception
        let add_exc = ExceptionsCommand::Add {
            target: "rust".to_string(),
            file: "worker.rs".to_string(),
            threshold: 79,
            reason: "process spawning".to_string(),
            remove_when: "mock harness".to_string(),
        };
        let result = run_exceptions_subcommand(dir.path(), &add_exc);
        assert!(result.is_ok());

        // Verify in config
        let config_path = config_dir.join("test-config.json");
        let config = test_config::load_test_config(&config_path).unwrap();
        let cov = config.targets[0].coverage.as_ref().unwrap();
        assert_eq!(cov.exceptions.len(), 1);
        assert_eq!(cov.exceptions[0].file, "worker.rs");
    }

    /// Helper: create a temp dir with a config containing one target named "rust".
    fn setup_config_with_target(dir: &std::path::Path) -> std::path::PathBuf {
        let config_dir = dir.join(".codeflow").join("config").join("testing");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("test-config.json"),
            r#"{
                "schema_version": "1.0",
                "targets": [{
                    "name": "rust",
                    "runner": "cargo",
                    "modes": {"full": {"command": "cargo test"}},
                    "coverage": {
                        "format": "lcov",
                        "path": "lcov.info",
                        "rules": [{"scope": "changed_files", "minimum": 85}]
                    }
                }]
            }"#,
        )
        .unwrap();
        config_dir
    }

    #[test]
    fn test_config_set_command() {
        let dir = tempfile::tempdir().unwrap();
        setup_config_with_target(dir.path());

        let cmd = ConfigCommand::SetCommand {
            target: "rust".to_string(),
            mode: "essential".to_string(),
            command: "cargo test --lib".to_string(),
        };
        let result = run_config_subcommand(dir.path(), &cmd);
        assert!(result.is_ok(), "set-command should succeed: {result:?}");

        let config_path = dir.path().join(".codeflow/config/testing/test-config.json");
        let config = test_config::load_test_config(&config_path).unwrap();
        assert!(config.targets[0].modes.contains_key("essential"));
        assert_eq!(
            config.targets[0].modes["essential"].command,
            "cargo test --lib"
        );
    }

    #[test]
    fn test_config_set_command_unknown_mode_rejected() {
        let dir = tempfile::tempdir().unwrap();
        setup_config_with_target(dir.path());

        let cmd = ConfigCommand::SetCommand {
            target: "rust".to_string(),
            mode: "turbo".to_string(),
            command: "cargo test".to_string(),
        };
        let result = run_config_subcommand(dir.path(), &cmd);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("unknown mode"),
            "should reject unknown mode"
        );
    }

    #[test]
    fn test_config_set_command_unbalanced_quotes_rejected() {
        let dir = tempfile::tempdir().unwrap();
        setup_config_with_target(dir.path());

        let cmd = ConfigCommand::SetCommand {
            target: "rust".to_string(),
            mode: "full".to_string(),
            command: "echo 'unbalanced".to_string(),
        };
        let result = run_config_subcommand(dir.path(), &cmd);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unbalanced"));
    }

    #[test]
    fn test_config_set_threshold_existing_rule() {
        let dir = tempfile::tempdir().unwrap();
        setup_config_with_target(dir.path());

        let cmd = ConfigCommand::SetThreshold {
            target: "rust".to_string(),
            scope: "changed_files".to_string(),
            minimum: 90,
            rule_index: None,
        };
        let result = run_config_subcommand(dir.path(), &cmd);
        assert!(result.is_ok(), "set-threshold should succeed: {result:?}");

        let config_path = dir.path().join(".codeflow/config/testing/test-config.json");
        let config = test_config::load_test_config(&config_path).unwrap();
        let cov = config.targets[0].coverage.as_ref().unwrap();
        assert_eq!(cov.rules[0].minimum, 90);
    }

    #[test]
    fn test_config_set_threshold_by_rule_index() {
        let dir = tempfile::tempdir().unwrap();
        setup_config_with_target(dir.path());

        let cmd = ConfigCommand::SetThreshold {
            target: "rust".to_string(),
            scope: "changed_files".to_string(),
            minimum: 95,
            rule_index: Some(0),
        };
        let result = run_config_subcommand(dir.path(), &cmd);
        assert!(result.is_ok());

        let config_path = dir.path().join(".codeflow/config/testing/test-config.json");
        let config = test_config::load_test_config(&config_path).unwrap();
        let cov = config.targets[0].coverage.as_ref().unwrap();
        assert_eq!(cov.rules[0].minimum, 95);
    }

    #[test]
    fn test_config_set_threshold_new_scope_adds_rule() {
        let dir = tempfile::tempdir().unwrap();
        setup_config_with_target(dir.path());

        let cmd = ConfigCommand::SetThreshold {
            target: "rust".to_string(),
            scope: "global".to_string(),
            minimum: 80,
            rule_index: None,
        };
        let result = run_config_subcommand(dir.path(), &cmd);
        assert!(result.is_ok());

        let config_path = dir.path().join(".codeflow/config/testing/test-config.json");
        let config = test_config::load_test_config(&config_path).unwrap();
        let cov = config.targets[0].coverage.as_ref().unwrap();
        assert_eq!(cov.rules.len(), 2);
    }

    #[test]
    fn test_config_set_threshold_invalid_rule_index() {
        let dir = tempfile::tempdir().unwrap();
        setup_config_with_target(dir.path());

        let cmd = ConfigCommand::SetThreshold {
            target: "rust".to_string(),
            scope: "changed_files".to_string(),
            minimum: 90,
            rule_index: Some(99),
        };
        let result = run_config_subcommand(dir.path(), &cmd);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("out of range"));
    }

    #[test]
    fn test_config_set_report() {
        let dir = tempfile::tempdir().unwrap();
        setup_config_with_target(dir.path());

        let cmd = ConfigCommand::SetReport {
            target: "rust".to_string(),
            format: "junit".to_string(),
            path: "target/nextest/junit.xml".to_string(),
            derive_from: None,
        };
        let result = run_config_subcommand(dir.path(), &cmd);
        assert!(result.is_ok(), "set-report should succeed: {result:?}");

        let config_path = dir.path().join(".codeflow/config/testing/test-config.json");
        let config = test_config::load_test_config(&config_path).unwrap();
        let report = config.targets[0].report.as_ref().unwrap();
        assert_eq!(
            report.format,
            codeflow_core::testing::config::ReportFormat::Junit
        );
        assert_eq!(report.path, "target/nextest/junit.xml");
    }

    #[test]
    fn test_config_set_report_with_derive_from() {
        let dir = tempfile::tempdir().unwrap();
        setup_config_with_target(dir.path());

        let cmd = ConfigCommand::SetReport {
            target: "rust".to_string(),
            format: "ctrf".to_string(),
            path: "target/ctrf.json".to_string(),
            derive_from: Some("junit".to_string()),
        };
        let result = run_config_subcommand(dir.path(), &cmd);
        assert!(result.is_ok());

        let config_path = dir.path().join(".codeflow/config/testing/test-config.json");
        let config = test_config::load_test_config(&config_path).unwrap();
        let report = config.targets[0].report.as_ref().unwrap();
        assert_eq!(report.derive_from.as_deref(), Some("junit"));
    }

    #[test]
    fn test_config_set_coverage() {
        let dir = tempfile::tempdir().unwrap();
        setup_config_with_target(dir.path());

        let cmd = ConfigCommand::SetCoverage {
            target: "rust".to_string(),
            format: "cobertura".to_string(),
            path: "coverage.xml".to_string(),
        };
        let result = run_config_subcommand(dir.path(), &cmd);
        assert!(result.is_ok(), "set-coverage should succeed: {result:?}");

        let config_path = dir.path().join(".codeflow/config/testing/test-config.json");
        let config = test_config::load_test_config(&config_path).unwrap();
        let cov = config.targets[0].coverage.as_ref().unwrap();
        assert_eq!(
            cov.format,
            codeflow_core::testing::config::CoverageFormat::Cobertura
        );
        assert_eq!(cov.path, "coverage.xml");
    }

    #[test]
    fn test_config_enable_disable() {
        let dir = tempfile::tempdir().unwrap();
        setup_config_with_target(dir.path());

        // Disable
        let cmd = ConfigCommand::Disable {
            target: "rust".to_string(),
        };
        let result = run_config_subcommand(dir.path(), &cmd);
        assert!(result.is_ok());

        let config_path = dir.path().join(".codeflow/config/testing/test-config.json");
        let config = test_config::load_test_config(&config_path).unwrap();
        assert!(!config.targets[0].enabled);

        // Enable
        let cmd = ConfigCommand::Enable {
            target: "rust".to_string(),
        };
        let result = run_config_subcommand(dir.path(), &cmd);
        assert!(result.is_ok());

        let config = test_config::load_test_config(&config_path).unwrap();
        assert!(config.targets[0].enabled);
    }

    #[test]
    fn test_exceptions_remove() {
        let dir = tempfile::tempdir().unwrap();
        setup_config_with_target(dir.path());

        // Add exception first
        let add = ExceptionsCommand::Add {
            target: "rust".to_string(),
            file: "worker.rs".to_string(),
            threshold: 79,
            reason: "test".to_string(),
            remove_when: "later".to_string(),
        };
        run_exceptions_subcommand(dir.path(), &add).unwrap();

        // Remove it
        let remove = ExceptionsCommand::Remove {
            target: "rust".to_string(),
            file: "worker.rs".to_string(),
        };
        let result = run_exceptions_subcommand(dir.path(), &remove);
        assert!(result.is_ok());

        let config_path = dir.path().join(".codeflow/config/testing/test-config.json");
        let config = test_config::load_test_config(&config_path).unwrap();
        let cov = config.targets[0].coverage.as_ref().unwrap();
        assert!(cov.exceptions.is_empty());
    }

    #[test]
    fn test_exceptions_remove_not_found() {
        let dir = tempfile::tempdir().unwrap();
        setup_config_with_target(dir.path());

        let remove = ExceptionsCommand::Remove {
            target: "rust".to_string(),
            file: "nonexistent.rs".to_string(),
        };
        let result = run_exceptions_subcommand(dir.path(), &remove);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_exceptions_list_with_filter() {
        let dir = tempfile::tempdir().unwrap();
        setup_config_with_target(dir.path());

        // Add exception
        let add = ExceptionsCommand::Add {
            target: "rust".to_string(),
            file: "test.rs".to_string(),
            threshold: 70,
            reason: "r".to_string(),
            remove_when: "w".to_string(),
        };
        run_exceptions_subcommand(dir.path(), &add).unwrap();

        // List with filter
        let list = ExceptionsCommand::List {
            target: Some("rust".to_string()),
        };
        let result = run_exceptions_subcommand(dir.path(), &list);
        assert!(result.is_ok());
    }

    #[test]
    fn test_exceptions_list_no_filter() {
        let dir = tempfile::tempdir().unwrap();
        setup_config_with_target(dir.path());

        let list = ExceptionsCommand::List { target: None };
        let result = run_exceptions_subcommand(dir.path(), &list);
        assert!(result.is_ok());
    }

    #[test]
    fn test_report_diff_run_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let cmd = ReportCommand::Diff {
            from: "run-1".to_string(),
            to: "run-2".to_string(),
        };
        let result = run_report_subcommand(dir.path(), &cmd);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("not found in ledger")
        );
    }

    #[test]
    fn test_report_convert_unsupported_format() {
        let cmd = ReportCommand::Convert {
            from: "ctrf".to_string(),
            to: "junit".to_string(),
            input: "/tmp/in.json".to_string(),
            output: "/tmp/out.xml".to_string(),
        };
        let dir = tempfile::tempdir().unwrap();
        let result = run_report_subcommand(dir.path(), &cmd);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("unsupported conversion")
        );
    }

    #[test]
    fn test_report_convert_input_not_found() {
        let cmd = ReportCommand::Convert {
            from: "junit".to_string(),
            to: "ctrf".to_string(),
            input: "/nonexistent/report.xml".to_string(),
            output: "/tmp/out.json".to_string(),
        };
        let dir = tempfile::tempdir().unwrap();
        let result = run_report_subcommand(dir.path(), &cmd);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_show() {
        let dir = tempfile::tempdir().unwrap();
        setup_config_with_target(dir.path());

        let cmd = ConfigCommand::Show;
        let result = run_config_subcommand(dir.path(), &cmd);
        assert!(result.is_ok());
    }

    #[test]
    fn test_config_set_command_target_not_found() {
        let dir = tempfile::tempdir().unwrap();
        setup_config_with_target(dir.path());

        let cmd = ConfigCommand::SetCommand {
            target: "nonexistent".to_string(),
            mode: "full".to_string(),
            command: "echo".to_string(),
        };
        let result = run_config_subcommand(dir.path(), &cmd);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_new_engine_with_failing_target() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow").join("config").join("testing");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("test-config.json"),
            r#"{
                "schema_version": "1.0",
                "targets": [{
                    "name": "fail-test",
                    "runner": "custom",
                    "modes": {"essential": {"command": "exit 1"}}
                }]
            }"#,
        )
        .unwrap();

        let args = TestArgs {
            mode: Some(TestMode::Essential),
            format: OutputFormat::Human,
            only: None,
            skip: None,
            fail_fast: false,
            subcommand: None,
        };
        let result = run_with_dir(dir.path(), &args);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("failed"),
            "failing target should produce error"
        );
    }

    #[test]
    fn test_new_engine_disabled_targets_produce_fresh_notice() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow").join("config").join("testing");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("test-config.json"),
            r#"{
                "schema_version": "1.0",
                "targets": [{
                    "name": "disabled-test",
                    "enabled": false,
                    "runner": "custom",
                    "modes": {"essential": {"command": "echo should not run"}}
                }]
            }"#,
        )
        .unwrap();

        let args = TestArgs {
            mode: None,
            format: OutputFormat::Human,
            only: None,
            skip: None,
            fail_fast: false,
            subcommand: None,
        };
        let result = run_with_dir(dir.path(), &args);
        assert!(result.is_ok());
    }

    // ── Setup subcommand tests ───────────────────────────────────────────

    fn make_args_with_subcommand(sub: TestSubcommand) -> TestArgs {
        TestArgs {
            mode: None,
            format: OutputFormat::Human,
            only: None,
            skip: None,
            fail_fast: false,
            subcommand: Some(sub),
        }
    }

    #[test]
    fn test_setup_list_empty() {
        let dir = tempfile::tempdir().unwrap();
        let args = make_args_with_subcommand(TestSubcommand::Setup {
            auto: false,
            template: None,
            list: true,
            add_target: false,
            force: false,
        });
        let result = run_with_dir(dir.path(), &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_setup_auto_empty_repo() {
        let dir = tempfile::tempdir().unwrap();
        let args = make_args_with_subcommand(TestSubcommand::Setup {
            auto: true,
            template: None,
            list: false,
            add_target: false,
            force: false,
        });
        let result = run_with_dir(dir.path(), &args);
        assert!(result.is_ok());
        // Config should be created
        let config_path = dir.path().join(".codeflow/config/testing/test-config.json");
        assert!(config_path.exists());
    }

    #[test]
    fn test_setup_template_with_force() {
        let dir = tempfile::tempdir().unwrap();
        // Create template dir with a minimal template
        let tpl_dir = dir.path().join(".codeflow/templates/test-config");
        std::fs::create_dir_all(&tpl_dir).unwrap();
        std::fs::write(
            tpl_dir.join("minimal.json"),
            r#"{"schema_version":"1.0","targets":[]}"#,
        )
        .unwrap();

        let args = make_args_with_subcommand(TestSubcommand::Setup {
            auto: false,
            template: Some("minimal.json".to_string()),
            list: false,
            add_target: false,
            force: true,
        });
        let result = run_with_dir(dir.path(), &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_doctor_human_no_config() {
        let dir = tempfile::tempdir().unwrap();
        // Doctor with missing config calls process::exit(1) via run_doctor,
        // so we test the internal functions directly.
        use codeflow_core::testing::doctor;
        let checks = doctor::run_all_checks(dir.path());
        let code = doctor::exit_code(&checks);
        assert_eq!(code, 1);
        let output = doctor::format_human(&checks);
        assert!(output.contains("FAIL"));
    }

    #[test]
    fn test_doctor_json_with_config() {
        let dir = tempfile::tempdir().unwrap();
        let cfg_dir = dir.path().join(".codeflow/config/testing");
        std::fs::create_dir_all(&cfg_dir).unwrap();
        std::fs::write(
            cfg_dir.join("test-config.json"),
            r#"{"schema_version":"1.0","targets":[]}"#,
        )
        .unwrap();
        use codeflow_core::testing::doctor;
        let checks = doctor::run_all_checks(dir.path());
        let code = doctor::exit_code(&checks);
        assert_eq!(code, 0);
        let output = doctor::format_json(&checks);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed.is_array());
    }

    // ── extract_report_from_event tests ─────────────────────────────────

    #[test]
    fn test_extract_report_from_event_with_counts() {
        let event = serde_json::json!({
            "total_passed": 10,
            "total_failed": 2,
        });
        let report = extract_report_from_event(&event, "run-1");
        assert_eq!(report.results.summary.passed, 10);
        assert_eq!(report.results.summary.failed, 2);
        assert_eq!(report.results.tests.len(), 12);
    }

    #[test]
    fn test_extract_report_from_event_zero_counts() {
        let event = serde_json::json!({});
        let report = extract_report_from_event(&event, "run-0");
        assert_eq!(report.results.summary.passed, 0);
        assert_eq!(report.results.summary.failed, 0);
        assert!(report.results.tests.is_empty());
    }

    #[test]
    fn test_extract_report_from_event_passed_only() {
        let event = serde_json::json!({
            "total_passed": 5,
        });
        let report = extract_report_from_event(&event, "run-p");
        assert_eq!(report.results.summary.passed, 5);
        assert_eq!(report.results.summary.failed, 0);
        assert_eq!(report.results.tests.len(), 5);
        assert!(
            report
                .results
                .tests
                .iter()
                .all(|t| { t.status == codeflow_core::testing::report::CtrfStatus::Passed })
        );
    }

    // ── run_report_show tests ───────────────────────────────────────────

    #[test]
    fn test_report_show_no_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_report_show(dir.path(), None, &OutputFormat::Human);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("no test reports"),
            "should fail when report dir missing"
        );
    }

    #[test]
    fn test_report_show_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".state/test-reports")).unwrap();
        let result = run_report_show(dir.path(), None, &OutputFormat::Human);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("no test report files"),
            "should fail when report dir is empty"
        );
    }

    #[test]
    fn test_report_show_with_report_human() {
        let dir = tempfile::tempdir().unwrap();
        let report_dir = dir.path().join(".state/test-reports");
        std::fs::create_dir_all(&report_dir).unwrap();
        std::fs::write(
            report_dir.join("run-001.json"),
            r#"{"run_id":"run-001","timestamp":"2026-04-16","targets":[]}"#,
        )
        .unwrap();
        let result = run_report_show(dir.path(), None, &OutputFormat::Human);
        assert!(result.is_ok(), "human format should succeed: {result:?}");
    }

    #[test]
    fn test_report_show_with_report_json() {
        let dir = tempfile::tempdir().unwrap();
        let report_dir = dir.path().join(".state/test-reports");
        std::fs::create_dir_all(&report_dir).unwrap();
        std::fs::write(
            report_dir.join("run-002.json"),
            r#"{"run_id":"run-002","targets":[{"name":"rust","exit_code":0,"duration_ms":1500}]}"#,
        )
        .unwrap();
        let result = run_report_show(dir.path(), None, &OutputFormat::Json);
        assert!(result.is_ok(), "json format should succeed: {result:?}");
    }

    #[test]
    fn test_report_show_run_id_filter_hits() {
        let dir = tempfile::tempdir().unwrap();
        let report_dir = dir.path().join(".state/test-reports");
        std::fs::create_dir_all(&report_dir).unwrap();
        std::fs::write(
            report_dir.join("run-a.json"),
            r#"{"run_id":"run-a","targets":[]}"#,
        )
        .unwrap();
        std::fs::write(
            report_dir.join("run-b.json"),
            r#"{"run_id":"run-b","targets":[]}"#,
        )
        .unwrap();
        // Should find the specific run by id, not the most recent one.
        let result = run_report_show(dir.path(), Some("run-a"), &OutputFormat::Json);
        assert!(result.is_ok(), "run-id filter should succeed: {result:?}");
    }

    #[test]
    fn test_report_show_run_id_filter_miss() {
        let dir = tempfile::tempdir().unwrap();
        let report_dir = dir.path().join(".state/test-reports");
        std::fs::create_dir_all(&report_dir).unwrap();
        std::fs::write(
            report_dir.join("run-a.json"),
            r#"{"run_id":"run-a","targets":[]}"#,
        )
        .unwrap();
        let result = run_report_show(dir.path(), Some("run-missing"), &OutputFormat::Human);
        assert!(result.is_err(), "missing run-id must return error");
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("no test report found for run-id 'run-missing'"),
            "error must mention the missing run-id"
        );
    }

    #[test]
    fn test_report_show_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let report_dir = dir.path().join(".state/test-reports");
        std::fs::create_dir_all(&report_dir).unwrap();
        std::fs::write(report_dir.join("bad.json"), "not json").unwrap();
        let result = run_report_show(dir.path(), None, &OutputFormat::Json);
        assert!(result.is_err(), "invalid JSON must surface a parse error");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("failed to parse report"),
            "error must mention parse failure; got: {msg}"
        );
    }

    // ── toggle_target tests ─────────────────────────────────────────────

    #[test]
    fn test_toggle_target_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow/config/testing");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("test-config.json"),
            r#"{"schema_version":"1.0","targets":[]}"#,
        )
        .unwrap();
        let result = toggle_target(&config_dir.join("test-config.json"), "nope", true);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    // ── load_or_create_config tests ─────────────────────────────────────

    #[test]
    fn test_load_or_create_config_missing_creates_default() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("nonexistent.json");
        let config = load_or_create_config(&config_path).unwrap();
        assert_eq!(config.schema_version, "1.0");
        assert!(config.targets.is_empty());
    }

    #[test]
    fn test_load_or_create_config_existing() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow/config/testing");
        std::fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("test-config.json");
        std::fs::write(
            &config_path,
            r#"{"schema_version":"1.0","targets":[{"name":"t","runner":"custom","modes":{}}]}"#,
        )
        .unwrap();
        let config = load_or_create_config(&config_path).unwrap();
        assert_eq!(config.targets.len(), 1);
        assert_eq!(config.targets[0].name, "t");
    }

    // ── find_target_mut tests ───────────────────────────────────────────

    #[test]
    fn test_find_target_mut_found() {
        let dir = tempfile::tempdir().unwrap();
        setup_config_with_target(dir.path());
        let config_path = dir.path().join(".codeflow/config/testing/test-config.json");
        let mut config = load_or_create_config(&config_path).unwrap();
        let t = find_target_mut(&mut config, "rust");
        assert!(t.is_ok());
        assert_eq!(t.unwrap().name, "rust");
    }

    #[test]
    fn test_find_target_mut_not_found() {
        let dir = tempfile::tempdir().unwrap();
        setup_config_with_target(dir.path());
        let config_path = dir.path().join(".codeflow/config/testing/test-config.json");
        let mut config = load_or_create_config(&config_path).unwrap();
        let t = find_target_mut(&mut config, "missing");
        assert!(t.is_err());
        assert!(t.unwrap_err().to_string().contains("not found"));
    }

    // ── new engine pipeline tests ───────────────────────────────────────

    #[test]
    fn test_new_engine_no_targets_fresh_project() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow/config/testing");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("test-config.json"),
            r#"{"schema_version":"1.0","targets":[]}"#,
        )
        .unwrap();
        let args = TestArgs {
            mode: None,
            format: OutputFormat::Human,
            only: None,
            skip: None,
            fail_fast: false,
            subcommand: None,
        };
        let result = run_with_dir(dir.path(), &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_new_engine_passing_target_with_pipeline() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow/config/testing");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("test-config.json"),
            r#"{
                "schema_version": "1.0",
                "targets": [{
                    "name": "echo-test",
                    "runner": "custom",
                    "modes": {"essential": {"command": "echo pass"}}
                }]
            }"#,
        )
        .unwrap();
        let args = TestArgs {
            mode: Some(TestMode::Essential),
            format: OutputFormat::Human,
            only: None,
            skip: None,
            fail_fast: false,
            subcommand: None,
        };
        let result = run_with_dir(dir.path(), &args);
        assert!(result.is_ok(), "echo-test should pass: {result:?}");

        // Verify report was written
        let report_dir = dir.path().join(".state/test-reports");
        assert!(report_dir.exists(), "report dir should be created");
    }

    #[test]
    fn test_new_engine_failing_target_pipeline() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow/config/testing");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("test-config.json"),
            r#"{
                "schema_version": "1.0",
                "targets": [{
                    "name": "fail-test",
                    "runner": "custom",
                    "modes": {"essential": {"command": "exit 1"}}
                }]
            }"#,
        )
        .unwrap();
        let args = TestArgs {
            mode: Some(TestMode::Essential),
            format: OutputFormat::Human,
            only: None,
            skip: None,
            fail_fast: false,
            subcommand: None,
        };
        let result = run_with_dir(dir.path(), &args);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("failed"));
    }

    #[test]
    fn test_new_engine_skip_and_only_filters() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow/config/testing");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("test-config.json"),
            r#"{
                "schema_version": "1.0",
                "targets": [
                    {"name": "a", "runner": "custom", "modes": {"essential": {"command": "echo a"}}},
                    {"name": "b", "runner": "custom", "modes": {"essential": {"command": "exit 1"}}}
                ]
            }"#,
        )
        .unwrap();

        // --only=a should skip the failing target b
        let args = TestArgs {
            mode: Some(TestMode::Essential),
            format: OutputFormat::Human,
            only: Some("a".to_string()),
            skip: None,
            fail_fast: false,
            subcommand: None,
        };
        let result = run_with_dir(dir.path(), &args);
        assert!(result.is_ok(), "only=a should skip b");

        // --skip=b should also work
        let args = TestArgs {
            mode: Some(TestMode::Essential),
            format: OutputFormat::Human,
            only: None,
            skip: Some("b".to_string()),
            fail_fast: false,
            subcommand: None,
        };
        let result = run_with_dir(dir.path(), &args);
        assert!(result.is_ok(), "skip=b should skip b");
    }
}
