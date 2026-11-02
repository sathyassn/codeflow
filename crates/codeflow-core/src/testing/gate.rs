//! The test gate: one entry point shared by `codeflow test` and `integrate`.
//!
//! Resolution order (charter §3.1, AC #7):
//! 1. `.codeflow/test-config.json` present → run its targets as configured.
//! 2. No config → runtime stack detection ([`crate::testing::setup::detect`]);
//!    detected targets run with best-guess commands.
//! 3. Nothing detected → [`GateOutcome::NoTargets`] — the caller MUST surface
//!    this loudly (it is a no-op, not a green run), but it is not an error.

use std::path::Path;

use crate::testing::config::{
    self, load_named_test_config, load_test_config, ExecutionConfig, TargetConfig,
};
use crate::testing::coverage::FileCoverage;
use crate::testing::delivery;
pub use crate::testing::delivery::{matches as inputs_match, GateOptions};
use crate::testing::error::TestingError;
use crate::testing::report::{CanonicalTestReport, CtrfStatus};
use crate::testing::runner::TargetRunResult;
use crate::testing::setup::detect::detect_stacks;
use crate::testing::validation;

/// Relative path of the test configuration inside a project.
pub const TEST_CONFIG_PATH: &str = ".codeflow/test-config.json";

/// Result of one target's run through the gate.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GateTargetResult {
    pub name: String,
    /// Process exit code; `-1` when the command could not run at all.
    pub exit_code: i32,
    pub duration_ms: u64,
    pub stdout: String,
    pub stderr: String,
    /// Failure detail beyond an ordinary exit code (timeout, signal, runner
    /// error, or a target that could not run).
    pub error: Option<String>,
    /// Structured failure detail parsed from the target's JUnit/CTRF report
    /// artifact, when one was configured and produced. `None` when the target
    /// declares no report or the artifact is missing/unparseable — callers then
    /// fall back to the raw stdout/stderr scrollback. Populated for passing
    /// targets too (its `failures` list is then empty) so `integrate`/hook
    /// consumers get counts regardless of verdict.
    pub report: Option<FailureReport>,
}

impl GateTargetResult {
    /// Whether this target passed.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.exit_code == 0 && self.error.is_none()
    }
}

/// Pass/fail/skip counts plus the individual failing tests parsed from a
/// target's report artifact. Surfaced above the raw scrollback so a failed run
/// names *what* failed (and where) instead of only dumping tool output.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FailureReport {
    pub passed: u64,
    pub failed: u64,
    pub skipped: u64,
    pub total: u64,
    /// One entry per failing test, in report order.
    pub failures: Vec<FailedTest>,
}

/// A single failing test extracted from the report.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FailedTest {
    /// Test identifier: `suite::name` when a suite is known, else `name`.
    pub id: String,
    /// Best-effort `file:line` pulled from the failure message or trace, when
    /// the tool emitted a recognizable source location.
    pub location: Option<String>,
    /// First line of the failure message, when present.
    pub message: Option<String>,
}

/// Coverage summary for one target (charter §3.1 coverage audit). A configured
/// threshold that a measured file misses fails the gate (`thresholds_failed`);
/// a full-mode run with no coverage data recorded is informational only.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CoverageReport {
    pub target: String,
    /// Overall line coverage percentage, or `None` when no coverage data was
    /// collected (missing or empty artifact).
    pub overall_percent: Option<f64>,
    pub thresholds_passed: usize,
    pub thresholds_failed: usize,
    pub exceptions_applied: usize,
    /// `(file, coverage_percent, threshold)` for each file below its (possibly
    /// exception-lowered) threshold. An exception lowers the bar; dropping below
    /// the lowered bar still fails.
    pub failing_files: Vec<(String, f64, u32)>,
    /// Set when coverage is configured but no usable data was produced.
    pub note: Option<String>,
    /// Coverage was configured with enforcing rules, but no usable artifact was
    /// produced (missing/empty/unparseable). Fails the gate — a required
    /// coverage measurement that silently vanishes must not pass (codex round-2:
    /// coverage fail-open on absent artifacts).
    pub data_missing: bool,
}

/// Outcome of running the test gate.
#[derive(Debug)]
pub enum GateOutcome {
    /// Nothing to run: no config targets and no detectable stack (or no
    /// target defines the requested mode). Callers must report this loudly.
    NoTargets { reason: String },
    /// Targets ran; `passed` is the gate verdict — every target's tests passed
    /// AND no configured coverage threshold was missed (`coverage`, populated
    /// for full-mode runs of targets that configure coverage).
    Completed {
        results: Vec<GateTargetResult>,
        passed: bool,
        /// Conditions that fail the gate independently of target exit statuses.
        failure_reasons: Vec<String>,
        coverage: Vec<CoverageReport>,
        /// Targets omitted by selection or CI configuration.
        skipped: Vec<String>,
        /// Count of requested targets skipped by candidate evidence.
        evidence_skipped: usize,
        /// Every full-mode target executed without selection limits.
        complete: bool,
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
    run_gate_resolved(project_dir, mode, true, &GateOptions::default())
}

/// Run only the targets that define `mode` themselves, with no `quick` to
/// `essential` alias. The pre-push hook uses this for its push set: a target
/// is in the push set exactly when it defines a `quick` mode, so a config
/// whose `essential` mode runs the whole test suite never turns into a slow
/// blocking push (TSK-132).
///
/// # Errors
///
/// Returns `TestingError` when a present config file cannot be loaded.
pub fn run_gate_exact(project_dir: &Path, mode: &str) -> Result<GateOutcome, TestingError> {
    run_gate_resolved(project_dir, mode, false, &GateOptions::default())
}

/// Run the gate with conservative comparison or explicit complete selection.
///
/// # Errors
/// Configuration, prerequisite, preflight and evidence-write failures refuse the run.
pub fn run_gate_with_options(
    project_dir: &Path,
    mode: &str,
    options: &GateOptions,
) -> Result<GateOutcome, TestingError> {
    run_gate_resolved(project_dir, mode, true, options)
}

#[allow(clippy::too_many_lines)]
fn run_gate_resolved(
    project_dir: &Path,
    mode: &str,
    alias_quick: bool,
    options: &GateOptions,
) -> Result<GateOutcome, TestingError> {
    let options = &GateOptions {
        all: options.all || (mode == "full" && options.since.is_none()),
        ..options.clone()
    };
    let resolve = |targets: &[TargetConfig]| {
        if alias_quick {
            resolve_mode(mode, targets)
        } else {
            mode.to_string()
        }
    };
    if let Some(named) = &options.config {
        let (config, bytes) = load_named_test_config(named)?;
        if config.targets.is_empty() {
            return Ok(GateOutcome::NoTargets {
                reason: format!("{} defines no targets", named.display()),
            });
        }
        let effective = resolve(&config.targets);
        // Diagnostics name the config this run read, never the default
        // path it ignored (TSK-1020 review 1).
        let default = project_dir.join(TEST_CONFIG_PATH);
        return run_configured(
            project_dir,
            mode,
            options,
            (config.targets, config.execution, effective),
            &delivery::digest(&bytes),
        )
        .map_err(|error| match error {
            TestingError::ConfigInvalid { path, message } if path == default => {
                TestingError::ConfigInvalid {
                    path: named.clone(),
                    message,
                }
            }
            other => other,
        });
    }
    let config_path = project_dir.join(TEST_CONFIG_PATH);

    let has_config = !crate::absence::proven_absent(&config_path)?;
    let (targets, execution, effective_mode) = if has_config {
        let config = load_test_config(&config_path)?;
        if config.targets.is_empty() {
            return Ok(GateOutcome::NoTargets {
                reason: format!("{TEST_CONFIG_PATH} defines no targets"),
            });
        }
        let effective = resolve(&config.targets);
        (config.targets, config.execution, effective)
    } else {
        let detected = detect_stacks(project_dir)?;
        if detected.is_empty() {
            return Ok(GateOutcome::NoTargets {
                reason: format!("no {TEST_CONFIG_PATH} and no test stack detected"),
            });
        }
        let targets: Vec<TargetConfig> = detected.into_iter().map(|d| d.config).collect();
        let effective = resolve(&targets);
        (targets, ExecutionConfig::default(), effective)
    };

    let config_digest = if has_config {
        // Bind evidence to the complete configured input, including defaults.
        delivery::digest(&std::fs::read(&config_path)?)
    } else {
        delivery::digest(&serde_json::to_vec(&(&targets, &execution))?)
    };
    run_configured(
        project_dir,
        mode,
        options,
        (targets, execution, effective_mode),
        &config_digest,
    )
}

/// Select and run `targets` from a loaded config whose evidence binds to
/// `config_digest`.
#[allow(clippy::too_many_lines)]
fn run_configured(
    project_dir: &Path,
    mode: &str,
    options: &GateOptions,
    (mut targets, mut execution, effective_mode): (Vec<TargetConfig>, ExecutionConfig, String),
    config_digest: &str,
) -> Result<GateOutcome, TestingError> {
    if let Some(seconds) = options.heavy_wait_seconds {
        execution.heavy_wait_seconds = seconds;
    }
    delivery::check_only(project_dir, &targets, &effective_mode, &options.only)?;
    let home = crate::registry::codeflow_home();
    let selection = delivery::select(&targets, &effective_mode, options);
    for target in &targets {
        let reason = selection.reasons.get(&target.name).map_or_else(
            || selection.reason.clone(),
            |reason| format!("{}; {reason}", selection.reason),
        );
        if selection.selected.contains(&target.name) {
            progress!(
                "[codeflow test] selected {}: {reason}; requires [{}]",
                target.name,
                target.requires.join(", ")
            );
        } else if selection.skipped.contains(&target.name) {
            progress!("[codeflow test] skipped {}: {reason}", target.name);
        } else if selection.excluded.contains(&target.name) {
            progress!("[codeflow test] excluded {}: {reason}", target.name);
        }
    }
    let declarations = targets.clone();
    targets.retain(|t| selection.selected.contains(&t.name));
    if targets.is_empty() && selection.skipped.is_empty() {
        return Ok(GateOutcome::NoTargets {
            reason: format!("no enabled target defines mode \"{effective_mode}\""),
        });
    }
    for target in &targets {
        for required in &target.requires {
            if targets
                .iter()
                .find(|t| &t.name == required)
                .is_some_and(|t| {
                    crate::testing::runner::is_ci_environment() && t.ci_skip == Some(true)
                })
            {
                return Err(delivery::invalid(
                    project_dir,
                    format!("prerequisite '{required}' is skipped in CI"),
                ));
            }
        }
    }
    // A target CI skips is still owed locally: a run without it is not
    // complete evidence (TSK-184 AC-5).
    let ci_skipped: Vec<String> = targets
        .iter()
        .filter(|t| crate::testing::runner::is_ci_environment() && t.ci_skip == Some(true))
        .map(|t| t.name.clone())
        .collect();
    targets.retain(|t| !ci_skipped.contains(&t.name));
    let mut tools =
        delivery::preflight(project_dir, &targets, &effective_mode).map_err(|error| {
            let message = declarations.iter().fold(error.to_string(), |text, target| {
                super::evidence::redact(target, &text)
            });
            delivery::invalid(project_dir, message)
        })?;
    for value in tools.values_mut() {
        for target in &declarations {
            *value = super::evidence::redact(target, value);
        }
    }
    let run_id = uuid::Uuid::new_v4().to_string();
    // Every evidence write walks from this anchor and never follows a link
    // or junction, the build-output folder itself included (TSK-1044).
    let (anchor, target_relative) = delivery::target_anchor(project_dir)?;
    std::fs::create_dir_all(&anchor)?;
    let run_relative = format!("{target_relative}/codeflow-gate/{run_id}");
    let anchor = create_run_directory(&anchor, &run_relative)
        .map_err(|error| delivery::evidence_refused(&anchor, &run_relative, &error))?;
    let target_dir = anchor.join(&target_relative);
    let run_dir = anchor.join(&run_relative);
    configure_run_targets(
        &mut targets,
        project_dir,
        &target_dir,
        &run_dir,
        options.all,
    )?;
    // Resolve fallback weight before scheduling; the original declarations remain evidence inputs.
    for target in &mut targets {
        if mode == "quick" && target.heavy.is_none() {
            target.heavy = Some(
                effective_mode == "essential"
                    || execution.quick_default_class == config::QuickTargetClass::Heavy,
            );
        }
    }
    let before = delivery::tracked(project_dir)?;
    let raw = crate::testing::runner::run_dependency_targets_with_admission(
        &targets,
        &effective_mode,
        project_dir,
        &execution,
        &run_dir,
        &declarations,
        mode == "quick",
    );
    let after = delivery::tracked(project_dir)?;
    // Producers are complete now. This is the named candidate; a writer that
    // changed tracked bytes cannot receive exact-candidate evidence.
    let tree = delivery::digest(&serde_json::to_vec(&after)?);
    let generation_unchanged = before == after;
    let revision = delivery::revision(project_dir).unwrap_or_default();

    if raw.is_empty() && selection.skipped.is_empty() && ci_skipped.is_empty() {
        return Ok(GateOutcome::NoTargets {
            reason: format!("no enabled target defines mode \"{effective_mode}\""),
        });
    }

    // Coverage: for a full run, parse each covered target's artifact and
    // summarize it. A missed threshold fails the gate verdict below; a run with
    // no coverage data is informational only.
    let coverage = if effective_mode == "full" {
        let ok_runs: Vec<TargetRunResult> = raw
            .iter()
            .filter_map(|r| r.as_ref().ok().cloned())
            .collect();
        collect_coverage_reports(&targets, &ok_runs)
    } else {
        Vec::new()
    };

    let target_evidence: Vec<_> = raw
        .iter()
        .filter_map(|r| r.as_ref().ok()?.evidence.clone())
        .collect();
    let results: Vec<GateTargetResult> = raw
        .into_iter()
        .map(|r| match r {
            Ok(run) => {
                // Parse the target's report artifact (JUnit/CTRF) so a failed run
                // surfaces failing test IDs + file:line above the raw scrollback.
                // Best-effort: a missing config, missing artifact, or parse error
                // yields None and the caller falls back to raw output.
                let report = targets
                    .iter()
                    .find(|t| t.name == run.target_name)
                    .and_then(|t| validation::parse_target_report(t, &run).ok().flatten())
                    .map(|rep| summarize_report(&rep));
                GateTargetResult {
                    name: run.target_name,
                    exit_code: run.exit_code,
                    duration_ms: run.duration_ms,
                    stdout: run.stdout,
                    stderr: run.stderr,
                    error: run.error,
                    report,
                }
            }
            Err(e) => GateTargetResult {
                name: "(gate)".to_string(),
                exit_code: -1,
                duration_ms: 0,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(e.to_string()),
                report: None,
            },
        })
        .collect();

    let mut failure_reasons = Vec::new();
    if !generation_unchanged {
        let changed: std::collections::BTreeSet<_> = before
            .keys()
            .chain(after.keys())
            .filter(|path| before.get(*path) != after.get(*path))
            .map(String::as_str)
            .collect();
        let changed = changed.into_iter().collect::<Vec<_>>().join(", ");
        failure_reasons.push(format!("tracked files changed during the gate: {changed}"));
        progress!(
            "[codeflow test] generation changed the candidate: {}",
            changed
        );
    }
    let passed = generation_unchanged && gate_verdict(&results, &coverage);
    let dirty_paths: Vec<String> = match git2::Repository::discover(project_dir) {
        Ok(repo) => match repo.statuses(None) {
            Ok(statuses) => statuses
                .iter()
                .filter(|e| !e.status().is_empty() && e.status() != git2::Status::IGNORED)
                .map(|e| format!("{}: {:?}", e.path().unwrap_or("<non-UTF8>"), e.status()))
                .collect(),
            Err(error) => vec![format!("status unavailable: {error}")],
        },
        Err(error) => vec![format!("repository unavailable: {error}")],
    };
    let clean = dirty_paths.is_empty();
    // A run limited by `--only` is one part of a split gate, never the
    // whole gate's evidence, even when its names happen to cover every
    // target: only an unlimited run is recorded complete.
    let complete = !results.is_empty()
        && options.only.is_empty()
        && selection.skipped.is_empty()
        && ci_skipped.is_empty()
        && results.len() == targets.len()
        && effective_mode == "full";
    if !ci_skipped.is_empty() && effective_mode == "full" {
        progress!(
            "[codeflow test] not complete: skipped in CI and still owed locally: {}",
            ci_skipped.join(", ")
        );
    }
    let artifact = serde_json::json!({
        "schema_version": 2, "run_id": run_id, "revision": revision,
        "git_tree": super::evidence::git_tree(project_dir), "target_evidence": target_evidence,
        "tree": tree, "config_digest": config_digest, "mode": effective_mode,
        "selection": selection, "passed": passed, "complete": complete, "clean": clean,
        "ci_skipped": ci_skipped,
        "generation_unchanged": generation_unchanged, "dirty_paths": dirty_paths,
        "failure_reasons": failure_reasons,
        "results": results, "coverage": coverage,
        "targets": targets.iter().map(|t| serde_json::json!({"name":t.name,"command":super::evidence::redact(declarations.iter().find(|d| d.name == t.name).unwrap_or(t), &t.modes[&effective_mode].command),"requires":t.requires})).collect::<Vec<_>>(),
        "environment": {"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"target_dir":target_dir},
        "tools": tools
    });
    // The targets have run and could have planted a link or file at this
    // name, so the record is created exclusively through the anchored walk
    // that made the run folder (TSK-1044).
    let record = format!("{run_relative}/run.json");
    let bytes = serde_json::to_vec_pretty(&artifact)?;
    crate::contained::Tree::open(&anchor)
        .and_then(|tree| tree.create_new(&record, &bytes))
        .map_err(|error| delivery::evidence_refused(&anchor, &record, &error))?;
    progress!(
        "[codeflow test] artifact: {}",
        run_dir.join("run.json").display()
    );
    if effective_mode == "full" && !revision.is_empty() {
        let home = home.ok_or_else(|| {
            delivery::invalid(
                project_dir,
                "CodeFlow home unavailable for durable full-run evidence",
            )
        })?;
        let destination = format!("{}/{run_id}", delivery::durable_relative(project_dir));
        copy_evidence(&anchor, &run_relative, &home, &destination)?;
        progress!(
            "[codeflow test] durable artifact: {}",
            home.join(&destination).join("run.json").display()
        );
    }
    Ok(GateOutcome::Completed {
        evidence_skipped: selection.skipped.len(),
        skipped: selection.skipped.into_iter().chain(ci_skipped).collect(),
        complete,
        results,
        passed,
        failure_reasons,
        coverage,
    })
}

// Each run owns its folder exclusively before any producer writes there.
// The walk from `anchor` creates the build-output and `codeflow-gate`
// folders and refuses a link planted at any of them or at the run's own
// name. Returns the anchor's canonical path, which the other writes reuse.
fn create_run_directory(anchor: &Path, run_relative: &str) -> std::io::Result<std::path::PathBuf> {
    let tree = crate::contained::Tree::open(anchor)?;
    tree.mkdir(run_relative)?;
    Ok(tree.path().to_path_buf())
}

fn configure_run_targets(
    targets: &mut [TargetConfig],
    root: &Path,
    target_dir: &Path,
    run_dir: &Path,
    with_repeat: bool,
) -> Result<(), TestingError> {
    let binary = targets
        .iter()
        .find(|t| t.name == "codeflow-bin")
        .and_then(|t| t.outputs.first())
        .map(|output| resolve_output(output, root, target_dir));
    // OS text rule (issue 79): these values are text in a target's
    // environment. A path that is not valid UTF-8 is left unset, so a target
    // that needs it fails on its own, never on a lossy lookalike path.
    let text = |path: &Path| path.to_str().map(str::to_string);
    for target in targets {
        if let Some(binary) = binary.as_deref().and_then(text) {
            if target.name != "codeflow-bin" {
                target.env.insert("CODEFLOW_BIN".into(), binary.clone());
                target.env.insert("CF_PRESENT_CODEFLOW".into(), binary);
            }
        }
        if let Some(results) = text(&run_dir.join("rust-coverage/junit.xml")) {
            target.env.insert("CODEFLOW_GATE_RESULTS".into(), results);
        }
        target.env.insert(
            "CODEFLOW_GATE_STARTED_AT".into(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64()
                .to_string(),
        );
        target.env.insert(
            "CODEFLOW_GATE_RUN_ID".into(),
            // Named by this tool in ASCII.
            run_dir
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or_default()
                .to_string(),
        );
        if target
            .modes
            .values()
            .any(|m| m.command.contains("cargo llvm-cov"))
        {
            if let Some(dir) = text(&target_dir.join("llvm-cov-target")) {
                target.env.insert("CARGO_LLVM_COV_TARGET_DIR".into(), dir);
            }
        }
        target.outputs = target
            .outputs
            .iter()
            .map(|p| {
                let resolved = resolve_output(p, root, target_dir);
                text(&resolved).ok_or_else(|| TestingError::ConfigInvalid {
                    path: resolved,
                    message: "resolved output path is not UTF-8; use a UTF-8 output directory"
                        .into(),
                })
            })
            .collect::<Result<_, _>>()?;
        if let Some(report) = &mut target.report {
            if report.path.starts_with("target/") {
                let resolved = resolve_output(&report.path, root, target_dir);
                report.path = text(&resolved).ok_or_else(|| TestingError::ConfigInvalid {
                    path: resolved,
                    message: "resolved report path is not UTF-8; use a UTF-8 output directory"
                        .into(),
                })?;
            }
        }
        if with_repeat {
            for command in target.modes.values_mut() {
                command.command = command.command.replace(
                    "node scripts/check-present-binary-delta.mjs",
                    "node scripts/check-present-binary-delta.mjs --with-repeat",
                );
            }
        }
    }
    Ok(())
}

fn resolve_output(output: &str, root: &Path, target_dir: &Path) -> std::path::PathBuf {
    output
        .strip_prefix("target/")
        .map_or_else(|| root.join(output), |p| target_dir.join(p))
}

/// The largest file and the most names in one folder that the durable copy
/// takes; a run folder beyond either is refused, never truncated.
const EVIDENCE_FILE_LIMIT: u64 = 1 << 30;
const EVIDENCE_ENTRY_LIMIT: usize = 100_000;

/// Copy the run folder `from`, beneath the evidence anchor `anchor`, to the
/// new folder `to` beneath the `CodeFlow` home (TSK-1044). Both sides walk
/// from a held root handle (the anchor, and the home's parent) and never
/// follow a link or junction at any component, the home folder included. A
/// link, a special file or a file with more than one name in the run folder
/// is refused; the destination folder and every file in it are created
/// exclusively, so a name planted there, a link or a hard link included, is
/// refused rather than written through. The folders above `to` are created
/// by the same walk. A refusal leaves what was already copied inside `to`.
fn copy_evidence(anchor: &Path, from: &str, home: &Path, to: &str) -> Result<(), TestingError> {
    // The home folder is entered from its parent, so a link at the home
    // itself is refused too.
    let (parent, leaf) = crate::contained::split_leaf(home)?;
    std::fs::create_dir_all(parent)?;
    let to = format!("{leaf}/{to}");
    let source = crate::contained::Tree::open(anchor)?;
    let destination = crate::contained::Tree::open(parent)?;
    destination
        .mkdir(&to)
        .map_err(|error| delivery::evidence_refused(parent, &to, &error))?;
    copy_folder(&source, from, &destination, &to).map_err(|(side, relative, error)| {
        delivery::evidence_refused(if side { parent } else { anchor }, &relative, &error)
    })
}

/// One folder of [`copy_evidence`]; an error carries whether it was on the
/// destination side and the path it names.
fn copy_folder(
    source: &crate::contained::Tree,
    from: &str,
    destination: &crate::contained::Tree,
    to: &str,
) -> Result<(), (bool, String, std::io::Error)> {
    use std::io::Read as _;
    let read_side = |relative: &str| {
        let relative = relative.to_string();
        move |error| (false, relative, error)
    };
    for name in source
        .entries(from, EVIDENCE_ENTRY_LIMIT)
        .map_err(read_side(from))?
    {
        let Some(name) = name.to_str() else {
            return Err((
                false,
                from.to_string(),
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "an evidence file name is not valid UTF-8",
                ),
            ));
        };
        let (child, target) = (format!("{from}/{name}"), format!("{to}/{name}"));
        // The type is read without following: a link or special file fails here.
        let metadata = source.metadata(&child).map_err(read_side(&child))?;
        if metadata.is_dir() {
            destination
                .mkdir(&target)
                .map_err(|error| (true, target.clone(), error))?;
            copy_folder(source, &child, destination, &target)?;
            continue;
        }
        let file = source.open_regular(&child).map_err(read_side(&child))?;
        crate::contained::single_name(&file).map_err(read_side(&child))?;
        let mut limited = file.take(EVIDENCE_FILE_LIMIT + 1);
        destination
            .create_new_with(&target, None, |out| {
                let copied = std::io::copy(&mut limited, out)?;
                if copied > EVIDENCE_FILE_LIMIT {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("{child} is larger than {EVIDENCE_FILE_LIMIT} bytes"),
                    ));
                }
                Ok(())
            })
            .map_err(|error| (true, target.clone(), error))?;
    }
    Ok(())
}

/// Whether a gate run in `mode` would run cargo: an enabled target that
/// defines the mode (after the `quick` alias) uses the cargo runner or names
/// `cargo` in its command, or, with no config, a Rust stack is detected. An
/// unreadable config reads as `false`; [`run_gate`] reports that error itself.
/// `named` is the config a caller names with `--config`, read instead.
#[must_use]
pub fn gate_uses_cargo(project_dir: &Path, mode: &str, named: Option<&Path>) -> bool {
    if let Some(named) = named {
        return load_named_test_config(named)
            .is_ok_and(|(config, _)| any_cargo(&config.targets, mode));
    }
    let config_path = project_dir.join(TEST_CONFIG_PATH);
    let Ok(absent) = crate::absence::proven_absent(&config_path) else {
        return false; // The actual gate reports this obtaining error.
    };
    let targets: Vec<TargetConfig> = if absent {
        let Ok(detected) = detect_stacks(project_dir) else {
            return false;
        };
        detected.into_iter().map(|d| d.config).collect()
    } else {
        match load_test_config(&config_path) {
            Ok(config) => config.targets,
            Err(_) => return false,
        }
    };
    any_cargo(&targets, mode)
}

/// Whether an enabled target that defines `mode` runs cargo.
fn any_cargo(targets: &[TargetConfig], mode: &str) -> bool {
    let effective = resolve_mode(mode, targets);
    targets
        .iter()
        .filter(|t| t.enabled)
        .filter_map(|t| t.modes.get(&effective).map(|m| (t, m)))
        .any(|(t, m)| {
            matches!(t.runner, crate::testing::config::RunnerType::Cargo)
                || m.command
                    .split([' ', '\t', '\n'])
                    .filter(|word| !word.is_empty())
                    .any(|word| word == "cargo")
        })
}

/// The gate verdict: every target's tests pass AND, for every coverage report,
/// no threshold failed AND no required coverage data went missing. A file below
/// its (possibly exception-lowered) threshold fails; a configured-but-absent
/// coverage artifact fails too (codex round-2: exceptions and missing artifacts
/// both let coverage fail open).
#[must_use]
pub fn gate_verdict(results: &[GateTargetResult], coverage: &[CoverageReport]) -> bool {
    results.iter().all(GateTargetResult::passed)
        && coverage
            .iter()
            .all(|c| c.thresholds_failed == 0 && !c.data_missing)
}

/// Build coverage summaries for the targets that ran successfully and configure
/// coverage. Reuses the post-test parsing/threshold machinery in
/// [`crate::testing::validation`]. Missing or unparseable artifacts are
/// informational for collection-only coverage and gate failures when threshold
/// rules make coverage enforcing.
fn collect_coverage_reports(
    targets: &[TargetConfig],
    runs: &[TargetRunResult],
) -> Vec<CoverageReport> {
    let mut reports = Vec::new();
    for run in runs {
        let Some(target) = targets.iter().find(|t| t.name == run.target_name) else {
            continue;
        };
        if target.coverage.is_none() {
            continue;
        }
        // Coverage with threshold rules is enforcing: if its artifact goes
        // missing, that must fail the gate, not pass silently. A coverage block
        // that only collects (no rules) has nothing to enforce, so a missing
        // artifact there is merely informational.
        let enforcing = target
            .coverage
            .as_ref()
            .is_some_and(|c| !c.rules.is_empty());

        let coverages = match validation::parse_target_coverage(target, run) {
            Ok(c) => c,
            Err(e) => {
                reports.push(CoverageReport {
                    target: run.target_name.clone(),
                    overall_percent: None,
                    thresholds_passed: 0,
                    thresholds_failed: 0,
                    exceptions_applied: 0,
                    failing_files: Vec::new(),
                    note: Some(format!("coverage artifact could not be parsed: {e}")),
                    data_missing: enforcing,
                });
                continue;
            }
        };
        if coverages.is_empty() {
            reports.push(CoverageReport {
                target: run.target_name.clone(),
                overall_percent: None,
                thresholds_passed: 0,
                thresholds_failed: 0,
                exceptions_applied: 0,
                failing_files: Vec::new(),
                note: Some("no coverage data collected".to_string()),
                data_missing: enforcing,
            });
            continue;
        }

        let total_found: u64 = coverages.iter().map(|c| c.lines_found).sum();
        let total_hit: u64 = coverages.iter().map(|c| c.lines_hit).sum();
        let overall_percent =
            (total_found > 0).then(|| FileCoverage::compute_percent(total_found, total_hit));

        let thresholds = validation::evaluate_target_thresholds(target, &coverages, &[]);
        let thresholds_passed = thresholds.iter().filter(|r| r.pass).count();
        // `pass` is already computed against the exception-lowered threshold, so
        // a failing result counts regardless of `exception_applied` — otherwise
        // an exception silently waives a file below even its lowered bar.
        let thresholds_failed = thresholds.iter().filter(|r| !r.pass).count();
        let exceptions_applied = thresholds.iter().filter(|r| r.exception_applied).count();
        let failing_files = thresholds
            .iter()
            .filter(|r| !r.pass)
            .map(|r| (r.file.clone(), r.coverage_percent, r.threshold))
            .collect();

        reports.push(CoverageReport {
            target: run.target_name.clone(),
            overall_percent,
            thresholds_passed,
            thresholds_failed,
            exceptions_applied,
            failing_files,
            note: None,
            data_missing: false,
        });
    }
    reports
}

/// Condense a parsed report into the counts + failing-test detail the gate
/// surfaces. Only failing tests are listed; passing/skipped ones are counted.
fn summarize_report(report: &CanonicalTestReport) -> FailureReport {
    let summary = &report.results.summary;
    let failures = report
        .results
        .tests
        .iter()
        .filter(|t| t.status == CtrfStatus::Failed)
        .map(|t| {
            let id = match &t.suite {
                Some(suite) if !suite.is_empty() => format!("{suite}::{}", t.name),
                _ => t.name.clone(),
            };
            let location = t
                .message
                .as_deref()
                .and_then(extract_location)
                .or_else(|| t.trace.as_deref().and_then(extract_location));
            let message = t.message.as_deref().and_then(first_line);
            FailedTest {
                id,
                location,
                message,
            }
        })
        .collect();
    FailureReport {
        passed: summary.passed,
        failed: summary.failed,
        skipped: summary.skipped,
        total: summary.total,
        failures,
    }
}

/// Best-effort extraction of a `file:line` source location from a failure
/// message or trace (e.g. `src/lib.rs:42`, `tests/test_api.py:10`). Returns the
/// first `path.ext:line` token found, or `None` when the tool emitted none.
fn extract_location(text: &str) -> Option<String> {
    // A path segment (word chars, `.`, `/`, `\`, `-`), then `.ext`, then `:line`.
    let re = regex::Regex::new(r"[\w./\\-]+\.[A-Za-z]\w*:\d+").ok()?;
    re.find(text).map(|m| m.as_str().to_string())
}

/// First non-empty-trimmed line of a message, or `None` when it is blank.
fn first_line(text: &str) -> Option<String> {
    let line = text.lines().next().unwrap_or("").trim();
    (!line.is_empty()).then(|| line.to_string())
}

/// Resolves the requested mode against the modes the targets actually define.
///
/// `quick` is an alias for `essential`. Every shipped test-config template and
/// runtime stack detection defines `essential`/`full`, never `quick`, while the
/// CLI default and the pre-push gate ([`crate::hooks::git_hook`]) speak
/// `quick`. When the targets define `essential` but no `quick`, `quick`
/// resolves to `essential`; otherwise the requested mode is passed through
/// unchanged, so an unsatisfiable mode still surfaces as a loud no-op
/// ([`GateOutcome::NoTargets`]) rather than silently mapping onto another mode.
fn resolve_mode(requested: &str, targets: &[TargetConfig]) -> String {
    let defines = |mode: &str| targets.iter().any(|t| t.modes.contains_key(mode));
    if requested == "quick" && defines("essential") && !defines("quick") {
        "essential".to_string()
    } else {
        requested.to_string()
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[test]
    fn r19_test_config_leaf_refuses_gate() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join(".codeflow");
        let path = config.join("test-config.json");
        std::fs::create_dir(&config).unwrap();
        std::os::unix::fs::symlink(dir.path().join("missing"), &path).unwrap();
        assert!(run_gate_exact(dir.path(), "quick").is_err());
    }
    #[cfg(unix)]
    #[test]
    fn r19_test_config_ancestor_refuses_gate() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join(".codeflow");
        std::os::unix::fs::symlink(dir.path().join("missing"), &config).unwrap();
        assert!(run_gate_exact(dir.path(), "quick").is_err());
    }

    fn cov(failed: usize) -> CoverageReport {
        CoverageReport {
            target: "t".into(),
            overall_percent: Some(80.0),
            thresholds_passed: 0,
            thresholds_failed: failed,
            exceptions_applied: 0,
            failing_files: vec![],
            note: None,
            data_missing: false,
        }
    }
    fn cov_missing() -> CoverageReport {
        CoverageReport {
            target: "t".into(),
            overall_percent: None,
            thresholds_passed: 0,
            thresholds_failed: 0,
            exceptions_applied: 0,
            failing_files: vec![],
            note: Some("no coverage data collected".into()),
            data_missing: true,
        }
    }
    fn ok_result() -> GateTargetResult {
        GateTargetResult {
            name: "t".into(),
            exit_code: 0,
            duration_ms: 1,
            stdout: String::new(),
            stderr: String::new(),
            error: None,
            report: None,
        }
    }
    #[test]
    fn test_coverage_threshold_fails_gate() {
        // codex pre-flip review: a missed coverage threshold must fail the gate.
        assert!(
            gate_verdict(&[ok_result()], &[cov(0)]),
            "tests pass, coverage ok"
        );
        assert!(
            !gate_verdict(&[ok_result()], &[cov(1)]),
            "missed threshold fails"
        );
        assert!(
            gate_verdict(&[ok_result()], &[]),
            "no coverage config still passes"
        );
    }

    // codex round-2: an enforcing coverage config whose artifact is missing/
    // unparseable must FAIL the gate, not pass silently (fail-open on absent data).
    #[test]
    fn test_missing_required_coverage_fails_gate() {
        assert!(
            !gate_verdict(&[ok_result()], &[cov_missing()]),
            "required coverage with no data must fail"
        );
    }

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
        let outcome = GateOutcome::NoTargets { reason: "x".into() };
        assert!(outcome.allows_proceed());
    }

    #[test]
    fn configured_passing_target_runs_green() {
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), &format!("[{}]", echo_target("ok", "exit 0")));

        let outcome = run_gate(dir.path(), "full").unwrap();
        match outcome {
            GateOutcome::Completed {
                results, passed, ..
            } => {
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
        assert!(!outcome.allows_proceed());
        match outcome {
            GateOutcome::Completed {
                results, passed, ..
            } => {
                assert!(!passed);
                assert_eq!(results[0].exit_code, 3);
            }
            GateOutcome::NoTargets { reason } => panic!("expected run, got NoTargets: {reason}"),
        }
    }

    #[test]
    fn failed_target_surfaces_parsed_report_above_raw() {
        // A failing target with a JUnit report artifact must surface parsed
        // counts + failing test IDs + file:line through the gate result, so the
        // CLI can print them above the raw scrollback (and integrate/hook
        // consumers get structured failures).
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("junit.xml"),
            r#"<testsuites><testsuite name="mysuite" tests="2" failures="1">
                <testcase name="test_ok" classname="mysuite" time="0.1"/>
                <testcase name="test_bad" classname="mysuite" time="0.2">
                    <failure message="assertion failed at src/lib.rs:42">trace</failure>
                </testcase>
               </testsuite></testsuites>"#,
        )
        .unwrap();
        write_config(
            dir.path(),
            r#"[{"name": "t", "runner": "custom",
                "report": {"format": "junit", "path": "junit.xml"},
                "modes": {"full": {"command": "exit 1"}}}]"#,
        );

        let outcome = run_gate(dir.path(), "full").unwrap();
        match outcome {
            GateOutcome::Completed {
                results, passed, ..
            } => {
                assert!(!passed);
                let rep = results[0]
                    .report
                    .as_ref()
                    .expect("failed target must carry a parsed report");
                assert_eq!(rep.passed, 1);
                assert_eq!(rep.failed, 1);
                assert_eq!(rep.failures.len(), 1);
                assert!(
                    rep.failures[0].id.contains("test_bad"),
                    "{:?}",
                    rep.failures[0]
                );
                assert_eq!(rep.failures[0].location.as_deref(), Some("src/lib.rs:42"));
            }
            GateOutcome::NoTargets { reason } => panic!("expected run, got NoTargets: {reason}"),
        }
    }

    #[test]
    fn failed_target_without_report_falls_back_to_raw() {
        // No report block configured → report is None; the CLI falls back to
        // raw stdout/stderr. Guards the fallback path.
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), &format!("[{}]", echo_target("bad", "exit 3")));

        let outcome = run_gate(dir.path(), "full").unwrap();
        match outcome {
            GateOutcome::Completed { results, .. } => {
                assert!(results[0].report.is_none(), "no report artifact → None");
            }
            GateOutcome::NoTargets { reason } => panic!("expected run, got NoTargets: {reason}"),
        }
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

    fn essential_full_target(name: &str, command: &str) -> String {
        format!(
            r#"{{"name": "{name}", "runner": "custom", "modes": {{"essential": {{"command": "{command}"}}, "full": {{"command": "{command}"}}}}}}"#
        )
    }

    #[test]
    fn quick_resolves_to_essential_on_template_config() {
        // Shipped templates (and stack detection) define `essential`/`full`,
        // never `quick`. A `--mode quick` run must resolve to `essential` and
        // actually execute. This no-op fixture is explicitly light so the
        // enclosing gate's reservation does not prevent testing mode resolution.
        let dir = tempfile::tempdir().unwrap();
        write_config(
            dir.path(),
            r#"[{"name":"ok","runner":"custom","heavy":false,"modes":{"essential":{"command":"exit 0"},"full":{"command":"exit 0"}}}]"#,
        );

        let outcome = run_gate(dir.path(), "quick").unwrap();
        match outcome {
            GateOutcome::Completed {
                results, passed, ..
            } => {
                assert!(passed, "quick must run the essential command set");
                assert_eq!(results.len(), 1);
                assert!(results[0].passed());
            }
            GateOutcome::NoTargets { reason } => {
                panic!("quick must resolve to essential and run, got NoTargets: {reason}")
            }
        }
    }

    #[test]
    fn essential_mode_is_accepted_and_runs() {
        let dir = tempfile::tempdir().unwrap();
        write_config(
            dir.path(),
            &format!("[{}]", essential_full_target("ok", "exit 0")),
        );

        let outcome = run_gate(dir.path(), "essential").unwrap();
        assert!(
            matches!(outcome, GateOutcome::Completed { passed: true, .. }),
            "essential mode must run, got {outcome:?}"
        );
    }

    #[test]
    fn quick_kept_when_config_defines_quick() {
        // A config that actually defines `quick` keeps it — no aliasing.
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), &format!("[{}]", echo_target("ok", "exit 0")));

        let outcome = run_gate(dir.path(), "quick").unwrap();
        assert!(
            matches!(outcome, GateOutcome::Completed { passed: true, .. }),
            "explicit quick mode must run its own command, got {outcome:?}"
        );
    }

    #[test]
    fn quick_without_essential_is_loud_no_op() {
        // Only `full` is defined: `quick` has nothing to alias onto, so it
        // stays `quick` and surfaces loudly rather than silently mapping.
        let dir = tempfile::tempdir().unwrap();
        write_config(
            dir.path(),
            r#"[{"name": "only-full", "runner": "custom", "modes": {"full": {"command": "exit 0"}}}]"#,
        );

        let outcome = run_gate(dir.path(), "quick").unwrap();
        match outcome {
            GateOutcome::NoTargets { reason } => assert!(reason.contains("quick"), "{reason}"),
            GateOutcome::Completed { .. } => panic!("expected loud NoTargets, got Completed"),
        }
    }

    #[test]
    fn resolve_mode_aliases_quick_only_when_unambiguous() {
        let parse = |modes: &[&str]| -> TargetConfig {
            let entries: Vec<String> = modes
                .iter()
                .map(|m| format!(r#""{m}": {{"command": "exit 0"}}"#))
                .collect();
            serde_json::from_str(&format!(
                r#"{{"name": "t", "runner": "custom", "modes": {{{}}}}}"#,
                entries.join(", ")
            ))
            .unwrap()
        };

        let essential_full = [parse(&["essential", "full"])];
        assert_eq!(resolve_mode("quick", &essential_full), "essential");
        assert_eq!(resolve_mode("full", &essential_full), "full");
        assert_eq!(resolve_mode("essential", &essential_full), "essential");

        assert_eq!(resolve_mode("quick", &[parse(&["quick", "full"])]), "quick");
        assert_eq!(resolve_mode("quick", &[parse(&["full"])]), "quick");
    }

    #[test]
    fn full_mode_coverage_shortfall_fails_the_gate() {
        // A target with coverage config + an lcov artifact below threshold must
        // FAIL the gate while surfacing the shortfall (codex pre-flip review:
        // thresholds were collected but never enforced).
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("cov.lcov"),
            "SF:src/lib.rs\nDA:1,1\nDA:2,0\nend_of_record\n",
        )
        .unwrap();
        write_config(
            dir.path(),
            r#"[{"name": "cov", "runner": "custom",
                "modes": {"full": {"command": "exit 0"}, "essential": {"command": "exit 0"}},
                "coverage": {"format": "lcov", "path": "cov.lcov",
                             "rules": [{"scope": "per_file", "minimum": 85}]}}]"#,
        );

        let outcome = run_gate(dir.path(), "full").unwrap();
        match outcome {
            GateOutcome::Completed {
                passed, coverage, ..
            } => {
                assert!(!passed, "a missed coverage threshold must fail the gate");
                assert_eq!(coverage.len(), 1);
                let cov = &coverage[0];
                assert_eq!(cov.target, "cov");
                assert_eq!(cov.overall_percent, Some(50.0));
                assert_eq!(cov.thresholds_failed, 1);
                assert_eq!(cov.failing_files.len(), 1);
                assert_eq!(cov.failing_files[0].0, "src/lib.rs");
            }
            GateOutcome::NoTargets { reason } => panic!("expected run, got NoTargets: {reason}"),
        }
    }

    #[test]
    fn aggregate_coverage_scopes_fail_the_gate() {
        for scope in ["global", "per_package", "per_module"] {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(
                dir.path().join("cov.lcov"),
                "SF:src/lib.rs\nDA:1,1\nDA:2,0\nend_of_record\n",
            )
            .unwrap();
            write_config(
                dir.path(),
                &format!(
                    r#"[{{"name": "cov", "runner": "custom",
                        "modes": {{"full": {{"command": "exit 0"}}}},
                        "coverage": {{"format": "lcov", "path": "cov.lcov",
                        "rules": [{{"scope": "{scope}", "minimum": 85}}]}}}}]"#
                ),
            );

            let outcome = run_gate(dir.path(), "full").unwrap();
            let GateOutcome::Completed {
                passed, coverage, ..
            } = outcome
            else {
                panic!("expected completed gate for {scope}");
            };
            assert!(!passed, "{scope} shortfall must fail the gate");
            assert_eq!(coverage[0].thresholds_failed, 1, "scope {scope}");
        }
    }

    #[test]
    fn quick_mode_collects_no_coverage() {
        // Coverage is a full-mode concern; quick(->essential) runs skip it.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("cov.lcov"),
            "SF:src/lib.rs\nDA:1,1\nend_of_record\n",
        )
        .unwrap();
        write_config(
            dir.path(),
            r#"[{"name": "cov", "runner": "custom",
                "modes": {"full": {"command": "exit 0"}, "essential": {"command": "exit 0"}},
                "coverage": {"format": "lcov", "path": "cov.lcov",
                             "rules": [{"scope": "per_file", "minimum": 85}]}}]"#,
        );

        let outcome = run_gate(dir.path(), "quick").unwrap();
        match outcome {
            GateOutcome::Completed { coverage, .. } => assert!(coverage.is_empty()),
            GateOutcome::NoTargets { reason } => panic!("expected run, got NoTargets: {reason}"),
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
    #[test]
    fn config_evidence_digest_includes_defaults() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".codeflow")).unwrap();
        let path = dir.path().join(TEST_CONFIG_PATH);
        let name = format!("digest-{}", uuid::Uuid::new_v4());
        for minimum in [80, 90] {
            let config = serde_json::json!({
                "schema_version":"1.0",
                "defaults":{"coverage":[{"scope":"global","minimum":minimum}]},
                "targets":[{"name":name,"runner":"custom","modes":{"quick":{"command":"echo pass"}}}]
            });
            std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
            assert!(run_gate(dir.path(), "quick").unwrap().allows_proceed());
        }
        let digests: std::collections::BTreeSet<String> =
            std::fs::read_dir(super::delivery::target_dir(dir.path()).join("codeflow-gate"))
                .unwrap()
                .filter_map(|entry| std::fs::read(entry.ok()?.path().join("run.json")).ok())
                .filter_map(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .filter(|artifact| artifact["targets"][0]["name"] == name)
                .map(|artifact| artifact["config_digest"].as_str().unwrap().to_owned())
                .collect();
        assert_eq!(
            digests.len(),
            2,
            "defaults are part of the evidence identity"
        );
    }

    /// Issue 79: a path that is not valid UTF-8 is left out of a target's
    /// environment, not written as its lossy spelling.
    #[cfg(unix)]
    #[test]
    fn a_target_dir_that_is_not_utf8_is_not_written_into_the_environment() {
        use std::os::unix::ffi::OsStrExt as _;
        let target: TargetConfig = serde_json::from_str(
            r#"{"name": "t", "runner": "custom", "modes": {"full": {"command": "exit 0"}}}"#,
        )
        .unwrap();
        let mut targets = [target];
        let root = Path::new("/project");
        let odd = Path::new(std::ffi::OsStr::from_bytes(b"/tmp/caf\xe9"));
        let run_dir = Path::new("/project/target/run-1");
        configure_run_targets(&mut targets, root, odd, run_dir, false).unwrap();
        assert!(!targets[0].env.contains_key("CARGO_LLVM_COV_TARGET_DIR"));
        assert_eq!(
            targets[0]
                .env
                .get("CODEFLOW_GATE_RUN_ID")
                .map(String::as_str),
            Some("run-1")
        );
    }
}

#[cfg(all(test, unix))]
mod r22_regressions {
    use super::*;

    #[test]
    fn r22_resolved_non_utf8_output_refuses() {
        use std::os::unix::ffi::OsStrExt;
        let target: TargetConfig = serde_json::from_str(r#"{"name":"t","runner":"custom","outputs":["target/out"],"modes":{"quick":{"command":"true"}}}"#).unwrap();
        let root = Path::new("/repo");
        let odd = Path::new(std::ffi::OsStr::from_bytes(b"/tmp/odd\xff"));
        assert!(configure_run_targets(&mut [target.clone()], root, odd, root, false).is_err());
        assert!(configure_run_targets(&mut [target], root, root, root, false).is_ok());
    }
}

#[cfg(all(test, unix))]
mod temporary_directory_tests {
    #[test]
    fn gate_output_refuses_planted_run_directory_link() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let path = root.path().join("codeflow-gate/run-id");
        std::fs::create_dir(path.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(outside.path(), &path).unwrap();
        assert!(super::create_run_directory(root.path(), "codeflow-gate/run-id").is_err());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
        assert!(std::fs::symlink_metadata(path).unwrap().is_symlink());
    }

    #[test]
    fn gate_output_refuses_planted_container_link() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let path = root.path().join("codeflow-gate");
        std::os::unix::fs::symlink(outside.path(), &path).unwrap();
        assert!(super::create_run_directory(root.path(), "codeflow-gate/run-id").is_err());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
        assert!(std::fs::symlink_metadata(path).unwrap().is_symlink());
    }
}

/// TSK-1044 AC-2: the durable copy writes only inside the evidence folder,
/// whatever links, hard links or special files are planted on either side.
#[cfg(all(test, unix))]
mod evidence_containment_tests {
    use super::{copy_evidence, copy_folder};
    use crate::contained::Tree;
    use std::path::Path;

    /// A build-output root with a two-file run folder, a home, and an
    /// outside folder holding the victim file.
    struct Setup {
        target: tempfile::TempDir,
        home: tempfile::TempDir,
        outside: tempfile::TempDir,
    }

    impl Setup {
        fn new() -> Self {
            let setup = Self {
                target: tempfile::tempdir().unwrap(),
                home: tempfile::tempdir().unwrap(),
                outside: tempfile::tempdir().unwrap(),
            };
            let run = setup.target.path().join("codeflow-gate/run");
            std::fs::create_dir_all(run.join("check")).unwrap();
            std::fs::write(run.join("run.json"), "{\"passed\":true}").unwrap();
            std::fs::write(run.join("check/stdout.log"), "output").unwrap();
            std::fs::write(setup.outside.path().join("victim"), "victim").unwrap();
            setup
        }

        fn copy(&self) -> Result<(), crate::testing::error::TestingError> {
            copy_evidence(
                self.target.path(),
                "codeflow-gate/run",
                self.home.path(),
                "gate-runs/repo/run",
            )
        }

        fn victim(&self) -> &Path {
            self.outside.path()
        }

        /// The outside folder still holds only the unchanged victim file.
        fn outside_unchanged(&self) {
            let names: Vec<_> = std::fs::read_dir(self.victim())
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            assert_eq!(names, ["victim"], "something was written outside");
            assert_eq!(
                std::fs::read(self.victim().join("victim")).unwrap(),
                b"victim"
            );
        }
    }

    #[test]
    fn a_plain_run_folder_is_copied_byte_for_byte() {
        let setup = Setup::new();
        setup.copy().unwrap();
        let copied = setup.home.path().join("gate-runs/repo/run");
        assert_eq!(
            std::fs::read(copied.join("run.json")).unwrap(),
            b"{\"passed\":true}"
        );
        assert_eq!(
            std::fs::read(copied.join("check/stdout.log")).unwrap(),
            b"output"
        );
    }

    /// The original finding: a link at a destination ancestor or at the
    /// run's own destination folder redirected the copy outside.
    #[test]
    fn a_link_at_any_destination_folder_is_refused_and_named() {
        for planted in ["gate-runs", "gate-runs/repo", "gate-runs/repo/run"] {
            let setup = Setup::new();
            let link = setup.home.path().join(planted);
            std::fs::create_dir_all(link.parent().unwrap()).unwrap();
            std::os::unix::fs::symlink(setup.victim(), &link).unwrap();
            let error = setup.copy().unwrap_err().to_string();
            assert!(
                error.contains("gate evidence refused at"),
                "{planted}: {error}"
            );
            assert!(error.contains(planted), "{planted}: {error}");
            setup.outside_unchanged();
            assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
        }
    }

    /// A name raced in at a final file after its folder was made: a link or
    /// a hard link to the victim is refused, never written through.
    #[test]
    fn a_raced_link_or_hard_link_at_a_final_file_is_never_written_through() {
        for hard in [false, true] {
            let setup = Setup::new();
            let destination = setup.home.path().join("gate-runs/repo/run");
            std::fs::create_dir_all(&destination).unwrap();
            let victim = setup.victim().join("victim");
            if hard {
                std::fs::hard_link(&victim, destination.join("run.json")).unwrap();
            } else {
                std::os::unix::fs::symlink(&victim, destination.join("run.json")).unwrap();
            }
            let source = Tree::open(setup.target.path()).unwrap();
            let home = Tree::open(setup.home.path()).unwrap();
            let (_, path, error) =
                copy_folder(&source, "codeflow-gate/run", &home, "gate-runs/repo/run").unwrap_err();
            assert_eq!(path, "gate-runs/repo/run/run.json", "{error}");
            assert_eq!(std::fs::read(&victim).unwrap(), b"victim", "hard {hard}");
        }
    }

    /// A link, a hard link or a special file a target planted in the run
    /// folder is refused, so nothing outside is copied into the evidence.
    #[test]
    fn links_hard_links_and_special_files_in_the_run_folder_are_refused() {
        for planted in ["link", "folder-link", "hard-link", "fifo"] {
            let setup = Setup::new();
            let path = setup.target.path().join("codeflow-gate/run/check/planted");
            let victim = setup.victim().join("victim");
            match planted {
                "link" => std::os::unix::fs::symlink(&victim, &path).unwrap(),
                "folder-link" => std::os::unix::fs::symlink(setup.victim(), &path).unwrap(),
                "hard-link" => {
                    // The victim must share the run folder's file system.
                    let local = setup.target.path().join("victim");
                    std::fs::write(&local, "victim").unwrap();
                    std::fs::hard_link(&local, &path).unwrap();
                }
                _ => {
                    let name = std::ffi::CString::new(std::os::unix::ffi::OsStrExt::as_bytes(
                        path.as_os_str(),
                    ))
                    .unwrap();
                    // SAFETY: valid NUL-terminated fixture path.
                    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
                }
            }
            let error = setup.copy().unwrap_err().to_string();
            assert!(
                error.contains("codeflow-gate/run/check/planted"),
                "{planted}: {error}"
            );
            setup.outside_unchanged();
            let copied = setup.home.path().join("gate-runs/repo/run/check/planted");
            assert!(
                std::fs::symlink_metadata(copied).is_err(),
                "{planted} copied"
            );
        }
    }
}

/// TSK-1044 AC-2 on Windows: a junction at a destination folder is refused
/// and nothing is written where it points.
#[cfg(all(test, windows))]
mod evidence_containment_windows_tests {
    #[test]
    fn a_junction_at_any_destination_folder_is_refused() {
        for planted in ["gate-runs", "gate-runs/repo"] {
            let target = tempfile::tempdir().unwrap();
            let home = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let run = target.path().join("codeflow-gate/run");
            std::fs::create_dir_all(&run).unwrap();
            std::fs::write(run.join("run.json"), "{}").unwrap();
            let link = home.path().join(planted);
            std::fs::create_dir_all(&link).unwrap();
            let redirect =
                crate::bounded_file::confined::windows::tests::junction(&link, outside.path())
                    .unwrap();
            let error = super::copy_evidence(
                target.path(),
                "codeflow-gate/run",
                home.path(),
                "gate-runs/repo/run",
            )
            .unwrap_err()
            .to_string();
            assert!(
                error.contains("gate evidence refused at"),
                "{planted}: {error}"
            );
            assert_eq!(
                std::fs::read_dir(outside.path()).unwrap().count(),
                0,
                "{planted}: nothing outside"
            );
            drop(redirect);
        }
    }

    #[test]
    fn a_junction_in_the_run_folder_is_refused() {
        let target = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("victim"), "victim").unwrap();
        let planted = target.path().join("codeflow-gate/run/planted");
        std::fs::create_dir_all(&planted).unwrap();
        let redirect =
            crate::bounded_file::confined::windows::tests::junction(&planted, outside.path())
                .unwrap();
        assert!(super::copy_evidence(
            target.path(),
            "codeflow-gate/run",
            home.path(),
            "gate-runs/repo/run",
        )
        .is_err());
        assert!(!home
            .path()
            .join("gate-runs/repo/run/planted/victim")
            .exists());
        drop(redirect);
    }
}

/// TSK-1044 review 1: a link planted at the build-output folder or at the
/// `CodeFlow` home itself is refused before any write, never trusted as a
/// root, so the folder it points at stays unchanged.
#[cfg(all(test, unix))]
mod evidence_root_link_tests {
    #[test]
    fn a_link_at_the_build_output_folder_is_refused_before_any_write() {
        let project = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".codeflow")).unwrap();
        std::fs::write(
            project.path().join(".codeflow/test-config.json"),
            r#"{"schema_version":"1.0","targets":[{"name":"t","runner":"custom","modes":{"full":{"command":"true"}}}]}"#,
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path(), project.path().join("target")).unwrap();
        let error = super::run_gate(project.path(), "full")
            .unwrap_err()
            .to_string();
        assert!(error.contains("gate evidence refused at"), "{error}");
        assert!(error.contains("target is a symbolic link"), "{error}");
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_link_at_the_home_folder_is_refused_by_the_durable_copy() {
        let target = tempfile::tempdir().unwrap();
        let parent = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(target.path().join("codeflow-gate/run")).unwrap();
        std::fs::write(target.path().join("codeflow-gate/run/run.json"), "{}").unwrap();
        let home = parent.path().join(".codeflow");
        std::os::unix::fs::symlink(outside.path(), &home).unwrap();
        let error = super::copy_evidence(
            target.path(),
            "codeflow-gate/run",
            &home,
            "gate-runs/repo/run",
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains(".codeflow is a symbolic link"), "{error}");
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    }
}

/// TSK-1044 review 1 on Windows: a hard-linked file in the run folder is
/// refused by its link count, read from the opened handle.
#[cfg(all(test, windows))]
mod evidence_hard_link_windows_tests {
    #[test]
    fn a_hard_link_in_the_run_folder_is_refused() {
        let target = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let run = target.path().join("codeflow-gate/run");
        std::fs::create_dir_all(&run).unwrap();
        let victim = target.path().join("victim");
        std::fs::write(&victim, "victim").unwrap();
        std::fs::hard_link(&victim, run.join("leak")).unwrap();
        let error = super::copy_evidence(
            target.path(),
            "codeflow-gate/run",
            &home.path().join(".codeflow"),
            "gate-runs/repo/run",
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("hard link"), "{error}");
        assert!(!home
            .path()
            .join(".codeflow/gate-runs/repo/run/leak")
            .exists());
    }
}
