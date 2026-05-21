//! Integration test for INF-TSK-050-015 (Post-PR auto-save runtime gap fix).
//!
//! Exercises the old-binary scenario end-to-end against a real git repo:
//! a worktree with HEAD pointing at a commit already on `origin/main`
//! (PR was merged), no `@{u}` upstream set, no `pr_pushed` gate sentinel,
//! and no `pr-merged` sentinel. Asserts that no rescue bundle is written
//! under the XDG cache root — i.e. Gate 0i (ref-walk) catches the merged
//! HEAD even though every other gate is blind to it.
//!
//! AC-05 of INF-TSK-050-015.
//!
//! This file lives in `codeflow-cli/core/tests/` so it compiles into a
//! SEPARATE test binary from the lib-level unit tests in
//! `worktree::cleanup::tests`. The boundary matters: an integration test
//! cannot import private helpers — it exercises only the public surface
//! the way an external caller (e.g. `session_end.rs`) does, via
//! `WorktreeManager::cleanup` which transitively invokes
//! `rescue_uncommitted_work` (force-mode path).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard, OnceLock};

use codeflow_core::worktree::{CleanupOpts, WorktreeManager};

/// Global lock — these tests mutate process-wide env vars (`HOME`,
/// `XDG_CACHE_HOME`) to redirect the rescue root. They must serialise
/// even when `cargo test` parallelises within this integration binary.
///
/// The lib-level `crate::test_util::xdg_env_lock` is NOT reachable from
/// an integration test (different crate boundary), so a local lock is
/// defined here. The two locks do not need to coordinate: lib-level
/// rescue tests and integration tests compile into separate binaries,
/// each with its own address space and its own `static` mutex.
fn env_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn redirect_xdg_cache(td_path: &Path) {
    // SAFETY: tests hold env_lock so no concurrent reader of HOME /
    // XDG_CACHE_HOME exists during these set_var calls.
    unsafe {
        std::env::set_var("HOME", td_path);
        std::env::set_var("XDG_CACHE_HOME", td_path.join(".cache"));
    }
}

/// Resolve the XDG rescue root the same way `crate::autorun::rescue::
/// xdg_rescue_root` does (without importing it — `autorun::rescue` is
/// not re-exported and integration tests cannot reach private modules).
///
/// On macOS `dirs::cache_dir()` returns `$HOME/Library/Caches`; on
/// Linux it honours `$XDG_CACHE_HOME` with a `~/.cache` fallback. Both
/// are driven by env vars `redirect_xdg_cache` sets, so the resolution
/// from this test matches what `rescue_uncommitted_work` computes
/// internally.
fn xdg_rescue_root_for_test() -> PathBuf {
    dirs::cache_dir()
        .expect("cache_dir must resolve under HOME redirection")
        .join("codeflow")
        .join("rescue")
}

/// Run `git` inside `dir` and panic on failure with the captured stderr.
fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("git {args:?} spawn failed: {e}"));
    assert!(
        out.status.success(),
        "git {args:?} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

/// Build the old-binary post-merge worktree topology:
/// 1. Bare origin with default branch `main`.
/// 2. A worktree-shaped directory under a fresh project root that:
///    - is a regular (non-bare) git repo on branch `main`;
///    - has a single commit;
///    - has `origin` pointing at the bare repo;
///    - has pushed `main` to origin (so `refs/remotes/origin/main` exists);
///    - has `refs/remotes/origin/HEAD` set so default-branch resolution
///      via `symbolic-ref` succeeds;
///    - has NO `@{u}` upstream configured (mimicking the PR-merged
///      branch-deleted state).
///
/// Returns `(project_tempdir, origin_tempdir, worktree_path)`. Callers
/// must keep the tempdirs alive for the duration of the test.
fn build_post_merge_worktree(sid: &str) -> (tempfile::TempDir, tempfile::TempDir, PathBuf) {
    let project = tempfile::tempdir().unwrap();
    let origin = tempfile::tempdir().unwrap();

    git(origin.path(), &["init", "--bare", "-q", "-b", "main"]);

    let wt = project
        .path()
        .join(".git-worktrees")
        .join(format!("worktree-{sid}"));
    fs::create_dir_all(&wt).unwrap();

    git(&wt, &["init", "-q", "-b", "main"]);
    git(&wt, &["config", "user.email", "test@test.com"]);
    git(&wt, &["config", "user.name", "Test"]);
    git(&wt, &["config", "commit.gpgsign", "false"]);

    fs::write(wt.join("seed.txt"), "seed-content").unwrap();
    git(&wt, &["add", "seed.txt"]);
    git(&wt, &["commit", "-m", "seed", "--no-verify"]);
    git(
        &wt,
        &["remote", "add", "origin", origin.path().to_str().unwrap()],
    );
    git(&wt, &["push", "origin", "HEAD:refs/heads/main"]);
    git(&wt, &["remote", "set-head", "origin", "main"]);

    // Sanity: @{u} is NOT set. `git rev-parse @{u}` should fail.
    let probe = Command::new("git")
        .args(["rev-parse", "@{u}"])
        .current_dir(&wt)
        .output()
        .unwrap();
    assert!(
        !probe.status.success(),
        "precondition violated: @{{u}} unexpectedly resolves; stdout={} stderr={}",
        String::from_utf8_lossy(&probe.stdout),
        String::from_utf8_lossy(&probe.stderr),
    );

    (project, origin, wt)
}

#[test]
fn old_binary_post_merge_session_writes_no_rescue_bundle() {
    let _g = env_lock();
    let xdg_td = tempfile::tempdir().unwrap();
    redirect_xdg_cache(xdg_td.path());

    let sid = "ses-ac05-oldbin";
    let (project, _origin, wt) = build_post_merge_worktree(sid);

    // Dirty the worktree — this is what makes the OLD binary fire rescue.
    fs::write(wt.join("seed.txt"), "modified-after-merge").unwrap();
    fs::write(wt.join("untracked.txt"), "stray").unwrap();

    // Drive cleanup through the public WorktreeManager API. force=true
    // selects the rescue path in cleanup_worktree (lines 213-216). No
    // pr_pushed gate sentinel, no pr-merged sentinel, no session-status
    // pr_pushed flag, no ledger pr_created event. The only thing standing
    // between this worktree and a stray auto-save bundle is Gate 0i
    // (INF-TSK-050-015 AC-01).
    let mgr = WorktreeManager::new(project.path());
    let opts = CleanupOpts {
        force: true,
        ..Default::default()
    };
    let result = mgr.cleanup(&format!("worktree-{sid}"), &opts);
    assert!(
        result.is_ok(),
        "cleanup must not error in the old-binary scenario: {result:?}"
    );

    // Verify the XDG rescue root has zero bundle subdirectories. The root
    // may also contain `sessions.jsonl` / `.last-scan` / `.migration-*`
    // markers — filter to directories only.
    let root = xdg_rescue_root_for_test();
    let bundle_dirs: Vec<_> = match fs::read_dir(&root) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter(|e| e.path().is_dir())
            .map(|e| e.path())
            .collect(),
        Err(_) => Vec::new(),
    };

    assert!(
        bundle_dirs.is_empty(),
        "Gate 0i must suppress rescue when HEAD is on origin/main without \
         @{{u}}; found bundles in {}: {bundle_dirs:?}",
        root.display(),
    );
}

#[test]
fn old_binary_post_merge_diverged_does_write_bundle() {
    // Companion regression check: if the worktree has a local commit AFTER
    // origin/main (genuinely unpushed work), Gate 0i must NOT suppress
    // rescue. This guards against an over-broad ref-walk gate that would
    // discard real work.
    let _g = env_lock();
    let xdg_td = tempfile::tempdir().unwrap();
    redirect_xdg_cache(xdg_td.path());

    let sid = "ses-ac05-diverged";
    let (project, _origin, wt) = build_post_merge_worktree(sid);

    // Add a local commit that is NOT on origin/main.
    fs::write(wt.join("local.txt"), "local-only").unwrap();
    git(&wt, &["add", "local.txt"]);
    git(&wt, &["commit", "-m", "local", "--no-verify"]);

    // Now dirty the worktree on top of that commit.
    fs::write(wt.join("local.txt"), "local-modified").unwrap();

    let mgr = WorktreeManager::new(project.path());
    let opts = CleanupOpts {
        force: true,
        ..Default::default()
    };
    let result = mgr.cleanup(&format!("worktree-{sid}"), &opts);
    assert!(result.is_ok(), "cleanup failed: {result:?}");

    let root = xdg_rescue_root_for_test();
    let bundle_dirs: Vec<_> = match fs::read_dir(&root) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter(|e| e.path().is_dir())
            .map(|e| e.path())
            .collect(),
        Err(_) => Vec::new(),
    };

    assert_eq!(
        bundle_dirs.len(),
        1,
        "rescue MUST run when HEAD has a commit not on origin/main; \
         expected 1 bundle in {} but found: {bundle_dirs:?}",
        root.display(),
    );
}

// ---------------------------------------------------------------------------
// INF-TSK-050-015 WS-REV iter-1 Finding 2: joint writer→reader integration
// ---------------------------------------------------------------------------
//
// cf-review flagged that the 6 unit tests for `write_pr_merged_sentinel` use
// flat tempdirs as `project_dir`, which never exercise the production case
// where `project_dir` is a worktree path of shape `.git-worktrees/worktree-X`.
// Before Finding 1 was fixed, the writer peeled up to the MAIN repo root for
// the ledger lookup; the ledger has been LOCAL per-worktree since PR #221
// (see `crate::ledger::jsonl::resolve_ledger_dir_inner`), so production
// `pr_merged` events landed at `{worktree}/.state/ledger/work-graph/` and the
// peel-up reader missed them. The fast-path sentinel was never planted, and
// only Gate 0i (the runtime-agnostic safety net) prevented the bug from
// surfacing as a stray auto-save commit.
//
// This integration test proves the fix:
// 1. Build a worktree-shaped layout (`{project}/.git-worktrees/worktree-X/`)
// 2. Plant a real `pr_merged` ledger event at the LOCAL worktree ledger path
//    (`{worktree}/.state/ledger/work-graph/work-graph.jsonl`)
// 3. Call `SessionEndCleanup::write_pr_merged_sentinel(worktree_path, sid, ...)`
//    — asserts sentinel landed at `{worktree}/.state/session/{sid}/pathflow/pr-merged`
// 4. Run `rescue_uncommitted_work(worktree_path, "worktree-X")` against a dirty
//    worktree — asserts Gate 0d.5 (which reads the same sentinel path) fires
//    and zero bundles are written
//
// Both halves of the writer↔reader contract are exercised end-to-end against
// the production topology cf-review identified.

use codeflow_core::hooks::session_end::{CleanupResult, SessionEndCleanup};

/// Build a worktree-shaped layout that mirrors production. Returns the
/// project root tempdir + the worktree path.
fn build_production_worktree_layout(sid: &str) -> (tempfile::TempDir, PathBuf) {
    let project = tempfile::tempdir().unwrap();
    let wt = project
        .path()
        .join(".git-worktrees")
        .join(format!("worktree-{sid}"));
    fs::create_dir_all(&wt).unwrap();
    // Make it a git repo so rescue_uncommitted_work's Gate 0a (not-a-git-repo)
    // doesn't short-circuit. A bare-bones `git init` is enough -- the rescue
    // gates don't require a remote for this test path.
    git(&wt, &["init", "-q", "-b", "main"]);
    git(&wt, &["config", "user.email", "test@test.com"]);
    git(&wt, &["config", "user.name", "Test"]);
    git(&wt, &["config", "commit.gpgsign", "false"]);
    fs::write(wt.join("seed.txt"), "seed").unwrap();
    git(&wt, &["add", "seed.txt"]);
    git(&wt, &["commit", "-m", "seed", "--no-verify"]);
    (project, wt)
}

/// Plant a `pr_merged` ledger event at the LOCAL per-worktree path
/// (matching the post-PR-#221 JsonlWriter target).
fn plant_pr_merged_event(worktree_path: &Path, sid: &str) {
    let dir = worktree_path
        .join(".state")
        .join("ledger")
        .join("work-graph");
    fs::create_dir_all(&dir).unwrap();
    let payload = serde_json::json!({
        "event_type": "pr_merged",
        "session_id": sid,
        "timestamp": "2026-05-19T13:00:00Z",
        "pr_number": 99,
        "merge_sha": "deadbeef",
    });
    let line = format!("{}\n", serde_json::to_string(&payload).unwrap());
    fs::write(dir.join("work-graph.jsonl"), line).unwrap();
}

#[test]
fn writer_to_reader_joint_in_production_worktree_layout() {
    let _g = env_lock();
    let xdg_td = tempfile::tempdir().unwrap();
    redirect_xdg_cache(xdg_td.path());

    let sid = "ses-joint-prod";
    let (project, wt) = build_production_worktree_layout(sid);

    // Step 1: plant the pr_merged event at the LOCAL worktree ledger path
    // -- the SAME path JsonlWriter::resolve_ledger_dir writes to in
    // production (see crate::ledger::jsonl::resolve_ledger_dir_inner).
    plant_pr_merged_event(&wt, sid);

    // Step 2: invoke the writer with worktree_path as project_dir, exactly
    // as SessionEndCleanup::run() does at Section 6b. Pre-fix this missed
    // the event because the peel-up reader looked at the main repo's
    // ledger (empty in this layout). Post-fix the lookup uses project_dir
    // directly and finds the event.
    let mut result = CleanupResult {
        session_id: sid.to_string(),
        pf7_valid: false,
        sentinels_cleaned: 0,
        task_preserved: false,
        team_name: String::new(),
        warnings: Vec::new(),
        messages: Vec::new(),
    };
    SessionEndCleanup::write_pr_merged_sentinel(&wt, sid, &mut result);

    // Assert: sentinel landed at the EXACT path the cleanup.rs reader
    // (`pr_merged_sentinel_present`) checks for. Both sides MUST agree on
    // `{wt}/.state/session/{sid}/pathflow/pr-merged`.
    let sentinel_path = wt
        .join(".state")
        .join("session")
        .join(sid)
        .join("pathflow")
        .join("pr-merged");
    assert!(
        sentinel_path.is_file(),
        "writer must plant sentinel at the reader's expected path; \
         messages={:?} warnings={:?}",
        result.messages,
        result.warnings,
    );
    assert!(
        result
            .messages
            .iter()
            .any(|m| m.contains("planted pr-merged sentinel")),
        "writer must record a success message; got {:?}",
        result.messages,
    );

    // Step 3: dirty the worktree and trigger the rescue path via the
    // public WorktreeManager API. force=true selects the
    // rescue_uncommitted_work path in cleanup_worktree (cleanup.rs:213-216).
    // Gate 0d.5 must see the sentinel the writer just planted and skip
    // bundle creation. We bind WorktreeManager to the PROJECT root (one
    // level above .git-worktrees/) so the cleanup name resolves correctly
    // to `worktree-{sid}` under `{project}/.git-worktrees/`.
    fs::write(wt.join("seed.txt"), "modified-after-merge").unwrap();
    fs::write(wt.join("untracked.txt"), "stray").unwrap();

    let mgr = WorktreeManager::new(project.path());
    let opts = CleanupOpts {
        force: true,
        ..Default::default()
    };
    let cleanup_result = mgr.cleanup(&format!("worktree-{sid}"), &opts);
    assert!(
        cleanup_result.is_ok(),
        "cleanup must not error: {cleanup_result:?}"
    );

    // Assert: no bundle written -- Gate 0d.5 suppressed the rescue path
    // because the sentinel the writer planted matches the path the reader
    // expects.
    let root = xdg_rescue_root_for_test();
    let bundle_dirs: Vec<_> = match fs::read_dir(&root) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter(|e| e.path().is_dir())
            .map(|e| e.path())
            .collect(),
        Err(_) => Vec::new(),
    };
    assert!(
        bundle_dirs.is_empty(),
        "Gate 0d.5 must suppress rescue once the writer plants the sentinel; \
         found bundles in {}: {bundle_dirs:?}",
        root.display(),
    );
}

#[test]
fn writer_skips_when_no_pr_merged_event_in_worktree_layout() {
    // Regression counterpart: under the same worktree-shaped layout, NO
    // pr_merged event in the local ledger -- writer must skip silently
    // and rescue MUST proceed to write a bundle (the runtime safety net
    // is the only thing standing between dirty work and the bundle).
    let _g = env_lock();
    let xdg_td = tempfile::tempdir().unwrap();
    redirect_xdg_cache(xdg_td.path());

    let sid = "ses-joint-noevent";
    let (project, wt) = build_production_worktree_layout(sid);

    // Do NOT plant any pr_merged event.

    let mut result = CleanupResult {
        session_id: sid.to_string(),
        pf7_valid: false,
        sentinels_cleaned: 0,
        task_preserved: false,
        team_name: String::new(),
        warnings: Vec::new(),
        messages: Vec::new(),
    };
    SessionEndCleanup::write_pr_merged_sentinel(&wt, sid, &mut result);

    let sentinel_path = wt
        .join(".state")
        .join("session")
        .join(sid)
        .join("pathflow")
        .join("pr-merged");
    assert!(
        !sentinel_path.exists(),
        "writer must NOT plant sentinel without a pr_merged event"
    );

    // Dirty the worktree and run the rescue path via the public API.
    fs::write(wt.join("seed.txt"), "modified").unwrap();

    let mgr = WorktreeManager::new(project.path());
    let opts = CleanupOpts {
        force: true,
        ..Default::default()
    };
    let cleanup_result = mgr.cleanup(&format!("worktree-{sid}"), &opts);
    assert!(cleanup_result.is_ok(), "cleanup must not error");

    // Gate 0d.5 not satisfied; Gate 0i Inconclusive (no remote configured);
    // Gate 8 Inconclusive (no `@{u}`); Gate 9 either Inconclusive or None.
    // The bundle WILL be written -- correct behaviour when no merge has
    // happened.
    let root = xdg_rescue_root_for_test();
    let bundle_dirs: Vec<_> = match fs::read_dir(&root) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter(|e| e.path().is_dir())
            .map(|e| e.path())
            .collect(),
        Err(_) => Vec::new(),
    };
    assert_eq!(
        bundle_dirs.len(),
        1,
        "rescue MUST write a bundle when no pr_merged event exists; \
         found bundles in {}: {bundle_dirs:?}",
        root.display(),
    );
}
