//! Per-target shell command runner.
//!
//! Spawns each enabled target's mode-specific command via `std::process::Command`
//! in the target's cwd, capturing stdout/stderr.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::testing::config::{Tag, TargetConfig};
use crate::testing::error::TestingError;

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
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub duration_ms: u64,
    pub report_path: Option<PathBuf>,
    pub coverage_path: Option<PathBuf>,
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
    let outcome = spawn_command(&mode_cmd.command, &cwd, &target.env, &target.name, timeout)?;
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
                outcome.stderr, target.name,
            ),
        )
    } else {
        (outcome.exit_code, outcome.stderr)
    };

    Ok(TargetRunResult {
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
    let in_ci = is_ci_environment();
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
                eprintln!(
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

fn run_sequential(
    targets: &[&TargetConfig],
    mode: &str,
    project_dir: &Path,
    fail_fast: bool,
) -> Vec<Result<TargetRunResult, TestingError>> {
    let mut results = Vec::new();
    for target in targets {
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
            thread::spawn(move || run_target(&target, &mode, &project_dir))
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

/// Returns `true` when the process is running inside a CI environment.
///
/// Honours the de-facto standard `CI` environment variable: any non-empty
/// value that is not the literal strings `"false"` or `"0"` is treated as
/// "in CI". GitHub Actions, GitLab CI, `CircleCI`, and Buildkite all set
/// `CI=true`; developers running locally typically leave it unset.
#[must_use]
pub fn is_ci_environment() -> bool {
    match std::env::var("CI") {
        Ok(v) => {
            let v = v.trim();
            !v.is_empty() && v != "false" && v != "0"
        }
        Err(_) => false,
    }
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
    stdout: String,
    stderr: String,
    timed_out: bool,
    stdout_truncated: bool,
    stderr_truncated: bool,
}

fn spawn_command(
    command: &str,
    cwd: &Path,
    env: &std::collections::BTreeMap<String, String>,
    target_name: &str,
    timeout: Duration,
) -> Result<CommandOutcome, TestingError> {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(command).current_dir(cwd);

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

    let mut child = cmd.spawn().map_err(spawn_err)?;

    let stdout_reader = child.stdout.take().map(spawn_reader);
    let stderr_reader = child.stderr.take().map(spawn_reader);

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
            let _ = child.kill();
            timed_out = true;
            break child.wait().map_err(spawn_err)?;
        }
        std::thread::sleep(POLL_INTERVAL);
    };

    let stdout = join_reader(stdout_reader);
    let stderr = join_reader(stderr_reader);

    Ok(CommandOutcome {
        exit_code: status.code().unwrap_or(-1),
        stdout: render_capture(&stdout),
        stderr: render_capture(&stderr),
        timed_out,
        stdout_truncated: stdout.truncated,
        stderr_truncated: stderr.truncated,
    })
}

struct CapturedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

/// Spawn a thread that drains a child pipe to EOF while retaining a bounded
/// tail. Draining continues after the cap so the child cannot block on a full
/// pipe.
fn spawn_reader<R: Read + Send + 'static>(mut pipe: R) -> std::thread::JoinHandle<CapturedOutput> {
    std::thread::spawn(move || {
        let mut retained = Vec::with_capacity(OUTPUT_CAPTURE_LIMIT);
        let mut chunk = [0_u8; 8192];
        let mut truncated = false;
        loop {
            let Ok(read) = pipe.read(&mut chunk) else {
                break;
            };
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

    fn make_target(name: &str, command: &str) -> TargetConfig {
        TargetConfig {
            name: name.to_string(),
            enabled: true,
            cwd: None,
            env: BTreeMap::new(),
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
        let target = make_target(
            "noisy",
            "yes x | head -c 1100000; yes e | head -c 1100000 >&2",
        );
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
        let mut target = make_target("test", "echo $MY_VAR");
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

    /// CI-environment opt-out: `ci_skip=true` targets must be skipped when
    /// `CI=true`. Uses a `#[serial(env_vars)]`-gated `set_var` to toggle the
    /// `CI` env without racing other tests.
    #[test]
    #[serial_test::serial(env_vars)]
    fn test_run_all_targets_ci_skip_honoured_under_ci() {
        // SAFETY: test-only env var manipulation, gated by #[serial].
        unsafe { std::env::set_var("CI", "true") };

        let mut slow = make_target("slow", "echo slow");
        slow.ci_skip = Some(true);
        slow.ci_skip_reason = Some("integration tests exceed CI time budget".to_string());
        let fast = make_target("fast", "echo fast");
        let dir = tempfile::tempdir().unwrap();

        let results = run_all_targets(
            &[slow, fast],
            "full",
            dir.path(),
            false,
            false,
            &[],
            &[],
            &[],
            &[],
        );

        // SAFETY: cleanup before assertions so failure doesn't leak CI=true.
        unsafe { std::env::remove_var("CI") };

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
    #[serial_test::serial(env_vars)]
    fn spawn_command_clears_inherited_git_env() {
        // SAFETY: test-only env var manipulation, gated by #[serial]. Simulate a
        // decoy git env as a pre-push hook would export.
        unsafe {
            std::env::set_var("GIT_DIR", "/decoy/.git");
            std::env::set_var("GIT_WORK_TREE", "/decoy");
            std::env::set_var("GIT_INDEX_FILE", "/decoy/index");
        }
        let out = spawn_command(
            "printf '%s|%s|%s' \"${GIT_DIR:-cleared}\" \"${GIT_WORK_TREE:-cleared}\" \"${GIT_INDEX_FILE:-cleared}\"",
            Path::new("."),
            &BTreeMap::new(),
            "probe",
            Duration::from_secs(DEFAULT_TIMEOUT_SECONDS),
        )
        .expect("probe command runs");
        // SAFETY: cleanup before asserting so a failure never leaks the decoy env.
        unsafe {
            std::env::remove_var("GIT_DIR");
            std::env::remove_var("GIT_WORK_TREE");
            std::env::remove_var("GIT_INDEX_FILE");
        }
        assert_eq!(
            out.stdout, "cleared|cleared|cleared",
            "the test gate must not leak an inherited git env into the test command"
        );
    }

    /// `ci_skip=true` targets still run OUTSIDE CI (local dev loop).
    #[test]
    #[serial_test::serial(env_vars)]
    fn test_run_all_targets_ci_skip_ignored_outside_ci() {
        // SAFETY: test-only env var manipulation, gated by #[serial].
        unsafe { std::env::remove_var("CI") };

        let mut slow = make_target("slow", "echo slow");
        slow.ci_skip = Some(true);
        slow.ci_skip_reason = Some("slow in CI only".to_string());
        let dir = tempfile::tempdir().unwrap();

        let results = run_all_targets(
            &[slow],
            "full",
            dir.path(),
            false,
            false,
            &[],
            &[],
            &[],
            &[],
        );

        assert_eq!(results.len(), 1, "ci_skip must not affect local runs");
        assert_eq!(results[0].as_ref().unwrap().target_name, "slow");
    }

    /// `is_ci_environment` contract: treats truthy and non-empty values as CI,
    /// but recognises the `false`/`0` conventions and unset state as non-CI.
    #[test]
    #[serial_test::serial(env_vars)]
    fn test_is_ci_environment_recognises_conventions() {
        // SAFETY: test-only env var manipulation, gated by #[serial].
        unsafe { std::env::remove_var("CI") };
        assert!(!is_ci_environment(), "unset CI → false");

        unsafe { std::env::set_var("CI", "") };
        assert!(!is_ci_environment(), "empty CI → false");

        unsafe { std::env::set_var("CI", "false") };
        assert!(!is_ci_environment(), "CI=false → false");

        unsafe { std::env::set_var("CI", "0") };
        assert!(!is_ci_environment(), "CI=0 → false");

        unsafe { std::env::set_var("CI", "true") };
        assert!(is_ci_environment(), "CI=true → true");

        unsafe { std::env::set_var("CI", "1") };
        assert!(is_ci_environment(), "CI=1 → true");

        // Cleanup.
        unsafe { std::env::remove_var("CI") };
    }

    #[test]
    fn test_target_env_includes_target_name() {
        let target = make_target("mytest", "echo $TARGET");
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
}
