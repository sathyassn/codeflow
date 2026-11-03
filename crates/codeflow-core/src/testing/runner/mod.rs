//! Per-target shell command runner.
//!
//! Spawns each enabled target's mode-specific command via `std::process::Command`
//! in the target's cwd, capturing stdout/stderr.

use std::fmt::Write as _;
use std::io::Read;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::testing::config::{CommandShell, Tag, TargetConfig};
use crate::testing::error::TestingError;

#[cfg(unix)]
mod group_watch;
#[cfg(windows)]
mod target_job;

#[cfg(any(windows, test))]
#[derive(Debug)]
enum AdoptError {
    Job(std::io::Error),
    Resume(std::io::Error),
}

#[cfg(any(windows, test))]
fn adopt_or_stop<T>(
    result: Result<T, AdoptError>,
    target_name: &str,
    end_child: impl FnOnce(),
) -> Result<T, TestingError> {
    result.map_err(|error| {
        end_child();
        let (step, source) = match error {
            AdoptError::Job(source) => ("join its kill-on-close job", source),
            AdoptError::Resume(source) => ("resume", source),
        };
        TestingError::CommandSpawnError {
            target: target_name.to_string(),
            message: format!("could not {step} target '{target_name}': {source}"),
        }
    })
}

/// Default per-target wall-clock timeout, applied when a target omits
/// `timeout_seconds`. A hanging test target must never block the gate or
/// pre-push forever, so every target has a finite ceiling.
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 600;

/// Exit code reported for a target killed after exceeding its timeout. Matches
/// the GNU `timeout(1)` convention so the failure reads unambiguously.
const TIMEOUT_EXIT_CODE: i32 = 124;

/// Polling interval while waiting for a target command to finish.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Maximum retained bytes for each child output stream. Readers keep the tail
/// so the most recent failure context survives noisy commands.
const OUTPUT_CAPTURE_LIMIT: usize = 1024 * 1024;

/// Result of running a single target's test command.
#[derive(Debug, Clone)]
pub struct TargetRunResult {
    pub target_name: String,
    pub exit_code: i32,
    /// Failure detail when no ordinary process exit code explains the result.
    pub error: Option<String>,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub duration_ms: u64,
    pub report_path: Option<PathBuf>,
    pub coverage_path: Option<PathBuf>,
    /// Per-target comparison evidence; absent for an execution outside the gate.
    pub evidence: Option<serde_json::Value>,
}

/// Run a single target's command for the specified mode.
///
/// # Errors
///
/// Returns `TestingError::ModeNotConfigured` if the target doesn't have the mode.
/// Returns `TestingError::CommandSpawnError` if the command cannot be spawned.
pub fn run_target(
    target: &TargetConfig,
    mode: &str,
    project_dir: &Path,
) -> Result<TargetRunResult, TestingError> {
    let mode_cmd = target
        .modes
        .get(mode)
        .ok_or_else(|| TestingError::ModeNotConfigured {
            target: target.name.clone(),
            mode: mode.to_string(),
        })?;

    let cwd = resolve_cwd(project_dir, target.cwd.as_deref())?;

    let timeout_secs = target.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT_SECONDS);
    let timeout = Duration::from_secs(timeout_secs);

    let start = Instant::now();
    let outcome = spawn_command(target, &mode_cmd.command, &cwd, timeout)?;
    let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

    let report_path = target.report.as_ref().map(|r| cwd.join(&r.path));
    let coverage_path = target.coverage.as_ref().map(|c| cwd.join(&c.path));

    // On timeout the target FAILS loudly: force a non-zero exit code and append
    // a message naming the timeout and the `timeout_seconds` config field, while
    // preserving whatever partial output was captured before the kill.
    let (exit_code, stderr) = if outcome.timed_out {
        (
            TIMEOUT_EXIT_CODE,
            format!(
                "{}\n[codeflow test] TIMEOUT: target '{}' exceeded its {timeout_secs}s wall-clock \
                 limit and was killed. Raise the `timeout_seconds` field for this target if the \
                 command legitimately needs longer.\n",
                outcome.stderr,
                crate::git::display_text(&target.name),
            ),
        )
    } else {
        (outcome.exit_code, outcome.stderr)
    };

    let error = if outcome.timed_out {
        Some(format!("timeout after {timeout_secs}s"))
    } else {
        outcome
            .signal
            .map(|signal| format!("killed by signal {signal}"))
    };

    Ok(TargetRunResult {
        error,
        evidence: None,
        target_name: target.name.clone(),
        exit_code,
        stdout: outcome.stdout,
        stderr,
        stdout_truncated: outcome.stdout_truncated,
        stderr_truncated: outcome.stderr_truncated,
        duration_ms,
        report_path,
        coverage_path,
    })
}

/// Run all enabled targets for a mode, sequentially or in parallel.
///
/// Tag filtering composes with target-name filtering: a target is included
/// when it passes BOTH the name filter (`only`/`skip`) AND the tag filter
/// (`only_tags`/`skip_tags`). A target is considered "tagged T" when T appears
/// in the union of its target-level `tags` and any of its `test_files` tags.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn run_all_targets(
    targets: &[TargetConfig],
    mode: &str,
    project_dir: &Path,
    parallel: bool,
    fail_fast: bool,
    only: &[String],
    skip: &[String],
    only_tags: &[Tag],
    skip_tags: &[Tag],
) -> Vec<Result<TargetRunResult, TestingError>> {
    run_all_targets_with_ci(
        targets,
        mode,
        project_dir,
        parallel,
        fail_fast,
        only,
        skip,
        only_tags,
        skip_tags,
        is_ci_environment(),
    )
}

#[allow(clippy::too_many_arguments)]
fn run_all_targets_with_ci(
    targets: &[TargetConfig],
    mode: &str,
    project_dir: &Path,
    parallel: bool,
    fail_fast: bool,
    only: &[String],
    skip: &[String],
    only_tags: &[Tag],
    skip_tags: &[Tag],
    in_ci: bool,
) -> Vec<Result<TargetRunResult, TestingError>> {
    let filtered: Vec<&TargetConfig> = targets
        .iter()
        .filter(|t| t.enabled)
        .filter(|t| {
            if only.is_empty() {
                true
            } else {
                only.iter().any(|o| o == &t.name)
            }
        })
        .filter(|t| !skip.iter().any(|s| s == &t.name))
        .filter(|t| target_matches_tag_filter(t, only_tags, skip_tags))
        .filter(|t| t.modes.contains_key(mode))
        .filter(|t| {
            // ci_skip: targets opted out when running under CI. Log the skip
            // so reviewers see the decision in job output; returning false
            // removes the target from execution entirely.
            if in_ci && t.ci_skip == Some(true) {
                progress!(
                    "[codeflow test] Skipping target '{}' in CI: {}",
                    t.name,
                    t.ci_skip_reason.as_deref().unwrap_or("ci_skip=true"),
                );
                false
            } else {
                true
            }
        })
        .collect();

    if parallel {
        run_parallel(&filtered, mode, project_dir)
    } else {
        run_sequential(&filtered, mode, project_dir, fail_fast)
    }
}

/// Compute the effective tag set for a target: the union of `target.tags` and
/// every entry in `target.test_files[*].tags`.
///
/// Returned in a stable, deduplicated order to support diffable output in
/// future tooling.
#[must_use]
pub fn target_tags(target: &TargetConfig) -> Vec<Tag> {
    let mut set: std::collections::BTreeSet<Tag> = target.tags.iter().copied().collect();
    for entry in &target.test_files {
        set.extend(entry.tags.iter().copied());
    }
    set.into_iter().collect()
}

/// Determine whether a target passes the tag filter.
///
/// Composition rules:
///
/// * `only_tags = []` → no positive filter (accept).
/// * `only_tags = [..]` → target must have at least one of these tags.
/// * `skip_tags = []` → no negative filter.
/// * `skip_tags = [..]` → target must NOT have ANY of these tags.
/// * `only_tags` and `skip_tags` may be combined; skip wins over only when
///   both match (e.g. `--only-tag critical --skip-tag critical` excludes
///   critical targets).
/// * Untagged targets (no tags anywhere) pass when `only_tags` is empty and
///   survive negative filters. They are EXCLUDED when `only_tags` is non-empty.
fn target_matches_tag_filter(target: &TargetConfig, only_tags: &[Tag], skip_tags: &[Tag]) -> bool {
    let tags = target_tags(target);

    if !skip_tags.is_empty() && tags.iter().any(|t| skip_tags.contains(t)) {
        return false;
    }

    if only_tags.is_empty() {
        return true;
    }

    tags.iter().any(|t| only_tags.contains(t))
}

/// The stderr line written as a target starts (TSK-094), so a killed gate's
/// log names the target it died in. One `eprintln!` holds the stderr lock for
/// the whole line, so parallel targets never interleave inside a line; stdout
/// and the final summary are unchanged.
#[must_use]
pub fn start_line(target: &str, mode: &str) -> String {
    format!("[codeflow test] starting target '{target}' ({mode} mode)")
}

fn announce_start(target: &TargetConfig, mode: &str) {
    progress!("{}", start_line(&target.name, mode));
}

fn run_sequential(
    targets: &[&TargetConfig],
    mode: &str,
    project_dir: &Path,
    fail_fast: bool,
) -> Vec<Result<TargetRunResult, TestingError>> {
    let mut results = Vec::new();
    for target in targets {
        announce_start(target, mode);
        let result = run_target(target, mode, project_dir);
        let should_stop = fail_fast && result.as_ref().is_ok_and(|r| r.exit_code != 0);
        results.push(result);
        if should_stop {
            break;
        }
    }
    results
}

fn run_parallel(
    targets: &[&TargetConfig],
    mode: &str,
    project_dir: &Path,
) -> Vec<Result<TargetRunResult, TestingError>> {
    use std::thread;

    let handles: Vec<_> = targets
        .iter()
        .map(|target| {
            let target = (*target).clone();
            let mode = mode.to_string();
            let project_dir = project_dir.to_path_buf();
            thread::spawn(move || {
                announce_start(&target, &mode);
                run_target(&target, &mode, &project_dir)
            })
        })
        .collect();

    handles
        .into_iter()
        .zip(targets.iter())
        .map(|(h, target)| {
            h.join().unwrap_or_else(|panic_val| {
                let message = panic_val
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| panic_val.downcast_ref::<&str>().copied())
                    .unwrap_or("unknown panic")
                    .to_string();
                Err(TestingError::ParallelExecutionError {
                    target: target.name.clone(),
                    message,
                })
            })
        })
        .collect()
}

/// The worker budget of a heavy run that shares the host with `runs` heavy
/// runs, itself included: always bounded by the host's job budget, even for
/// the first run and when `worker_budget` is unset, which only caps it
/// further (TSK-1145).
fn run_budget(execution: &super::config::ExecutionConfig, runs: usize) -> usize {
    let host = super::capacity::host_jobs(execution.host_jobs);
    let jobs = super::capacity::run_jobs(host, execution.worker_budget, runs);
    if super::capacity::lowered(host, execution.worker_budget, runs) {
        progress!(
            "[codeflow test] {runs} heavy runs share this host's {host} jobs; this run uses {jobs}"
        );
    }
    jobs
}

/// Run a validated dependency graph with bounded concurrency and exclusive targets.
/// Prerequisite failures remain named results, never fresh passes.
#[must_use]
pub fn run_dependency_targets(
    targets: &[TargetConfig],
    mode: &str,
    project_dir: &Path,
    execution: &super::config::ExecutionConfig,
    run_dir: &Path,
    declarations: &[TargetConfig],
) -> Vec<Result<TargetRunResult, TestingError>> {
    run_dependency_targets_with_admission(
        targets,
        mode,
        project_dir,
        execution,
        run_dir,
        declarations,
        false,
    )
}

/// Run a gate's dependency graph, admitting heavy quick work through the shared lock.
#[must_use]
#[allow(clippy::too_many_lines)] // The dependency scheduler keeps admission and completion in one state machine.
pub fn run_dependency_targets_with_admission(
    targets: &[TargetConfig],
    mode: &str,
    project_dir: &Path,
    execution: &super::config::ExecutionConfig,
    run_dir: &Path,
    declarations: &[TargetConfig],
    quick: bool,
) -> Vec<Result<TargetRunResult, TestingError>> {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::mpsc::channel;
    let bound = parallel_bound(execution.parallel, execution.max_parallel);
    let mut admission: Option<super::gate_guard::GateLock> = None;
    let mut wait_started = None;
    // The jobs heavy targets share: the run's own budget, lowered when more
    // heavy runs share the host than its job budget holds (TSK-1145).
    let mut budget = super::gate_guard::held_runs().map_or(execution.worker_budget, |runs| {
        Some(run_budget(execution, runs))
    });
    let mut host_hold: Option<(usize, super::gate_guard::HostExclusive)> = None;
    let mut allocations = BTreeMap::<usize, usize>::new();
    let baseline = match super::delivery::tracked(project_dir) {
        Ok(snapshot) => snapshot,
        Err(error) => return vec![Err(error)],
    };
    let (tx, rx) = channel();
    let mut pending: BTreeSet<usize> = (0..targets.len()).collect();
    let mut running = BTreeSet::new();
    let mut completed = BTreeMap::<String, bool>::new();
    let mut results = BTreeMap::new();
    let mut exclusive_running = false;
    while !pending.is_empty() || !running.is_empty() {
        let mut waiting = false;
        let mut candidates: Vec<_> = pending.iter().copied().collect();
        // Admit independent light work first, including when a heavy refusal triggers fail_fast.
        if quick {
            candidates.sort_by_key(|&i| targets[i].heavy == Some(true));
        }
        for index in candidates {
            let target = &targets[index];
            let prerequisite_failed = target
                .requires
                .iter()
                .any(|r| completed.get(r) == Some(&false));
            if prerequisite_failed || (execution.fail_fast && completed.values().any(|ok| !ok)) {
                let reason = if prerequisite_failed {
                    "not run: prerequisite failed"
                } else {
                    "not run: fail_fast"
                };
                progress!("[codeflow test] {}: {reason}", target.name);
                let result = not_run(target, reason);
                completed.insert(target.name.clone(), false);
                results.insert(index, Ok(result));
                pending.remove(&index);
                continue;
            }
            if exclusive_running
                || running.len() >= bound
                || !target
                    .requires
                    .iter()
                    .all(|r| completed.get(r) == Some(&true))
            {
                continue;
            }
            if (target.exclusive || target.host_exclusive) && !running.is_empty() {
                break;
            }
            // A host-exclusive target needs the host, so it is admitted as heavy work.
            let heavy = target.heavy == Some(true) || target.host_exclusive;
            let free_workers =
                budget.map(|budget| budget.saturating_sub(allocations.values().sum()));
            // A declared width above the run's budget runs at the budget, so a
            // lowered budget never leaves a target waiting forever (R483-04).
            let need = budget.map_or(target.workers.unwrap_or(1), |budget| {
                target.workers.unwrap_or(1).clamp(1, budget.max(1))
            });
            if heavy && free_workers.is_some_and(|free| free < need) {
                continue;
            }
            if quick && heavy && admission.is_none() {
                // Heavy runs never wait for each other (TSK-1145): admission
                // refuses only while a host-exclusive target holds the host,
                // and then waits up to the run's bound.
                let bound = Duration::from_secs(execution.heavy_wait_seconds);
                match super::gate_guard::acquire_project_gate_lock(project_dir, "quick") {
                    Ok(lock) => {
                        for note in &lock.notes {
                            progress!("[codeflow test] {note}");
                        }
                        budget = Some(run_budget(execution, lock.runs));
                        admission = Some(lock);
                        wait_started = None;
                    }
                    Err(message) => {
                        let first_wait = wait_started.is_none();
                        let since = wait_started.get_or_insert_with(Instant::now);
                        if message.contains("host-exclusive") && since.elapsed() < bound {
                            if first_wait {
                                progress!(
                                    "[codeflow test] waiting up to {}s for heavy work: {message}",
                                    execution.heavy_wait_seconds
                                );
                            }
                            waiting = true;
                            continue;
                        }
                        let reason = format!("not run: heavy target refused: {message}");
                        progress!("[codeflow test] {}: {reason}", target.name);
                        results.insert(index, Ok(not_run(target, &reason)));
                        completed.insert(target.name.clone(), false);
                        pending.remove(&index);
                        continue;
                    }
                }
            }
            if target.host_exclusive {
                let bound = Duration::from_secs(execution.heavy_wait_seconds);
                match super::gate_guard::hold_host_exclusive(project_dir, bound) {
                    Ok(hold) => {
                        progress!(
                            "[codeflow test] {} runs alone on this host (host_exclusive)",
                            target.name
                        );
                        host_hold = Some((index, hold));
                    }
                    Err(message) => {
                        let reason = format!("not run: host-exclusive target refused: {message}");
                        progress!("[codeflow test] {}: {reason}", target.name);
                        results.insert(index, Ok(not_run(target, &reason)));
                        completed.insert(target.name.clone(), false);
                        pending.remove(&index);
                        continue;
                    }
                }
            }
            // Admission may have lowered the budget for this run.
            let free_workers =
                budget.map(|budget| budget.saturating_sub(allocations.values().sum()));
            let mut target = target.clone();
            // Heavy targets always take a share of the host-bounded budget. A
            // budget below what the run would otherwise use (its
            // `worker_budget`, else every core) also sizes each target not
            // declared light, so its tools do not take the whole host.
            let unbounded = execution.worker_budget.unwrap_or_else(|| {
                std::thread::available_parallelism().map_or(1, std::num::NonZero::get)
            });
            let sized =
                heavy || (target.heavy != Some(false) && budget.is_some_and(|b| b < unbounded));
            if sized {
                if let Some(free) = free_workers {
                    let ready = pending
                        .iter()
                        .filter(|&&i| {
                            targets[i].heavy == Some(true)
                                && targets[i]
                                    .requires
                                    .iter()
                                    .all(|r| completed.get(r) == Some(&true))
                        })
                        .count();
                    let shares = if target.exclusive || target.host_exclusive {
                        1
                    } else {
                        ready.min(bound - running.len()).max(1)
                    };
                    // Never above what is free: admission may have lowered
                    // the budget after the fit check (R483-04).
                    let workers = target
                        .workers
                        .map_or_else(|| (free / shares).max(1), |width| width.min(free).max(1));
                    allocations.insert(index, workers);
                    for key in ["CARGO_BUILD_JOBS", "NEXTEST_TEST_THREADS"] {
                        target.env.insert(key.into(), workers.to_string());
                    }
                    // A nextest slot can itself run a Rust test process or the shell oracle.
                    // Give those nested workers one thread so they do not multiply the budget.
                    target.env.insert("RUST_TEST_THREADS".into(), "1".into());
                    let oracle = if target.modes[mode].command.contains("nextest") {
                        1
                    } else {
                        workers
                    };
                    target
                        .env
                        .insert("CODEFLOW_ORACLE_WORKERS".into(), oracle.to_string());
                    progress!("[codeflow test] workers for '{}': jobs={workers}, nextest={workers}, rust-tests=1, oracle={oracle}", target.name);
                }
            }
            pending.remove(&index);
            running.insert(index);
            exclusive_running = target.exclusive || target.host_exclusive;
            let declarations = declarations.to_vec();
            let mode = mode.to_string();
            let root = project_dir.to_path_buf();
            let baseline = baseline.clone();
            let tx = tx.clone();
            let run_dir = run_dir.to_path_buf();
            std::thread::spawn(move || {
                announce_start(&target, &mode);
                let result = std::panic::catch_unwind(|| {
                    run_owed_target(&target, &mode, &root, &baseline, &run_dir, &declarations)
                })
                .unwrap_or_else(|_| {
                    Err(TestingError::ParallelExecutionError {
                        target: target.name.clone(),
                        message: "target worker panicked".into(),
                    })
                });
                let _ = tx.send((index, result));
            });
            if exclusive_running {
                break;
            }
        }
        if running.is_empty() {
            if waiting {
                std::thread::sleep(POLL_INTERVAL);
                continue;
            }
            if !pending.is_empty() {
                return vec![Err(super::delivery::invalid(
                    project_dir,
                    "unsatisfied prerequisites in selected graph",
                ))];
            }
            break;
        }
        let received = if waiting {
            rx.recv_timeout(POLL_INTERVAL).ok()
        } else {
            rx.recv().ok()
        };
        let Some((index, result)) = received else {
            if waiting {
                continue;
            }
            break;
        };
        running.remove(&index);
        allocations.remove(&index);
        if !running
            .iter()
            .any(|&i| targets[i].heavy == Some(true) || targets[i].host_exclusive)
        {
            admission = None;
        }
        if targets[index].exclusive || targets[index].host_exclusive {
            exclusive_running = false;
        }
        if host_hold.as_ref().is_some_and(|(held, _)| *held == index) {
            host_hold = None;
        }
        let passed = result.as_ref().is_ok_and(|r| r.exit_code == 0);
        progress!(
            "[codeflow test] completed target '{}': {}",
            targets[index].name,
            if passed { "passed" } else { "FAILED" }
        );
        completed.insert(targets[index].name.clone(), passed);
        let result = result.unwrap_or_else(|error| runner_failure(&targets[index], &error));
        results.insert(index, Ok(result));
    }
    results.into_values().collect()
}

fn parallel_bound(parallel: bool, configured: Option<usize>) -> usize {
    if parallel {
        configured.unwrap_or_else(|| {
            std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
        })
    } else {
        1
    }
}

// Keep the target identity when setup, artifact recording or the worker fails
// before it can return a command result.
fn runner_failure(target: &TargetConfig, error: &TestingError) -> TargetRunResult {
    let detail = super::evidence::redact(target, &error.to_string());
    not_run(target, &format!("runner error: {detail}"))
}

fn not_run(target: &TargetConfig, reason: &str) -> TargetRunResult {
    TargetRunResult {
        evidence: None,
        target_name: target.name.clone(),
        exit_code: -1,
        error: Some(reason.into()),
        stdout: String::new(),
        stderr: reason.into(),
        stdout_truncated: false,
        stderr_truncated: false,
        duration_ms: 0,
        report_path: None,
        coverage_path: None,
    }
}

// The two merged gate changes (TSK-331 admission, TSK-411 failure detail)
// meet here; one flow keeps each outcome next to the step that causes it.
#[allow(clippy::too_many_lines)]
fn run_owed_target(
    target: &TargetConfig,
    mode: &str,
    root: &Path,
    baseline: &std::collections::BTreeMap<String, String>,
    run_dir: &Path,
    declarations: &[TargetConfig],
) -> Result<TargetRunResult, TestingError> {
    // Paths a target names are reached through the anchored walk, never
    // following a link or junction at any component (TSK-1044).
    let (anchor, output) = super::delivery::target_anchor(root)?;
    let locate = |path: &Path| super::delivery::locate(root, &anchor, &output, path);
    // Held until this function returns, so the target's output is written
    // and collected before another run's same target can touch it.
    let _leases = evidence_leases(target, root, &anchor, &output)?;
    if let Some(report) = &target.report {
        if target.modes[mode].command.contains("nextest") {
            let path = resolve_cwd(root, target.cwd.as_deref())?.join(&report.path);
            locate(&path)
                .and_then(|(tree, relative)| tree.remove(&relative))
                .map_err(|error| {
                    TestingError::Io(std::io::Error::new(
                        error.kind(),
                        format!(
                            "stale report {} refused: {error}; nothing was removed through it",
                            path.display()
                        ),
                    ))
                })?;
        }
    }
    let declared = declarations
        .iter()
        .find(|t| t.name == target.name)
        .ok_or_else(|| super::delivery::invalid(root, "unknown target declaration"))?;
    let home = crate::registry::codeflow_home();
    let before = super::evidence::observe(root, declared, declarations, home.as_deref());
    let git_tree = super::evidence::git_tree(root);
    let mut result = run_target(target, mode, root).map_err(|error| {
        super::delivery::invalid(root, super::evidence::redact(declared, &error.to_string()))
    })?;
    let after = super::evidence::observe(root, declared, declarations, home.as_deref());
    result.evidence = Some(serde_json::to_value(super::evidence::Execution {
        name: target.name.clone(),
        mode: mode.into(),
        command: super::evidence::redact(declared, &target.modes[mode].command),
        git_tree,
        before,
        after,
    })?);

    if !target.outputs.is_empty() && result.exit_code == 0 {
        let mut failures = Vec::new();
        let after = super::delivery::tracked(root)?;
        let changed: Vec<_> = after
            .iter()
            .filter(|(p, hash)| baseline.get(*p) != Some(*hash))
            .map(|(p, _)| p.as_str())
            .collect();
        if !changed.is_empty() {
            failures.push(format!(
                "generation changed the candidate: {}",
                changed.join(", ")
            ));
            result.exit_code = 1;
            let _ = write!(
                result.stderr,
                "\ngeneration changed the candidate: {}",
                changed.join(", ")
            );
        }
        for output in &target.outputs {
            let failure = match locate(&root.join(output))
                .and_then(|(tree, relative)| tree.metadata(&relative))
            {
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    format!("producer output missing: {output}")
                }
                Err(error) => format!("producer output refused: {output}: {error}"),
            };
            result.exit_code = 1;
            let _ = write!(result.stderr, "\n{failure}");
            failures.push(failure);
        }
        if !failures.is_empty() {
            result.error = Some(super::evidence::redact(declared, &failures.join("; ")));
        }
    }
    let directory = if !target.name.is_empty()
        && target.name != "."
        && target.name != ".."
        && target
            .name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    {
        target.name.clone()
    } else {
        format!(
            "target-{}",
            &super::delivery::digest(target.name.as_bytes())[..16]
        )
    };
    // Every write below goes through the anchored walk the gate made the run
    // folder with, never following a link or junction, so a link another
    // target planted in the run folder is refused rather than written
    // through (TSK-1044).
    let (tree, run) = run_anchor(root, &anchor, &output, run_dir)?;
    let destination = run_dir.join(&directory);
    let saved_at = format!("{run}/{directory}");
    tree.create_dir_all(&saved_at)
        .map_err(|error| super::delivery::evidence_refused(tree.path(), &saved_at, &error))?;
    if let Some(path) = &result.report_path {
        let saved = format!("{saved_at}/junit.xml");
        if save_redacted_report(&locate, path, &tree, &saved, declared)? {
            result.report_path = Some(destination.join("junit.xml"));
        }
    }
    if let Some(path) = &result.coverage_path {
        let saved = format!("{saved_at}/coverage");
        if save_redacted_report(&locate, path, &tree, &saved, declared)? {
            result.coverage_path = Some(destination.join("coverage"));
        }
    }
    if target.heavy == Some(true) || target.host_exclusive {
        if let Some(workers) = target.env.get("CARGO_BUILD_JOBS") {
            let oracle = target
                .env
                .get("CODEFLOW_ORACLE_WORKERS")
                .map_or("unset", String::as_str);
            let _ = writeln!(result.stderr, "[codeflow test] workers: jobs={workers}, nextest={workers}, rust-tests=1, oracle={oracle}");
        }
    }
    result.stdout = super::evidence::redact(declared, &result.stdout);
    result.stderr = super::evidence::redact(declared, &result.stderr);
    for (name, text) in [
        ("stdout.log", &result.stdout),
        ("stderr.log", &result.stderr),
    ] {
        let log = format!("{saved_at}/{name}");
        tree.write(&log, text.as_bytes())
            .map_err(|error| super::delivery::evidence_refused(tree.path(), &log, &error))?;
    }
    Ok(result)
}

/// The outputs of `target` that every run of it writes at one fixed path,
/// each named with the tool that fixes it. Two runs that share a build
/// output folder (two gates in one worktree, or worktrees that name one
/// target folder) would otherwise remove, overwrite or collect each other's
/// files between production and collection (R483-03).
///
/// None of these tools can be pointed at a per-run path without a rebuild:
/// - `nextest` writes its `JUnit` report under its store folder, which the
///   profile in `.config/nextest.toml` fixes;
/// - cargo-llvm-cov sets its own `LLVM_PROFILE_FILE`, removes every profile
///   in its target folder before the run and merges every one after it;
/// - a configured report or coverage file is written wherever the target's
///   own command writes it.
///
/// Everything else a run writes is already its own: the run folder, its
/// `CODEFLOW_GATE_RESULTS` path and its saved reports carry the run id.
fn shared_outputs(target: &TargetConfig, root: &Path) -> Result<Vec<PathBuf>, TestingError> {
    let cwd = resolve_cwd(root, target.cwd.as_deref())?;
    let mut shared = Vec::new();
    if let Some(report) = &target.report {
        shared.push(cwd.join(&report.path));
    }
    if let Some(coverage) = &target.coverage {
        shared.push(cwd.join(&coverage.path));
    }
    if let Some(dir) = target.env.get("CARGO_LLVM_COV_TARGET_DIR") {
        shared.push(PathBuf::from(dir));
    }
    shared.sort();
    shared.dedup();
    Ok(shared)
}

/// Exclusive leases on the [`shared_outputs`] of `target`, one lock file per
/// output under `<build output>/codeflow-gate/leases/`, taken in a fixed
/// order so two runs never wait on each other in a cycle. A run whose output
/// another run is writing waits for that target to finish, at most the
/// waiting target's own `timeout_seconds`, then the target is reported not
/// run; runs with no output in common never wait.
/// # Errors
/// When a lease file cannot be opened, or a lease stays held past the bound.
fn evidence_leases(
    target: &TargetConfig,
    root: &Path,
    anchor: &Path,
    output: &str,
) -> Result<Vec<std::fs::File>, TestingError> {
    use fs2::FileExt;
    let shared = shared_outputs(target, root)?;
    if shared.is_empty() {
        return Ok(Vec::new());
    }
    let tree = crate::contained::Tree::open(anchor)?;
    let folder = format!("{output}/codeflow-gate/leases");
    tree.create_dir_all(&folder)
        .map_err(|error| super::delivery::evidence_refused(tree.path(), &folder, &error))?;
    let mut leases = Vec::new();
    for path in shared {
        let key = super::delivery::digest(path.as_os_str().as_encoded_bytes());
        let relative = format!("{folder}/{}.lock", &key[..32]);
        // Opened or created with read access only, as the gate locks are:
        // on Windows a creating handle with `DELETE` access makes every
        // other run's open fail with a sharing violation instead of waiting.
        let file = tree
            .create_read_only(&relative)
            .map_err(|error| super::delivery::evidence_refused(tree.path(), &relative, &error))?;
        if file.try_lock_exclusive().is_err() {
            let bound = target.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT_SECONDS);
            eprintln!(
                "[codeflow test] {}: another run is writing {}; waiting up to {bound}s for it",
                target.name,
                crate::git::display_path(&path)
            );
            let start = Instant::now();
            while file.try_lock_exclusive().is_err() {
                if start.elapsed() >= Duration::from_secs(bound) {
                    return Err(TestingError::Io(std::io::Error::other(format!(
                        "another run still wrote {} after {bound}s (the target's timeout_seconds); \
                         lease: {}",
                        crate::git::display_path(&path),
                        crate::git::display_path(&tree.path().join(&relative))
                    ))));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        leases.push(file);
    }
    Ok(leases)
}

/// The evidence anchor of the project at `root` and the run folder's path
/// beneath it (`<build output>/codeflow-gate/<run id>`), as the gate made it.
fn run_anchor(
    root: &Path,
    anchor: &Path,
    target: &str,
    run_dir: &Path,
) -> Result<(crate::contained::Tree, String), TestingError> {
    let run_id = run_dir
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| {
            super::delivery::invalid(
                root,
                format!("gate run folder {} is not valid UTF-8", run_dir.display()),
            )
        })?;
    Ok((
        crate::contained::Tree::open(anchor)?,
        format!("{target}/codeflow-gate/{run_id}"),
    ))
}

/// Copy a target's report into the run folder with environment inputs
/// redacted; `false` when the report is absent.
fn save_redacted_report(
    locate: &impl Fn(&Path) -> std::io::Result<(crate::contained::Tree, String)>,
    source: &Path,
    tree: &crate::contained::Tree,
    destination: &str,
    target: &TargetConfig,
) -> Result<bool, TestingError> {
    let refused = |error: std::io::Error| {
        TestingError::Io(std::io::Error::new(
            error.kind(),
            format!("report {} refused: {error}", source.display()),
        ))
    };
    let Some(mut file) = open_report(locate(source).map_err(refused)?).map_err(refused)? else {
        return Ok(false);
    };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(refused)?;
    let text = String::from_utf8(bytes).map_err(|_| {
        super::delivery::invalid(
            source,
            "report is not UTF-8; cannot redact environment inputs safely",
        )
    })?;
    let redacted = super::evidence::redact(target, &text);
    tree.write(destination, redacted.as_bytes())
        .map_err(|error| super::delivery::evidence_refused(tree.path(), destination, &error))?;
    Ok(true)
}

/// Open a target's report, located by [`super::delivery::locate`] beneath
/// the build-output folder or the project, through an anchored walk that
/// never follows a link or junction at any component (TSK-1044). The opened
/// file must be a regular file with one name, read from its handle on every
/// platform, so a link or hard link planted at the report path or above it
/// cannot copy another file into the evidence. A missing report is `None`.
fn open_report(
    (tree, relative): (crate::contained::Tree, String),
) -> std::io::Result<Option<std::fs::File>> {
    match tree.open_regular(&relative) {
        Ok(file) => {
            crate::contained::single_name(&file)?;
            Ok(Some(file))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Returns `true` when the process is running inside a CI environment.
///
/// Honours the de-facto standard `CI` environment variable: any non-empty
/// value that is not the literal strings `"false"` or `"0"` is treated as
/// "in CI". GitHub Actions, GitLab CI, `CircleCI`, and Buildkite all set
/// `CI=true`; developers running locally typically leave it unset.
#[must_use]
pub fn is_ci_environment() -> bool {
    std::env::var_os("CI")
        .is_some_and(|value| !value.is_empty() && value != "false" && value != "0")
}

#[cfg(test)]
fn is_ci_value(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.is_empty() && value != "false" && value != "0")
}

fn resolve_cwd(project_dir: &Path, target_cwd: Option<&str>) -> Result<PathBuf, TestingError> {
    match target_cwd {
        Some(cwd) if !cwd.is_empty() && cwd != "." => {
            let joined = project_dir.join(cwd);
            // Check for path traversal: reject if any component is ".."
            for component in std::path::Path::new(cwd).components() {
                if matches!(component, std::path::Component::ParentDir) {
                    return Err(TestingError::CwdEscapesRoot(cwd.to_string()));
                }
            }
            // Verify the resolved path is still under project_dir
            if !joined.starts_with(project_dir) {
                return Err(TestingError::CwdEscapesRoot(cwd.to_string()));
            }
            Ok(joined)
        }
        _ => Ok(project_dir.to_path_buf()),
    }
}

/// Outcome of running a single target command, capturing whatever output was
/// produced and whether the wall-clock timeout fired.
struct CommandOutcome {
    exit_code: i32,
    signal: Option<i32>,
    stdout: String,
    stderr: String,
    timed_out: bool,
    stdout_truncated: bool,
    stderr_truncated: bool,
}

fn spawn_command(
    target: &TargetConfig,
    command: &str,
    cwd: &Path,
    timeout: Duration,
) -> Result<CommandOutcome, TestingError> {
    let cmd = build_command(command, cwd, &target.env, &target.name, target.shell)?;
    spawn_prepared(
        cmd,
        &target.name,
        timeout,
        super::evidence::redacted_values(target).is_empty(),
        target.heavy == Some(true) || target.host_exclusive,
        false,
    )
}

/// Run exact command arguments under the caller's gate reservation.
/// # Errors
/// Returns an error if the command or its process-tree supervisor cannot start.
pub fn run_reserved_command(
    args: &[std::ffi::OsString],
    cwd: &Path,
    timeout: Duration,
    workers: Option<usize>,
) -> Result<TargetRunResult, TestingError> {
    let Some(program) = args.first() else {
        return Err(TestingError::CommandSpawnError {
            target: "reserve".into(),
            message: "a command is required".into(),
        });
    };
    // Only this very binary receives the lock descriptors (TSK-1128): it
    // makes them close-on-exec at start, so no other tool, script or daemon
    // can keep or unlock them. The command then runs that checked binary.
    let same_binary = this_binary_if_same(program, cwd);
    let hand_over = same_binary.is_some();
    let mut command = crate::git::process(
        same_binary
            .as_deref()
            .map_or(program.as_os_str(), Path::as_os_str),
    );
    command.args(&args[1..]).current_dir(cwd);
    if let Some(workers) = workers {
        let oracle = if args.iter().any(|arg| arg == "nextest") {
            1
        } else {
            workers
        };
        command
            .env("CARGO_BUILD_JOBS", workers.to_string())
            .env("NEXTEST_TEST_THREADS", workers.to_string())
            .env("RUST_TEST_THREADS", "1")
            .env("CODEFLOW_ORACLE_WORKERS", oracle.to_string());
        progress!("[codeflow reserve] workers: jobs={workers}, nextest={workers}, rust-tests=1, oracle={oracle}");
    }
    let start = Instant::now();
    let result = spawn_prepared(command, "reserve", timeout, true, true, hand_over)?;
    Ok(TargetRunResult {
        target_name: "reserve".into(),
        error: None,
        exit_code: if result.timed_out {
            TIMEOUT_EXIT_CODE
        } else {
            result.exit_code
        },
        stdout: result.stdout,
        stderr: result.stderr,
        stdout_truncated: result.stdout_truncated,
        stderr_truncated: result.stderr_truncated,
        duration_ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
        report_path: None,
        coverage_path: None,
        evidence: None,
    })
}

/// The running binary's path when `program`, resolved as exec would (a
/// path against `cwd`, a bare name through `PATH`) and with links followed,
/// is the same file as the running binary, compared by device and inode.
/// A script or any other program with the same name is not.
fn this_binary_if_same(program: &std::ffi::OsStr, cwd: &Path) -> Option<std::path::PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let identity = |path: &Path| std::fs::metadata(path).ok().map(|m| (m.dev(), m.ino()));
        let current = std::env::current_exe().ok()?;
        let named = Path::new(program);
        let resolved = if named.components().count() > 1 || named.is_absolute() {
            Some(cwd.join(named))
        } else {
            std::env::split_paths(&std::env::var_os("PATH")?)
                .map(|dir| {
                    if dir.is_absolute() {
                        dir
                    } else {
                        cwd.join(dir)
                    }
                })
                .map(|dir| dir.join(named))
                .find(|path| {
                    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.mode() & 0o111 != 0)
                })
        }?;
        (identity(&resolved)? == identity(&current)?).then_some(current)
    }
    #[cfg(not(unix))]
    {
        let _ = (program, cwd);
        None
    }
}

// The two merged gate changes (TSK-331 admission, TSK-411 failure detail)
// meet here; one flow keeps each outcome next to the step that causes it.
#[allow(clippy::too_many_lines)]
fn spawn_prepared(
    mut cmd: Command,
    target_name: &str,
    timeout: Duration,
    emit_progress: bool,
    supervised: bool,
    hand_over: bool,
) -> Result<CommandOutcome, TestingError> {
    #[cfg(not(unix))]
    let _ = hand_over;
    // Pipe output so it can be captured while we poll for the timeout. Reader
    // threads drain the pipes concurrently — without them a chatty command
    // could fill the pipe buffer and deadlock while we sit in try_wait().
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let spawn_err = |e: std::io::Error| TestingError::CommandSpawnError {
        target: target_name.to_string(),
        message: format!("{e}"),
    };

    // Run the command in its own process group (Unix) so a timeout can kill
    // the WHOLE tree. `sh -c` may fork the real command as a grandchild that
    // inherits the pipe write-ends; killing only `sh` then leaves the reader
    // threads blocked until the orphan exits (observed on Linux CI: a
    // timed-out `sleep 30` held the gate for the full 30s). The trade: a
    // terminal Ctrl-C no longer reaches the test child automatically —
    // acceptable, since the deadline below reaps the whole group either way.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    // On Windows the target starts suspended so it joins its job object
    // before it can start a child of its own (TSK-142 AC-3).
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(target_job::CREATE_SUSPENDED);
    }

    #[cfg(unix)]
    let mut watcher = if supervised {
        Some(group_watch::GroupWatch::prepare(&mut cmd, hand_over).map_err(spawn_err)?)
    } else {
        None
    };
    let mut child = cmd.spawn().map_err(spawn_err)?;
    #[cfg(unix)]
    if let Some(watcher) = &mut watcher {
        watcher.spawned();
    }
    // The target's process group (its pid, set above) outlives this process
    // if the gate is killed; the full-gate lock stays held while it runs.
    #[cfg(unix)]
    if let Err(error) = crate::testing::gate_guard::target_group_started(child.id()) {
        progress!("[codeflow test] warning: {error}");
    }
    // On Windows the target's tree ends with this process instead: the job
    // is closed when the gate exits or is killed, so the lock never frees
    // while a target of the gate runs.
    #[cfg(windows)]
    let target_job = adopt_or_stop(target_job::TargetJob::adopt(&child), target_name, || {
        let _ = child.kill();
        let _ = child.wait();
    })?;

    if supervised {
        if target_name == "reserve" {
            progress!("[codeflow reserve] started process {}", child.id());
        } else {
            progress!(
                "[codeflow test] supervising heavy target '{target_name}' in process group {}",
                child.id()
            );
        }
    }
    let stdout_reader = child.stdout.take().map(|pipe| {
        spawn_reader_with_progress(pipe, emit_progress.then(|| target_name.to_string()))
    });
    let stderr_reader = child.stderr.take().map(|pipe| {
        spawn_reader_with_progress(pipe, emit_progress.then(|| target_name.to_string()))
    });

    let start = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(spawn_err)? {
            break status;
        }
        if start.elapsed() >= timeout {
            // Deadline hit: kill the whole process group (not just `sh`) so
            // grandchildren die too and the reader threads' pipes close —
            // otherwise an orphan holding a pipe write-end blocks read_to_end
            // below for its full runtime. `--` keeps the negative (group) pid
            // from parsing as a flag; best-effort, with child.kill() as the
            // direct-child backstop.
            #[cfg(unix)]
            {
                let _ = Command::new("kill")
                    .args(["-KILL", "--", &format!("-{}", child.id())])
                    .status();
            }
            #[cfg(windows)]
            {
                // The job ends the whole tree at once. The direct-child
                // kill below is the last backstop.
                target_job.terminate();
                let _ = Command::new("taskkill")
                    .args(["/PID", &child.id().to_string(), "/T", "/F"])
                    .status();
            }
            let _ = child.kill();
            timed_out = true;
            break child.wait().map_err(spawn_err)?;
        }
        std::thread::sleep(POLL_INTERVAL);
    };

    // Reap any background descendants before draining pipes they may still hold.
    #[cfg(unix)]
    drop(watcher);
    #[cfg(windows)]
    if supervised {
        target_job.terminate();
    }
    let stdout = join_reader(stdout_reader);
    let stderr = join_reader(stderr_reader);
    #[cfg(unix)]
    if let Err(error) = crate::testing::gate_guard::target_group_finished(child.id()) {
        progress!("[codeflow test] warning: {error}");
    }

    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        status.signal()
    };
    #[cfg(not(unix))]
    let signal = None;

    Ok(CommandOutcome {
        signal,
        exit_code: status.code().unwrap_or(-1),
        stdout: render_capture(&stdout),
        stderr: render_capture(&stderr),
        timed_out,
        stdout_truncated: stdout.truncated,
        stderr_truncated: stderr.truncated,
    })
}

fn build_command(
    command: &str,
    cwd: &Path,
    env: &std::collections::BTreeMap<String, String>,
    target_name: &str,
    shell: CommandShell,
) -> Result<Command, TestingError> {
    let effective = effective_shell(shell, cfg!(windows));
    let mut cmd = match effective {
        CommandShell::Auto => unreachable!("auto shell resolved above"),
        CommandShell::Sh => {
            let mut command_process = Command::new("sh");
            command_process.args(["-c", command]);
            command_process
        }
        CommandShell::Powershell => {
            let executable = if cfg!(windows) {
                "powershell.exe"
            } else {
                "pwsh"
            };
            let mut command_process = Command::new(executable);
            command_process.args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                command,
            ]);
            command_process
        }
        CommandShell::Cmd => {
            let mut command_process = Command::new("cmd.exe");
            // cmd.exe parses the command line itself, so preserve the inner quotes.
            #[cfg(windows)]
            command_process
                .args(["/D", "/S", "/C"])
                .raw_arg(format!("\"{command}\""));
            #[cfg(not(windows))]
            command_process.args(["/D", "/S", "/C", command]);
            command_process
        }
    };
    cmd.current_dir(cwd);

    configure_environment(&mut cmd, env, target_name)?;
    Ok(cmd)
}

pub(super) fn configure_environment(
    cmd: &mut Command,
    env: &std::collections::BTreeMap<String, String>,
    target_name: &str,
) -> Result<(), TestingError> {
    // The test gate can run inside a git hook (the pre-push `test_gate_on_push`,
    // or `integrate`'s internal gate), where git exports GIT_DIR / GIT_WORK_TREE
    // / GIT_INDEX_FILE. Those would redirect any `git` the test command spawns
    // at the outer repo instead of the test's own fixtures — silently mutating
    // the real repository (this is what flipped `core.bare` and planted a stray
    // commit; ADR-0007 follow-up). Clear them so the test command and its
    // children discover git normally from `cwd`.
    cmd.env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE");

    // Set TARGET env var
    cmd.env("TARGET", target_name);

    // Set target-local env vars (validated)
    for (key, val) in env {
        validate_env_key(key, target_name)?;
        cmd.env(key, val);
    }
    Ok(())
}

fn effective_shell(shell: CommandShell, windows: bool) -> CommandShell {
    match shell {
        CommandShell::Auto if windows => CommandShell::Cmd,
        CommandShell::Auto => CommandShell::Sh,
        explicit => explicit,
    }
}

struct CapturedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

/// Spawn a thread that drains a child pipe to EOF while retaining a bounded
/// tail. Draining continues after the cap so the child cannot block on a full
/// pipe.
fn spawn_reader_with_progress<R: Read + Send + 'static>(
    mut pipe: R,
    name: Option<String>,
) -> std::thread::JoinHandle<CapturedOutput> {
    std::thread::spawn(move || {
        let mut retained = Vec::with_capacity(OUTPUT_CAPTURE_LIMIT);
        let mut chunk = [0_u8; 8192];
        let mut truncated = false;
        loop {
            let Ok(read) = pipe.read(&mut chunk) else {
                break;
            };
            if let Some(name) = &name {
                for line in String::from_utf8_lossy(&chunk[..read]).lines() {
                    if line.contains("Starting")
                        || line.contains("Summary")
                        || line.starts_with("journey gate:")
                        || line.contains(" PASS ")
                        || line.contains(" FAIL ")
                    {
                        // A target's own output, one line (TSK-388).
                        progress!("[codeflow test] {name}: {line}");
                    }
                }
            }
            if read == 0 {
                break;
            }
            retained.extend_from_slice(&chunk[..read]);
            if retained.len() > OUTPUT_CAPTURE_LIMIT {
                let excess = retained.len() - OUTPUT_CAPTURE_LIMIT;
                retained.drain(..excess);
                truncated = true;
            }
        }
        CapturedOutput {
            bytes: retained,
            truncated,
        }
    })
}

/// Join a reader thread, yielding its captured bytes (empty if absent or the
/// thread panicked — capture is best-effort and never masks the run result).
fn join_reader(handle: Option<std::thread::JoinHandle<CapturedOutput>>) -> CapturedOutput {
    handle
        .and_then(|h| h.join().ok())
        .unwrap_or(CapturedOutput {
            bytes: Vec::new(),
            truncated: false,
        })
}

fn render_capture(capture: &CapturedOutput) -> String {
    let content = String::from_utf8_lossy(&capture.bytes);
    if capture.truncated {
        format!("[codeflow test] output truncated; retained last {OUTPUT_CAPTURE_LIMIT} bytes\n{content}")
    } else {
        content.into_owned()
    }
}

/// Validate an environment variable key.
///
/// Rejects keys that are empty, contain `=`, or contain null bytes.
fn validate_env_key(key: &str, _target_name: &str) -> Result<(), TestingError> {
    if key.is_empty() {
        return Err(TestingError::InvalidEnvKey {
            key: key.to_string(),
            reason: "must not be empty".to_string(),
        });
    }
    if key.contains('=') {
        return Err(TestingError::InvalidEnvKey {
            key: key.to_string(),
            reason: "must not contain '='".to_string(),
        });
    }
    if key.contains('\0') {
        return Err(TestingError::InvalidEnvKey {
            key: key.to_string(),
            reason: "must not contain null bytes".to_string(),
        });
    }
    Ok(())
}

/// Check if a command string has unbalanced quotes (basic shell lint).
///
/// # Errors
///
/// Returns `TestingError::UnbalancedQuotes` if quotes are unbalanced.
pub fn check_balanced_quotes(command: &str) -> Result<(), TestingError> {
    let mut single_count = 0u32;
    let mut double_count = 0u32;
    let mut escaped = false;

    for ch in command.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '\'' => single_count += 1,
            '"' => double_count += 1,
            _ => {}
        }
    }

    if !single_count.is_multiple_of(2) || !double_count.is_multiple_of(2) {
        return Err(TestingError::UnbalancedQuotes(command.to_string()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::config::{ModeCommand, RunnerType};
    use std::collections::BTreeMap;

    #[test]
    fn adoption_decision_stops_each_failed_child_and_names_the_target() {
        let mut stopped = 0;
        let joined: Result<u8, AdoptError> = Ok(7);
        assert_eq!(adopt_or_stop(joined, "sample", || stopped += 1).unwrap(), 7);
        assert_eq!(stopped, 0);
        for error in [
            AdoptError::Job(std::io::Error::from_raw_os_error(5)),
            AdoptError::Resume(std::io::Error::from_raw_os_error(6)),
        ] {
            let result: Result<u8, _> = Err(error);
            let failure = adopt_or_stop(result, "sample", || stopped += 1).unwrap_err();
            let text = failure.to_string();
            assert!(text.contains("sample"), "{text}");
            assert!(text.contains("os error"), "{text}");
        }
        assert_eq!(stopped, 2);
    }
    /// A lease another run holds past the waiting target's timeout makes
    /// the wait end with the target's bound named, never an unbounded wait.
    #[test]
    fn an_output_lease_wait_ends_at_the_targets_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let mut target = make_target("reporter", "true");
        target.report = Some(crate::testing::config::ReportConfig {
            format: crate::testing::config::ReportFormat::Junit,
            path: "out/junit.xml".into(),
            derive_from: None,
        });
        target.timeout_seconds = Some(1);
        let held = evidence_leases(&target, dir.path(), dir.path(), "target").unwrap();
        assert_eq!(held.len(), 1);
        let start = Instant::now();
        let refused = evidence_leases(&target, dir.path(), dir.path(), "target").unwrap_err();
        // The refusal must be the bound, not an open that failed at once
        // (on Windows a sharing violation did, before the read-only open).
        assert!(refused.to_string().contains("after 1s"), "{refused}");
        assert!(start.elapsed() >= Duration::from_secs(1));
        drop(held);
        assert_eq!(
            evidence_leases(&target, dir.path(), dir.path(), "target")
                .unwrap()
                .len(),
            1
        );
    }

    fn make_target(name: &str, command: &str) -> TargetConfig {
        TargetConfig {
            name: name.to_string(),
            enabled: true,
            cwd: None,
            env: BTreeMap::new(),
            shell: CommandShell::Auto,
            requires: Vec::new(),
            outputs: Vec::new(),
            narrow: Vec::new(),
            input_class: None,
            evidence: None,
            exclusive: false,
            host_exclusive: false,
            heavy: None,
            workers: None,
            runner: RunnerType::Custom,
            modes: BTreeMap::from([(
                "full".to_string(),
                ModeCommand {
                    command: command.to_string(),
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

    #[test]
    fn test_run_target_simple() {
        let target = make_target("test", "echo hello");
        let dir = tempfile::tempdir().unwrap();
        let result = run_target(&target, "full", dir.path()).unwrap();
        assert_eq!(result.target_name, "test");
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));
    }

    #[test]
    fn test_run_target_caps_output_and_flags_truncation() {
        let mut target = if cfg!(windows) {
            make_target(
                "noisy",
                "[Console]::Out.Write('x' * 1100000); [Console]::Error.Write('e' * 1100000)",
            )
        } else {
            make_target(
                "noisy",
                "yes x | head -c 1100000; yes e | head -c 1100000 >&2",
            )
        };
        if cfg!(windows) {
            target.shell = CommandShell::Powershell;
        }
        let dir = tempfile::tempdir().unwrap();
        let result = run_target(&target, "full", dir.path()).unwrap();

        assert!(result.stdout_truncated);
        assert!(result.stderr_truncated);
        assert!(result.stdout.contains("output truncated"));
        assert!(result.stderr.contains("output truncated"));
        assert!(result.stdout.len() < 1_050_000);
        assert!(result.stderr.len() < 1_050_000);
    }

    #[test]
    fn test_run_target_mode_not_configured() {
        let target = make_target("test", "echo");
        let dir = tempfile::tempdir().unwrap();
        let err = run_target(&target, "quick", dir.path()).unwrap_err();
        match err {
            TestingError::ModeNotConfigured { target: t, mode: m } => {
                assert_eq!(t, "test");
                assert_eq!(m, "quick");
            }
            other => panic!("expected ModeNotConfigured, got: {other}"),
        }
    }

    #[test]
    fn test_run_target_failing_command() {
        let target = make_target("test", "exit 1");
        let dir = tempfile::tempdir().unwrap();
        let result = run_target(&target, "full", dir.path()).unwrap();
        assert_eq!(result.exit_code, 1);
    }

    /// A target whose command hangs past its `timeout_seconds` is killed and
    /// FAILS fast with a loud message naming the timeout and the config field —
    /// it must not block for the full sleep duration.
    #[test]
    fn test_run_target_times_out_and_fails_fast() {
        let mut target = make_target("hanger", "sleep 30");
        target.timeout_seconds = Some(1);
        let dir = tempfile::tempdir().unwrap();

        let start = Instant::now();
        let result = run_target(&target, "full", dir.path()).unwrap();
        let elapsed = start.elapsed();

        assert_ne!(result.exit_code, 0, "timed-out target must fail");
        assert_eq!(result.exit_code, TIMEOUT_EXIT_CODE);
        assert!(
            result.stderr.contains("TIMEOUT"),
            "loud timeout message expected, got: {}",
            result.stderr
        );
        assert!(
            result.stderr.contains("timeout_seconds"),
            "message must name the config field, got: {}",
            result.stderr
        );
        assert!(
            elapsed < Duration::from_secs(15),
            "must fail fast, not wait out the full sleep (took {elapsed:?})"
        );
    }

    /// A normal, fast target is unaffected by the timeout machinery: it exits 0
    /// with its output intact and no timeout message.
    #[test]
    fn test_run_target_within_timeout_unaffected() {
        let mut target = make_target("quick", "echo done");
        target.timeout_seconds = Some(60);
        let dir = tempfile::tempdir().unwrap();
        let result = run_target(&target, "full", dir.path()).unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("done"));
        assert!(!result.stderr.contains("TIMEOUT"));
    }

    #[test]
    fn test_run_target_with_env() {
        let command = if cfg!(windows) {
            "echo %MY_VAR%"
        } else {
            "echo $MY_VAR"
        };
        let mut target = make_target("test", command);
        target
            .env
            .insert("MY_VAR".to_string(), "hello_from_env".to_string());
        let dir = tempfile::tempdir().unwrap();
        let result = run_target(&target, "full", dir.path()).unwrap();
        assert!(result.stdout.contains("hello_from_env"));
    }

    #[test]
    fn test_run_target_with_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("subdir");
        std::fs::create_dir(&subdir).unwrap();
        let mut target = make_target("test", "pwd");
        target.cwd = Some("subdir".to_string());
        let result = run_target(&target, "full", dir.path()).unwrap();
        assert!(result.stdout.trim().ends_with("subdir"));
    }

    #[test]
    fn test_resolve_cwd_none() {
        let base = Path::new("/project");
        assert_eq!(resolve_cwd(base, None).unwrap(), PathBuf::from("/project"));
    }

    #[test]
    fn test_resolve_cwd_dot() {
        let base = Path::new("/project");
        assert_eq!(
            resolve_cwd(base, Some(".")).unwrap(),
            PathBuf::from("/project")
        );
    }

    #[test]
    fn test_resolve_cwd_subdir() {
        let base = Path::new("/project");
        assert_eq!(
            resolve_cwd(base, Some("codeflow-cli")).unwrap(),
            PathBuf::from("/project/codeflow-cli")
        );
    }

    #[test]
    fn test_resolve_cwd_path_traversal_rejected() {
        let base = Path::new("/project");
        let result = resolve_cwd(base, Some("../etc"));
        assert!(result.is_err());
        match result.unwrap_err() {
            TestingError::CwdEscapesRoot(cwd) => assert_eq!(cwd, "../etc"),
            other => panic!("expected CwdEscapesRoot, got: {other}"),
        }
    }

    #[test]
    fn test_resolve_cwd_dotdot_in_middle_rejected() {
        let base = Path::new("/project");
        let result = resolve_cwd(base, Some("subdir/../../etc"));
        assert!(result.is_err());
        match result.unwrap_err() {
            TestingError::CwdEscapesRoot(_) => {}
            other => panic!("expected CwdEscapesRoot, got: {other}"),
        }
    }

    #[test]
    fn test_resolve_cwd_nested_subdir_ok() {
        let base = Path::new("/project");
        assert_eq!(
            resolve_cwd(base, Some("a/b/c")).unwrap(),
            PathBuf::from("/project/a/b/c")
        );
    }

    #[test]
    fn test_validate_env_key_valid() {
        assert!(validate_env_key("MY_VAR", "t").is_ok());
        assert!(validate_env_key("PATH", "t").is_ok());
        assert!(validate_env_key("a", "t").is_ok());
    }

    #[test]
    fn test_validate_env_key_empty_rejected() {
        let err = validate_env_key("", "t").unwrap_err();
        match err {
            TestingError::InvalidEnvKey { key, reason } => {
                assert_eq!(key, "");
                assert!(reason.contains("empty"));
            }
            other => panic!("expected InvalidEnvKey, got: {other}"),
        }
    }

    #[test]
    fn test_validate_env_key_equals_rejected() {
        let err = validate_env_key("FOO=BAR", "t").unwrap_err();
        match err {
            TestingError::InvalidEnvKey { key, reason } => {
                assert_eq!(key, "FOO=BAR");
                assert!(reason.contains("'='"));
            }
            other => panic!("expected InvalidEnvKey, got: {other}"),
        }
    }

    #[test]
    fn test_validate_env_key_null_rejected() {
        let err = validate_env_key("FOO\0BAR", "t").unwrap_err();
        match err {
            TestingError::InvalidEnvKey { key, reason } => {
                assert!(reason.contains("null"));
                let _ = key;
            }
            other => panic!("expected InvalidEnvKey, got: {other}"),
        }
    }

    #[test]
    fn test_check_balanced_quotes_ok() {
        assert!(check_balanced_quotes("echo 'hello world'").is_ok());
        assert!(check_balanced_quotes(r#"echo "hello world""#).is_ok());
        assert!(check_balanced_quotes("echo hello").is_ok());
        assert!(check_balanced_quotes("echo 'a' \"b\"").is_ok());
    }

    #[test]
    fn test_check_balanced_quotes_unbalanced() {
        assert!(check_balanced_quotes("echo 'hello").is_err());
        assert!(check_balanced_quotes("echo \"hello").is_err());
    }

    #[test]
    fn test_check_balanced_quotes_escaped() {
        assert!(check_balanced_quotes("echo \\'hello").is_ok());
    }

    #[test]
    fn test_run_all_targets_only_filter() {
        let targets = vec![
            make_target("a", "echo a"),
            make_target("b", "echo b"),
            make_target("c", "echo c"),
        ];
        let dir = tempfile::tempdir().unwrap();
        let results = run_all_targets(
            &targets,
            "full",
            dir.path(),
            false,
            false,
            &["a".to_string()],
            &[],
            &[],
            &[],
        );
        assert_eq!(results.len(), 1);
        assert!(results[0].as_ref().unwrap().stdout.contains('a'));
    }

    #[test]
    fn test_run_all_targets_skip_filter() {
        let targets = vec![make_target("a", "echo a"), make_target("b", "echo b")];
        let dir = tempfile::tempdir().unwrap();
        let results = run_all_targets(
            &targets,
            "full",
            dir.path(),
            false,
            false,
            &[],
            &["b".to_string()],
            &[],
            &[],
        );
        assert_eq!(results.len(), 1);
        assert!(results[0].as_ref().unwrap().stdout.contains('a'));
    }

    #[test]
    fn test_run_all_targets_disabled_skipped() {
        let mut target = make_target("disabled", "echo should_not_run");
        target.enabled = false;
        let dir = tempfile::tempdir().unwrap();
        let results = run_all_targets(
            &[target],
            "full",
            dir.path(),
            false,
            false,
            &[],
            &[],
            &[],
            &[],
        );
        assert!(results.is_empty());
    }

    #[test]
    fn test_run_all_targets_mode_missing_skipped() {
        let target = make_target("test", "echo hello");
        // Only has "full" mode, requesting "quick" should skip
        let dir = tempfile::tempdir().unwrap();
        let results = run_all_targets(
            &[target],
            "quick",
            dir.path(),
            false,
            false,
            &[],
            &[],
            &[],
            &[],
        );
        assert!(results.is_empty());
    }

    /// CI-environment opt-out: `ci_skip=true` targets must be skipped in CI.
    #[test]
    fn test_run_all_targets_ci_skip_honoured_under_ci() {
        let mut slow = make_target("slow", "echo slow");
        slow.ci_skip = Some(true);
        slow.ci_skip_reason = Some("integration tests exceed CI time budget".to_string());
        let fast = make_target("fast", "echo fast");
        let dir = tempfile::tempdir().unwrap();

        let results = run_all_targets_with_ci(
            &[slow, fast],
            "full",
            dir.path(),
            false,
            false,
            &[],
            &[],
            &[],
            &[],
            true,
        );

        assert_eq!(
            results.len(),
            1,
            "ci_skip=true target must be skipped in CI"
        );
        assert_eq!(results[0].as_ref().unwrap().target_name, "fast");
    }

    /// Regression (ADR-0007 follow-up): the test gate runs inside git's pre-push
    /// hook, where git exports `GIT_DIR` / `GIT_WORK_TREE` / `GIT_INDEX_FILE`. If
    /// those leaked into the test command, any `git` it spawns would mutate the
    /// OUTER repo instead of its own fixtures — this is what flipped `core.bare`
    /// and planted a stray commit. `spawn_command` must clear all three so the
    /// test command discovers git from `cwd`.
    #[test]
    fn spawn_command_clears_inherited_git_env() {
        let command = build_command(
            "true",
            Path::new("."),
            &BTreeMap::new(),
            "probe",
            CommandShell::Auto,
        )
        .expect("probe command builds");
        let removed: std::collections::BTreeSet<_> = command
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(key, _)| key.to_owned())
            .collect();
        for key in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"] {
            assert!(
                removed.contains(std::ffi::OsStr::new(key)),
                "{key} must be removed from the child environment"
            );
        }
    }

    #[test]
    fn auto_shell_follows_runtime_platform_and_explicit_shell_wins() {
        assert_eq!(effective_shell(CommandShell::Auto, false), CommandShell::Sh);
        assert_eq!(effective_shell(CommandShell::Auto, true), CommandShell::Cmd);
        assert_eq!(effective_shell(CommandShell::Cmd, false), CommandShell::Cmd);
    }

    /// `ci_skip=true` targets still run OUTSIDE CI (local dev loop).
    #[test]
    fn test_run_all_targets_ci_skip_ignored_outside_ci() {
        let mut slow = make_target("slow", "echo slow");
        slow.ci_skip = Some(true);
        slow.ci_skip_reason = Some("slow in CI only".to_string());
        let dir = tempfile::tempdir().unwrap();

        let results = run_all_targets_with_ci(
            &[slow],
            "full",
            dir.path(),
            false,
            false,
            &[],
            &[],
            &[],
            &[],
            false,
        );

        assert_eq!(results.len(), 1, "ci_skip must not affect local runs");
        assert_eq!(results[0].as_ref().unwrap().target_name, "slow");
    }

    /// `is_ci_environment` contract: treats truthy and non-empty values as CI,
    /// but recognises the `false`/`0` conventions and unset state as non-CI.
    #[test]
    fn test_is_ci_value_recognises_conventions() {
        assert!(!is_ci_value(None), "unset CI → false");
        assert!(!is_ci_value(Some("")), "empty CI → false");
        assert!(!is_ci_value(Some("false")), "CI=false → false");
        assert!(!is_ci_value(Some("0")), "CI=0 → false");
        assert!(is_ci_value(Some("true")), "CI=true → true");
        assert!(is_ci_value(Some("1")), "CI=1 → true");
    }

    #[test]
    fn test_target_env_includes_target_name() {
        let command = if cfg!(windows) {
            "echo %TARGET%"
        } else {
            "echo $TARGET"
        };
        let target = make_target("mytest", command);
        let dir = tempfile::tempdir().unwrap();
        let result = run_target(&target, "full", dir.path()).unwrap();
        assert!(result.stdout.contains("mytest"));
    }

    // --- Tag filtering ---------------------------------------------------

    fn tagged_target(name: &str, tags: &[Tag]) -> TargetConfig {
        let mut t = make_target(name, "echo ok");
        t.tags = tags.to_vec();
        t
    }

    #[test]
    fn target_tags_unions_target_and_file_tags() {
        use crate::testing::config::TestFileEntry;
        let mut t = tagged_target("shell", &[Tag::Critical]);
        t.test_files = vec![
            TestFileEntry {
                path: "a.sh".to_string(),
                tags: vec![Tag::High],
            },
            TestFileEntry {
                path: "b.sh".to_string(),
                tags: vec![Tag::Critical, Tag::Medium],
            },
        ];
        let all = target_tags(&t);
        assert!(all.contains(&Tag::Critical));
        assert!(all.contains(&Tag::High));
        assert!(all.contains(&Tag::Medium));
        assert_eq!(all.len(), 3, "duplicates should be deduplicated");
    }

    #[test]
    fn tag_filter_only_tag_matches_subset() {
        let t = tagged_target("x", &[Tag::Critical]);
        assert!(target_matches_tag_filter(&t, &[Tag::Critical], &[]));
        assert!(!target_matches_tag_filter(&t, &[Tag::Low], &[]));
    }

    #[test]
    fn tag_filter_empty_only_tag_accepts_all() {
        let t = tagged_target("x", &[Tag::High]);
        assert!(target_matches_tag_filter(&t, &[], &[]));
        let untagged = make_target("u", "echo");
        assert!(target_matches_tag_filter(&untagged, &[], &[]));
    }

    #[test]
    fn tag_filter_skip_tag_rejects_matching() {
        let t = tagged_target("x", &[Tag::Low]);
        assert!(!target_matches_tag_filter(&t, &[], &[Tag::Low]));
    }

    #[test]
    fn tag_filter_skip_wins_over_only() {
        // Target is tagged Critical; --only-tag critical + --skip-tag critical
        // should EXCLUDE (skip wins).
        let t = tagged_target("x", &[Tag::Critical]);
        assert!(!target_matches_tag_filter(
            &t,
            &[Tag::Critical],
            &[Tag::Critical]
        ));
    }

    #[test]
    fn tag_filter_untagged_excluded_when_only_tag_nonempty() {
        let t = make_target("u", "echo");
        assert!(!target_matches_tag_filter(&t, &[Tag::Critical], &[]));
    }

    #[test]
    fn tag_filter_multi_tag_target_matches_any_only() {
        // Target has Critical + High; --only-tag high → match.
        let t = tagged_target("x", &[Tag::Critical, Tag::High]);
        assert!(target_matches_tag_filter(&t, &[Tag::High], &[]));
    }

    #[test]
    fn run_all_targets_only_tag_selects_critical() {
        let a = tagged_target("a", &[Tag::Critical]);
        let b = tagged_target("b", &[Tag::Low]);
        let dir = tempfile::tempdir().unwrap();
        let results = run_all_targets(
            &[a, b],
            "full",
            dir.path(),
            false,
            false,
            &[],
            &[],
            &[Tag::Critical],
            &[],
        );
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].as_ref().unwrap().target_name, "a");
    }

    #[test]
    fn run_all_targets_skip_tag_excludes_critical() {
        let a = tagged_target("a", &[Tag::Critical]);
        let b = tagged_target("b", &[Tag::Low]);
        let dir = tempfile::tempdir().unwrap();
        let results = run_all_targets(
            &[a, b],
            "full",
            dir.path(),
            false,
            false,
            &[],
            &[],
            &[],
            &[Tag::Critical],
        );
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].as_ref().unwrap().target_name, "b");
    }

    #[test]
    fn run_all_targets_tag_filter_composes_with_name_filter() {
        // Three targets: a(critical), b(critical), c(low). --only=b,c + --only-tag=critical → only b.
        let a = tagged_target("a", &[Tag::Critical]);
        let b = tagged_target("b", &[Tag::Critical]);
        let c = tagged_target("c", &[Tag::Low]);
        let dir = tempfile::tempdir().unwrap();
        let results = run_all_targets(
            &[a, b, c],
            "full",
            dir.path(),
            false,
            false,
            &["b".to_string(), "c".to_string()],
            &[],
            &[Tag::Critical],
            &[],
        );
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].as_ref().unwrap().target_name, "b");
    }

    #[test]
    fn run_all_targets_no_tag_filter_includes_untagged() {
        let a = tagged_target("a", &[Tag::Critical]);
        let b = make_target("untagged", "echo u");
        let dir = tempfile::tempdir().unwrap();
        let results = run_all_targets(
            &[a, b],
            "full",
            dir.path(),
            false,
            false,
            &[],
            &[],
            &[],
            &[],
        );
        assert_eq!(results.len(), 2);
    }

    /// Directory handed to [`windows_gate_helper`] when it is run as a gate.
    #[cfg(windows)]
    const GATE_HELPER_DIR: &str = "CODEFLOW_TEST_GATE_HELPER_DIR";

    /// Whether process `pid` still runs, read from `tasklist`.
    #[cfg(windows)]
    fn process_alive(pid: u32) -> bool {
        let out = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output()
            .expect("tasklist runs");
        String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\""))
    }

    /// Not a test on its own: run by
    /// [`a_killed_gate_ends_its_target_tree_on_windows`] as the gate process,
    /// it runs one long target whose grandchild records its pid. Without
    /// the environment variable it returns at once.
    #[cfg(windows)]
    #[test]
    fn windows_gate_helper() {
        let Some(dir) = std::env::var_os(GATE_HELPER_DIR) else {
            return;
        };
        let target = make_target(
            "long",
            "powershell -NoProfile -NonInteractive -Command \"Set-Content -Path pid.txt -Value $PID; Start-Sleep -Seconds 120\"",
        );
        // A start that fails, or a target that ends by itself with an error,
        // fails this helper, so its exit and its output name the cause.
        match run_target(&target, "full", Path::new(&dir)) {
            Err(error) => panic!("the gate could not run its target: {error}"),
            Ok(done) => assert_eq!(done.exit_code, 0, "the target ended early: {}", done.stderr),
        }
    }

    #[cfg(windows)]
    #[test]
    fn cmd_preserves_a_quoted_argument_with_shell_operators() {
        let dir = tempfile::tempdir().unwrap();
        let target = make_target(
            "quoted",
            "python -c \"import sys; print(len(sys.argv))\" \"a && b\"",
        );
        let result = run_target(&target, "full", dir.path()).unwrap();
        assert_eq!(result.exit_code, 0, "{}", result.stderr);
        assert_eq!(result.stdout.trim(), "2");
    }

    /// How long [`wait_for_target_pid`] waits for the target to record its pid
    /// in the killed-gate test. It is a backstop: the wait ends sooner when the
    /// helper exits, and the failure at the bound says what was not observed.
    #[cfg(windows)]
    const TARGET_PID_BOUND: Duration = Duration::from_secs(60);

    /// A gate process the test ends on every exit path, a panic included.
    struct GateGuard {
        gate: Option<std::process::Child>,
    }

    impl GateGuard {
        fn new(gate: std::process::Child) -> Self {
            Self { gate: Some(gate) }
        }

        fn child(&mut self) -> &mut std::process::Child {
            self.gate.as_mut().expect("the gate was not yet killed")
        }

        fn id(&self) -> u32 {
            self.gate
                .as_ref()
                .expect("the gate was not yet killed")
                .id()
        }

        /// Kill the gate and wait for it, leaving the end of its target tree
        /// to whatever the test checks.
        #[cfg(windows)]
        fn kill_gate(&mut self) {
            if let Some(mut gate) = self.gate.take() {
                gate.kill().unwrap();
                gate.wait().unwrap();
            }
        }
    }

    impl Drop for GateGuard {
        fn drop(&mut self) {
            if let Some(mut gate) = self.gate.take() {
                // The gate is still alive here, so the test ended early: end
                // its whole tree, not only the gate.
                #[cfg(windows)]
                {
                    let _ = Command::new("taskkill")
                        .args(["/PID", &gate.id().to_string(), "/T", "/F"])
                        .output();
                }
                let _ = gate.kill();
                let _ = gate.wait();
            }
        }
    }

    /// Ends the recorded target process and its descendants when a test
    /// fails after the pid was read, unless the test saw the tree end.
    #[cfg(windows)]
    struct TargetGuard(Option<u32>);

    #[cfg(windows)]
    impl Drop for TargetGuard {
        fn drop(&mut self) {
            if let Some(pid) = self.0.take() {
                let _ = Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .output();
            }
        }
    }

    /// What the gate helper wrote to the two output files in `out`.
    fn gate_output(out: &Path) -> String {
        ["gate.stdout", "gate.stderr"]
            .iter()
            .map(|name| {
                let text = std::fs::read_to_string(out.join(name)).unwrap_or_default();
                format!("{name}: {}", text.trim())
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Wait for the target the `gate` helper runs to record its pid in
    /// `pid_file`. Returns the pid and how long the wait took. It fails at
    /// once, with the helper's status and output, when the helper exits first,
    /// and at `bound` with what was and was not observed.
    fn wait_for_target_pid(
        gate: &mut std::process::Child,
        pid_file: &Path,
        out: &Path,
        bound: Duration,
    ) -> Result<(u32, Duration), String> {
        let start = Instant::now();
        loop {
            // The exit is read first, so the pid file read after it is final.
            let exited = gate.try_wait().map_err(|error| error.to_string())?;
            let held = std::fs::read_to_string(pid_file);
            let state = match &held {
                Ok(text) if text.trim().is_empty() => "was empty".to_string(),
                Ok(text) => format!("held {text:?}"),
                Err(error) => format!("could not be read ({error})"),
            };
            let pid = held.ok().and_then(|text| text.trim().parse::<u32>().ok());
            if let Some(status) = exited {
                let recorded = pid.map_or_else(
                    || "before its target recorded a pid".to_string(),
                    |pid| format!("after its target recorded pid {pid}"),
                );
                return Err(format!(
                    "the gate helper ended ({status}) after {:?}, {recorded}; pid.txt {state}\n{}",
                    start.elapsed(),
                    gate_output(out)
                ));
            }
            if let Some(pid) = pid {
                return Ok((pid, start.elapsed()));
            }
            if start.elapsed() >= bound {
                return Err(format!(
                    "no pid after {bound:?} and the gate helper (process {}) was still running; pid.txt {state}\n{}\nnot observed: whether the helper's start, the target's start or PowerShell's start was the slow step",
                    gate.id(),
                    gate_output(out)
                ));
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    /// A `gate` standing in for the helper: `script` is run by the platform
    /// shell with its output in `out`.
    fn scripted_gate(unix: &str, windows: &str, dir: &Path, out: &Path) -> GateGuard {
        let mut command = if cfg!(windows) {
            let mut command = Command::new("cmd");
            command.args(["/D", "/C", windows]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args(["-c", unix]);
            command
        };
        let gate = command
            .current_dir(dir)
            .stdout(std::fs::File::create(out.join("gate.stdout")).unwrap())
            .stderr(std::fs::File::create(out.join("gate.stderr")).unwrap())
            .spawn()
            .unwrap();
        GateGuard::new(gate)
    }

    /// A gate that is one long-running process of its own, with no child, so
    /// its pid stands for everything the test started.
    fn sleeping_gate(out: &Path) -> GateGuard {
        let mut command = if cfg!(windows) {
            let mut command = Command::new("ping");
            command.args(["-n", "31", "127.0.0.1"]);
            command
        } else {
            let mut command = Command::new("sleep");
            command.arg("30");
            command
        };
        let gate = command
            .stdout(std::fs::File::create(out.join("gate.stdout")).unwrap())
            .stderr(std::fs::File::create(out.join("gate.stderr")).unwrap())
            .spawn()
            .unwrap();
        GateGuard::new(gate)
    }

    /// Whether the process `pid` still runs.
    fn process_runs(pid: u32) -> bool {
        #[cfg(windows)]
        {
            process_alive(pid)
        }
        #[cfg(not(windows))]
        {
            Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        }
    }

    /// TSK-1062 AC-2: a helper that fails early ends the wait at once with
    /// its status and its own message, never with a timeout.
    #[test]
    fn a_helper_that_exits_early_fails_the_wait_with_its_status_and_output() {
        let dir = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let mut gate = scripted_gate(
            "echo helper-said-boom >&2; exit 3",
            "echo helper-said-boom 1>&2 & exit 3",
            dir.path(),
            out.path(),
        );
        let message = wait_for_target_pid(
            gate.child(),
            &dir.path().join("pid.txt"),
            out.path(),
            Duration::from_secs(120),
        )
        .unwrap_err();
        assert!(message.contains("the gate helper ended"), "{message}");
        assert!(
            message.contains("exit status: 3") || message.contains("exit code: 3"),
            "the status: {message}"
        );
        assert!(message.contains("helper-said-boom"), "{message}");
        assert!(message.contains("pid.txt could not be read"), "{message}");
        assert!(!message.contains("still running"), "{message}");
    }

    /// A helper that records its pid and then exits is reported as having
    /// recorded it, with the pid file's text and the helper's output.
    #[test]
    fn a_helper_that_exits_after_its_pid_reports_the_pid_and_its_output() {
        let dir = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let mut gate = scripted_gate(
            "echo 4242 > pid.txt; echo helper-said-boom >&2; exit 3",
            "echo 4242 > pid.txt & echo helper-said-boom 1>&2 & exit 3",
            dir.path(),
            out.path(),
        );
        // The wait reports an exit only once it sees one, so let the helper
        // finish first; a helper still running with its pid written is
        // correctly answered with that pid.
        gate.child().wait().unwrap();
        let message = wait_for_target_pid(
            gate.child(),
            &dir.path().join("pid.txt"),
            out.path(),
            Duration::from_secs(120),
        )
        .unwrap_err();
        assert!(
            message.contains("after its target recorded pid 4242"),
            "{message}"
        );
        assert!(!message.contains("before its target"), "{message}");
        assert!(message.contains("4242"), "the pid file text: {message}");
        assert!(message.contains("helper-said-boom"), "{message}");
    }

    /// The wait returns a pid the target has recorded while its helper runs.
    #[test]
    fn the_wait_returns_a_recorded_pid_while_the_helper_runs() {
        let dir = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pid.txt"), "4242 \r\n").unwrap();
        let mut gate = sleeping_gate(out.path());
        let (pid, _) = wait_for_target_pid(
            gate.child(),
            &dir.path().join("pid.txt"),
            out.path(),
            Duration::from_secs(120),
        )
        .unwrap();
        assert_eq!(pid, 4242);
    }

    /// TSK-1062 AC-3: at its bound the wait names what it saw and what it
    /// could not tell.
    #[test]
    fn the_wait_at_its_bound_names_what_was_not_observed() {
        let dir = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let mut gate = sleeping_gate(out.path());
        let message = wait_for_target_pid(
            gate.child(),
            &dir.path().join("pid.txt"),
            out.path(),
            Duration::from_millis(300),
        )
        .unwrap_err();
        assert!(message.contains("still running"), "{message}");
        assert!(message.contains("pid.txt could not be read"), "{message}");
        assert!(message.contains("not observed"), "{message}");
    }

    /// TSK-1062 AC-4: a panic in a test that holds the gate still ends the
    /// gate.
    #[test]
    fn a_panic_while_the_gate_runs_still_ends_the_gate() {
        let out = tempfile::tempdir().unwrap();
        let mut gate_pid = 0;
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let gate = sleeping_gate(out.path());
            gate_pid = gate.id();
            assert!(process_runs(gate_pid), "the gate runs");
            panic!("the test failed while its gate ran");
        }));
        assert!(outcome.is_err());
        assert!(!process_runs(gate_pid), "the panic left the gate running");
    }

    /// TSK-142 AC-3: when a gate is killed on Windows, its running target's
    /// whole tree ends with it, so the OS never frees the gate lock while a
    /// target of that gate still runs. The gate is this test binary run as
    /// [`windows_gate_helper`]; the recorded pid is the target's grandchild
    /// (`cmd.exe` runs `powershell`), so the check covers the tree.
    ///
    /// TSK-1062: the wait for the pid watches the helper, shows the helper's
    /// output when it fails, and the gate and target end on every exit path.
    #[cfg(windows)]
    #[test]
    fn a_killed_gate_ends_its_target_tree_on_windows() {
        let dir = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let mut gate = start_gate_helper(dir.path(), out.path());
        let (pid, waited) = wait_for_target_pid(
            gate.child(),
            &dir.path().join("pid.txt"),
            out.path(),
            TARGET_PID_BOUND,
        )
        .unwrap_or_else(|message| panic!("{message}"));
        let mut target = TargetGuard(Some(pid));
        eprintln!("the target recorded its pid {waited:?} after the gate started");
        assert!(process_alive(pid), "the target runs under its gate");

        gate.kill_gate();
        let gone = (0..200).any(|_| {
            let alive = process_alive(pid);
            if alive {
                std::thread::sleep(Duration::from_millis(50));
            }
            !alive
        });
        assert!(
            gone,
            "process {pid} still ran 10 s after its gate was killed, so the gate's job did not end its tree"
        );
        target.0 = None;
    }

    /// TSK-1062 AC-2: the real helper, given a folder it cannot run in, ends
    /// at once and the wait reports the helper's own message.
    #[cfg(windows)]
    #[test]
    fn a_gate_helper_that_cannot_run_its_target_fails_the_wait_with_its_message() {
        let dir = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let missing = dir.path().join("no-such-folder");
        let mut gate = start_gate_helper(&missing, out.path());
        let message = wait_for_target_pid(
            gate.child(),
            &dir.path().join("pid.txt"),
            out.path(),
            Duration::from_secs(120),
        )
        .unwrap_err();
        assert!(message.contains("the gate helper ended"), "{message}");
        assert!(message.contains("panicked"), "{message}");
        assert!(!message.contains("still running"), "{message}");
    }

    /// Start this test binary as [`windows_gate_helper`] with its target
    /// running in `dir`, its output in `out`.
    #[cfg(windows)]
    fn start_gate_helper(dir: &Path, out: &Path) -> GateGuard {
        let gate = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "testing::runner::tests::windows_gate_helper",
                "--test-threads",
                "1",
                "--nocapture",
            ])
            .env(GATE_HELPER_DIR, dir)
            .stdout(std::fs::File::create(out.join("gate.stdout")).unwrap())
            .stderr(std::fs::File::create(out.join("gate.stderr")).unwrap())
            .spawn()
            .unwrap();
        GateGuard::new(gate)
    }

    #[cfg(windows)]
    #[test]
    fn a_job_failure_ends_the_suspended_target_before_its_command_runs() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid.txt");
        target_job::fail_adoption_for_test(Some(pid_file.clone()));
        let target = make_target("unguarded", "echo ran > marker.txt");
        let result = run_target(&target, "full", dir.path());
        target_job::fail_adoption_for_test(None);
        let message = result.unwrap_err().to_string();
        assert!(message.contains("unguarded"), "{message}");
        assert!(message.contains("os error 5"), "{message}");
        let pid: u32 = std::fs::read_to_string(pid_file).unwrap().parse().unwrap();
        assert!(!process_alive(pid), "the failed target process is gone");
        assert!(
            !dir.path().join("marker.txt").exists(),
            "the command never ran"
        );
    }
}

/// TSK-1044 review 1: a report is opened through the anchored walk, so a
/// link or junction at any component, a link at the report itself, or a
/// hard link never copies an outside file into the evidence.
#[cfg(test)]
mod report_open_tests {
    use std::path::Path;

    fn open_report(root: &Path, path: &Path) -> std::io::Result<Option<std::fs::File>> {
        super::open_report(crate::testing::delivery::locate(
            root, root, "target", path,
        )?)
    }

    #[test]
    fn a_plain_report_opens_and_a_missing_one_is_absent() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("reports")).unwrap();
        std::fs::write(root.path().join("reports/junit.xml"), "<testsuites/>").unwrap();
        let path = root.path().join("reports/junit.xml");
        assert!(open_report(root.path(), &path).unwrap().is_some());
        let missing = root.path().join("absent/junit.xml");
        assert!(open_report(root.path(), &missing).unwrap().is_none());
        let outside = tempfile::tempdir().unwrap();
        let elsewhere = outside.path().join("junit.xml");
        std::fs::write(&elsewhere, "<testsuites/>").unwrap();
        assert!(open_report(root.path(), &elsewhere).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn links_above_or_at_the_report_and_hard_links_are_refused() {
        for planted in ["ancestor", "leaf", "hard"] {
            let root = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            std::fs::write(outside.path().join("junit.xml"), "victim").unwrap();
            let reports = root.path().join("reports");
            match planted {
                "ancestor" => std::os::unix::fs::symlink(outside.path(), &reports).unwrap(),
                "leaf" => {
                    std::fs::create_dir(&reports).unwrap();
                    std::os::unix::fs::symlink(
                        outside.path().join("junit.xml"),
                        reports.join("junit.xml"),
                    )
                    .unwrap();
                }
                _ => {
                    std::fs::create_dir(&reports).unwrap();
                    let near = root.path().join("victim");
                    std::fs::write(&near, "victim").unwrap();
                    std::fs::hard_link(&near, reports.join("junit.xml")).unwrap();
                }
            }
            let result = open_report(root.path(), &reports.join("junit.xml"));
            assert!(result.is_err(), "{planted}: opened");
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_junction_above_the_report_and_a_hard_link_are_refused() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("junit.xml"), "victim").unwrap();
        let reports = root.path().join("reports");
        std::fs::create_dir(&reports).unwrap();
        let redirect =
            crate::bounded_file::confined::windows::tests::junction(&reports, outside.path())
                .unwrap();
        assert!(open_report(root.path(), &reports.join("junit.xml")).is_err());
        drop(redirect);
        let other = root.path().join("other");
        std::fs::create_dir(&other).unwrap();
        let near = root.path().join("victim");
        std::fs::write(&near, "victim").unwrap();
        std::fs::hard_link(&near, other.join("junit.xml")).unwrap();
        let error = open_report(root.path(), &other.join("junit.xml")).unwrap_err();
        assert!(error.to_string().contains("hard link"), "{error}");
    }
}
