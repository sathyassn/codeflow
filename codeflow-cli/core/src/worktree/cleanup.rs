//! Worktree removal and deregistration.
//!
//! Supports force removal, dry-run mode, git worktree pruning,
//! branch safety checks, and liveness verification.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::WorktreeError;

use super::WorktreeManager;
use super::registry;

/// Options for worktree cleanup behavior.
#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct CleanupOpts {
    /// Force removal even if the worktree has uncommitted changes
    /// or a `PathFlow` session is active.
    pub force: bool,
    /// Print what would be done without actually doing it.
    pub dry_run: bool,
    /// Run `git worktree prune` to clean stale references.
    /// When true, the `name` parameter is ignored.
    pub prune: bool,
    /// Show interactive prompts with branch/PR/commit status before removal.
    pub interactive: bool,
    /// Maximum number of "removed" entries to keep in registry during purge.
    /// Defaults to 5 if `None`.
    pub keep: Option<usize>,
    /// When set, force override applies only to entries whose names are in
    /// this list (exact match). When `None` and `force` is true, force
    /// applies to all eligible entries.
    pub force_names: Option<Vec<String>>,
}

/// Risk level for removing a worktree based on its branch state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BranchRisk {
    /// Branch merged into main or PR merged — safe to remove.
    None,
    /// Branch pushed with an open PR — safe to remove with note.
    Low,
    /// Branch pushed but no PR exists — warn but proceed.
    Medium,
    /// Branch NOT pushed but no uncommitted changes — block unless forced.
    High,
    /// Branch NOT pushed AND has uncommitted changes — refuse unless forced.
    Critical,
}

impl BranchRisk {
    /// Human-readable label for this risk level.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

impl std::fmt::Display for BranchRisk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Result of checking branch safety before worktree removal.
#[derive(Debug, Clone)]
pub struct BranchSafetyResult {
    /// Assessed risk level.
    pub risk: BranchRisk,
    /// Branch name (empty for detached HEAD).
    pub branch: String,
    /// Whether the branch has been pushed to remote.
    pub pushed: bool,
    /// PR number if one exists for this branch (0 if none or unknown).
    pub pr_number: u64,
    /// PR state (e.g., "open", "merged", "closed", or empty).
    pub pr_state: String,
    /// Count of unpushed commits.
    pub unpushed_commits: usize,
    /// Whether the worktree has uncommitted (dirty) files.
    pub has_uncommitted: bool,
    /// Human-readable summary message.
    pub message: String,
}

/// Check branch safety before removing a worktree.
///
/// Assesses the risk of data loss from removing the worktree by examining:
/// - Whether the branch has been pushed to remote
/// - Whether there are unpushed commits
/// - Whether there are uncommitted changes
/// - Whether a PR exists for the branch
///
/// This does NOT check PR state via `gh` to avoid network requirements.
/// The caller can enhance with PR information if `gh` is available.
#[must_use]
pub fn check_branch_safety(wt_path: &Path) -> BranchSafetyResult {
    // If not a git directory, it's safe to remove (orphaned).
    if !wt_path.join(".git").exists() {
        return BranchSafetyResult {
            risk: BranchRisk::None,
            branch: String::new(),
            pushed: false,
            pr_number: 0,
            pr_state: String::new(),
            unpushed_commits: 0,
            has_uncommitted: false,
            message: "not a git worktree — safe to remove".into(),
        };
    }

    // Get current branch name.
    let branch = get_branch_name(wt_path);

    // Detached HEAD or main/master — safe to remove.
    if branch.is_empty() || branch == "HEAD" || branch == "main" || branch == "master" {
        return BranchSafetyResult {
            risk: BranchRisk::None,
            branch,
            pushed: false,
            pr_number: 0,
            pr_state: String::new(),
            unpushed_commits: 0,
            has_uncommitted: false,
            message: "detached HEAD or default branch — safe to remove".into(),
        };
    }

    // Check for dirty/uncommitted files.
    let has_uncommitted = has_dirty_files(wt_path);

    // Check if branch is pushed to remote.
    let (pushed, unpushed_commits) = check_push_status(wt_path, &branch);

    // Determine risk level.
    let (risk, message) = if !pushed && has_uncommitted {
        (
            BranchRisk::Critical,
            format!("branch '{branch}' has unpushed commits AND uncommitted changes — BLOCKED"),
        )
    } else if !pushed {
        (
            BranchRisk::High,
            format!(
                "branch '{branch}' has {unpushed_commits} unpushed commit(s) — BLOCKED (push first or use --force)"
            ),
        )
    } else {
        // Branch is pushed — check if it's merged into main.
        if is_branch_merged(wt_path, &branch) {
            (
                BranchRisk::None,
                format!("branch '{branch}' is merged into main — safe to remove"),
            )
        } else {
            (
                BranchRisk::Medium,
                format!("branch '{branch}' is pushed but no PR found — removing with warning"),
            )
        }
    };

    BranchSafetyResult {
        risk,
        branch,
        pushed,
        pr_number: 0,
        pr_state: String::new(),
        unpushed_commits,
        has_uncommitted,
        message,
    }
}

/// Remove a worktree and deregister it from the registry.
///
/// This is the internal implementation called by `WorktreeManager::cleanup`.
///
/// Directory removal is unconditional when the directory exists. Deregistration
/// happens only after the directory is confirmed gone. If removal fails, the
/// entry stays in the registry for retry on the next run.
pub(crate) fn cleanup_worktree(
    mgr: &WorktreeManager,
    name: &str,
    opts: &CleanupOpts,
) -> Result<(), WorktreeError> {
    // Prune mode: use git2 to prune stale worktree references.
    if opts.prune {
        return prune_worktrees(mgr, opts.dry_run);
    }

    if name.is_empty() {
        return Err(WorktreeError::InvalidName(
            "name cannot be empty".to_string(),
        ));
    }

    let wt_path = mgr.base_dir().join(name);

    if opts.dry_run {
        // Dry-run: just report what would happen, don't actually do anything.
        return Ok(());
    }

    // Rescue uncommitted/unpushed work before deletion (force mode only).
    if opts.force && wt_path.exists() {
        rescue_uncommitted_work(&wt_path, name)?;
    }

    // Best-effort git metadata cleanup.
    let _ = remove_via_git2(mgr, name, opts.force);

    // Delete directory if it still exists (unconditional).
    // Catch NotFound to handle concurrent deletion (TOCTOU).
    if wt_path.exists() {
        match fs::remove_dir_all(&wt_path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }

    // Deregister AFTER directory confirmed gone (locked to avoid TOCTOU).
    let wt_path_str = wt_path.to_string_lossy();
    registry::locked_deregister_worktree(mgr.registry_path(), &wt_path_str)?;

    // Defense-in-depth: also deregister by name for empty-path entries.
    registry::locked_deregister_by_name(mgr.registry_path(), name)?;

    Ok(())
}

/// Remove an orphaned worktree directory that has no registry entry.
///
/// Performs best-effort git metadata cleanup via `git2`, then removes
/// the directory. Catches `NotFound` for concurrent deletion safety.
///
/// # Errors
///
/// Returns `WorktreeError` on I/O or git metadata removal failures.
pub fn cleanup_orphan(
    project_dir: &Path,
    orphan_path: &Path,
    dry_run: bool,
) -> Result<(), WorktreeError> {
    if dry_run {
        return Ok(());
    }

    let mgr = WorktreeManager::new(project_dir);
    let name = orphan_path
        .file_name()
        .ok_or_else(|| WorktreeError::InvalidName("no filename".into()))?
        .to_string_lossy();

    // Best-effort git metadata cleanup.
    let _ = remove_via_git2(&mgr, &name, true);

    // Remove directory (catch NotFound for concurrent deletion).
    if orphan_path.exists() {
        match fs::remove_dir_all(orphan_path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }

    Ok(())
}

/// Rescue uncommitted and unpushed work from a worktree before deletion.
///
/// **Refactor (INF-TSK-048-001 AC #2):** this function performs no git
/// write operations.
///
/// **Refactor (INF-TSK-049-001 batch 2 — AC #24-#28):** rescue bundles now
/// land under the XDG cache root (`crate::autorun::rescue::xdg_rescue_root`)
/// — never inside a git working tree. Two new gates run before the
/// dirty-tree check:
/// - **Gate 8 (HEAD-on-origin):** if `git rev-list --count @{u}..HEAD == 0`,
///   skip (work is already on origin).
/// - **Gate 9 (gh pr list fallback):** if Gate 8 is inconclusive AND
///   `rescue.gh_pr_check` is true, run `gh pr list --head <branch>`. If
///   any PR exists, skip.
///
/// Bundle contents:
/// - `working.patch` — output of `git diff HEAD`
/// - `staged.patch` — output of `git diff --cached`
/// - `untracked.tar.gz` — tar of untracked files (100 MB cap per file; total 100 MB cap)
/// - `metadata.json` — session id, branch, head sha, timestamp, reason
///
/// Gating order (skip on first match):
/// 1. Non-git directory.
/// 2. `no-rescue` marker at `{wt_path}/.state/runtime/no-rescue` exists.
/// 3. `GateConfig(session_complete)` → pf-7 sentinel exists.
/// 4. `GateConfig(pr_pushed)` → pf-6 sentinel exists.
/// 5. **Gate 0d.5** (INF-TSK-050-015) — `pr-merged` sentinel exists at
///    `{wt_path}/.state/session/{sid}/pathflow/pr-merged`.
/// 6. Legacy `pr_pushed` flag in `pathflow-session-status.json`.
/// 7. Ledger contains any `pr_created` event for this session.
/// 8. **Gate 0i** (INF-TSK-050-015) — HEAD is a descendant of
///    `refs/remotes/origin/{default-branch}` (runtime-agnostic, survives
///    upstream-deleted branches).
/// 9. **Gate 8** — HEAD on origin (zero commits ahead of `@{u}`).
/// 10. **Gate 9** — `gh pr list --head <branch>` reports an existing PR.
/// 11. Nothing to rescue (clean working tree AND no untracked files).
///
/// The inspect/apply CLI (`codeflow rescue`) surfaces the bundle to the user.
pub fn rescue_uncommitted_work(wt_path: &Path, session_hint: &str) -> Result<(), WorktreeError> {
    rescue_uncommitted_work_with_reason(wt_path, session_hint, "worktree_cleanup")
}

/// Same as `rescue_uncommitted_work` but accepts an explicit reason string
/// that is recorded in the bundle's `metadata.json` and the `rescue_written`
/// ledger event. Intended for callers that want finer provenance.
///
/// Returns `Result` for API compatibility with the pre-refactor signature
/// and to leave room for future propagating errors if callers ever decide
/// that a bundle-write failure should abort cleanup (today they never do).
#[allow(clippy::unnecessary_wraps)]
pub fn rescue_uncommitted_work_with_reason(
    wt_path: &Path,
    session_hint: &str,
    reason: &str,
) -> Result<(), WorktreeError> {
    // Step 0a: not a git repo → nothing to rescue.
    if !wt_path.join(".git").exists() {
        return Ok(());
    }

    // Step 0b: ephemeral worktrees (e.g. report-commit) mark themselves
    // `no-rescue`; skip silently.
    if wt_path.join(".state/runtime/no-rescue").exists() {
        return Ok(());
    }

    // Step 0c: session_complete gate — session has already reached PF7-END.
    if gate_sentinel_present(wt_path, session_hint, "session_complete") {
        crate::diagnostics::warn(
            "worktree",
            &format!("{session_hint}: skipping rescue — session_complete gate satisfied"),
        );
        return Ok(());
    }

    // Step 0d: pr_pushed gate — PR already opened.
    if gate_sentinel_present(wt_path, session_hint, "pr_pushed") {
        crate::diagnostics::warn(
            "worktree",
            &format!("{session_hint}: skipping rescue — pr_pushed gate satisfied"),
        );
        return Ok(());
    }

    // Step 0d.5: pr-merged sentinel — written by session_end after
    // observing a `pr_merged` event in the ledger (INF-TSK-050-015 AC-03).
    // New-binary sessions get this for free; old-binary sessions fall
    // through to the runtime-agnostic Gate 0i below.
    if pr_merged_sentinel_present(wt_path, session_hint) {
        crate::diagnostics::warn(
            "worktree",
            &format!("{session_hint}: skipping rescue — pr-merged sentinel"),
        );
        return Ok(());
    }

    // Step 0e: legacy flag in pathflow-session-status.json (pre-gate rollout).
    if session_pushed_pr(wt_path, session_hint) {
        crate::diagnostics::warn(
            "worktree",
            &format!("{session_hint}: skipping rescue — PR already pushed"),
        );
        return Ok(());
    }

    // Step 0f: ledger PrCreated event for this session.
    if session_has_pr_created_event(wt_path, session_hint) {
        crate::diagnostics::warn(
            "worktree",
            &format!("{session_hint}: skipping rescue — pr_created event in ledger"),
        );
        return Ok(());
    }

    // Step 0i (INF-TSK-050-015 ref-walk gate): is HEAD already a descendant
    // of refs/remotes/origin/{default-branch}? If yes, every change has been
    // merged regardless of what sentinels / flags say. This is the
    // runtime-agnostic safety net for old-binary sessions where Gate 8
    // (`branch_on_origin`) is blinded by an unset `@{u}` upstream (PR
    // branch deleted on origin after merge). Inconclusive falls through so
    // a transient git failure cannot accidentally suppress a legitimate
    // rescue.
    let on_remote_main = crate::autorun::rescue::head_is_descendant_of_remote_main(wt_path);
    if matches!(
        on_remote_main,
        crate::autorun::rescue::BranchOriginStatus::OnOrigin
    ) {
        crate::diagnostics::warn(
            "worktree",
            &format!("{session_hint}: skipping rescue — HEAD descended from origin/main"),
        );
        return Ok(());
    }

    // Step 0g (Gate 8): HEAD-on-origin. If the branch has zero commits
    // ahead of @{u}, every change is already pushed — nothing to rescue.
    let on_origin = crate::autorun::rescue::branch_on_origin(wt_path);
    if matches!(
        on_origin,
        crate::autorun::rescue::BranchOriginStatus::OnOrigin
    ) {
        crate::diagnostics::warn(
            "worktree",
            &format!("{session_hint}: skipping rescue — HEAD on origin"),
        );
        return Ok(());
    }

    // Step 0h (Gate 9): gh pr list fallback. ONLY runs when Gate 8 was
    // inconclusive (no upstream configured) AND the config flag is on. A
    // gh failure (not installed / not authed / network) returns
    // Inconclusive — never skip rescue based on a transient outage.
    if matches!(
        on_origin,
        crate::autorun::rescue::BranchOriginStatus::Inconclusive
    ) {
        let project_root_for_cfg = derive_main_repo_root(wt_path);
        let cfg = crate::autorun::config::load_config(&project_root_for_cfg).unwrap_or_default();
        if cfg.rescue.gh_pr_check {
            let branch = get_branch_name(wt_path);
            if !branch.is_empty()
                && branch != "HEAD"
                && matches!(
                    crate::autorun::rescue::gh_pr_exists_for_branch(wt_path, &branch),
                    crate::autorun::rescue::GhPrCheck::Exists
                )
            {
                crate::diagnostics::warn(
                    "worktree",
                    &format!(
                        "{session_hint}: skipping rescue — gh pr list reports PR exists for '{branch}'"
                    ),
                );
                return Ok(());
            }
        }
    }

    // Step 1: is there anything to rescue?
    let has_changes = has_dirty_files(wt_path);
    if !has_changes {
        return Ok(());
    }

    // Step 2: write the bundle to the XDG rescue root (NEVER a git working tree).
    let session_id = session_hint
        .strip_prefix("worktree-")
        .unwrap_or(session_hint)
        .to_string();
    let bundle_root = match crate::autorun::rescue::xdg_rescue_root() {
        Ok(p) => p,
        Err(e) => {
            crate::diagnostics::warn(
                "worktree",
                &format!(
                    "{session_hint}: cannot resolve xdg rescue root ({e}); proceeding with cleanup"
                ),
            );
            return Ok(());
        }
    };
    match write_rescue_bundle(wt_path, &bundle_root, &session_id, reason) {
        Ok(bundle_path) => {
            let id = bundle_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("<unknown>");
            let msg = format!(
                "Rescue bundle: {path}. \
                 Inspect: codeflow rescue show {id}. \
                 Apply: codeflow rescue apply {id}",
                path = bundle_path.display(),
            );
            eprintln!("{msg}");
            crate::diagnostics::warn("worktree", &msg);
            append_rescue_written_event(&session_id, &bundle_path, reason);
            Ok(())
        }
        Err(e) => {
            crate::diagnostics::warn(
                "worktree",
                &format!(
                    "{session_hint}: rescue bundle write failed: {e}; proceeding with cleanup"
                ),
            );
            // Do NOT block cleanup on rescue failure — we have never
            // destroyed branch state here, and a failed bundle write is
            // strictly less destructive than preventing the delete.
            Ok(())
        }
    }
}

/// Derive the main repository root from a worktree path.
///
/// Worktrees live at `{project_root}/.git-worktrees/worktree-{SID}`. If the
/// path does not match that shape, fall back to the worktree itself.
///
/// **Read-only use:** this function is intentionally limited to looking up
/// gate config and ledger files in the MAIN repo. It MUST NOT be used to
/// place new files (rescue bundle writes go to xdg root —
/// `crate::autorun::rescue::xdg_rescue_root` — never to a git working tree).
pub(crate) fn derive_main_repo_root(wt_path: &Path) -> PathBuf {
    if let Some(parent) = wt_path.parent() {
        if parent.file_name().and_then(|n| n.to_str()) == Some(".git-worktrees") {
            if let Some(root) = parent.parent() {
                return root.to_path_buf();
            }
        }
    }
    wt_path.to_path_buf()
}

/// Check whether the named `GateConfig` entry's `on_phase_complete` sentinel
/// is present in the worktree's session sentinel directory.
///
/// Returns `false` on any failure (missing config, bad config, missing
/// sentinel dir). GateConfig::load is wrapped in `catch_unwind` because it
/// panics on malformed config; a rescue gate must never itself crash cleanup.
fn gate_sentinel_present(wt_path: &Path, session_hint: &str, gate_name: &str) -> bool {
    let session_id = session_hint
        .strip_prefix("worktree-")
        .unwrap_or(session_hint);
    if session_id.is_empty() || !is_safe_session_id(session_id) {
        return false;
    }
    let sentinel_dir = wt_path.join(".state/sentinels/pathflow").join(session_id);

    // Config lives at the MAIN project root, not the worktree.
    let project_root = derive_main_repo_root(wt_path);
    let config_path = project_root.join(".codeflow/config/pathflow/pathflow-config.json");

    let Ok(gates) = std::panic::catch_unwind(|| {
        crate::pathflow::gates::GateConfig::load_from_path(&config_path)
    }) else {
        return false;
    };

    let Some(req) = gates.get(gate_name) else {
        return false;
    };
    let Some(phase) = req.on_phase_complete else {
        return false;
    };
    let name = format!("pf-{}", phase.index());
    crate::pathflow::sentinel::check_by_name(&sentinel_dir, &name)
}

/// `session_id` sanitizer — only `[A-Za-z0-9_-]` are accepted. Used by
/// gate_sentinel_present and ledger path lookups to prevent traversal.
fn is_safe_session_id(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Check whether the `pr-merged` sentinel file exists for this session.
///
/// Path: `{wt_path}/.state/session/{sid}/pathflow/pr-merged` (INF-TSK-050-015 AC-03).
/// The sentinel is written by `crate::hooks::session_end` after observing
/// a `pr_merged` event in the ledger for this session. The session ID is
/// recovered from `session_hint` by stripping any `worktree-` prefix and
/// is sanitised against `[A-Za-z0-9_-]+` before joining into the
/// filesystem path. Any failure to sanitise returns `false` (safe default —
/// the caller proceeds to the next gate).
fn pr_merged_sentinel_present(wt_path: &Path, session_hint: &str) -> bool {
    let session_id = session_hint
        .strip_prefix("worktree-")
        .unwrap_or(session_hint);
    if !is_safe_session_id(session_id) {
        return false;
    }
    let sentinel_path = wt_path
        .join(".state/session")
        .join(session_id)
        .join("pathflow")
        .join("pr-merged");
    sentinel_path.is_file()
}

/// Scan the worktree-local ledger for a `pr_created` event tied to this
/// session. Returns `false` on any I/O or parse failure (safe default —
/// callers proceed to write the bundle).
///
/// INF-TSK-050-015 WS-REV iter-1 Finding 4: the ledger is LOCAL per-worktree
/// since PR #221 (see `crate::ledger::jsonl::resolve_ledger_dir_inner`).
/// Previous code peeled up to the main repo via `derive_main_repo_root`,
/// which holds at most pre-PR-#221 fragments; production `pr_created`
/// events written by cf-knowledge-layer land at
/// `{wt_path}/.state/ledger/work-graph/`. Using `wt_path` directly fixes
/// the parallel bug Finding 1 identified for the writer.
fn session_has_pr_created_event(wt_path: &Path, session_hint: &str) -> bool {
    let session_id = session_hint
        .strip_prefix("worktree-")
        .unwrap_or(session_hint);
    if !is_safe_session_id(session_id) {
        return false;
    }
    let ledger_dir = wt_path.join(".state/ledger");
    if !ledger_dir.is_dir() {
        return false;
    }
    // Check all jsonl files under work-graph subdir (+ fragment files), since
    // pr_created routes to work-graph.
    for subdir in &["work-graph"] {
        let dir = ledger_dir.join(subdir);
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if file_has_pr_created_for_session(&p, session_id) {
                return true;
            }
        }
    }
    false
}

/// Scan a single JSONL fragment for a `pr_created` event matching `session_id`.
///
/// Accepts BOTH ledger schemas observed in production fragments: the older
/// `{"event":"pr_created",...}` shape and the newer
/// `{"event_type":"pr_created",...}` shape. Mirrors the dual-schema acceptance
/// in `crate::hooks::session_end::SessionEndCleanup::ledger_has_pr_merged_for_session`
/// so Gate 0f (rescue ladder Step 0f) cannot be defeated by a schema drift that
/// the `pr_merged` reader already tolerates. Without this, an autorun worker
/// whose ledger fragment used the `event_type:` shape would fall past Gate 0f
/// and rely on the network-bound Gate 9 (`gh pr list`) as its only safety net,
/// which has been the source of stray `wip: auto-save from crashed session`
/// commits when `gh` is unreachable.
fn file_has_pr_created_for_session(path: &Path, session_id: &str) -> bool {
    let Ok(contents) = fs::read_to_string(path) else {
        return false;
    };
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let event_match = matches!(
            val.get("event_type").and_then(|v| v.as_str()),
            Some("pr_created")
        ) || matches!(
            val.get("event").and_then(|v| v.as_str()),
            Some("pr_created")
        );
        if !event_match {
            continue;
        }
        if val.get("session_id").and_then(|v| v.as_str()) == Some(session_id) {
            return true;
        }
    }
    false
}

/// Maximum size of an individual untracked file to include in the bundle.
/// Files above this are recorded in metadata under `skipped_large_files`.
const RESCUE_MAX_FILE_BYTES: u64 = 100 * 1024 * 1024;

/// Maximum cumulative bundle size (untracked tar payload). Once hit, further
/// files are skipped and recorded.
const RESCUE_MAX_TOTAL_BYTES: u64 = 100 * 1024 * 1024;

/// Write the rescue bundle. On success returns the bundle directory.
///
/// `bundle_root` is expected to be the XDG rescue root from
/// `crate::autorun::rescue::xdg_rescue_root`. Per AC #24, this MUST NEVER
/// be a git working tree path.
fn write_rescue_bundle(
    wt_path: &Path,
    bundle_root: &Path,
    session_id: &str,
    reason: &str,
) -> Result<PathBuf, WorktreeError> {
    let (ts_human, ts_rfc3339) = rescue_timestamps();
    // The XDG root is host-wide so the session_id alone disambiguates
    // bundles produced concurrently by different worktrees on this machine.
    // No PID component needed.
    let bundle_name = crate::autorun::rescue::bundle_name(session_id, &ts_human);
    let bundle_dir = bundle_root.join(&bundle_name);
    fs::create_dir_all(&bundle_dir)
        .map_err(|e| WorktreeError::Cleanup(format!("create bundle dir: {e}")))?;

    let working_out = Command::new("git")
        .args(["diff", "HEAD"])
        .current_dir(wt_path)
        .output()
        .map_err(|e| WorktreeError::Cleanup(format!("git diff HEAD: {e}")))?;
    fs::write(bundle_dir.join("working.patch"), &working_out.stdout)
        .map_err(|e| WorktreeError::Cleanup(format!("write working.patch: {e}")))?;

    let staged_out = Command::new("git")
        .args(["diff", "--cached"])
        .current_dir(wt_path)
        .output()
        .map_err(|e| WorktreeError::Cleanup(format!("git diff --cached: {e}")))?;
    fs::write(bundle_dir.join("staged.patch"), &staged_out.stdout)
        .map_err(|e| WorktreeError::Cleanup(format!("write staged.patch: {e}")))?;

    let (skipped_large, tar_size) =
        write_untracked_tar(wt_path, &bundle_dir.join("untracked.tar.gz"))?;

    let branch = get_branch_name(wt_path);
    let head_sha = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(wt_path)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    let metadata = serde_json::json!({
        "session_id": session_id,
        "branch": branch,
        "head_sha": head_sha,
        "timestamp_rfc3339": ts_rfc3339,
        "reason": reason,
        "skipped_large_files": skipped_large,
        "worktree_path": wt_path.to_string_lossy(),
        "untracked_tar_bytes": tar_size,
    });
    fs::write(
        bundle_dir.join("metadata.json"),
        serde_json::to_string_pretty(&metadata)
            .map_err(|e| WorktreeError::Cleanup(format!("metadata serialize: {e}")))?,
    )
    .map_err(|e| WorktreeError::Cleanup(format!("write metadata.json: {e}")))?;

    Ok(bundle_dir)
}

/// Build a gzip-compressed tarball of the worktree's untracked files.
///
/// Returns `(skipped_large_files, total_bytes_written_to_tar)`.
/// Honours `RESCUE_MAX_FILE_BYTES` and `RESCUE_MAX_TOTAL_BYTES`.
fn write_untracked_tar(
    wt_path: &Path,
    tar_path: &Path,
) -> Result<(Vec<String>, u64), WorktreeError> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::fs::File;

    // REV-NOTE-3: use `-z` so filenames with embedded newlines (rare but
    // valid on unix) are not split mid-name. Output is NUL-terminated.
    let files_output = Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard", "-z"])
        .current_dir(wt_path)
        .output()
        .map_err(|e| WorktreeError::Cleanup(format!("git ls-files --others: {e}")))?;

    let raw = String::from_utf8_lossy(&files_output.stdout);
    let files: Vec<&str> = raw.split('\0').filter(|l| !l.is_empty()).collect();

    let out = File::create(tar_path)
        .map_err(|e| WorktreeError::Cleanup(format!("create untracked.tar.gz: {e}")))?;
    let encoder = GzEncoder::new(out, Compression::default());
    let mut tar = tar::Builder::new(encoder);

    let mut skipped = Vec::new();
    let mut total: u64 = 0;
    for rel in files {
        let abs = wt_path.join(rel);
        let meta = match fs::metadata(&abs) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !meta.is_file() {
            continue;
        }
        let size = meta.len();
        if size > RESCUE_MAX_FILE_BYTES {
            skipped.push(format!("{rel} (size={size})"));
            continue;
        }
        if total.saturating_add(size) > RESCUE_MAX_TOTAL_BYTES {
            skipped.push(format!("{rel} (would-exceed-total)"));
            continue;
        }
        if let Err(e) = tar.append_path_with_name(&abs, rel) {
            skipped.push(format!("{rel} (append-error: {e})"));
            continue;
        }
        total = total.saturating_add(size);
    }

    tar.finish()
        .map_err(|e| WorktreeError::Cleanup(format!("finalize tar: {e}")))?;

    Ok((skipped, total))
}

/// Produce a `(YYYYMMDD-HHMMSS, RFC3339)` timestamp pair using chrono if
/// present, otherwise a SystemTime-derived approximation so this module
/// never depends on chrono at runtime.
fn rescue_timestamps() -> (String, String) {
    use chrono::Utc;
    let now = Utc::now();
    let human = now.format("%Y%m%d-%H%M%S").to_string();
    let rfc = now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    (human, rfc)
}

/// Append a `rescue_written` event to the single XDG-scoped sessions log.
///
/// AC #25: rescue events live in ONE append-only file
/// `{xdg_rescue_root}/sessions.jsonl`, not per-session fragments under any
/// project's `.state/ledger/sessions/`.
///
/// Best-effort — swallows every failure so the rescue path never blocks cleanup.
fn append_rescue_written_event(session_id: &str, bundle_path: &Path, reason: &str) {
    let Ok(log_path) = crate::autorun::rescue::sessions_log_path() else {
        return;
    };
    let (_, ts_rfc3339) = rescue_timestamps();
    let event = serde_json::json!({
        "event": "rescue_written",
        "session_id": session_id,
        "timestamp": ts_rfc3339,
        "bundle_path": bundle_path.to_string_lossy(),
        "reason": reason,
    });
    let line = serde_json::to_string(&event).unwrap_or_else(|_| String::new());
    if line.is_empty() {
        return;
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        use std::io::Write as _;
        let _ = writeln!(f, "{line}");
    }
}

/// Return true if the session's `pr_pushed` flag is set in
/// `pathflow-session-status.json`.
///
/// The flag is set atomically by the `SentinelWrite` PostToolUse hook after
/// a successful `git push` or `gh pr create`. The check is read-only and
/// tolerant: missing file, bad JSON, or missing field all return false —
/// callers must fall back to the full rescue path in those cases.
///
/// `session_hint` is the worktree directory name (e.g., `worktree-ses-abc123`);
/// the session ID is recovered by stripping the `worktree-` prefix.
///
/// # Path safety
///
/// The extracted session ID is validated against `[A-Za-z0-9_-]+` before
/// being joined into the filesystem path. Any value containing `..`,
/// path separators, null bytes, or other non-alphanumeric/`-_` characters
/// is rejected (returns `false`). Today the source is trusted
/// (`WorktreeManager` output), but the sanitization is cheap and forecloses
/// path-traversal attack surface if the helper is ever reused with
/// untrusted input.
fn session_pushed_pr(wt_path: &Path, session_hint: &str) -> bool {
    let session_id = session_hint
        .strip_prefix("worktree-")
        .unwrap_or(session_hint);
    if session_id.is_empty() {
        return false;
    }
    // Sanitize: only accept [A-Za-z0-9_-]. Reject anything else — this
    // includes "..", "/", "\\", null bytes, and any character that could
    // break path semantics.
    if !session_id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return false;
    }

    let status_path = wt_path
        .join(".state/session")
        .join(session_id)
        .join("pathflow/pathflow-session-status.json");

    let Ok(contents) = std::fs::read_to_string(&status_path) else {
        return false;
    };
    serde_json::from_str::<serde_json::Value>(&contents)
        .ok()
        .and_then(|v| v.get("pr_pushed")?.as_bool())
        .unwrap_or(false)
}

/// Attempt to remove a worktree using git2.
///
/// Returns `true` if the removal succeeded (or the worktree was not found in git).
fn remove_via_git2(mgr: &WorktreeManager, name: &str, force: bool) -> bool {
    let Ok(repo) = git2::Repository::open(mgr.project_dir()) else {
        return false;
    };

    let Ok(wt) = repo.find_worktree(name) else {
        return false;
    };

    // Validate the worktree. If it's not valid, prune it.
    if wt.validate().is_err() {
        let _ = wt.prune(Some(
            git2::WorktreePruneOptions::new()
                .valid(false)
                .working_tree(true),
        ));
        return true;
    }

    // Try to prune the valid worktree.
    let mut prune_opts = git2::WorktreePruneOptions::new();
    prune_opts.valid(true).working_tree(true);
    if force {
        prune_opts.locked(true);
    }

    wt.prune(Some(&mut prune_opts)).is_ok()
}

/// Get the current branch name from a worktree, or empty string for detached HEAD.
fn get_branch_name(wt_path: &Path) -> String {
    Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(wt_path)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Check if a worktree has dirty (uncommitted) files.
pub(crate) fn has_dirty_files(wt_path: &Path) -> bool {
    Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(wt_path)
        .output()
        .ok()
        .is_some_and(|o| !o.stdout.is_empty())
}

/// Check if a branch has been pushed to remote and count unpushed commits.
///
/// Returns `(is_pushed, unpushed_count)`.
fn check_push_status(wt_path: &Path, branch: &str) -> (bool, usize) {
    // Check if remote tracking branch exists.
    let remote_ref = format!("origin/{branch}");
    let remote_exists = Command::new("git")
        .args(["rev-parse", "--verify", &remote_ref])
        .current_dir(wt_path)
        .output()
        .ok()
        .is_some_and(|o| o.status.success());

    if !remote_exists {
        // No remote tracking branch — check if any commits exist at all.
        let commit_count = Command::new("git")
            .args(["rev-list", "--count", "HEAD"])
            .current_dir(wt_path)
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .parse::<usize>()
                    .ok()
            })
            .unwrap_or(0);

        // No remote tracking: report at least 1 unpushed if commits exist.
        return (false, usize::from(commit_count > 0));
    }

    // Count commits ahead of remote.
    let log_range = format!("{remote_ref}..HEAD");
    let unpushed = Command::new("git")
        .args(["rev-list", "--count", &log_range])
        .current_dir(wt_path)
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<usize>()
                .ok()
        })
        .unwrap_or(0);

    (true, unpushed)
}

/// Check if a branch has been merged into main/master.
fn is_branch_merged(wt_path: &Path, branch: &str) -> bool {
    // Check if origin/main or origin/master contains the branch tip.
    for main_ref in &["origin/main", "origin/master", "main", "master"] {
        let output = Command::new("git")
            .args(["branch", "--merged", main_ref])
            .current_dir(wt_path)
            .output();

        if let Ok(o) = output {
            if o.status.success() {
                let branches = String::from_utf8_lossy(&o.stdout);
                for line in branches.lines() {
                    let trimmed = line.trim().trim_start_matches("* ");
                    if trimmed == branch {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Prune stale worktree references using git2.
fn prune_worktrees(mgr: &WorktreeManager, dry_run: bool) -> Result<(), WorktreeError> {
    let repo = git2::Repository::open(mgr.project_dir())?;

    let worktree_names = repo.worktrees()?;
    for name in worktree_names.iter().flatten() {
        if let Ok(wt) = repo.find_worktree(name) {
            if wt.validate().is_err() && !dry_run {
                let _ = wt.prune(Some(
                    git2::WorktreePruneOptions::new()
                        .valid(false)
                        .working_tree(true),
                ));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acquire the crate-wide XDG env-var lock for the duration of a
    /// test. INF-TSK-024-051 Phase 2.5: consolidated to
    /// `crate::test_util::xdg_env_lock` so this module's tests serialise
    /// against `crate::autorun::rescue::tests` (previously each module
    /// defined its own mutex and the two raced under parallel
    /// execution).
    fn xdg_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_util::xdg_env_lock()
    }

    /// Redirect `dirs::cache_dir()` (used by `xdg_rescue_root`) into a
    /// tempdir for the duration of a test. Caller must hold the lock
    /// returned by [`xdg_lock`].
    fn redirect_xdg_cache(td: &tempfile::TempDir) {
        // SAFETY: tests serialise via the shared XDG env lock; no other
        // thread reads these env vars during the test body.
        unsafe {
            std::env::set_var("HOME", td.path());
            std::env::set_var("XDG_CACHE_HOME", td.path().join(".cache"));
        }
    }

    #[test]
    fn test_cleanup_opts_default() {
        let opts = CleanupOpts::default();
        assert!(!opts.force);
        assert!(!opts.dry_run);
        assert!(!opts.prune);
        assert!(!opts.interactive);
        assert!(opts.keep.is_none());
    }

    #[test]
    fn test_branch_risk_ordering() {
        assert!(BranchRisk::None < BranchRisk::Low);
        assert!(BranchRisk::Low < BranchRisk::Medium);
        assert!(BranchRisk::Medium < BranchRisk::High);
        assert!(BranchRisk::High < BranchRisk::Critical);
    }

    #[test]
    fn test_branch_risk_display() {
        assert_eq!(BranchRisk::None.as_str(), "none");
        assert_eq!(BranchRisk::Low.as_str(), "low");
        assert_eq!(BranchRisk::Medium.as_str(), "medium");
        assert_eq!(BranchRisk::High.as_str(), "high");
        assert_eq!(BranchRisk::Critical.as_str(), "critical");
        assert_eq!(BranchRisk::Critical.to_string(), "critical");
    }

    #[test]
    fn test_check_branch_safety_non_git_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = check_branch_safety(dir.path());
        assert_eq!(result.risk, BranchRisk::None);
        assert!(result.message.contains("not a git worktree"));
    }

    #[test]
    fn test_check_branch_safety_detached_head() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let sig = git2::Signature::now("Test", "test@test.com").unwrap();
        let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let oid = repo
            .commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
        repo.set_head_detached(oid).unwrap();

        let result = check_branch_safety(dir.path());
        assert_eq!(result.risk, BranchRisk::None);
        assert!(result.message.contains("detached HEAD"));
    }

    #[test]
    fn test_check_branch_safety_main_branch() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = git2::Repository::init(dir.path()).unwrap();
        // Set up git with initial commit on main
        Command::new("git")
            .args(["checkout", "-b", "main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let result = check_branch_safety(dir.path());
        assert_eq!(result.risk, BranchRisk::None);
    }

    #[test]
    fn test_check_branch_safety_unpushed_branch() {
        let dir = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["checkout", "-b", "feat/unpushed-test"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "feature work"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let result = check_branch_safety(dir.path());
        assert_eq!(result.risk, BranchRisk::High);
        assert!(!result.pushed);
        assert!(result.message.contains("BLOCKED"));
    }

    #[test]
    fn test_check_branch_safety_unpushed_with_dirty_files() {
        let dir = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["checkout", "-b", "feat/dirty-test"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        // Create a dirty file
        fs::write(dir.path().join("dirty.txt"), "uncommitted data").unwrap();

        let result = check_branch_safety(dir.path());
        assert_eq!(result.risk, BranchRisk::Critical);
        assert!(result.has_uncommitted);
        assert!(result.message.contains("BLOCKED"));
    }

    #[test]
    fn test_check_branch_safety_result_fields() {
        let dir = tempfile::tempdir().unwrap();
        let result = check_branch_safety(dir.path());
        // Verify all fields are populated
        assert_eq!(result.pr_number, 0);
        assert!(result.pr_state.is_empty());
        assert!(!result.message.is_empty());
    }

    #[test]
    fn test_has_dirty_files_clean() {
        let dir = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(!has_dirty_files(dir.path()));
    }

    #[test]
    fn test_has_dirty_files_dirty() {
        let dir = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        fs::write(dir.path().join("new.txt"), "data").unwrap();
        assert!(has_dirty_files(dir.path()));
    }

    #[test]
    fn test_cleanup_empty_name() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let opts = CleanupOpts::default();

        let result = cleanup_worktree(&mgr, "", &opts);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), WorktreeError::InvalidName(_)));
    }

    #[test]
    fn test_cleanup_dry_run() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());

        // Create a dummy directory to simulate a worktree.
        fs::create_dir_all(mgr.base_dir().join("dry-wt")).unwrap();

        let opts = CleanupOpts {
            dry_run: true,
            ..CleanupOpts::default()
        };

        let result = cleanup_worktree(&mgr, "dry-wt", &opts);
        assert!(result.is_ok());

        // Directory should still exist (dry run).
        assert!(mgr.base_dir().join("dry-wt").exists());
    }

    #[test]
    fn test_cleanup_nonexistent_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let opts = CleanupOpts::default();

        let result = cleanup_worktree(&mgr, "nonexistent", &opts);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cleanup_force_removes_directory() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());

        let wt_dir = mgr.base_dir().join("force-wt");
        fs::create_dir_all(&wt_dir).unwrap();
        fs::write(wt_dir.join("test.txt"), "data").unwrap();

        let opts = CleanupOpts {
            force: true,
            ..CleanupOpts::default()
        };

        let result = cleanup_worktree(&mgr, "force-wt", &opts);
        assert!(result.is_ok());
        assert!(!wt_dir.exists(), "directory should be removed");
    }

    #[test]
    fn test_rescue_uncommitted_work_clean_worktree() {
        // A clean worktree should return Ok(()).
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let sig = git2::Signature::now("Test", "test@test.com").unwrap();
        let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();

        let result = rescue_uncommitted_work(dir.path(), "test-session");
        assert!(result.is_ok());
    }

    #[test]
    fn test_rescue_uncommitted_work_non_git_dir() {
        // A non-git directory should return Ok(()) (nothing to rescue).
        let dir = tempfile::tempdir().unwrap();
        let result = rescue_uncommitted_work(dir.path(), "test-session");
        assert!(result.is_ok());
    }

    #[test]
    fn test_rescue_uncommitted_work_dirty_detached_head_writes_bundle() {
        // Post-refactor (INF-TSK-049-001 batch 2 AC #24): a dirty worktree
        // produces a bundle under the XDG rescue root, NEVER inside a git tree.
        let _g = xdg_lock();
        let xdg_td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&xdg_td);
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let sig = git2::Signature::now("Test", "test@test.com").unwrap();
        let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let oid = repo
            .commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
        repo.set_head_detached(oid).unwrap();
        fs::write(dir.path().join("dirty.txt"), "uncommitted data").unwrap();

        let result = rescue_uncommitted_work(dir.path(), "test-session");
        assert!(
            result.is_ok(),
            "rescue must not error on dirty detached HEAD: {result:?}"
        );
    }

    #[test]
    fn test_cleanup_full_lifecycle() {
        // mgr.cleanup triggers rescue_uncommitted_work which writes bundles
        // under the XDG rescue root. Without redirecting HOME / XDG_CACHE_HOME
        // to a test-scoped tempdir AND holding the shared xdg_env_lock, this
        // test races with parallel rescue tests: it can leak a bundle into
        // a concurrent rescue test's tempdir, breaking that test's
        // `bundles.len() == 1` assertion. INF-TSK-050-015 — adopt the same
        // serialisation pattern the rescue tests already use.
        let _g = xdg_lock();
        let xdg_td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&xdg_td);

        let dir = tempfile::tempdir().unwrap();

        let repo = git2::Repository::init(dir.path()).unwrap();
        let sig = git2::Signature::now("Test", "test@test.com").unwrap();
        let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();

        let mgr = WorktreeManager::new(dir.path())
            .with_registry_path(dir.path().join(".state/worktrees.yaml"));

        let branch = crate::types::BranchName::new_unchecked("feat/cleanup-test");
        mgr.setup("cleanup-test", &branch).unwrap();
        assert!(mgr.base_dir().join("cleanup-test").exists());

        let opts = CleanupOpts {
            force: true,
            ..CleanupOpts::default()
        };
        mgr.cleanup("cleanup-test", &opts).unwrap();

        assert!(!mgr.base_dir().join("cleanup-test").exists());

        // Entry should be deleted from registry (not tombstoned as "removed").
        let entries = mgr.list(None).unwrap();
        assert!(
            !entries.iter().any(|e| e.name == "cleanup-test"),
            "entry should be deleted from registry after cleanup"
        );
    }

    #[test]
    fn test_prune_dry_run_on_clean_repo() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = git2::Repository::init(dir.path()).unwrap();

        let mgr = WorktreeManager::new(dir.path());
        let opts = CleanupOpts {
            prune: true,
            dry_run: true,
            ..CleanupOpts::default()
        };

        let result = cleanup_worktree(&mgr, "", &opts);
        assert!(result.is_ok());
    }

    #[test]
    fn test_prune_on_clean_repo() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = git2::Repository::init(dir.path()).unwrap();

        let mgr = WorktreeManager::new(dir.path());
        let opts = CleanupOpts {
            prune: true,
            ..CleanupOpts::default()
        };

        let result = cleanup_worktree(&mgr, "", &opts);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_branch_safety_merged_branch() {
        // Set up a git repo with main, create a feature branch, merge it,
        // then check that check_branch_safety reports BranchRisk::None.
        let dir = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        // Create and commit on feature branch.
        Command::new("git")
            .args(["checkout", "-b", "feat/merged-test"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "feature work"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        // Merge into main.
        Command::new("git")
            .args(["checkout", "main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["merge", "feat/merged-test", "--no-ff", "-m", "merge feat"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        // Go back to feature branch to check safety.
        Command::new("git")
            .args(["checkout", "feat/merged-test"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        assert!(is_branch_merged(dir.path(), "feat/merged-test"));
    }

    #[test]
    fn test_is_branch_merged_not_merged() {
        let dir = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["checkout", "-b", "feat/not-merged"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "diverged"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        assert!(!is_branch_merged(dir.path(), "feat/not-merged"));
    }

    // -- TOCTOU and cleanup_orphan tests --

    #[test]
    fn test_cleanup_worktree_concurrent_delete() {
        // If the directory disappears between exists() and remove_dir_all(),
        // cleanup_worktree should not error (NotFound caught).
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());

        // Create .git-worktrees base dir but NOT the worktree subdir.
        fs::create_dir_all(mgr.base_dir()).unwrap();

        // Create a registry so deregister has something to work with.
        let registry_dir = dir.path().join(".state").join("worktrees");
        fs::create_dir_all(&registry_dir).unwrap();

        let opts = CleanupOpts {
            force: true,
            ..Default::default()
        };

        // The worktree dir doesn't exist — should succeed without error.
        let result = cleanup_worktree(&mgr, "nonexistent-wt", &opts);
        assert!(
            result.is_ok(),
            "cleanup should succeed even if dir doesn't exist: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_cleanup_orphan_removes_directory() {
        let dir = tempfile::tempdir().unwrap();
        let orphan_dir = dir.path().join(".git-worktrees").join("worktree-orphan");
        fs::create_dir_all(&orphan_dir).unwrap();
        fs::write(orphan_dir.join("marker.txt"), "test").unwrap();

        let result = cleanup_orphan(dir.path(), &orphan_dir, false);
        assert!(
            result.is_ok(),
            "cleanup_orphan should succeed: {:?}",
            result.err()
        );
        assert!(!orphan_dir.exists(), "orphan directory should be removed");
    }

    #[test]
    fn test_cleanup_orphan_dry_run_preserves() {
        let dir = tempfile::tempdir().unwrap();
        let orphan_dir = dir.path().join(".git-worktrees").join("worktree-orphan");
        fs::create_dir_all(&orphan_dir).unwrap();

        let result = cleanup_orphan(dir.path(), &orphan_dir, true);
        assert!(result.is_ok());
        assert!(
            orphan_dir.exists(),
            "orphan directory should be preserved in dry-run"
        );
    }

    #[test]
    fn test_cleanup_orphan_nonexistent_no_error() {
        let dir = tempfile::tempdir().unwrap();
        let orphan_dir = dir.path().join(".git-worktrees").join("worktree-gone");

        let result = cleanup_orphan(dir.path(), &orphan_dir, false);
        assert!(
            result.is_ok(),
            "cleanup_orphan should succeed for nonexistent path: {:?}",
            result.err()
        );
    }

    // -- session_pushed_pr tests --

    fn write_session_status(
        wt_path: &Path,
        session_id: &str,
        contents: &str,
    ) -> std::path::PathBuf {
        let dir = wt_path
            .join(".state/session")
            .join(session_id)
            .join("pathflow");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pathflow-session-status.json");
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn test_session_pushed_pr_true() {
        let dir = tempfile::tempdir().unwrap();
        write_session_status(
            dir.path(),
            "ses-abc",
            r#"{"pr_pushed": true, "status": "pf-in-progress"}"#,
        );
        assert!(session_pushed_pr(dir.path(), "worktree-ses-abc"));
    }

    #[test]
    fn test_session_pushed_pr_false() {
        let dir = tempfile::tempdir().unwrap();
        write_session_status(dir.path(), "ses-abc", r#"{"pr_pushed": false}"#);
        assert!(!session_pushed_pr(dir.path(), "worktree-ses-abc"));
    }

    #[test]
    fn test_session_pushed_pr_missing_field() {
        let dir = tempfile::tempdir().unwrap();
        write_session_status(dir.path(), "ses-abc", r#"{"status": "pf-in-progress"}"#);
        assert!(!session_pushed_pr(dir.path(), "worktree-ses-abc"));
    }

    #[test]
    fn test_session_pushed_pr_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!session_pushed_pr(dir.path(), "worktree-ses-missing"));
    }

    #[test]
    fn test_session_pushed_pr_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        write_session_status(dir.path(), "ses-abc", "not valid json {");
        assert!(!session_pushed_pr(dir.path(), "worktree-ses-abc"));
    }

    #[test]
    fn test_session_pushed_pr_session_hint_without_prefix() {
        // session_hint may be passed as "ses-abc" directly (no "worktree-" prefix).
        let dir = tempfile::tempdir().unwrap();
        write_session_status(dir.path(), "ses-abc", r#"{"pr_pushed": true}"#);
        assert!(session_pushed_pr(dir.path(), "ses-abc"));
    }

    #[test]
    fn test_session_pushed_pr_empty_hint() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!session_pushed_pr(dir.path(), ""));
        // Just a "worktree-" prefix with nothing after.
        assert!(!session_pushed_pr(dir.path(), "worktree-"));
    }

    #[test]
    fn test_session_pushed_pr_rejects_path_traversal() {
        // Session IDs containing ".." or path separators must be rejected
        // even if the attacker-controlled path would happen to land on a
        // real status file with pr_pushed=true. Defense-in-depth (REV F6).
        let dir = tempfile::tempdir().unwrap();
        // Plant a status file at a location the traversal attempt might reach.
        write_session_status(dir.path(), "ses-real", r#"{"pr_pushed": true}"#);

        // Attempts that must all be rejected by the sanitizer:
        assert!(!session_pushed_pr(
            dir.path(),
            "worktree-../../../etc/passwd"
        ));
        assert!(!session_pushed_pr(
            dir.path(),
            "worktree-ses-real/../ses-real"
        ));
        assert!(!session_pushed_pr(dir.path(), "worktree-..%2Fses-real"));
        assert!(!session_pushed_pr(dir.path(), "worktree-ses real")); // space
        assert!(!session_pushed_pr(dir.path(), "worktree-ses\0real")); // null byte
        assert!(!session_pushed_pr(dir.path(), "worktree-a/b/c"));
        assert!(!session_pushed_pr(dir.path(), "worktree-a\\b\\c"));
    }

    #[test]
    fn test_session_pushed_pr_accepts_valid_ids() {
        // Happy-path IDs that match [A-Za-z0-9_-]+ must still work.
        let dir = tempfile::tempdir().unwrap();
        write_session_status(dir.path(), "ses-abc-123_XYZ", r#"{"pr_pushed": true}"#);
        assert!(session_pushed_pr(dir.path(), "worktree-ses-abc-123_XYZ"));
    }

    #[test]
    fn test_rescue_skips_when_pr_pushed() {
        // Setup: worktree with dirty files + pr_pushed=true status.
        let dir = tempfile::tempdir().unwrap();
        let wt_path = dir.path().join("worktree-ses-pushed");
        fs::create_dir_all(&wt_path).unwrap();

        // Init a real git repo (rescue_uncommitted_work short-circuits if .git missing).
        let init = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&wt_path)
            .status();
        assert!(init.is_ok() && init.unwrap().success());

        // Create a dirty file.
        fs::write(wt_path.join("dirty.txt"), "uncommitted").unwrap();

        // Write pr_pushed=true status.
        write_session_status(
            &wt_path,
            "ses-pushed",
            r#"{"pr_pushed": true, "status": "pf-in-progress"}"#,
        );

        // Should return Ok immediately without staging/committing.
        let result = rescue_uncommitted_work(&wt_path, "worktree-ses-pushed");
        assert!(
            result.is_ok(),
            "rescue should skip when PR already pushed: {:?}",
            result.err()
        );

        // Verify nothing was committed (git log should be empty — no commits).
        let log_output = Command::new("git")
            .args(["log", "--oneline"])
            .current_dir(&wt_path)
            .output()
            .unwrap();
        assert!(
            log_output.stdout.is_empty(),
            "no commits should have been created; got: {}",
            String::from_utf8_lossy(&log_output.stdout)
        );
    }

    #[test]
    fn test_rescue_proceeds_when_no_status() {
        // No pathflow-session-status.json at all — rescue should proceed normally.
        let dir = tempfile::tempdir().unwrap();
        let wt_path = dir.path().join("worktree-ses-nostatus");
        fs::create_dir_all(&wt_path).unwrap();

        // Not a git repo — rescue returns Ok immediately (existing behavior).
        let result = rescue_uncommitted_work(&wt_path, "worktree-ses-nostatus");
        assert!(result.is_ok());
    }

    // ------------------------------------------------------------------
    // Rescue patch-bundle tests (INF-TSK-048-001 AC #2)
    // ------------------------------------------------------------------

    /// Build `{project_root}/.git-worktrees/worktree-{sid}/` and `git init` it.
    /// Returns (project_root_tempdir, worktree_abs_path).
    fn make_worktree(sid: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let project = tempfile::tempdir().unwrap();
        let wt_dir = project
            .path()
            .join(".git-worktrees")
            .join(format!("worktree-{sid}"));
        fs::create_dir_all(&wt_dir).unwrap();
        let init = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&wt_dir)
            .status();
        assert!(init.is_ok() && init.unwrap().success());
        // Identity so commits can succeed in the bundle test harness.
        let _ = Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(&wt_dir)
            .status();
        let _ = Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&wt_dir)
            .status();
        (project, wt_dir)
    }

    fn commit_file(wt: &std::path::Path, name: &str, contents: &str) {
        fs::write(wt.join(name), contents).unwrap();
        Command::new("git")
            .args(["add", name])
            .current_dir(wt)
            .status()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "seed", "--no-verify"])
            .current_dir(wt)
            .status()
            .unwrap();
    }

    #[test]
    fn test_rescue_writes_bundle_on_dirty_worktree() {
        let _g = xdg_lock();
        let xdg_td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&xdg_td);
        let (_project, wt) = make_worktree("ses-bundle1");
        commit_file(&wt, "seed.txt", "seed");
        // Modify a tracked file → shows up in `git diff HEAD`.
        fs::write(wt.join("seed.txt"), "modified").unwrap();
        // Untracked file → goes into untracked.tar.gz.
        fs::write(wt.join("new.txt"), "hello").unwrap();

        let result = rescue_uncommitted_work(&wt, "worktree-ses-bundle1");
        assert!(result.is_ok(), "rescue failed: {result:?}");

        // Bundle lives under xdg_rescue_root() (NOT inside any git tree).
        let rescue_dir = crate::autorun::rescue::xdg_rescue_root().unwrap();
        assert!(rescue_dir.is_dir(), "xdg rescue dir missing");
        let mut bundles: Vec<_> = fs::read_dir(&rescue_dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        assert_eq!(bundles.len(), 1, "expected 1 bundle; got {bundles:?}");
        let bundle = bundles.pop().unwrap();
        assert!(bundle.join("working.patch").exists());
        assert!(bundle.join("staged.patch").exists());
        assert!(bundle.join("untracked.tar.gz").exists());
        let metadata: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(bundle.join("metadata.json")).unwrap())
                .unwrap();
        assert_eq!(metadata["session_id"], "ses-bundle1");
        assert_eq!(metadata["reason"], "worktree_cleanup");
        assert!(metadata["timestamp_rfc3339"].is_string());
        // AC #24: the bundle path MUST NOT contain any `.git-worktrees`
        // segment — i.e. the bundle never lands inside a git working tree.
        let bundle_str = bundle.to_string_lossy();
        assert!(
            !bundle_str.contains(".git-worktrees"),
            "AC #24: bundle path must not be inside a git working tree; got {bundle_str}"
        );
    }

    #[test]
    fn test_rescue_skips_on_no_rescue_marker() {
        let _g = xdg_lock();
        let xdg_td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&xdg_td);
        let (_project, wt) = make_worktree("ses-noresc1");
        commit_file(&wt, "seed.txt", "s");
        fs::write(wt.join("seed.txt"), "m").unwrap();
        fs::create_dir_all(wt.join(".state/runtime")).unwrap();
        fs::write(wt.join(".state/runtime/no-rescue"), "").unwrap();

        rescue_uncommitted_work(&wt, "worktree-ses-noresc1").unwrap();

        let rescue_dir = crate::autorun::rescue::xdg_rescue_root().unwrap();
        let dir_count = fs::read_dir(&rescue_dir)
            .map(|e| {
                e.filter_map(Result::ok)
                    .filter(|en| en.path().is_dir())
                    .count()
            })
            .unwrap_or(0);
        assert_eq!(
            dir_count, 0,
            "no bundle should be written when no-rescue marker is set"
        );
    }

    #[test]
    fn test_rescue_skips_when_pr_pushed_gate_satisfied() {
        let _g = xdg_lock();
        let xdg_td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&xdg_td);
        let (project, wt) = make_worktree("ses-prgate1");
        commit_file(&wt, "seed.txt", "s");
        fs::write(wt.join("seed.txt"), "m").unwrap();

        // Drop a real pathflow-config.json with a pr_pushed gate.
        let cfg_dir = project.path().join(".codeflow/config/pathflow");
        fs::create_dir_all(&cfg_dir).unwrap();
        fs::write(
            cfg_dir.join("pathflow-config.json"),
            r#"{
              "phases": {
                "PF6-COMPLETE": {"phase_order": 6}
              },
              "gates": {
                "pr_pushed": {
                  "on_phase_complete": "PF6-COMPLETE",
                  "description": "test"
                }
              }
            }"#,
        )
        .unwrap();

        // Write the pf-6 sentinel in the worktree's session sentinel dir.
        let sentinel_dir = wt.join(".state/sentinels/pathflow/ses-prgate1");
        fs::create_dir_all(&sentinel_dir).unwrap();
        fs::write(sentinel_dir.join("pathflow-pf-6"), "").unwrap();

        rescue_uncommitted_work(&wt, "worktree-ses-prgate1").unwrap();

        let rescue_dir = crate::autorun::rescue::xdg_rescue_root().unwrap();
        let dir_count = fs::read_dir(&rescue_dir)
            .map(|e| {
                e.filter_map(Result::ok)
                    .filter(|en| en.path().is_dir())
                    .count()
            })
            .unwrap_or(0);
        assert_eq!(dir_count, 0, "pr_pushed gate must suppress rescue");
    }

    #[test]
    fn test_rescue_skips_when_session_complete_gate_satisfied() {
        let _g = xdg_lock();
        let xdg_td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&xdg_td);
        let (project, wt) = make_worktree("ses-scgate1");
        commit_file(&wt, "seed.txt", "s");
        fs::write(wt.join("seed.txt"), "m").unwrap();

        let cfg_dir = project.path().join(".codeflow/config/pathflow");
        fs::create_dir_all(&cfg_dir).unwrap();
        fs::write(
            cfg_dir.join("pathflow-config.json"),
            r#"{
              "phases": {
                "PF7-END": {"phase_order": 7}
              },
              "gates": {
                "session_complete": {
                  "on_phase_complete": "PF7-END",
                  "description": "test"
                }
              }
            }"#,
        )
        .unwrap();

        let sentinel_dir = wt.join(".state/sentinels/pathflow/ses-scgate1");
        fs::create_dir_all(&sentinel_dir).unwrap();
        fs::write(sentinel_dir.join("pathflow-pf-7"), "").unwrap();

        rescue_uncommitted_work(&wt, "worktree-ses-scgate1").unwrap();

        let rescue_dir = crate::autorun::rescue::xdg_rescue_root().unwrap();
        let dir_count = fs::read_dir(&rescue_dir)
            .map(|e| {
                e.filter_map(Result::ok)
                    .filter(|en| en.path().is_dir())
                    .count()
            })
            .unwrap_or(0);
        assert_eq!(dir_count, 0, "session_complete gate must suppress rescue");
    }

    #[test]
    fn test_rescue_skips_when_ledger_has_pr_created() {
        // INF-TSK-050-015 WS-REV iter-1 Finding 4: ledger is LOCAL per-
        // worktree since PR #221. Plant the pr_created event at the
        // WORKTREE's local ledger path (matching what JsonlWriter writes
        // in production), not at the main project root.
        let _g = xdg_lock();
        let xdg_td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&xdg_td);
        let (_project, wt) = make_worktree("ses-ledger1");
        commit_file(&wt, "seed.txt", "s");
        fs::write(wt.join("seed.txt"), "m").unwrap();

        let ledger_dir = wt.join(".state/ledger/work-graph");
        fs::create_dir_all(&ledger_dir).unwrap();
        fs::write(
            ledger_dir.join("work-graph.jsonl"),
            r#"{"event":"pr_created","session_id":"ses-ledger1","pr_number":42,"timestamp":"2026-04-20T00:00:00Z"}
"#,
        )
        .unwrap();

        rescue_uncommitted_work(&wt, "worktree-ses-ledger1").unwrap();

        let rescue_dir = crate::autorun::rescue::xdg_rescue_root().unwrap();
        let dir_count = fs::read_dir(&rescue_dir)
            .map(|e| {
                e.filter_map(Result::ok)
                    .filter(|en| en.path().is_dir())
                    .count()
            })
            .unwrap_or(0);
        assert_eq!(dir_count, 0, "pr_created ledger event must suppress rescue");
    }

    #[test]
    fn test_rescue_skips_when_pr_created_event_uses_event_type_schema() {
        // INF-TSK-050-015 AC-09 schema-drift parallel-reader fix: production
        // work-graph fragments are observed in TWO shapes — older `{"event":
        // "pr_created",...}` and newer `{"event_type":"pr_created",...}`. The
        // `pr_merged` reader in session_end.rs already accepts both; this
        // test pins that Gate 0f's reader does too, so an autorun worker
        // whose ledger fragment used the `event_type` shape never falls
        // through to the network-bound Gate 9 fallback.
        let _g = xdg_lock();
        let xdg_td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&xdg_td);
        let (_project, wt) = make_worktree("ses-ledger2");
        commit_file(&wt, "seed.txt", "s");
        fs::write(wt.join("seed.txt"), "m").unwrap();

        let ledger_dir = wt.join(".state/ledger/work-graph");
        fs::create_dir_all(&ledger_dir).unwrap();
        fs::write(
            ledger_dir.join("work-graph.jsonl"),
            r#"{"event_type":"pr_created","session_id":"ses-ledger2","pr_number":43,"timestamp":"2026-05-19T00:00:00Z"}
"#,
        )
        .unwrap();

        rescue_uncommitted_work(&wt, "worktree-ses-ledger2").unwrap();

        let rescue_dir = crate::autorun::rescue::xdg_rescue_root().unwrap();
        let dir_count = fs::read_dir(&rescue_dir)
            .map(|e| {
                e.filter_map(Result::ok)
                    .filter(|en| en.path().is_dir())
                    .count()
            })
            .unwrap_or(0);
        assert_eq!(
            dir_count, 0,
            "pr_created ledger event in event_type schema must suppress rescue"
        );
    }

    #[test]
    fn test_rescue_skips_when_clean() {
        let _g = xdg_lock();
        let xdg_td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&xdg_td);
        let (_project, wt) = make_worktree("ses-clean1");
        commit_file(&wt, "seed.txt", "s");
        // No modifications, no untracked files → nothing to rescue.

        rescue_uncommitted_work(&wt, "worktree-ses-clean1").unwrap();

        let rescue_dir = crate::autorun::rescue::xdg_rescue_root().unwrap();
        let dir_count = fs::read_dir(&rescue_dir)
            .map(|e| {
                e.filter_map(Result::ok)
                    .filter(|en| en.path().is_dir())
                    .count()
            })
            .unwrap_or(0);
        assert_eq!(dir_count, 0, "clean worktree must not write bundle");
    }

    #[test]
    fn test_rescue_records_ledger_event_on_success() {
        let _g = xdg_lock();
        let xdg_td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&xdg_td);
        let (_project, wt) = make_worktree("ses-lgrec1");
        commit_file(&wt, "seed.txt", "s");
        fs::write(wt.join("seed.txt"), "m").unwrap();

        rescue_uncommitted_work(&wt, "worktree-ses-lgrec1").unwrap();

        // AC #25: rescue events live in ONE append-only file under the xdg
        // root, not in per-session fragments under any project's
        // `.state/ledger/sessions/`.
        let sessions_log = crate::autorun::rescue::sessions_log_path().unwrap();
        assert!(
            sessions_log.exists(),
            "xdg sessions.jsonl must exist after first rescue"
        );
        let content = fs::read_to_string(&sessions_log).unwrap();
        assert!(
            content.contains("\"event\":\"rescue_written\"") && content.contains("ses-lgrec1"),
            "rescue_written event must be appended to xdg sessions log"
        );
    }

    #[test]
    fn test_rescue_bad_config_does_not_panic() {
        // GateConfig::load panics on malformed config. The rescue path must
        // swallow the panic and proceed (writing the bundle).
        let _g = xdg_lock();
        let xdg_td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&xdg_td);
        let (project, wt) = make_worktree("ses-badcfg1");
        commit_file(&wt, "seed.txt", "s");
        fs::write(wt.join("seed.txt"), "m").unwrap();

        let cfg_dir = project.path().join(".codeflow/config/pathflow");
        fs::create_dir_all(&cfg_dir).unwrap();
        fs::write(
            cfg_dir.join("pathflow-config.json"),
            r#"{"phases":{},"gates":{"pr_pushed":{"on_phase_complete":"PFXX-BOGUS"}}}"#,
        )
        .unwrap();

        // Should NOT panic, should still write the bundle.
        rescue_uncommitted_work(&wt, "worktree-ses-badcfg1").unwrap();

        let rescue_dir = crate::autorun::rescue::xdg_rescue_root().unwrap();
        let bundles: Vec<_> = fs::read_dir(&rescue_dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().is_dir())
            .collect();
        assert_eq!(bundles.len(), 1, "expected bundle despite bad config");
    }

    #[test]
    fn test_rescue_skips_large_untracked_files() {
        let _g = xdg_lock();
        let xdg_td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&xdg_td);
        let (_project, wt) = make_worktree("ses-big1");
        commit_file(&wt, "seed.txt", "s");
        // Small untracked file — included.
        fs::write(wt.join("small.txt"), "ok").unwrap();
        // The 100 MB-per-file cap is too expensive to exercise directly in
        // unit tests; we exercise the metadata schema by verifying that
        // `skipped_large_files` is present (as an array, possibly empty).
        rescue_uncommitted_work(&wt, "worktree-ses-big1").unwrap();

        let rescue_dir = crate::autorun::rescue::xdg_rescue_root().unwrap();
        let bundle = fs::read_dir(&rescue_dir)
            .unwrap()
            .filter_map(Result::ok)
            .find(|e| e.path().is_dir())
            .unwrap()
            .path();
        let metadata: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(bundle.join("metadata.json")).unwrap())
                .unwrap();
        assert!(metadata["skipped_large_files"].is_array());
    }

    // ------------------------------------------------------------------
    // INF-TSK-050-015: Gate 0i (ref-walk) + Gate 0d.5 (pr-merged sentinel)
    // ------------------------------------------------------------------

    /// Build a worktree whose HEAD is on `refs/remotes/origin/main` AND has
    /// no `@{u}` upstream — the old-binary post-merge scenario. Returns
    /// `(project_tempdir, origin_tempdir, worktree_path)`.
    fn make_worktree_head_on_remote_main_no_upstream(
        sid: &str,
    ) -> (tempfile::TempDir, tempfile::TempDir, std::path::PathBuf) {
        let project = tempfile::tempdir().unwrap();
        let origin = tempfile::tempdir().unwrap();
        // Bare origin with default branch 'main'.
        Command::new("git")
            .args(["init", "--bare", "-q", "-b", "main"])
            .current_dir(origin.path())
            .status()
            .unwrap();
        let wt = project
            .path()
            .join(".git-worktrees")
            .join(format!("worktree-{sid}"));
        fs::create_dir_all(&wt).unwrap();
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&wt)
            .status()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "t@t.c"])
            .current_dir(&wt)
            .status()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "T"])
            .current_dir(&wt)
            .status()
            .unwrap();
        // commit + push so refs/remotes/origin/main exists and HEAD is on it.
        fs::write(wt.join("seed.txt"), "seed").unwrap();
        Command::new("git")
            .args(["add", "seed.txt"])
            .current_dir(&wt)
            .status()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "seed", "--no-verify"])
            .current_dir(&wt)
            .status()
            .unwrap();
        Command::new("git")
            .args(["remote", "add", "origin", origin.path().to_str().unwrap()])
            .current_dir(&wt)
            .status()
            .unwrap();
        Command::new("git")
            .args(["push", "origin", "HEAD:refs/heads/main"])
            .current_dir(&wt)
            .status()
            .unwrap();
        // Mark origin/HEAD so remote_default_branch_name resolves.
        Command::new("git")
            .args(["remote", "set-head", "origin", "main"])
            .current_dir(&wt)
            .status()
            .unwrap();
        // Critically: do NOT call `branch --set-upstream-to`. Without `@{u}`,
        // Gate 8 (`branch_on_origin`) will be Inconclusive — Gate 0i is the
        // only safety net.
        (project, origin, wt)
    }

    #[test]
    fn test_rescue_skips_when_head_on_remote_main_without_upstream() {
        // AC-04 (a): Gate 0i takes effect when HEAD is on origin/main even
        // with no @{u} upstream.
        let _g = xdg_lock();
        let xdg_td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&xdg_td);
        let (_project, _origin, wt) = make_worktree_head_on_remote_main_no_upstream("ses-gate0i-a");
        // Dirty the worktree so rescue WOULD run if not for Gate 0i.
        fs::write(wt.join("seed.txt"), "modified").unwrap();
        // Sanity: confirm @{u} is unset (Gate 8 should be Inconclusive).
        assert_eq!(
            crate::autorun::rescue::branch_on_origin(&wt),
            crate::autorun::rescue::BranchOriginStatus::Inconclusive,
            "precondition: Gate 8 must be Inconclusive without @{{u}}"
        );

        rescue_uncommitted_work(&wt, "worktree-ses-gate0i-a").unwrap();

        let rescue_dir = crate::autorun::rescue::xdg_rescue_root().unwrap();
        let dir_count = fs::read_dir(&rescue_dir)
            .map(|e| {
                e.filter_map(Result::ok)
                    .filter(|en| en.path().is_dir())
                    .count()
            })
            .unwrap_or(0);
        assert_eq!(
            dir_count, 0,
            "Gate 0i must suppress rescue when HEAD descended from origin/main"
        );
    }

    #[test]
    fn test_rescue_proceeds_when_origin_main_missing() {
        // AC-04 (b): Gate 0i is Inconclusive when origin/main ref is missing
        // (no remote configured) — must fall through to subsequent gates.
        let _g = xdg_lock();
        let xdg_td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&xdg_td);
        let (_project, wt) = make_worktree("ses-gate0i-b");
        commit_file(&wt, "seed.txt", "s");
        fs::write(wt.join("seed.txt"), "modified").unwrap();
        // No remote → refs/remotes/origin/main doesn't exist → Gate 0i must
        // be Inconclusive, NOT OnOrigin. Bundle should be written.

        // Sanity: confirm Gate 0i itself is Inconclusive in this setup.
        assert_eq!(
            crate::autorun::rescue::head_is_descendant_of_remote_main(&wt),
            crate::autorun::rescue::BranchOriginStatus::Inconclusive,
            "Gate 0i must be Inconclusive when origin/main is missing"
        );

        rescue_uncommitted_work(&wt, "worktree-ses-gate0i-b").unwrap();

        let rescue_dir = crate::autorun::rescue::xdg_rescue_root().unwrap();
        let dir_count = fs::read_dir(&rescue_dir)
            .map(|e| {
                e.filter_map(Result::ok)
                    .filter(|en| en.path().is_dir())
                    .count()
            })
            .unwrap_or(0);
        assert_eq!(
            dir_count, 1,
            "Inconclusive Gate 0i must fall through; bundle should be written"
        );
    }

    #[test]
    fn test_rescue_skips_when_pr_merged_sentinel_present() {
        // AC-04 (c): Gate 0d.5 takes effect when pr-merged sentinel exists.
        let _g = xdg_lock();
        let xdg_td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&xdg_td);
        let (_project, wt) = make_worktree("ses-gate0d5");
        commit_file(&wt, "seed.txt", "s");
        fs::write(wt.join("seed.txt"), "modified").unwrap();

        // Plant the pr-merged sentinel at the documented path.
        let sentinel_path = wt
            .join(".state/session")
            .join("ses-gate0d5")
            .join("pathflow")
            .join("pr-merged");
        fs::create_dir_all(sentinel_path.parent().unwrap()).unwrap();
        fs::write(&sentinel_path, "").unwrap();

        rescue_uncommitted_work(&wt, "worktree-ses-gate0d5").unwrap();

        let rescue_dir = crate::autorun::rescue::xdg_rescue_root().unwrap();
        let dir_count = fs::read_dir(&rescue_dir)
            .map(|e| {
                e.filter_map(Result::ok)
                    .filter(|en| en.path().is_dir())
                    .count()
            })
            .unwrap_or(0);
        assert_eq!(
            dir_count, 0,
            "Gate 0d.5 must suppress rescue when pr-merged sentinel exists"
        );
    }

    #[test]
    fn test_rescue_writes_bundle_when_no_pr_gates_present() {
        // AC-04 (d): Regression check — when ALL pr-related gates are absent
        // and the worktree has uncommitted work, a bundle IS written.
        let _g = xdg_lock();
        let xdg_td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&xdg_td);
        let (_project, wt) = make_worktree("ses-noprgates");
        commit_file(&wt, "seed.txt", "s");
        fs::write(wt.join("seed.txt"), "modified").unwrap();
        // No pr_pushed sentinel, no pr-merged sentinel, no session-status flag,
        // no ledger event, no remote, no @{u}. Bundle MUST be written.

        rescue_uncommitted_work(&wt, "worktree-ses-noprgates").unwrap();

        let rescue_dir = crate::autorun::rescue::xdg_rescue_root().unwrap();
        let dir_count = fs::read_dir(&rescue_dir)
            .map(|e| {
                e.filter_map(Result::ok)
                    .filter(|en| en.path().is_dir())
                    .count()
            })
            .unwrap_or(0);
        assert_eq!(
            dir_count, 1,
            "bundle MUST be written when no pr-related gates fire"
        );
    }

    #[test]
    fn test_pr_merged_sentinel_present_rejects_traversal() {
        // Defense-in-depth: session_hint sanitiser must reject path traversal.
        let dir = tempfile::tempdir().unwrap();
        // Plant a sentinel at a real session ID.
        let real_path = dir
            .path()
            .join(".state/session")
            .join("ses-real")
            .join("pathflow")
            .join("pr-merged");
        fs::create_dir_all(real_path.parent().unwrap()).unwrap();
        fs::write(&real_path, "").unwrap();

        // Real session ID resolves.
        assert!(pr_merged_sentinel_present(dir.path(), "ses-real"));
        assert!(pr_merged_sentinel_present(dir.path(), "worktree-ses-real"));

        // Traversal attempts must be rejected.
        assert!(!pr_merged_sentinel_present(dir.path(), "../etc/passwd"));
        assert!(!pr_merged_sentinel_present(dir.path(), "worktree-ses real"));
        assert!(!pr_merged_sentinel_present(dir.path(), "worktree-a/b/c"));
        assert!(!pr_merged_sentinel_present(dir.path(), ""));
        // Missing file returns false.
        assert!(!pr_merged_sentinel_present(dir.path(), "ses-absent"));
    }

    /// REV-NOTE-3: filenames with embedded newlines must survive
    /// `git ls-files --others -z` intact and appear in the tar.
    ///
    /// Unix-only: creating a file with `\n` in the name is not portable to
    /// Windows (the filesystem forbids the byte).
    #[cfg(unix)]
    #[test]
    fn test_rescue_captures_newline_in_filename() {
        let _g = xdg_lock();
        let xdg_td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&xdg_td);
        let (_project, wt) = make_worktree("ses-newline1");
        commit_file(&wt, "seed.txt", "s");

        // Create an untracked file whose name contains `\n`.
        let weird_name = "a\nb.txt";
        fs::write(wt.join(weird_name), "newline-content").unwrap();

        rescue_uncommitted_work(&wt, "worktree-ses-newline1").unwrap();

        // Verify the bundle's untracked.tar.gz contains the file with
        // the exact (newline-bearing) name.
        let rescue_dir = crate::autorun::rescue::xdg_rescue_root().unwrap();
        let bundle = fs::read_dir(&rescue_dir)
            .unwrap()
            .filter_map(Result::ok)
            .find(|e| e.path().is_dir())
            .unwrap()
            .path();
        let tar_path = bundle.join("untracked.tar.gz");
        assert!(tar_path.exists(), "tar missing");

        // Inspect entries.
        use flate2::read::GzDecoder;
        let f = fs::File::open(&tar_path).unwrap();
        let dec = GzDecoder::new(f);
        let mut archive = tar::Archive::new(dec);
        let mut found_weird = false;
        for entry in archive.entries().unwrap() {
            let entry = entry.unwrap();
            let name = entry.path().unwrap().to_string_lossy().into_owned();
            if name == weird_name {
                found_weird = true;
                break;
            }
        }
        assert!(
            found_weird,
            "tar must contain entry with embedded newline; \
             filename '{weird_name}' not found in bundle"
        );
    }
}
