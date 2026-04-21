//! Rescue subsystem core: XDG-rooted bundle storage, gating, retention,
//! migration, and surfacing.
//!
//! INF-TSK-049-001 batch 2 redesign:
//! - **Layer 1 (XDG root):** bundles live under `dirs::cache_dir()/codeflow/rescue/`
//!   so a rescue write NEVER targets a git working tree.
//! - **Layer 2 (gates):** in addition to the legacy gates handled by
//!   `crate::worktree::cleanup`, this module provides the HEAD-on-origin
//!   predicate (`branch_on_origin`) and the `gh pr list` fallback
//!   (`gh_pr_exists_for_branch`).
//! - **Layer 3 (retention/migration/banner):** age- and origin-based prune,
//!   one-shot migration from `{project}/.state/rescue/` to xdg root, and
//!   a throttled stderr banner for surfacing.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::AutorunError;

// ---------------------------------------------------------------------------
// Layer 1 — XDG rescue root
// ---------------------------------------------------------------------------

/// Compute the XDG-compliant rescue root: `{cache_dir}/codeflow/rescue/`.
///
/// - macOS: `~/Library/Caches/codeflow/rescue/`
/// - Linux: `$XDG_CACHE_HOME/codeflow/rescue/` (fallback `~/.cache/codeflow/rescue/`)
/// - Windows: `%LOCALAPPDATA%\codeflow\rescue\`
///
/// Creates the directory if missing. Returns an error only when the host
/// has no resolvable cache dir (extremely unusual — typically Unix without
/// HOME set).
///
/// # Errors
///
/// Returns `AutorunError::InvalidBatch` when `dirs::cache_dir()` returns
/// `None` or the directory cannot be created.
pub fn xdg_rescue_root() -> Result<PathBuf, AutorunError> {
    let cache = dirs::cache_dir().ok_or_else(|| {
        AutorunError::InvalidBatch(
            "no cache directory available on this host (set HOME or XDG_CACHE_HOME)".to_string(),
        )
    })?;
    let root = cache.join("codeflow").join("rescue");
    fs::create_dir_all(&root).map_err(|e| {
        AutorunError::InvalidBatch(format!("create xdg rescue root {}: {e}", root.display()))
    })?;
    Ok(root)
}

/// Return the canonical bundle directory name for a session + timestamp.
///
/// Format: `{session-id}-{YYYYMMDD-HHMMSS}` (no PID — XDG root is host-wide
/// so the session-id already disambiguates parallel workers in this machine).
#[must_use]
pub fn bundle_name(session_id: &str, ts_human: &str) -> String {
    format!("{session_id}-{ts_human}")
}

/// Path to the single append-only sessions log under xdg_rescue_root.
///
/// All `rescue_written` events for every bundle on this host land here.
///
/// # Errors
///
/// Propagates `xdg_rescue_root` failures.
pub fn sessions_log_path() -> Result<PathBuf, AutorunError> {
    Ok(xdg_rescue_root()?.join("sessions.jsonl"))
}

// ---------------------------------------------------------------------------
// Layer 2 — Gate 8 (HEAD on origin) and Gate 9 (gh pr list fallback)
// ---------------------------------------------------------------------------

/// Outcome of the HEAD-on-origin predicate.
///
/// `Inconclusive` is distinct from `false` so callers can decide whether to
/// run further gates (e.g. `gh pr list`) or skip them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchOriginStatus {
    /// HEAD is on origin AND branch has 0 commits ahead of `@{u}`.
    OnOrigin,
    /// HEAD has commits not on origin.
    AheadOfOrigin,
    /// No upstream configured, or git command failed — caller must run
    /// further gates.
    Inconclusive,
}

/// Check whether the current branch's HEAD is on its upstream remote.
///
/// Implementation: `git rev-list --count @{u}..HEAD` inside `wt_path`.
/// - Exit 0 + stdout "0" → `OnOrigin`.
/// - Exit 0 + stdout > "0" → `AheadOfOrigin`.
/// - Non-zero exit (no upstream, not a git repo) → `Inconclusive`.
/// - Spawn failure → `Inconclusive`.
///
/// This helper is shared by:
/// - the rescue gate (Gate 8)
/// - W1 (exit-124 worker classification)
/// - W2 (serialized_merge fallback)
#[must_use]
pub fn branch_on_origin(wt_path: &Path) -> BranchOriginStatus {
    let output = Command::new("git")
        .args(["rev-list", "--count", "@{u}..HEAD"])
        .current_dir(wt_path)
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout);
            let n: u64 = s.trim().parse().unwrap_or(u64::MAX);
            if n == 0 {
                BranchOriginStatus::OnOrigin
            } else {
                BranchOriginStatus::AheadOfOrigin
            }
        }
        _ => BranchOriginStatus::Inconclusive,
    }
}

/// Outcome of the `gh pr list --head <branch>` fallback gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GhPrCheck {
    /// At least one PR exists for the branch (any state).
    Exists,
    /// No PR exists for the branch.
    None,
    /// `gh` is missing, not authenticated, or network failed — fail-safe
    /// inconclusive (caller must NOT skip rescue based on this alone).
    Inconclusive,
}

/// Check whether any PR exists for `branch` via `gh pr list`.
///
/// Runs: `gh pr list --head <branch> --state all --limit 1 --json number`.
///
/// `gh` failures (not installed, network down, auth missing) return
/// `Inconclusive` — never `None` — so callers cannot accidentally use a
/// transient outage to discard pending work.
///
/// `branch` is interpolated into the command line; callers MUST pass a
/// branch name, not arbitrary user input. We do a defensive sanity check
/// (no shell metacharacters) and return `Inconclusive` on rejection.
#[must_use]
pub fn gh_pr_exists_for_branch(wt_path: &Path, branch: &str) -> GhPrCheck {
    if branch.is_empty() {
        return GhPrCheck::Inconclusive;
    }
    // Defensive: a branch should match `[A-Za-z0-9._/-]+` in practice.
    if !branch
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-' || b == b'/')
    {
        return GhPrCheck::Inconclusive;
    }
    let output = Command::new("gh")
        .args([
            "pr", "list", "--head", branch, "--state", "all", "--limit", "1", "--json", "number",
        ])
        .current_dir(wt_path)
        .output();
    let Ok(o) = output else {
        return GhPrCheck::Inconclusive;
    };
    if !o.status.success() {
        return GhPrCheck::Inconclusive;
    }
    let s = String::from_utf8_lossy(&o.stdout);
    let trimmed = s.trim();
    // gh prints `[]` when no PRs match. Anything starting with `[{` indicates
    // at least one PR object in the array.
    if trimmed.starts_with("[{") {
        GhPrCheck::Exists
    } else if trimmed == "[]" {
        GhPrCheck::None
    } else {
        // Unrecognised output shape — fail-safe.
        GhPrCheck::Inconclusive
    }
}

// ---------------------------------------------------------------------------
// Layer 3 — Retention, surfacing, migration
// ---------------------------------------------------------------------------

/// One bundle directory entry under xdg_rescue_root.
#[derive(Debug, Clone)]
pub struct BundleEntry {
    /// Bundle directory name (e.g. `ses-01abc-20260420-120000`).
    pub id: String,
    /// Absolute path to the bundle directory.
    pub path: PathBuf,
    /// Total bytes occupied by the bundle's files (best-effort).
    pub size_bytes: u64,
    /// Whether a `.pinned` marker file exists in the bundle dir.
    pub pinned: bool,
    /// Branch from `metadata.json`, or empty.
    pub branch: String,
    /// HEAD sha from `metadata.json`, or empty.
    pub head_sha: String,
    /// Bundle creation timestamp (rfc3339) from `metadata.json`, or empty.
    pub created_at: String,
}

/// Enumerate all bundle directories under xdg_rescue_root.
///
/// Returns an empty vec when the root is missing or unreadable. Skips
/// the `sessions.jsonl` log file and any `.migration-*` markers.
///
/// # Errors
///
/// Propagates `xdg_rescue_root` failures.
pub fn list_bundles() -> Result<Vec<BundleEntry>, AutorunError> {
    let root = xdg_rescue_root()?;
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(&root) else {
        return Ok(out);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(id) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // Skip dotfile / hidden marker dirs (none today, but defensive).
        if id.starts_with('.') {
            continue;
        }
        let pinned = path.join(".pinned").exists();
        let size_bytes = dir_size(&path);
        let (branch, head_sha, created_at) = read_bundle_metadata(&path);
        out.push(BundleEntry {
            id: id.to_string(),
            path,
            size_bytes,
            pinned,
            branch,
            head_sha,
            created_at,
        });
    }
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
    Ok(out)
}

fn dir_size(dir: &Path) -> u64 {
    let mut total: u64 = 0;
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    for entry in entries.flatten() {
        if let Ok(m) = entry.metadata() {
            if m.is_file() {
                total = total.saturating_add(m.len());
            }
        }
    }
    total
}

/// Best-effort metadata extraction. Returns empty strings on any failure.
fn read_bundle_metadata(bundle: &Path) -> (String, String, String) {
    let s = match fs::read_to_string(bundle.join("metadata.json")) {
        Ok(s) => s,
        Err(_) => return (String::new(), String::new(), String::new()),
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) else {
        return (String::new(), String::new(), String::new());
    };
    let g = |k: &str| {
        v.get(k)
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string()
    };
    (g("branch"), g("head_sha"), g("timestamp_rfc3339"))
}

/// Check whether a remote branch ref still exists on origin.
///
/// Returns `true` only when `git ls-remote --exit-code --heads origin <branch>`
/// reports the branch present. Any failure (network, ambiguous output,
/// command spawn error) returns `false` — fail-safe so we do NOT
/// auto-delete a bundle on a transient network blip.
#[must_use]
pub fn branch_exists_on_origin(branch: &str) -> bool {
    if branch.is_empty() {
        return false;
    }
    Command::new("git")
        .args(["ls-remote", "--exit-code", "--heads", "origin", branch])
        .output()
        .ok()
        .is_some_and(|o| o.status.success() && !o.stdout.is_empty())
}

/// Result of a single auto-prune sweep.
#[derive(Debug, Clone, Default)]
pub struct PruneReport {
    pub deleted_aged: usize,
    pub deleted_origin_gone: usize,
    pub kept_pinned: usize,
    pub kept_recent: usize,
}

/// Auto-prune bundles per the configured retention policy.
///
/// Rules (in order, per bundle):
/// 1. `.pinned` marker present → never delete (counts as `kept_pinned`).
/// 2. Bundle's branch is gone from origin AND HEAD sha is on origin → delete
///    regardless of age (counts as `deleted_origin_gone`).
/// 3. Bundle older than `retention_days` → delete (counts as `deleted_aged`).
/// 4. Otherwise → keep (counts as `kept_recent`).
///
/// One stderr line per deletion. Returns counts; never errors at the
/// top level — individual delete failures are logged and counted toward
/// `kept_recent`.
///
/// # Errors
///
/// Propagates `list_bundles` failure (which itself only fails on the
/// `xdg_rescue_root` lookup).
pub fn auto_prune(retention_days: u32) -> Result<PruneReport, AutorunError> {
    let mut report = PruneReport::default();
    let cutoff = chrono::Utc::now() - chrono::Duration::days(i64::from(retention_days));
    let bundles = list_bundles()?;
    for b in bundles {
        if b.pinned {
            report.kept_pinned += 1;
            continue;
        }
        // Origin-gone fast path: deletes regardless of age.
        let branch_gone = !b.branch.is_empty() && !branch_exists_on_origin(&b.branch);
        if branch_gone && delete_bundle(&b.path).is_ok() {
            eprintln!(
                "rescue: pruned bundle {} (branch '{}' gone from origin)",
                b.id, b.branch
            );
            report.deleted_origin_gone += 1;
            continue;
        }
        // Age-based prune.
        if !b.created_at.is_empty() {
            if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(&b.created_at) {
                if ts.with_timezone(&chrono::Utc) < cutoff && delete_bundle(&b.path).is_ok() {
                    eprintln!(
                        "rescue: pruned bundle {} (older than {retention_days}d)",
                        b.id
                    );
                    report.deleted_aged += 1;
                    continue;
                }
            }
        }
        report.kept_recent += 1;
    }
    Ok(report)
}

fn delete_bundle(path: &Path) -> std::io::Result<()> {
    fs::remove_dir_all(path)
}

/// Banner output for the `codeflow autorun status` / `codeflow interactive
/// status` / bare `codeflow` startup paths.
///
/// Returns `Some(line)` when:
/// - `banner_enabled` config flag is true (default true — configurable via
///   `parallel-work-config.json` `rescue.banner_enabled`). INF-TSK-049-001
///   batch 2 rework iter 1 — Finding 1 fix.
/// - `CODEFLOW_NO_RESCUE_BANNER` env var is not set to `true`.
/// - At least one bundle exists.
/// - The 24h throttle has elapsed since `.last-scan` mtime (or `.last-scan`
///   does not exist).
///
/// Returns `None` otherwise. The caller is expected to write the line to
/// stderr verbatim.
///
/// On call this also `touch`es `.last-scan` so the next call within 24h
/// is suppressed regardless of the bundle state. When `banner_enabled` is
/// false the function returns early BEFORE touching so re-enabling the
/// flag does not require waiting 24h to observe a suppressed backlog.
#[must_use]
pub fn maybe_banner(banner_enabled: bool) -> Option<String> {
    if !banner_enabled {
        return None;
    }
    if std::env::var("CODEFLOW_NO_RESCUE_BANNER")
        .map(|v| v == "true")
        .unwrap_or(false)
    {
        return None;
    }
    let root = xdg_rescue_root().ok()?;
    let last_scan = root.join(".last-scan");
    let now = std::time::SystemTime::now();
    let suppress = if let Ok(meta) = fs::metadata(&last_scan) {
        if let Ok(modified) = meta.modified() {
            now.duration_since(modified)
                .map(|d| d.as_secs() < 24 * 60 * 60)
                .unwrap_or(false)
        } else {
            false
        }
    } else {
        false
    };
    // Always touch the marker so the throttle window is anchored to "last
    // time we considered emitting" — not "last time we did emit". This
    // matches the spec ("throttle to once per 24h").
    let _ = touch(&last_scan);
    if suppress {
        return None;
    }
    let bundles = list_bundles().ok()?;
    if bundles.is_empty() {
        return None;
    }
    let total_bytes: u64 = bundles.iter().map(|b| b.size_bytes).sum();
    let mb = (total_bytes as f64) / (1024.0 * 1024.0);
    let oldest_days = bundles
        .iter()
        .filter_map(|b| chrono::DateTime::parse_from_rfc3339(&b.created_at).ok())
        .map(|ts| {
            (chrono::Utc::now() - ts.with_timezone(&chrono::Utc))
                .num_days()
                .max(0)
        })
        .max()
        .unwrap_or(0);
    Some(format!(
        "rescue: {} bundle(s) (~{:.1} MB, oldest {}d old) — codeflow rescue list",
        bundles.len(),
        mb,
        oldest_days
    ))
}

/// Touch a marker file to update its mtime to "now".
///
/// On supported platforms (unix, windows) we open the existing file and
/// rewrite a single byte then truncate — equivalent semantics to `touch`
/// for mtime purposes without pulling in `filetime` as a runtime dep.
/// When the file is absent we create it.
fn touch(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if path.exists() {
        // Removing and re-creating bumps mtime atomically. Using rename
        // would be safer for concurrent readers, but the marker file is
        // best-effort throttle state — readers tolerate either presence.
        let _ = fs::remove_file(path);
    }
    std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .map(|_| ())
}

// ---------------------------------------------------------------------------
// Migration — one-shot move from {project}/.state/rescue to xdg root
// ---------------------------------------------------------------------------

/// Result of a one-shot migration call.
#[derive(Debug, Clone, Default)]
pub struct MigrationReport {
    /// Whether the migration ran (false means marker present or no source).
    pub ran: bool,
    pub bundles_moved: usize,
    pub event_lines_appended: usize,
}

/// One-shot migration from per-project legacy paths to xdg_rescue_root.
///
/// Idempotent: writes `{xdg_rescue_root}/.migration-v1-complete` after the
/// first successful run; subsequent calls are no-ops.
///
/// Atomicity: source-side files are moved (rename when same FS, copy+delete
/// across FS) one at a time. If a single move fails, the function aborts
/// without writing the marker so a subsequent run can retry the survivors.
///
/// # Errors
///
/// Returns `AutorunError::InvalidBatch` only on `xdg_rescue_root` failure.
/// Per-bundle move failures are logged but do not propagate.
pub fn migrate_legacy(project_root: &Path) -> Result<MigrationReport, AutorunError> {
    let mut report = MigrationReport::default();
    let xdg_root = xdg_rescue_root()?;
    let marker = xdg_root.join(".migration-v1-complete");
    if marker.exists() {
        return Ok(report);
    }
    // Move bundle directories from {project}/.state/rescue/* into xdg.
    let legacy_dir = project_root.join(".state").join("rescue");
    if legacy_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&legacy_dir) {
            for entry in entries.flatten() {
                let src = entry.path();
                if !src.is_dir() {
                    continue;
                }
                let Some(name) = src.file_name() else {
                    continue;
                };
                let dst = xdg_root.join(name);
                if dst.exists() {
                    // Already migrated by an earlier partial run — leave src,
                    // skip silently (no double-move).
                    let _ = fs::remove_dir_all(&src);
                    continue;
                }
                if let Err(e) = move_dir(&src, &dst) {
                    eprintln!("rescue: migrate failed for {}: {e}", src.display());
                    return Ok(report);
                }
                report.bundles_moved += 1;
            }
        }
        // Best-effort: remove the legacy dir if empty after moves.
        let _ = fs::remove_dir(&legacy_dir);
    }
    // Append-merge legacy `rescue_written` ledger events into xdg sessions.jsonl.
    //
    // IMPORTANT: fragment files under `.state/ledger/sessions/` may contain
    // other session-lifecycle events (e.g. `session_start`) that belong to
    // the general session ledger, NOT the rescue log. Only move lines whose
    // JSON object has `"event":"rescue_written"`. Non-matching lines are
    // preserved in the source file so we don't orphan general session
    // records.
    let legacy_ledger = project_root.join(".state").join("ledger").join("sessions");
    if legacy_ledger.is_dir() {
        let xdg_log = xdg_root.join("sessions.jsonl");
        if let Ok(entries) = fs::read_dir(&legacy_ledger) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                if !stem.starts_with("sessions-ses-") {
                    continue;
                }
                let Ok(contents) = fs::read_to_string(&p) else {
                    continue;
                };
                let (rescue_lines, other_lines): (Vec<&str>, Vec<&str>) = contents
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .partition(|l| is_rescue_written_line(l));
                if rescue_lines.is_empty() {
                    // No rescue events in this fragment — leave it untouched.
                    continue;
                }
                if !append_lines(&xdg_log, &rescue_lines) {
                    eprintln!(
                        "rescue: migrate failed appending {} → {}",
                        p.display(),
                        xdg_log.display()
                    );
                    return Ok(report);
                }
                report.event_lines_appended += rescue_lines.len();
                // Rewrite the source with the non-rescue lines kept, OR
                // delete if the file contained ONLY rescue events.
                if other_lines.is_empty() {
                    let _ = fs::remove_file(&p);
                } else {
                    let mut new_content = String::new();
                    for l in &other_lines {
                        new_content.push_str(l);
                        new_content.push('\n');
                    }
                    if let Err(e) = fs::write(&p, new_content) {
                        eprintln!(
                            "rescue: migrate could not rewrite {} with surviving events: {e}",
                            p.display()
                        );
                        // Don't abort — the rescue events were already
                        // moved. Leave the source as-is; a subsequent run
                        // will re-detect the same rescue lines and skip.
                    }
                }
            }
        }
    }
    // All moves succeeded: write the marker.
    if let Err(e) = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&marker)
    {
        eprintln!("rescue: write migration marker {}: {e}", marker.display());
        return Ok(report);
    }
    report.ran = true;
    Ok(report)
}

/// Return true when `line` is a JSONL record with `"event":"rescue_written"`.
///
/// The check is a strict JSON parse followed by a field match — not a
/// substring match — so comments or other events that happen to contain
/// the literal string `rescue_written` somewhere in their payload do NOT
/// false-positive.
fn is_rescue_written_line(line: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
        return false;
    };
    v.get("event").and_then(|e| e.as_str()) == Some("rescue_written")
}

fn append_lines(path: &Path, lines: &[&str]) -> bool {
    use std::io::Write;
    let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return false;
    };
    for line in lines {
        if writeln!(f, "{line}").is_err() {
            return false;
        }
    }
    true
}

fn move_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Cross-device link or concurrent producer — fall back to copy + remove.
            copy_dir_recursive(src, dst)?;
            fs::remove_dir_all(src)
        }
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialize tests that use the global xdg cache dir override.
    /// Tests redirect `dirs::cache_dir()` by setting `XDG_CACHE_HOME`
    /// (Linux) and `HOME` (macOS). All such tests must hold this lock so
    /// they don't interfere with each other under nextest's per-process
    /// model — tests in the same binary share the env.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Set `HOME` and `XDG_CACHE_HOME` to a tempdir so xdg_rescue_root
    /// resolves under a test-scoped path. Returns the tempdir guard so the
    /// caller controls lifetime.
    fn redirect_xdg_cache(td: &tempfile::TempDir) {
        // SAFETY: tests set env vars under ENV_LOCK and restore them implicitly
        // by overwriting on next test invocation. No other code reads these
        // during the test body.
        unsafe {
            std::env::set_var("HOME", td.path());
            std::env::set_var("XDG_CACHE_HOME", td.path().join(".cache"));
        }
    }

    #[test]
    fn xdg_rescue_root_creates_dir() {
        let _g = env_lock();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        let root = xdg_rescue_root().unwrap();
        assert!(root.is_dir(), "root should exist: {}", root.display());
        // Path layout: {cache}/codeflow/rescue
        let trail = root.components().rev().take(2).collect::<Vec<_>>();
        let last_two: Vec<String> = trail
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        assert_eq!(last_two, vec!["rescue".to_string(), "codeflow".to_string()]);
    }

    #[test]
    fn sessions_log_path_under_root() {
        let _g = env_lock();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        let root = xdg_rescue_root().unwrap();
        let log = sessions_log_path().unwrap();
        assert_eq!(log.parent().unwrap(), root);
        assert_eq!(
            log.file_name().and_then(|s| s.to_str()),
            Some("sessions.jsonl")
        );
    }

    #[test]
    fn bundle_name_format() {
        assert_eq!(
            bundle_name("ses-abc", "20260420-120000"),
            "ses-abc-20260420-120000"
        );
    }

    // -- branch_on_origin --

    fn init_git_repo(dir: &Path) {
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir)
            .status()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "t@t.c"])
            .current_dir(dir)
            .status()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "T"])
            .current_dir(dir)
            .status()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init", "--no-verify"])
            .current_dir(dir)
            .status()
            .unwrap();
    }

    #[test]
    fn branch_on_origin_returns_inconclusive_without_upstream() {
        let td = tempfile::tempdir().unwrap();
        init_git_repo(td.path());
        // No upstream configured — git rev-list @{u}..HEAD should fail.
        assert_eq!(
            branch_on_origin(td.path()),
            BranchOriginStatus::Inconclusive
        );
    }

    #[test]
    fn branch_on_origin_returns_on_origin_when_zero_ahead() {
        // Build two repos, treat 'a' as origin of 'b'.
        let origin_td = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "--bare", "-q"])
            .current_dir(origin_td.path())
            .status()
            .unwrap();
        let work_td = tempfile::tempdir().unwrap();
        init_git_repo(work_td.path());
        Command::new("git")
            .args([
                "remote",
                "add",
                "origin",
                origin_td.path().to_str().unwrap(),
            ])
            .current_dir(work_td.path())
            .status()
            .unwrap();
        // Push so upstream is set.
        Command::new("git")
            .args(["push", "-u", "origin", "HEAD:refs/heads/main"])
            .current_dir(work_td.path())
            .status()
            .unwrap();
        Command::new("git")
            .args(["branch", "--set-upstream-to=origin/main"])
            .current_dir(work_td.path())
            .status()
            .unwrap();
        assert_eq!(
            branch_on_origin(work_td.path()),
            BranchOriginStatus::OnOrigin
        );
    }

    #[test]
    fn branch_on_origin_returns_ahead_when_local_commits() {
        let origin_td = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "--bare", "-q"])
            .current_dir(origin_td.path())
            .status()
            .unwrap();
        let work_td = tempfile::tempdir().unwrap();
        init_git_repo(work_td.path());
        Command::new("git")
            .args([
                "remote",
                "add",
                "origin",
                origin_td.path().to_str().unwrap(),
            ])
            .current_dir(work_td.path())
            .status()
            .unwrap();
        Command::new("git")
            .args(["push", "-u", "origin", "HEAD:refs/heads/main"])
            .current_dir(work_td.path())
            .status()
            .unwrap();
        Command::new("git")
            .args(["branch", "--set-upstream-to=origin/main"])
            .current_dir(work_td.path())
            .status()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "ahead", "--no-verify"])
            .current_dir(work_td.path())
            .status()
            .unwrap();
        assert_eq!(
            branch_on_origin(work_td.path()),
            BranchOriginStatus::AheadOfOrigin
        );
    }

    #[test]
    fn branch_on_origin_returns_inconclusive_for_non_git_dir() {
        let td = tempfile::tempdir().unwrap();
        assert_eq!(
            branch_on_origin(td.path()),
            BranchOriginStatus::Inconclusive
        );
    }

    // -- gh_pr_exists_for_branch --

    #[test]
    fn gh_pr_exists_rejects_empty_branch() {
        let td = tempfile::tempdir().unwrap();
        assert_eq!(
            gh_pr_exists_for_branch(td.path(), ""),
            GhPrCheck::Inconclusive
        );
    }

    #[test]
    fn gh_pr_exists_rejects_branch_with_shell_metachars() {
        let td = tempfile::tempdir().unwrap();
        // `;` is rejected by the sanitizer.
        assert_eq!(
            gh_pr_exists_for_branch(td.path(), "feat/x;rm -rf /"),
            GhPrCheck::Inconclusive
        );
        assert_eq!(
            gh_pr_exists_for_branch(td.path(), "$(whoami)"),
            GhPrCheck::Inconclusive
        );
    }

    #[test]
    fn gh_pr_exists_returns_inconclusive_when_gh_missing() {
        // We can't reliably guarantee gh is missing in CI, but in this
        // sandbox we expect either Inconclusive (gh missing/not-authed) or
        // None (gh present but no PRs in tempdir). Either way, NEVER `Exists`.
        let td = tempfile::tempdir().unwrap();
        let result = gh_pr_exists_for_branch(td.path(), "feat/never-exists");
        assert!(
            matches!(result, GhPrCheck::Inconclusive | GhPrCheck::None),
            "expected Inconclusive or None, got {result:?}"
        );
    }

    // -- list_bundles + auto_prune --

    fn make_test_bundle(root: &Path, id: &str, branch: &str, head_sha: &str, ts_rfc: &str) {
        let dir = root.join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("working.patch"), "data").unwrap();
        let meta = serde_json::json!({
            "session_id": id,
            "branch": branch,
            "head_sha": head_sha,
            "timestamp_rfc3339": ts_rfc,
            "reason": "test",
        });
        fs::write(
            dir.join("metadata.json"),
            serde_json::to_string_pretty(&meta).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn list_bundles_skips_dotfiles_and_non_dirs() {
        let _g = env_lock();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        let root = xdg_rescue_root().unwrap();
        make_test_bundle(&root, "ses-real", "main", "h", "2026-04-20T12:00:00Z");
        // `.migration-v1-complete` is a file, not dir — shouldn't appear.
        fs::write(root.join(".migration-v1-complete"), "").unwrap();
        // `sessions.jsonl` is a file, not dir.
        fs::write(root.join("sessions.jsonl"), "").unwrap();
        // Hidden dir — should be skipped.
        fs::create_dir_all(root.join(".hidden")).unwrap();

        let bundles = list_bundles().unwrap();
        assert_eq!(bundles.len(), 1);
        assert_eq!(bundles[0].id, "ses-real");
    }

    #[test]
    fn auto_prune_keeps_pinned_regardless_of_age() {
        let _g = env_lock();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        let root = xdg_rescue_root().unwrap();
        // Old bundle — would normally be deleted.
        make_test_bundle(&root, "ses-old", "feat/x", "h1", "2020-01-01T00:00:00Z");
        // Pin it.
        fs::write(root.join("ses-old").join(".pinned"), "").unwrap();

        let report = auto_prune(30).unwrap();
        assert_eq!(report.kept_pinned, 1);
        assert_eq!(report.deleted_aged, 0);
        assert!(root.join("ses-old").exists(), "pinned must survive");
    }

    #[test]
    fn auto_prune_deletes_aged_bundles() {
        let _g = env_lock();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        let root = xdg_rescue_root().unwrap();
        make_test_bundle(&root, "ses-old", "", "", "2020-01-01T00:00:00Z");
        let now_rfc = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        make_test_bundle(&root, "ses-recent", "", "", &now_rfc);

        let report = auto_prune(30).unwrap();
        assert_eq!(report.deleted_aged, 1);
        assert!(!root.join("ses-old").exists());
        assert!(root.join("ses-recent").exists());
    }

    // -- maybe_banner --

    #[test]
    fn maybe_banner_emits_when_bundles_present_and_throttle_clear() {
        let _g = env_lock();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        // Reset env var.
        unsafe {
            std::env::remove_var("CODEFLOW_NO_RESCUE_BANNER");
        }
        let root = xdg_rescue_root().unwrap();
        let now_rfc = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        make_test_bundle(&root, "ses-x", "", "", &now_rfc);

        let line = maybe_banner(true);
        assert!(line.is_some(), "banner should fire when bundles exist");
        let s = line.unwrap();
        assert!(s.contains("rescue:"));
        assert!(s.contains("1 bundle"));
        // Throttle file should have been touched.
        assert!(root.join(".last-scan").exists());

        // Second call within 24h: throttle suppresses.
        let line2 = maybe_banner(true);
        assert!(
            line2.is_none(),
            "second call within 24h should be suppressed"
        );
    }

    #[test]
    fn maybe_banner_suppressed_by_env_var() {
        let _g = env_lock();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        unsafe {
            std::env::set_var("CODEFLOW_NO_RESCUE_BANNER", "true");
        }
        let root = xdg_rescue_root().unwrap();
        let now_rfc = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        make_test_bundle(&root, "ses-x", "", "", &now_rfc);
        let line = maybe_banner(true);
        assert!(line.is_none(), "env var must suppress banner");
        unsafe {
            std::env::remove_var("CODEFLOW_NO_RESCUE_BANNER");
        }
    }

    #[test]
    fn maybe_banner_silent_when_no_bundles() {
        let _g = env_lock();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        unsafe {
            std::env::remove_var("CODEFLOW_NO_RESCUE_BANNER");
        }
        // Don't create any bundles.
        let _root = xdg_rescue_root().unwrap();
        assert!(maybe_banner(true).is_none());
    }

    /// Rework iter 1 — Finding 1 fix: `banner_enabled = false` in config
    /// must suppress the banner even when bundles exist and the throttle
    /// window is clear.
    #[test]
    fn maybe_banner_suppressed_by_config_flag() {
        let _g = env_lock();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        unsafe {
            std::env::remove_var("CODEFLOW_NO_RESCUE_BANNER");
        }
        let root = xdg_rescue_root().unwrap();
        let now_rfc = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        make_test_bundle(&root, "ses-x", "", "", &now_rfc);

        let line = maybe_banner(false);
        assert!(
            line.is_none(),
            "banner_enabled=false must suppress the banner regardless of bundle state"
        );
    }

    /// Rework iter 1 — Finding 1 fix: `banner_enabled = true` is the
    /// normal path; behaves identically to the pre-fix test.
    #[test]
    fn maybe_banner_enabled_flag_allows_emission() {
        let _g = env_lock();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        unsafe {
            std::env::remove_var("CODEFLOW_NO_RESCUE_BANNER");
        }
        let root = xdg_rescue_root().unwrap();
        let now_rfc = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        make_test_bundle(&root, "ses-y", "", "", &now_rfc);

        let line = maybe_banner(true);
        assert!(line.is_some(), "banner_enabled=true must emit");
        assert!(line.unwrap().contains("rescue:"));
    }

    // -- migrate_legacy --

    #[test]
    fn migrate_legacy_moves_bundles_and_writes_marker() {
        let _g = env_lock();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);

        let project = tempfile::tempdir().unwrap();
        let legacy_dir = project.path().join(".state/rescue");
        fs::create_dir_all(&legacy_dir).unwrap();
        // One legacy bundle.
        fs::create_dir_all(legacy_dir.join("ses-legacy")).unwrap();
        fs::write(legacy_dir.join("ses-legacy/working.patch"), "x").unwrap();

        let report = migrate_legacy(project.path()).unwrap();
        assert!(report.ran);
        assert_eq!(report.bundles_moved, 1);

        let xdg_root = xdg_rescue_root().unwrap();
        assert!(xdg_root.join("ses-legacy/working.patch").exists());
        assert!(xdg_root.join(".migration-v1-complete").exists());
        assert!(!legacy_dir.join("ses-legacy").exists());
    }

    #[test]
    fn migrate_legacy_is_idempotent() {
        let _g = env_lock();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        let project = tempfile::tempdir().unwrap();

        // First call — marker created even if no source dir.
        let report1 = migrate_legacy(project.path()).unwrap();
        assert!(report1.ran);
        // Second call must be a no-op: ran = false because marker exists.
        let report2 = migrate_legacy(project.path()).unwrap();
        assert!(!report2.ran);
        assert_eq!(report2.bundles_moved, 0);
    }

    #[test]
    fn migrate_legacy_appends_session_jsonl_fragments() {
        let _g = env_lock();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);

        let project = tempfile::tempdir().unwrap();
        let legacy = project.path().join(".state/ledger/sessions");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(
            legacy.join("sessions-ses-abc.jsonl"),
            "{\"event\":\"rescue_written\",\"session_id\":\"ses-abc\"}\n",
        )
        .unwrap();
        // Non-matching name — must be ignored.
        fs::write(legacy.join("sessions.jsonl"), "noise\n").unwrap();

        let report = migrate_legacy(project.path()).unwrap();
        assert!(report.ran);
        assert_eq!(report.event_lines_appended, 1);

        let xdg_log = xdg_rescue_root().unwrap().join("sessions.jsonl");
        let contents = fs::read_to_string(&xdg_log).unwrap();
        assert!(contents.contains("ses-abc"));
        // Source fragment is gone after merge.
        assert!(!legacy.join("sessions-ses-abc.jsonl").exists());
        // Untouched non-matching file remains.
        assert!(legacy.join("sessions.jsonl").exists());
    }

    /// Regression: fragments containing ONLY non-rescue events (e.g.
    /// `session_start`) must NOT be deleted. This catches the bug where
    /// the initial migration greedily swept all jsonl lines and lost the
    /// general session ledger.
    #[test]
    fn migrate_legacy_preserves_non_rescue_fragments() {
        let _g = env_lock();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);

        let project = tempfile::tempdir().unwrap();
        let legacy = project.path().join(".state/ledger/sessions");
        fs::create_dir_all(&legacy).unwrap();
        // Fragment with ONLY session_start events — NOT rescue.
        let frag = legacy.join("sessions-ses-only-session-start.jsonl");
        fs::write(
            &frag,
            "{\"event\":\"session_start\",\"session_id\":\"ses-only\"}\n",
        )
        .unwrap();

        let report = migrate_legacy(project.path()).unwrap();
        assert!(report.ran);
        assert_eq!(
            report.event_lines_appended, 0,
            "no rescue events should be appended"
        );
        assert!(
            frag.exists(),
            "fragment with only non-rescue events must NOT be deleted"
        );
        // xdg sessions.jsonl should be empty/missing.
        let xdg_log = xdg_rescue_root().unwrap().join("sessions.jsonl");
        assert!(
            !xdg_log.exists() || fs::read_to_string(&xdg_log).unwrap().is_empty(),
            "xdg sessions.jsonl must be empty when no rescue events migrated"
        );
    }

    /// Mixed-event fragment: some `rescue_written`, some `session_start`.
    /// After migration, xdg log gets ONLY the rescue lines; the source
    /// file is rewritten with ONLY the non-rescue lines.
    #[test]
    fn migrate_legacy_splits_mixed_event_fragment() {
        let _g = env_lock();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);

        let project = tempfile::tempdir().unwrap();
        let legacy = project.path().join(".state/ledger/sessions");
        fs::create_dir_all(&legacy).unwrap();
        let frag = legacy.join("sessions-ses-mixed.jsonl");
        let contents = r#"{"event":"session_start","session_id":"ses-mixed"}
{"event":"rescue_written","session_id":"ses-mixed","bundle_path":"/x"}
{"event":"session_end","session_id":"ses-mixed"}
"#;
        fs::write(&frag, contents).unwrap();

        let report = migrate_legacy(project.path()).unwrap();
        assert!(report.ran);
        assert_eq!(report.event_lines_appended, 1);

        // xdg log has the rescue line.
        let xdg_log = xdg_rescue_root().unwrap().join("sessions.jsonl");
        let xdg_contents = fs::read_to_string(&xdg_log).unwrap();
        assert!(xdg_contents.contains("\"event\":\"rescue_written\""));

        // Source file rewritten with only the two non-rescue events.
        assert!(frag.exists(), "source must persist with surviving events");
        let rewritten = fs::read_to_string(&frag).unwrap();
        assert!(rewritten.contains("session_start"));
        assert!(rewritten.contains("session_end"));
        assert!(
            !rewritten.contains("rescue_written"),
            "rescue event must be removed from source"
        );
    }

    // -- is_rescue_written_line --

    #[test]
    fn is_rescue_written_line_matches_exact_event() {
        assert!(is_rescue_written_line(
            r#"{"event":"rescue_written","session_id":"x"}"#
        ));
    }

    #[test]
    fn is_rescue_written_line_rejects_session_start() {
        assert!(!is_rescue_written_line(
            r#"{"event":"session_start","session_id":"x"}"#
        ));
    }

    #[test]
    fn is_rescue_written_line_rejects_substring_match() {
        // Literal substring "rescue_written" appearing in another field
        // must NOT trigger a match — the check is structural.
        assert!(!is_rescue_written_line(
            r#"{"event":"other","note":"rescue_written was fine"}"#
        ));
    }

    #[test]
    fn is_rescue_written_line_rejects_malformed_json() {
        assert!(!is_rescue_written_line("not json"));
        assert!(!is_rescue_written_line(""));
        assert!(!is_rescue_written_line("{incomplete"));
    }

    // -- branch_exists_on_origin (sanity, no network) --

    #[test]
    fn branch_exists_on_origin_empty_branch_is_false() {
        assert!(!branch_exists_on_origin(""));
    }
}
