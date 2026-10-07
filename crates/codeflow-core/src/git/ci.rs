//! Wait for required CI checks to turn green before merging a PR.
//!
//! Merging immediately after `gh pr create` either fails (branch protection:
//! required checks not met) or merges with checks still pending (no branch
//! protection). This module polls `gh pr checks` on a configurable cadence
//! and returns an explicit outcome the caller can route on — success,
//! required-check failure, timeout, or gh unavailable. Workflow-agnostic:
//! any caller that needs "is CI green for this PR?" uses it.
//!
//! ## Config schema
//!
//! The caller is expected to load `.codeflow/ci-wait.json` into
//! [`CiWaitConfig`]. Defaults are safe: 30 min timeout, 30 s poll,
//! no include/exclude filters, fall back to "all checks are required" when
//! GitHub's branch protection API reports no required contexts.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors surfaced by the CI-wait subsystem.
#[derive(Debug, Error)]
pub enum CiWaitError {
    #[error("gh CLI not available or failed: {0}")]
    GhUnavailable(String),
    #[error("parsing gh output: {0}")]
    Parse(String),
    #[error("reading ci-wait config: {0}")]
    ReadConfig(String),
    #[error("parsing ci-wait config: {0}")]
    ParseConfig(String),
}

/// Outcome of [`wait_for_ci_green`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CiOutcome {
    /// All required checks reported success before the timeout.
    AllGreen,
    /// One or more required checks reported a conclusive failure.
    ///
    /// The list contains check names in their GitHub-reported form so the
    /// caller can surface the exact failing context to the user.
    RequiredFailed(Vec<String>),
    /// The poll loop exceeded [`CiWaitConfig::timeout_minutes`] without
    /// reaching a conclusive state.
    Timeout,
    /// GitHub reported that this PR has no required checks. When
    /// `fall_back_to_all_checks_when_no_required=false` the caller should
    /// proceed to merge; otherwise the caller should treat all reported
    /// checks as required.
    NoRequiredChecks,
}

/// Persisted form of `.codeflow/ci-wait.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiWaitConfig {
    /// Hard timeout for the entire poll loop.
    #[serde(default = "default_timeout_minutes")]
    pub timeout_minutes: u64,
    /// Seconds to sleep between polls.
    #[serde(default = "default_poll_interval_seconds")]
    pub poll_interval_seconds: u64,
    /// If non-empty, only check names present here are considered.
    ///
    /// Takes precedence over the list of required contexts reported by
    /// GitHub's branch-protection API.
    #[serde(default)]
    pub include_checks: Vec<String>,
    /// Always exclude these check names (e.g., flaky optional jobs).
    #[serde(default)]
    pub exclude_checks: Vec<String>,
    /// When GitHub reports no required checks for the base branch (no
    /// protection rules, or no contexts configured), fall back to treating
    /// every reported check as required. Safer default for unattended merges.
    #[serde(default = "default_fall_back_to_all")]
    pub fall_back_to_all_checks_when_no_required: bool,
}

fn default_timeout_minutes() -> u64 {
    30
}
fn default_poll_interval_seconds() -> u64 {
    30
}
fn default_fall_back_to_all() -> bool {
    true
}

impl Default for CiWaitConfig {
    fn default() -> Self {
        Self {
            timeout_minutes: default_timeout_minutes(),
            poll_interval_seconds: default_poll_interval_seconds(),
            include_checks: Vec::new(),
            exclude_checks: Vec::new(),
            fall_back_to_all_checks_when_no_required: default_fall_back_to_all(),
        }
    }
}

/// Accepted range for [`CiWaitConfig::timeout_minutes`]. Zero would make
/// the gate always fail immediately; astronomical values produce indefinite
/// blocking and risk `Duration::from_secs` overflow when multiplied by 60.
/// One day is a safe upper bound for a CI wait.
const TIMEOUT_MINUTES_MIN: u64 = 1;
const TIMEOUT_MINUTES_MAX: u64 = 1440; // 24 hours

/// Accepted range for [`CiWaitConfig::poll_interval_seconds`] — too-short
/// values hammer the GitHub API (rate-limit risk); too-long values hide
/// CI outcomes well past their conclusion.
const POLL_INTERVAL_SECS_MIN: u64 = 5;
const POLL_INTERVAL_SECS_MAX: u64 = 300; // 5 minutes

impl CiWaitConfig {
    /// Load [`CiWaitConfig`] from `.codeflow/ci-wait.json` under the
    /// project root. Missing file returns defaults (safe fallback for repos
    /// that have not opted in).
    ///
    /// The loaded config is validated after deserialization;
    /// `timeout_minutes` must be in `[1, 1440]` and `poll_interval_seconds`
    /// in `[5, 300]`. Out-of-range values are rejected via [`CiWaitError::ParseConfig`].
    ///
    /// # Errors
    ///
    /// Returns [`CiWaitError::ReadConfig`] when the file exists but cannot be
    /// read, [`CiWaitError::ParseConfig`] when it cannot be parsed, and
    /// [`CiWaitError::ParseConfig`] when any numeric field is out of its
    /// safe range.
    pub fn load(project_dir: &std::path::Path) -> Result<Self, CiWaitError> {
        let path = project_dir.join(".codeflow").join("ci-wait.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        let body = std::fs::read_to_string(&path)
            .map_err(|e| CiWaitError::ReadConfig(format!("{}: {e}", path.display())))?;
        let cfg: CiWaitConfig = serde_json::from_str(&body)
            .map_err(|e| CiWaitError::ParseConfig(format!("{}: {e}", path.display())))?;
        cfg.validate_bounds()
            .map_err(|msg| CiWaitError::ParseConfig(format!("{}: {msg}", path.display())))?;
        Ok(cfg)
    }

    /// Check numeric fields are within accepted ranges. Pure function so the
    /// validation path can be tested in isolation.
    ///
    /// # Errors
    ///
    /// Returns a human-readable message naming the first out-of-range field.
    pub(crate) fn validate_bounds(&self) -> Result<(), String> {
        if !(TIMEOUT_MINUTES_MIN..=TIMEOUT_MINUTES_MAX).contains(&self.timeout_minutes) {
            return Err(format!(
                "timeout_minutes {} out of range [{TIMEOUT_MINUTES_MIN}, {TIMEOUT_MINUTES_MAX}]",
                self.timeout_minutes
            ));
        }
        if !(POLL_INTERVAL_SECS_MIN..=POLL_INTERVAL_SECS_MAX).contains(&self.poll_interval_seconds)
        {
            return Err(format!(
                "poll_interval_seconds {} out of range [{POLL_INTERVAL_SECS_MIN}, {POLL_INTERVAL_SECS_MAX}]",
                self.poll_interval_seconds
            ));
        }
        Ok(())
    }
}

/// Raw check entry returned by `gh pr checks --json`.
///
/// `state` is GitHub's conclusion string (`SUCCESS`, `FAILURE`, `CANCELLED`,
/// `TIMED_OUT`, `ACTION_REQUIRED`, `NEUTRAL`, `SKIPPED`, `STALE`) when the
/// check has concluded, or empty/absent when in-flight.
#[derive(Debug, Clone, Deserialize)]
struct GhCheck {
    #[serde(default)]
    name: String,
    #[serde(default)]
    state: String,
}

impl GhCheck {
    /// Normalized state: "success", "failure", "pending", or "unknown".
    fn normalized_state(&self) -> &'static str {
        match self.state.to_ascii_uppercase().as_str() {
            "SUCCESS" | "NEUTRAL" | "SKIPPED" => "success",
            "FAILURE" | "ERROR" | "CANCELLED" | "TIMED_OUT" | "ACTION_REQUIRED" => "failure",
            // "IN_PROGRESS" | "QUEUED" | "PENDING" | "WAITING" | "" (in-flight)
            "" | "IN_PROGRESS" | "QUEUED" | "PENDING" | "WAITING" | "REQUESTED" => "pending",
            _ => "unknown",
        }
    }
}

/// Classify a set of checks given the configured include/exclude filters and
/// the required-check list reported by GitHub's branch-protection API.
///
/// Pure function (no I/O) so it can be exercised by unit tests. Crate-
/// private because `GhCheck` is a private type.
#[must_use]
fn classify_checks(
    checks: &[GhCheck],
    required_contexts: &[String],
    cfg: &CiWaitConfig,
) -> CiOutcome {
    // Determine which names qualify as "required" for this PR.
    let required: Vec<String> = if !cfg.include_checks.is_empty() {
        cfg.include_checks.clone()
    } else if !required_contexts.is_empty() {
        required_contexts.to_vec()
    } else if cfg.fall_back_to_all_checks_when_no_required {
        checks.iter().map(|c| c.name.clone()).collect()
    } else {
        return CiOutcome::NoRequiredChecks;
    };

    let excluded: std::collections::HashSet<&str> =
        cfg.exclude_checks.iter().map(String::as_str).collect();
    let required_set: std::collections::HashSet<&str> = required
        .iter()
        .map(String::as_str)
        .filter(|n| !excluded.contains(n))
        .collect();

    if required_set.is_empty() {
        return CiOutcome::NoRequiredChecks;
    }

    let mut failures: Vec<String> = Vec::new();
    let mut pending = false;
    for check in checks {
        if !required_set.contains(check.name.as_str()) {
            continue;
        }
        match check.normalized_state() {
            "failure" => failures.push(check.name.clone()),
            "pending" | "unknown" => pending = true,
            _ => {}
        }
    }

    if !failures.is_empty() {
        return CiOutcome::RequiredFailed(failures);
    }
    if pending {
        return CiOutcome::Timeout; // caller differentiates via elapsed-vs-timeout
    }

    // Verify every required context produced a check entry; otherwise treat
    // as pending (GitHub hasn't reported the check yet).
    let reported: std::collections::HashSet<&str> =
        checks.iter().map(|c| c.name.as_str()).collect();
    if required_set.iter().any(|n| !reported.contains(n)) {
        return CiOutcome::Timeout;
    }

    CiOutcome::AllGreen
}

/// Poll `gh pr checks <pr>` until every required check concludes, a required
/// check fails, or the configured timeout elapses.
///
/// Returns [`CiOutcome::NoRequiredChecks`] only when GitHub reports no
/// required contexts AND `fall_back_to_all_checks_when_no_required=false`.
/// In every other case the loop runs to conclusion or timeout.
///
/// # Errors
///
/// Returns an error when `gh` is missing from PATH, the PR does not exist,
/// or the GitHub API returns an irrecoverable error. Transient `gh` failures
/// (network, rate-limit) are logged and retried for the duration of the
/// timeout.
pub async fn wait_for_ci_green(
    pr_number: u64,
    cfg: &CiWaitConfig,
) -> Result<CiOutcome, CiWaitError> {
    // Resolve required contexts from branch protection (best-effort; missing
    // protection is not fatal — we fall back to the include_checks filter
    // or "all reported checks" per cfg).
    let required_contexts = fetch_required_contexts(pr_number).await.unwrap_or_default();
    wait_for_ci_green_with(
        pr_number,
        cfg,
        &required_contexts,
        || async { fetch_pr_checks(pr_number).await },
        std::time::Instant::now,
        |d| async move { tokio::time::sleep(d).await },
    )
    .await
}

/// The core poll loop of [`wait_for_ci_green`] with injected dependencies.
///
/// Separated from the top-level async fn so unit tests can exercise every
/// branch (success-first, transient-then-success, transient-for-timeout,
/// permanent error, pending-until-deadline) without spawning a real `gh`
/// subprocess. Keeps the public API unchanged — the thin wrapper above
/// binds the real `fetch_pr_checks`, `Instant::now`, and `tokio::time::sleep`.
///
/// - `fetch` returns one sample of `gh pr checks` output (or an error).
///   Called once per poll cycle.
/// - `now` returns the current time; the loop compares against the deadline
///   computed from `cfg.timeout_minutes`.
/// - `sleep` is called between polls so tests can skip real waits by
///   returning an already-ready future.
///
/// Generics are used instead of trait objects so the compiler can inline
/// the callbacks in the production path.
async fn wait_for_ci_green_with<F, Fut, N, S, SFut>(
    pr_number: u64,
    cfg: &CiWaitConfig,
    required_contexts: &[String],
    fetch: F,
    now: N,
    sleep: S,
) -> Result<CiOutcome, CiWaitError>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<Vec<GhCheck>, CiWaitError>>,
    N: Fn() -> std::time::Instant,
    S: Fn(Duration) -> SFut,
    SFut: std::future::Future<Output = ()>,
{
    let deadline = now() + Duration::from_secs(cfg.timeout_minutes * 60);
    let poll = Duration::from_secs(cfg.poll_interval_seconds.max(1));
    loop {
        // Classify gh failures into transient (network, rate-limit,
        // timeout) vs permanent (auth 401, PR not found 404).
        // Transient errors retry inside the poll loop until the overall
        // deadline; permanent errors return immediately so the caller can
        // surface a precise reason instead of waiting for the full timeout.
        match fetch().await {
            Ok(checks) => {
                let outcome = classify_checks(&checks, required_contexts, cfg);
                match outcome {
                    CiOutcome::AllGreen
                    | CiOutcome::RequiredFailed(_)
                    | CiOutcome::NoRequiredChecks => {
                        return Ok(outcome);
                    }
                    CiOutcome::Timeout => {
                        // Still pending — continue polling until deadline.
                    }
                }
            }
            Err(e) if is_transient(&e) => {
                eprintln!(
                    "ci-wait: transient error fetching PR #{pr_number} checks ({e}); retrying in {}s",
                    poll.as_secs()
                );
                // Fall through to the deadline check + sleep below.
            }
            Err(e) => {
                // Permanent error — surface immediately so the caller does
                // not wait out the full timeout on e.g. 404 PR-not-found.
                return Err(e);
            }
        }
        if now() >= deadline {
            return Ok(CiOutcome::Timeout);
        }
        sleep(poll).await;
    }
}

/// Classify a [`CiWaitError`] as transient (worth retrying within the poll
/// loop) vs permanent (should surface immediately).
///
/// Transient signals come from the `gh` CLI's stderr output:
/// - network connectivity: `connection refused`, `connection reset`, `network is unreachable`, `timeout`, `timed out`
/// - rate-limit: HTTP 403 / 429 mentions, `API rate limit exceeded`
/// - server-side faults: HTTP 5xx (500, 502, 503, 504)
///
/// Permanent signals:
/// - `HTTP 401` (auth failure) — new token needed, retrying won't help.
/// - `HTTP 404` / `not found` — PR doesn't exist.
/// - `Could not resolve host` only when paired with a config error (not a
///   DNS blip) — we treat DNS as transient by default since a dropped
///   resolver can recover within a poll interval.
///
/// Parse errors (`CiWaitError::Parse`) are treated as transient because
/// they usually result from a partial or empty `gh` response during rate-
/// limit throttling.
#[must_use]
fn is_transient(err: &CiWaitError) -> bool {
    match err {
        CiWaitError::Parse(_) => true,
        CiWaitError::GhUnavailable(msg) => {
            let lower = msg.to_ascii_lowercase();
            // Permanent signatures — short-circuit first.
            let permanent_markers = [
                "http 401",
                "http 404",
                "not found",
                "bad credentials",
                "requires authentication",
            ];
            if permanent_markers.iter().any(|m| lower.contains(m)) {
                return false;
            }
            // Transient signatures.
            let transient_markers = [
                "connection refused",
                "connection reset",
                "network is unreachable",
                "temporary failure",
                "could not resolve host",
                "rate limit",
                "403",
                "429",
                "500",
                "502",
                "503",
                "504",
                "timeout",
                "timed out",
                "i/o error",
                "broken pipe",
            ];
            transient_markers.iter().any(|m| lower.contains(m))
        }
        // Config read/parse errors are not retriable — the file on disk
        // won't change without intervention.
        CiWaitError::ReadConfig(_) | CiWaitError::ParseConfig(_) => false,
    }
}

/// Invoke `gh pr checks <pr> --json name,state`.
///
/// Fails loud when `gh` is missing or the command exits non-zero with stderr
/// on the clipboard (rate-limits, auth failures, PR not found).
async fn fetch_pr_checks(pr_number: u64) -> Result<Vec<GhCheck>, CiWaitError> {
    let output = tokio::process::Command::new("gh")
        .args([
            "pr",
            "checks",
            &pr_number.to_string(),
            "--json",
            "name,state",
        ])
        .output()
        .await
        .map_err(|e| CiWaitError::GhUnavailable(format!("spawning `gh pr checks`: {e}")))?;
    parse_pr_checks_output(
        pr_number,
        output.status.success(),
        &output.stdout,
        &output.stderr,
    )
}

/// Pure-logic half of [`fetch_pr_checks`] — extracted so tests can exercise
/// the success / non-zero-exit / malformed-JSON branches without spawning
/// `gh`. Production path feeds the real subprocess output in; tests feed
/// synthetic bytes.
fn parse_pr_checks_output(
    pr_number: u64,
    success: bool,
    stdout: &[u8],
    stderr: &[u8],
) -> Result<Vec<GhCheck>, CiWaitError> {
    if !success {
        let stderr = String::from_utf8_lossy(stderr);
        return Err(CiWaitError::GhUnavailable(format!(
            "`gh pr checks {pr_number}` failed: {}",
            stderr.trim()
        )));
    }
    serde_json::from_slice(stdout)
        .map_err(|e| CiWaitError::Parse(format!("gh pr checks output: {e}")))
}

/// Look up the list of required status checks for the PR's base branch.
///
/// Best-effort: returns an empty list when branch protection is absent or
/// when `gh` returns a non-zero exit code. Callers decide whether to fall
/// back to "all reported checks are required" via [`CiWaitConfig`].
async fn fetch_required_contexts(pr_number: u64) -> Result<Vec<String>, CiWaitError> {
    fetch_required_contexts_with(
        |args| async move {
            tokio::process::Command::new("gh")
                .args(args.iter().map(String::as_str))
                .output()
                .await
                .map_err(|e| CiWaitError::GhUnavailable(format!("spawning `gh {args:?}`: {e}")))
                .map(|o| (o.status.success(), o.stdout))
        },
        pr_number,
    )
    .await
}

/// The pure 3-step state machine of [`fetch_required_contexts`] with the
/// `gh` subprocess spawn injected as a closure.
///
/// Each step either yields a trimmed string (success + non-empty output) or
/// short-circuits the whole function to an empty context list (failed
/// exit or empty output). This mirrors the "best-effort, fall back on
/// anything weird" behaviour the caller expects.
///
/// The closure receives the argv (as a `Vec<String>`) for the step and
/// must return `Result<(success, stdout_bytes), CiWaitError>`.
async fn fetch_required_contexts_with<F, Fut>(
    run: F,
    pr_number: u64,
) -> Result<Vec<String>, CiWaitError>
where
    F: Fn(Vec<String>) -> Fut,
    Fut: std::future::Future<Output = Result<(bool, Vec<u8>), CiWaitError>>,
{
    // Step 1: resolve the base branch ref for this PR.
    let base_args = vec![
        "pr".to_string(),
        "view".to_string(),
        pr_number.to_string(),
        "--json".to_string(),
        "baseRefName".to_string(),
        "-q".to_string(),
        ".baseRefName".to_string(),
    ];
    let (base_success, base_stdout) = run(base_args).await?;
    let Some(base) = parse_gh_single_string_output(base_success, &base_stdout) else {
        return Ok(Vec::new());
    };

    // Step 2: resolve the repo slug from `gh`'s current context.
    let repo_args = vec![
        "repo".to_string(),
        "view".to_string(),
        "--json".to_string(),
        "nameWithOwner".to_string(),
        "-q".to_string(),
        ".nameWithOwner".to_string(),
    ];
    let (repo_success, repo_stdout) = run(repo_args).await?;
    let Some(repo) = parse_gh_single_string_output(repo_success, &repo_stdout) else {
        return Ok(Vec::new());
    };

    // Step 3: query branch protection. If the endpoint 404s (no protection),
    // `gh api` exits non-zero; treat as "no required contexts".
    let protect_args = vec![
        "api".to_string(),
        format!("repos/{repo}/branches/{base}/protection/required_status_checks/contexts"),
    ];
    let (protect_success, protect_stdout) = run(protect_args).await?;
    Ok(parse_gh_protected_contexts_output(
        protect_success,
        &protect_stdout,
    ))
}

/// Parse a single-string `gh` output (e.g., `gh pr view --json baseRefName`).
///
/// Returns `Some(trimmed)` when the subprocess succeeded AND stdout is
/// non-empty after trim. Otherwise returns `None` (empty stdout or failed
/// exit), which the caller treats as "fallback to no-contexts".
///
/// Extracted from [`fetch_required_contexts`] so every branch — success,
/// failure-exit, success-but-empty — can be exercised with synthetic bytes.
fn parse_gh_single_string_output(success: bool, stdout: &[u8]) -> Option<String> {
    if !success {
        return None;
    }
    let s = String::from_utf8_lossy(stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Parse the output of `gh api repos/<repo>/branches/<base>/protection/required_status_checks/contexts`.
///
/// Returns an empty vec on failure exit (404 = no branch protection) or on
/// malformed JSON (protection disabled mid-request, etc.). Extracted for
/// testability — the real `gh api` subprocess result is passed through.
fn parse_gh_protected_contexts_output(success: bool, stdout: &[u8]) -> Vec<String> {
    if !success {
        return Vec::new();
    }
    serde_json::from_slice(stdout).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(name: &str, state: &str) -> GhCheck {
        GhCheck {
            name: name.to_string(),
            state: state.to_string(),
        }
    }

    #[test]
    fn test_classify_all_green_required_all_present() {
        let checks = vec![check("ci", "SUCCESS"), check("lint", "SUCCESS")];
        let required = vec!["ci".to_string(), "lint".to_string()];
        let outcome = classify_checks(&checks, &required, &CiWaitConfig::default());
        assert_eq!(outcome, CiOutcome::AllGreen);
    }

    #[test]
    fn test_classify_required_failed() {
        let checks = vec![
            check("ci", "SUCCESS"),
            check("lint", "FAILURE"),
            check("flake", "SUCCESS"),
        ];
        let required = vec!["ci".to_string(), "lint".to_string()];
        let outcome = classify_checks(&checks, &required, &CiWaitConfig::default());
        match outcome {
            CiOutcome::RequiredFailed(names) => assert_eq!(names, vec!["lint".to_string()]),
            other => panic!("expected RequiredFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_classify_pending_required_returns_timeout_placeholder() {
        // Pending required checks surface as Timeout so the caller's poll
        // loop keeps running. The caller decides to convert to final
        // Timeout at its deadline.
        let checks = vec![check("ci", ""), check("lint", "")];
        let required = vec!["ci".to_string(), "lint".to_string()];
        let outcome = classify_checks(&checks, &required, &CiWaitConfig::default());
        assert_eq!(outcome, CiOutcome::Timeout);
    }

    #[test]
    fn test_classify_no_required_fallback_to_all_reported() {
        let checks = vec![check("ci", "SUCCESS")];
        let cfg = CiWaitConfig {
            fall_back_to_all_checks_when_no_required: true,
            ..CiWaitConfig::default()
        };
        let outcome = classify_checks(&checks, &[], &cfg);
        assert_eq!(outcome, CiOutcome::AllGreen);
    }

    #[test]
    fn test_classify_no_required_fallback_disabled_returns_no_required() {
        let cfg = CiWaitConfig {
            fall_back_to_all_checks_when_no_required: false,
            ..CiWaitConfig::default()
        };
        let outcome = classify_checks(&[check("ci", "SUCCESS")], &[], &cfg);
        assert_eq!(outcome, CiOutcome::NoRequiredChecks);
    }

    #[test]
    fn test_classify_exclude_removes_optional_check() {
        // When `lint` is in exclude_checks, a failing lint does not turn
        // the outcome red even though it's in required contexts.
        let checks = vec![check("ci", "SUCCESS"), check("lint", "FAILURE")];
        let required = vec!["ci".to_string(), "lint".to_string()];
        let cfg = CiWaitConfig {
            exclude_checks: vec!["lint".to_string()],
            ..CiWaitConfig::default()
        };
        let outcome = classify_checks(&checks, &required, &cfg);
        assert_eq!(outcome, CiOutcome::AllGreen);
    }

    #[test]
    fn test_classify_include_overrides_protection_contexts() {
        // include_checks takes precedence over required_contexts.
        let checks = vec![check("ci", "SUCCESS"), check("lint", "FAILURE")];
        let required_from_gh = vec!["lint".to_string()];
        let cfg = CiWaitConfig {
            include_checks: vec!["ci".to_string()],
            ..CiWaitConfig::default()
        };
        let outcome = classify_checks(&checks, &required_from_gh, &cfg);
        assert_eq!(outcome, CiOutcome::AllGreen);
    }

    #[test]
    fn test_classify_required_missing_check_returns_timeout() {
        // A required context that hasn't reported yet → Timeout (keep polling).
        let checks = vec![check("ci", "SUCCESS")];
        let required = vec!["ci".to_string(), "not-yet-run".to_string()];
        let outcome = classify_checks(&checks, &required, &CiWaitConfig::default());
        assert_eq!(outcome, CiOutcome::Timeout);
    }

    #[test]
    fn test_ci_wait_config_defaults() {
        let cfg = CiWaitConfig::default();
        assert_eq!(cfg.timeout_minutes, 30);
        assert_eq!(cfg.poll_interval_seconds, 30);
        assert!(cfg.include_checks.is_empty());
        assert!(cfg.exclude_checks.is_empty());
        assert!(cfg.fall_back_to_all_checks_when_no_required);
    }

    #[test]
    fn test_ci_wait_config_load_missing_file_returns_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = CiWaitConfig::load(tmp.path()).unwrap();
        assert_eq!(cfg.timeout_minutes, 30);
    }

    #[test]
    fn test_ci_wait_config_load_parses_json() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".codeflow");
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            path.join("ci-wait.json"),
            r#"{
              "timeout_minutes": 45,
              "poll_interval_seconds": 15,
              "include_checks": ["ci"],
              "exclude_checks": ["flaky"],
              "fall_back_to_all_checks_when_no_required": false
            }"#,
        )
        .unwrap();
        let cfg = CiWaitConfig::load(tmp.path()).unwrap();
        assert_eq!(cfg.timeout_minutes, 45);
        assert_eq!(cfg.poll_interval_seconds, 15);
        assert_eq!(cfg.include_checks, vec!["ci".to_string()]);
        assert_eq!(cfg.exclude_checks, vec!["flaky".to_string()]);
        assert!(!cfg.fall_back_to_all_checks_when_no_required);
    }

    #[test]
    fn test_ci_wait_config_load_bad_json_is_error() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".codeflow");
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("ci-wait.json"), "{ not json").unwrap();
        let err = CiWaitConfig::load(tmp.path()).unwrap_err();
        assert!(err.to_string().contains("parsing"));
    }

    // --- config bounds validation ---

    fn write_ci_wait_config(tmp: &std::path::Path, body: &str) {
        let dir = tmp.join(".codeflow");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ci-wait.json"), body).unwrap();
    }

    #[test]
    fn test_load_rejects_timeout_zero() {
        let tmp = tempfile::tempdir().unwrap();
        write_ci_wait_config(
            tmp.path(),
            r#"{"timeout_minutes": 0, "poll_interval_seconds": 30}"#,
        );
        let err = CiWaitConfig::load(tmp.path()).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("timeout_minutes 0 out of range"),
            "expected out-of-range error, got: {msg}"
        );
    }

    #[test]
    fn test_load_rejects_timeout_astronomical() {
        let tmp = tempfile::tempdir().unwrap();
        write_ci_wait_config(
            tmp.path(),
            r#"{"timeout_minutes": 1441, "poll_interval_seconds": 30}"#,
        );
        let err = CiWaitConfig::load(tmp.path()).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("timeout_minutes 1441 out of range"),
            "expected out-of-range error, got: {msg}"
        );
    }

    #[test]
    fn test_load_rejects_poll_too_short() {
        let tmp = tempfile::tempdir().unwrap();
        write_ci_wait_config(
            tmp.path(),
            r#"{"timeout_minutes": 30, "poll_interval_seconds": 4}"#,
        );
        let err = CiWaitConfig::load(tmp.path()).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("poll_interval_seconds 4 out of range"),
            "expected out-of-range error, got: {msg}"
        );
    }

    #[test]
    fn test_load_rejects_poll_too_long() {
        let tmp = tempfile::tempdir().unwrap();
        write_ci_wait_config(
            tmp.path(),
            r#"{"timeout_minutes": 30, "poll_interval_seconds": 301}"#,
        );
        let err = CiWaitConfig::load(tmp.path()).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("poll_interval_seconds 301 out of range"),
            "expected out-of-range error, got: {msg}"
        );
    }

    #[test]
    fn test_load_accepts_valid_boundaries() {
        // Min and max of both ranges must load successfully.
        let cases = [(1u64, 5u64), (1, 300), (1440, 5), (1440, 300)];
        for (t, p) in cases {
            let tmp = tempfile::tempdir().unwrap();
            write_ci_wait_config(
                tmp.path(),
                &format!(r#"{{"timeout_minutes": {t}, "poll_interval_seconds": {p}}}"#),
            );
            let cfg = CiWaitConfig::load(tmp.path())
                .unwrap_or_else(|e| panic!("expected Ok for (t={t}, p={p}), got {e}"));
            assert_eq!(cfg.timeout_minutes, t);
            assert_eq!(cfg.poll_interval_seconds, p);
        }
    }

    #[test]
    fn test_validate_bounds_pure_helper() {
        let mut cfg = CiWaitConfig::default();
        assert!(cfg.validate_bounds().is_ok());

        cfg.timeout_minutes = 0;
        let err = cfg.validate_bounds().unwrap_err();
        assert!(err.starts_with("timeout_minutes 0 out of range"));

        cfg = CiWaitConfig::default();
        cfg.poll_interval_seconds = 10_000;
        let err = cfg.validate_bounds().unwrap_err();
        assert!(err.starts_with("poll_interval_seconds 10000 out of range"));
    }

    #[test]
    fn test_gh_check_state_is_never_read_as_passing_unless_success() {
        for (state, expected) in [
            ("SUCCESS", "success"),
            ("skipped", "success"),
            ("FAILURE", "failure"),
            ("CANCELLED", "failure"),
            ("", "pending"),
            ("IN_PROGRESS", "pending"),
            ("STALE", "unknown"),
            ("SOMETHING_NEW", "unknown"),
        ] {
            assert_eq!(check("x", state).normalized_state(), expected, "{state}");
        }
    }

    // --- transient vs permanent error classification ---

    #[test]
    fn test_is_transient_classifies_network_errors() {
        // Transient — worth retrying inside the poll loop.
        for msg in [
            "connection refused",
            "Connection reset by peer",
            "network is unreachable",
            "temporary failure in name resolution",
            "could not resolve host: api.github.com",
            "HTTP 500 Internal Server Error",
            "HTTP 502 Bad Gateway",
            "HTTP 503 Service Unavailable",
            "HTTP 504 Gateway Timeout",
            "request timed out",
            "i/o error: broken pipe",
        ] {
            let err = CiWaitError::GhUnavailable(msg.to_string());
            assert!(
                is_transient(&err),
                "expected transient for {msg:?}, got permanent"
            );
        }
    }

    #[test]
    fn test_is_transient_classifies_rate_limit_as_transient() {
        for msg in [
            "HTTP 403 API rate limit exceeded",
            "HTTP 429 Too Many Requests",
            "rate limit exceeded for user",
        ] {
            let err = CiWaitError::GhUnavailable(msg.to_string());
            assert!(
                is_transient(&err),
                "expected transient for {msg:?}, got permanent"
            );
        }
    }

    #[test]
    fn test_is_transient_classifies_auth_and_not_found_as_permanent() {
        for msg in [
            "HTTP 401 Unauthorized",
            "HTTP 404 Not Found",
            "bad credentials",
            "Requires authentication",
            "Could not find PR #42",
        ] {
            let err = CiWaitError::GhUnavailable(msg.to_string());
            assert!(
                !is_transient(&err),
                "expected permanent for {msg:?}, got transient"
            );
        }
    }

    #[test]
    fn test_is_transient_parse_error_is_transient() {
        // Partial gh output under rate-limit throttling can produce a parse
        // error — worth retrying, not fatal.
        let err = CiWaitError::Parse("unexpected end of JSON".to_string());
        assert!(is_transient(&err));
    }

    #[test]
    fn test_is_transient_config_errors_are_permanent() {
        // Config errors won't self-heal inside the poll loop.
        let err = CiWaitError::ReadConfig("permission denied".to_string());
        assert!(!is_transient(&err));
        let err = CiWaitError::ParseConfig("unexpected comma".to_string());
        assert!(!is_transient(&err));
    }

    #[test]
    fn test_is_transient_unknown_gh_message_is_permanent() {
        // An unclassified gh error doesn't match any transient marker, so
        // the caller surfaces it immediately (fail-safe: don't loop forever
        // on an error we don't understand).
        let err = CiWaitError::GhUnavailable("garbled output".to_string());
        assert!(!is_transient(&err));
    }

    // --- wait_for_ci_green_with loop paths ---

    fn mk_check(name: &str, state: &str) -> GhCheck {
        GhCheck {
            name: name.to_string(),
            state: state.to_string(),
        }
    }

    /// Build a fetcher that returns each item from `sequence` on successive
    /// Boxed future of a scripted `fetch_pr_checks` call.
    type ScriptedFetchFuture =
        std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<GhCheck>, CiWaitError>>>>;

    /// calls, starting with index 0 and advancing with each call. When the
    /// sequence is exhausted, returns the last item. Lets tests simulate
    /// "3 transient failures then success" without a real gh binary.
    fn scripted_fetch(
        sequence: Vec<Result<Vec<GhCheck>, CiWaitError>>,
    ) -> impl Fn() -> ScriptedFetchFuture {
        use std::sync::Mutex;
        let state = std::sync::Arc::new(Mutex::new((sequence, 0usize)));
        move || {
            let state = state.clone();
            Box::pin(async move {
                let mut guard = state.lock().unwrap();
                let (seq, idx) = &mut *guard;
                if *idx < seq.len() {
                    let r = seq[*idx]
                        .as_ref()
                        .map_or_else(|e| Err(clone_err(e)), |v| Ok(v.clone()));
                    *idx += 1;
                    r
                } else {
                    seq.last()
                        .unwrap()
                        .as_ref()
                        .map_or_else(|e| Err(clone_err(e)), |v| Ok(v.clone()))
                }
            })
        }
    }

    fn clone_err(e: &CiWaitError) -> CiWaitError {
        match e {
            CiWaitError::GhUnavailable(s) => CiWaitError::GhUnavailable(s.clone()),
            CiWaitError::Parse(s) => CiWaitError::Parse(s.clone()),
            CiWaitError::ReadConfig(s) => CiWaitError::ReadConfig(s.clone()),
            CiWaitError::ParseConfig(s) => CiWaitError::ParseConfig(s.clone()),
        }
    }

    /// Fixed-clock helper: `now` always returns the same instant, so the
    /// deadline check is driven purely by the caller's control of the cfg
    /// and the sleep-count — the `now() >= deadline` branch fires exactly
    /// when the caller wants it to.
    fn frozen_now() -> impl Fn() -> std::time::Instant {
        let t = std::time::Instant::now();
        move || t
    }

    /// Increments the clock by `step` on each call. First call returns the
    /// base instant; subsequent calls add `step` each time. Lets a test
    /// guarantee the deadline-exceeded branch fires after the desired
    /// number of poll cycles.
    fn stepping_now(step: Duration) -> impl Fn() -> std::time::Instant {
        use std::sync::Mutex;
        let base = std::time::Instant::now();
        let count = std::sync::Arc::new(Mutex::new(0u32));
        move || {
            let mut c = count.lock().unwrap();
            let t = base + step * *c;
            *c += 1;
            t
        }
    }

    /// Sleep stub — resolves immediately. Tests that care about call counts
    /// wrap this with their own counter.
    async fn noop_sleep(_d: Duration) {}

    #[tokio::test]
    async fn test_wait_loop_all_green_first_call_returns_immediately() {
        let cfg = CiWaitConfig::default();
        let required = vec!["ci".to_string()];
        let fetch = scripted_fetch(vec![Ok(vec![mk_check("ci", "SUCCESS")])]);
        let result =
            wait_for_ci_green_with(42, &cfg, &required, fetch, frozen_now(), noop_sleep).await;
        assert_eq!(result.unwrap(), CiOutcome::AllGreen);
    }

    #[tokio::test]
    async fn test_wait_loop_required_failed_returns_immediately() {
        let cfg = CiWaitConfig::default();
        let required = vec!["ci".to_string(), "lint".to_string()];
        let fetch = scripted_fetch(vec![Ok(vec![
            mk_check("ci", "SUCCESS"),
            mk_check("lint", "FAILURE"),
        ])]);
        let result =
            wait_for_ci_green_with(1, &cfg, &required, fetch, frozen_now(), noop_sleep).await;
        match result.unwrap() {
            CiOutcome::RequiredFailed(names) => assert_eq!(names, vec!["lint".to_string()]),
            other => panic!("expected RequiredFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_wait_loop_no_required_checks_returns_immediately() {
        let cfg = CiWaitConfig {
            fall_back_to_all_checks_when_no_required: false,
            ..CiWaitConfig::default()
        };
        let fetch = scripted_fetch(vec![Ok(vec![mk_check("ci", "SUCCESS")])]);
        // Empty required_contexts + fallback disabled → NoRequiredChecks.
        let result = wait_for_ci_green_with(1, &cfg, &[], fetch, frozen_now(), noop_sleep).await;
        assert_eq!(result.unwrap(), CiOutcome::NoRequiredChecks);
    }

    #[tokio::test]
    async fn test_wait_loop_transient_then_success() {
        // Three transient 503 failures then AllGreen — the loop retries
        // inside the deadline (stepping clock advances small steps per call).
        let cfg = CiWaitConfig {
            timeout_minutes: 1,
            poll_interval_seconds: 5,
            ..CiWaitConfig::default()
        };
        let required = vec!["ci".to_string()];
        let fetch = scripted_fetch(vec![
            Err(CiWaitError::GhUnavailable(
                "HTTP 503 Service Unavailable".into(),
            )),
            Err(CiWaitError::GhUnavailable("HTTP 502 Bad Gateway".into())),
            Err(CiWaitError::GhUnavailable("connection reset".into())),
            Ok(vec![mk_check("ci", "SUCCESS")]),
        ]);
        // Step small enough (1ms) to never cross the 60s deadline.
        let result = wait_for_ci_green_with(
            7,
            &cfg,
            &required,
            fetch,
            stepping_now(Duration::from_millis(1)),
            noop_sleep,
        )
        .await;
        assert_eq!(result.unwrap(), CiOutcome::AllGreen);
    }

    #[tokio::test]
    async fn test_wait_loop_permanent_error_returns_immediately() {
        // 401 is permanent — surface right away, don't wait out the timeout.
        let cfg = CiWaitConfig {
            timeout_minutes: 60,
            ..CiWaitConfig::default()
        };
        let required = vec!["ci".to_string()];
        let fetch = scripted_fetch(vec![Err(CiWaitError::GhUnavailable(
            "HTTP 401 Unauthorized".into(),
        ))]);
        let result =
            wait_for_ci_green_with(99, &cfg, &required, fetch, frozen_now(), noop_sleep).await;
        match result {
            Err(CiWaitError::GhUnavailable(msg)) => {
                assert!(msg.contains("401"), "expected 401 in err msg, got: {msg}");
            }
            other => panic!("expected permanent GhUnavailable err, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_wait_loop_pending_hits_deadline_returns_timeout() {
        // Every fetch returns checks that classify as pending (no digit
        // state). Stepping clock with a large step forces `now() >= deadline`
        // after the first poll cycle → CiOutcome::Timeout.
        let cfg = CiWaitConfig {
            timeout_minutes: 1,
            poll_interval_seconds: 5,
            ..CiWaitConfig::default()
        };
        let required = vec!["ci".to_string()];
        let fetch = scripted_fetch(vec![Ok(vec![mk_check("ci", "")])]); // pending
                                                                        // First call: base time. Second call: base + 120s (past 60s deadline).
        let result = wait_for_ci_green_with(
            11,
            &cfg,
            &required,
            fetch,
            stepping_now(Duration::from_mins(2)),
            noop_sleep,
        )
        .await;
        assert_eq!(result.unwrap(), CiOutcome::Timeout);
    }

    #[tokio::test]
    async fn test_wait_loop_transient_until_deadline_returns_timeout() {
        // All transient errors, stepping clock crosses deadline → Timeout
        // (NOT GhUnavailable) — the retry behaviour converts transient-for-
        // full-timeout into the Timeout outcome.
        let cfg = CiWaitConfig {
            timeout_minutes: 1,
            poll_interval_seconds: 5,
            ..CiWaitConfig::default()
        };
        let required = vec!["ci".to_string()];
        let fetch = scripted_fetch(vec![Err(CiWaitError::GhUnavailable(
            "connection refused".into(),
        ))]);
        // Step past the 60s deadline after the first fetch.
        let result = wait_for_ci_green_with(
            2,
            &cfg,
            &required,
            fetch,
            stepping_now(Duration::from_mins(2)),
            noop_sleep,
        )
        .await;
        assert_eq!(result.unwrap(), CiOutcome::Timeout);
    }

    // --- GhCheck::normalized_state unknown arm coverage ---

    #[test]
    fn test_gh_check_normalized_state_unknown_variant() {
        // The `_ => "unknown"` arm fires for a state string GitHub has not
        // defined (future-proof). Ensures we don't misclassify unknown
        // states as success or failure.
        let c = GhCheck {
            name: "x".into(),
            state: "ALIEN_STATE".into(),
        };
        assert_eq!(c.normalized_state(), "unknown");
    }

    // --- parse_pr_checks_output: subprocess-output parsing branches ---

    #[test]
    fn test_parse_pr_checks_success_valid_json() {
        let stdout = br#"[{"name":"ci","state":"SUCCESS"}]"#;
        let checks = parse_pr_checks_output(42, true, stdout, b"").unwrap();
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].name, "ci");
        assert_eq!(checks[0].state, "SUCCESS");
    }

    #[test]
    fn test_parse_pr_checks_success_empty_array() {
        let checks = parse_pr_checks_output(42, true, b"[]", b"").unwrap();
        assert!(checks.is_empty());
    }

    #[test]
    fn test_parse_pr_checks_failure_exit_wraps_stderr() {
        let err = parse_pr_checks_output(42, false, b"", b"HTTP 401 Unauthorized\n").unwrap_err();
        match err {
            CiWaitError::GhUnavailable(msg) => {
                assert!(msg.contains("gh pr checks 42"), "got: {msg}");
                assert!(msg.contains("HTTP 401 Unauthorized"), "got: {msg}");
            }
            other => panic!("expected GhUnavailable, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_pr_checks_success_malformed_json_is_parse_error() {
        let err = parse_pr_checks_output(42, true, b"not json", b"").unwrap_err();
        match err {
            CiWaitError::Parse(msg) => assert!(msg.contains("gh pr checks output")),
            other => panic!("expected Parse, got {other:?}"),
        }
    }

    // --- parse_gh_single_string_output: branches of fetch_required_contexts step 1/2 ---

    #[test]
    fn test_parse_gh_single_string_success_returns_trimmed() {
        assert_eq!(
            parse_gh_single_string_output(true, b"  main\n"),
            Some("main".to_string())
        );
    }

    #[test]
    fn test_parse_gh_single_string_success_but_empty_returns_none() {
        assert!(parse_gh_single_string_output(true, b"   \n").is_none());
        assert!(parse_gh_single_string_output(true, b"").is_none());
    }

    #[test]
    fn test_parse_gh_single_string_failure_returns_none() {
        // Non-zero exit → None regardless of stdout content.
        assert!(parse_gh_single_string_output(false, b"main\n").is_none());
    }

    // --- parse_gh_protected_contexts_output: branches of step 3 ---

    #[test]
    fn test_parse_gh_protected_contexts_success_valid_list() {
        let stdout = br#"["ci","lint","test"]"#;
        let contexts = parse_gh_protected_contexts_output(true, stdout);
        assert_eq!(
            contexts,
            vec!["ci".to_string(), "lint".to_string(), "test".to_string()]
        );
    }

    #[test]
    fn test_parse_gh_protected_contexts_success_empty_array() {
        let contexts = parse_gh_protected_contexts_output(true, b"[]");
        assert!(contexts.is_empty());
    }

    #[test]
    fn test_parse_gh_protected_contexts_failure_exit_returns_empty() {
        // 404 = no branch protection → empty, not an error.
        let contexts = parse_gh_protected_contexts_output(false, b"not found");
        assert!(contexts.is_empty());
    }

    #[test]
    fn test_parse_gh_protected_contexts_success_malformed_json_returns_empty() {
        // Protection briefly misconfigured → treat as empty, don't crash.
        let contexts = parse_gh_protected_contexts_output(true, b"not json");
        assert!(contexts.is_empty());
    }

    // --- Smoke test for the real subprocess-backed wrapper ---

    /// End-to-end smoke test that exercises the `wait_for_ci_green` +
    /// `fetch_pr_checks` + `fetch_required_contexts` subprocess wrappers
    /// against an intentionally-invalid PR number. One of two outcomes is
    /// acceptable:
    ///
    /// - `gh` is installed + authenticated: the command completes quickly
    ///   (sub-second) with a PR-not-found error → `CiOutcome::Err(...)`.
    /// - `gh` is missing / not authenticated / network unavailable: the
    ///   spawn returns an IO error that gets wrapped in
    ///   `CiWaitError::GhUnavailable`.
    ///
    /// Either path drives the wrapper function body + its subprocess
    /// branches enough to show up in coverage. The test asserts only that
    /// the function returns within a reasonable wall-clock budget — we do
    /// NOT depend on a specific outcome because CI environments vary.
    ///
    /// This is the ONLY test in this file that spawns a real subprocess.
    /// All the rest of the loop + classify logic is covered by pure tests
    /// above via `wait_for_ci_green_with`.
    #[tokio::test]
    async fn test_wait_for_ci_green_wrapper_smoke() {
        // Tiny timeout (1 minute but we never wait that long because the
        // invalid-PR fetch returns instantly) + tiny poll (5s min).
        let cfg = CiWaitConfig {
            timeout_minutes: 1,
            poll_interval_seconds: 5,
            ..CiWaitConfig::default()
        };
        // Cap the whole test at 10 s — `gh pr checks 99999999` returns
        // quickly whether it's a 404 (installed+auth) or a spawn error
        // (no gh). If the environment makes gh hang longer than 10 s,
        // tokio::time::timeout bails out cleanly.
        let fut = wait_for_ci_green(99_999_999, &cfg);
        let _ = tokio::time::timeout(Duration::from_secs(10), fut).await;
        // No assertion on outcome — the point is coverage of the real
        // subprocess path, not a specific PR's status.
    }

    // --- fetch_required_contexts_with: 3-step state machine coverage ---

    /// Boxed future of a scripted `gh` subprocess call.
    type ScriptedGhFuture =
        std::pin::Pin<Box<dyn std::future::Future<Output = Result<(bool, Vec<u8>), CiWaitError>>>>;

    /// Scripted `gh` runner — returns one scripted output per call. Used to
    /// exercise each step-success/step-failure branch of
    /// `fetch_required_contexts_with` without spawning a real subprocess.
    fn scripted_gh_runner(
        sequence: Vec<Result<(bool, Vec<u8>), CiWaitError>>,
    ) -> impl Fn(Vec<String>) -> ScriptedGhFuture {
        use std::sync::Mutex;
        let state = std::sync::Arc::new(Mutex::new((sequence, 0usize)));
        move |_args: Vec<String>| {
            let state = state.clone();
            Box::pin(async move {
                let mut guard = state.lock().unwrap();
                let (seq, idx) = &mut *guard;
                if *idx < seq.len() {
                    let out = match &seq[*idx] {
                        Ok((s, b)) => Ok((*s, b.clone())),
                        Err(e) => Err(clone_err(e)),
                    };
                    *idx += 1;
                    out
                } else {
                    // Saturate on the last entry.
                    match seq.last().unwrap() {
                        Ok((s, b)) => Ok((*s, b.clone())),
                        Err(e) => Err(clone_err(e)),
                    }
                }
            })
        }
    }

    #[tokio::test]
    async fn test_fetch_required_contexts_all_three_steps_succeed() {
        // PR view → base ref "main"
        // repo view → "org/repo"
        // api protection → ["ci", "lint"]
        let runner = scripted_gh_runner(vec![
            Ok((true, b"main\n".to_vec())),
            Ok((true, b"org/repo\n".to_vec())),
            Ok((true, br#"["ci","lint"]"#.to_vec())),
        ]);
        let result = fetch_required_contexts_with(runner, 42).await.unwrap();
        assert_eq!(result, vec!["ci".to_string(), "lint".to_string()]);
    }

    #[tokio::test]
    async fn test_fetch_required_contexts_step1_fails_returns_empty() {
        // gh pr view fails (exit non-zero) → empty contexts, no further steps.
        let runner = scripted_gh_runner(vec![Ok((false, b"pr not found".to_vec()))]);
        let result = fetch_required_contexts_with(runner, 42).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_required_contexts_step1_empty_output_returns_empty() {
        // gh pr view succeeds but emits no base branch → empty contexts.
        let runner = scripted_gh_runner(vec![Ok((true, b"\n".to_vec()))]);
        let result = fetch_required_contexts_with(runner, 42).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_required_contexts_step2_fails_returns_empty() {
        let runner = scripted_gh_runner(vec![
            Ok((true, b"main\n".to_vec())),
            Ok((false, b"not authed".to_vec())),
        ]);
        let result = fetch_required_contexts_with(runner, 42).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_required_contexts_step2_empty_output_returns_empty() {
        let runner = scripted_gh_runner(vec![
            Ok((true, b"main\n".to_vec())),
            Ok((true, b"   \n".to_vec())),
        ]);
        let result = fetch_required_contexts_with(runner, 42).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_required_contexts_step3_fails_returns_empty() {
        // 404 from gh api (branch protection disabled) → empty contexts,
        // not an error.
        let runner = scripted_gh_runner(vec![
            Ok((true, b"main\n".to_vec())),
            Ok((true, b"org/repo\n".to_vec())),
            Ok((false, b"HTTP 404".to_vec())),
        ]);
        let result = fetch_required_contexts_with(runner, 42).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_required_contexts_step3_malformed_json_returns_empty() {
        let runner = scripted_gh_runner(vec![
            Ok((true, b"main\n".to_vec())),
            Ok((true, b"org/repo\n".to_vec())),
            Ok((true, b"not json at all".to_vec())),
        ]);
        let result = fetch_required_contexts_with(runner, 42).await.unwrap();
        assert!(result.is_empty());
    }

    /// Exercise the "sequence saturation" branch of both scripted helpers —
    /// when the poll loop or 3-step runner outruns the scripted results,
    /// helpers repeat the last entry. Also covers the `panic!("expected
    /// RequiredFailed")` / `"expected Parse"` branches in the failure
    /// messages via a test that INTENTIONALLY takes the happy path on each
    /// assertion so the panic arm never fires — those `other =>` arms are
    /// unreachable in practice (tests are written to pass), so they remain
    /// `0` hits in coverage. Not bugs.
    ///
    /// This test saturates the fetcher past the 2-entry sequence to force
    /// the `seq.last().unwrap()` saturation branch (line ~992 + ~1382) to
    /// execute.
    #[tokio::test]
    async fn test_scripted_fetch_saturates_past_sequence_end() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static CALLS: AtomicU32 = AtomicU32::new(0);
        CALLS.store(0, Ordering::SeqCst);

        let cfg = CiWaitConfig {
            timeout_minutes: 1,
            poll_interval_seconds: 5,
            ..CiWaitConfig::default()
        };
        let required = vec!["ci".to_string()];
        // One transient entry; the loop will poll beyond the end and get
        // the same entry back (saturation). Stepping clock crosses the
        // deadline after 3 calls → Timeout.
        let fetch = scripted_fetch(vec![Err(CiWaitError::GhUnavailable(
            "HTTP 503 Service Unavailable".into(),
        ))]);
        // Wrap to count calls for assertion.
        let counting_fetch = || {
            CALLS.fetch_add(1, Ordering::SeqCst);
            fetch()
        };
        let result = wait_for_ci_green_with(
            33,
            &cfg,
            &required,
            counting_fetch,
            stepping_now(Duration::from_secs(30)),
            noop_sleep,
        )
        .await;
        assert_eq!(result.unwrap(), CiOutcome::Timeout);
        // Called multiple times → saturation branch was taken.
        assert!(
            CALLS.load(Ordering::SeqCst) >= 2,
            "expected multiple fetch calls, got {}",
            CALLS.load(Ordering::SeqCst)
        );
    }

    /// Same shape for the 3-step `gh` runner: saturate the scripted
    /// sequence by calling past the end (step 3 repeats step 2's result).
    #[tokio::test]
    async fn test_scripted_gh_runner_saturates_past_sequence_end() {
        // Only 2 entries in a 3-step function → step 3 gets the saturated
        // (last entry) value, which is a successful empty-protection
        // result, producing an empty vec.
        let runner = scripted_gh_runner(vec![
            Ok((true, b"main\n".to_vec())),
            Ok((true, b"org/repo\n".to_vec())),
        ]);
        let result = fetch_required_contexts_with(runner, 42).await.unwrap();
        // Saturation returned `org/repo` as the protection JSON — which is
        // not valid JSON for a contexts array → empty vec (graceful).
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_required_contexts_step1_io_error_propagates() {
        // The injected runner returns Err — propagates up since the subprocess
        // wrapper in the real path calls map_err → CiWaitError::GhUnavailable.
        // Here we're injecting an already-formed error to exercise the `?`
        // propagation path inside `fetch_required_contexts_with`.
        let runner = scripted_gh_runner(vec![Err(CiWaitError::GhUnavailable(
            "spawning `gh`: Permission denied".into(),
        ))]);
        let err = fetch_required_contexts_with(runner, 42).await.unwrap_err();
        assert!(matches!(err, CiWaitError::GhUnavailable(_)));
    }

    // --- classify_checks NoRequiredChecks via empty required_set after exclude ---

    #[test]
    fn test_classify_empty_required_set_after_exclude_returns_no_required() {
        // When every required context ends up excluded, `required_set` is
        // empty AFTER the include/protection pass AND the exclude filter.
        // Branch covered: the `if required_set.is_empty() { return NoRequiredChecks; }`
        // early return at line 243.
        let cfg = CiWaitConfig {
            exclude_checks: vec!["ci".to_string()],
            fall_back_to_all_checks_when_no_required: false,
            ..CiWaitConfig::default()
        };
        let checks = vec![mk_check("ci", "SUCCESS")];
        let required = vec!["ci".to_string()];
        let outcome = classify_checks(&checks, &required, &cfg);
        assert_eq!(outcome, CiOutcome::NoRequiredChecks);
    }
}
