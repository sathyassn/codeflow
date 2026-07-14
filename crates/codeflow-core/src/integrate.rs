//! The `integrate` primitive: flock(rebase → test → ff-merge).
//!
//! The local sanctioned path for landing work on a protected branch
//! (charter §6.2, D9): rebase the branch onto the target, run the test
//! gate, then fast-forward the target — all under an exclusive lock on
//! `.git/codeflow/integrate.lock`. Failure at any stage leaves the target
//! ref untouched.
//!
//! The merge subprocess runs with [`GATE_TOKEN_ENV`] set so the hook plane
//! (git hooks + git-guard, workstream f-hooks) recognizes the sanctioned
//! path and permits the protected-branch merge. The token is a discipline
//! aid for agents, not a security boundary — CI + remote protection are
//! the hard line (charter §6.5).

use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

use crate::error::GitError;
use crate::file_lock::{PathLock, lock_path_exclusive};
use crate::git::conflict::{RebaseResult, attempt_rebase};
use crate::hooks::policy::Policy;
use crate::testing::error::TestingError;
use crate::testing::gate::{GateOutcome, run_gate};

/// Environment variable carrying the gate-context token during the
/// integrate merge. The git hooks and git-guard (workstream f-hooks) check
/// this exact name to permit a protected-branch merge commit; both planes
/// must reference this constant rather than restating the string.
pub const GATE_TOKEN_ENV: &str = "CODEFLOW_INTEGRATE_TOKEN";

/// Relative path (under the git common dir) of the integrate lock file.
const INTEGRATE_LOCK: &str = "codeflow/integrate.lock";

/// Errors from the integrate flow. Each is a stage failure; in every case
/// the target branch ref has not moved.
#[derive(Debug, Error)]
pub enum IntegrateError {
    #[error("integrate lock: {0}")]
    Lock(String),

    #[error("preflight: {0}")]
    Preflight(String),

    #[error(
        "working tree is dirty — integrate refuses to run with uncommitted changes:\n{}",
        files.join("\n")
    )]
    DirtyTree { files: Vec<String> },

    #[error("checkout of '{branch}' failed: {message}")]
    CheckoutFailed { branch: String, message: String },

    #[error(
        "rebase of '{branch}' onto '{target}' hit conflicts and was aborted; \
         nothing was merged.\nconflicting files:\n{}\n\
         resolve on the branch (rebase manually) and re-run integrate",
        conflicting_files.iter().map(|f| format!("  {f}")).collect::<Vec<_>>().join("\n")
    )]
    RebaseConflict {
        branch: String,
        target: String,
        conflicting_files: Vec<String>,
    },

    #[error("test gate failed — nothing was merged:\n{summary}")]
    TestGateFailed { summary: String },

    #[error("fast-forward merge into '{target}' failed: {message}")]
    MergeFailed { target: String, message: String },

    #[error(transparent)]
    Git(#[from] GitError),

    #[error("test gate: {0}")]
    Testing(#[from] TestingError),
}

/// How the test gate concluded inside an integration.
#[derive(Debug, Clone)]
pub enum TestGateSummary {
    /// No targets configured or detected — gate skipped with a loud warning.
    SkippedNoTargets { reason: String },
    /// Gate ran and passed.
    Passed { targets: Vec<String> },
}

/// Report of a completed integration.
#[derive(Debug, Clone)]
pub struct IntegrateOutcome {
    pub branch: String,
    pub target: String,
    /// Whether the target matched `policy.json` `protected_branches`.
    pub target_protected: bool,
    /// Target commit before the merge (short id).
    pub old_target: String,
    /// Target commit after the merge (short id).
    pub new_target: String,
    /// Commits landed on the target.
    pub commits_landed: usize,
    pub test_gate: TestGateSummary,
    /// Branch checked out when integrate finished.
    pub final_checkout: String,
    /// Non-fatal partial-success conditions that need human attention.
    pub warnings: Vec<String>,
}

impl std::fmt::Display for IntegrateOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "integrated '{}' into '{}'", self.branch, self.target)?;
        writeln!(
            f,
            "  target:    {} -> {} ({} commit{})",
            self.old_target,
            self.new_target,
            self.commits_landed,
            if self.commits_landed == 1 { "" } else { "s" }
        )?;
        writeln!(
            f,
            "  protected: {}",
            if self.target_protected {
                "yes (landed via the integrate gate token)"
            } else {
                "no"
            }
        )?;
        match &self.test_gate {
            TestGateSummary::SkippedNoTargets { reason } => {
                writeln!(f, "  test gate: SKIPPED — {reason}")?;
                writeln!(
                    f,
                    "             WARNING: no tests ran; this merge is NOT test-verified"
                )?;
            }
            TestGateSummary::Passed { targets } => {
                writeln!(f, "  test gate: passed ({})", targets.join(", "))?;
            }
        }
        writeln!(f, "  checkout:  {}", self.final_checkout)?;
        for warning in &self.warnings {
            writeln!(f, "  WARNING:   {warning}")?;
        }
        Ok(())
    }
}

/// Integrate `branch` into `target`: flock → preflight → rebase → test →
/// ff-merge. The target ref moves only when every stage succeeds.
///
/// # Errors
///
/// Returns an [`IntegrateError`] naming the failed stage; on any error the
/// target branch ref is unchanged and no rebase is left in progress.
pub fn integrate(
    repo_root: &Path,
    branch: &str,
    target: &str,
) -> Result<IntegrateOutcome, IntegrateError> {
    let repo = git2::Repository::open(repo_root)
        .map_err(|e| GitError::RepoOpen(format!("{}: {e}", repo_root.display())))?;

    // Stage 0: exclusive lock — one integrate at a time per repo, across
    // all worktrees (the lock lives in the shared git common dir).
    let _lock = acquire_lock(&repo)?;

    // Stage 1: preflight.
    if branch == target {
        return Err(IntegrateError::Preflight(format!(
            "branch and target are both '{branch}'"
        )));
    }
    resolve_branch(&repo, branch)?; // branch must exist and have a commit
    let target_oid = resolve_branch(&repo, target)?;
    let dirty = dirty_files(repo_root)?;
    if !dirty.is_empty() {
        return Err(IntegrateError::DirtyTree { files: dirty });
    }

    // Canonical policy (git.protected_branches with glob support), not the
    // obsolete flat SecurityPolicy — so a custom protected target is honored.
    let (policy, _) = Policy::load_effective(repo_root);
    let target_protected = policy.git.branch_is_protected(target);

    let original = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().map(ToString::to_string))
        .unwrap_or_else(|| target.to_string());
    let old_target_short = short_id(&repo, target_oid);

    // Stage 2: rebase the branch onto the target.
    checkout(repo_root, branch)?;
    match attempt_rebase(repo_root, target)? {
        RebaseResult::Success => {}
        RebaseResult::ConflictAborted { conflicting_files } => {
            let _ = restore_checkout(repo_root, &original);
            return Err(IntegrateError::RebaseConflict {
                branch: branch.to_string(),
                target: target.to_string(),
                conflicting_files,
            });
        }
    }

    // Pin the exact rebased tip that the test gate runs against, so a concurrent
    // branch move during testing cannot land untested content (the target CAS
    // below only guards the target ref, not the branch).
    let tested_oid = resolve_branch(&repo, branch)?;

    // Stage 3: test gate (full mode).
    let test_gate = run_test_stage(repo_root, &original)?;

    // The branch must still point at the exact commit that was tested, and the
    // target must be an ancestor of it (a real fast-forward), before we advance.
    let current_tip = resolve_branch(&repo, branch)?;
    if current_tip != tested_oid {
        let _ = restore_checkout(repo_root, &original);
        return Err(IntegrateError::MergeFailed {
            target: target.to_string(),
            message: format!(
                "branch '{branch}' moved during integrate (tested {}, now {}) — \
                 refusing to land untested content",
                short_id(&repo, tested_oid),
                short_id(&repo, current_tip)
            ),
        });
    }
    let is_fast_forward = tested_oid == target_oid
        || repo
            .graph_descendant_of(tested_oid, target_oid)
            .unwrap_or(false);
    if !is_fast_forward {
        let _ = restore_checkout(repo_root, &original);
        return Err(IntegrateError::MergeFailed {
            target: target.to_string(),
            message: format!(
                "'{branch}' is not a fast-forward of '{target}' — refusing non-ff advance"
            ),
        });
    }

    // Stage 4: fast-forward the target ref WITHOUT checking it out. The mandated
    // worktree doctrine keeps a protected target checked out at the repo root, so
    // a second `git checkout <target>` in a task worktree is refused by git.
    // `update-ref` advances the ref in place — compare-and-swap against the
    // pre-rebase oid (safe under concurrency), with the gate token set so the
    // reference-transaction hook admits this sanctioned landing.
    let update = Command::new("git")
        .args([
            "update-ref",
            &format!("refs/heads/{target}"),
            &tested_oid.to_string(),
            &target_oid.to_string(),
        ])
        .env(GATE_TOKEN_ENV, ulid::Ulid::new().to_string())
        .current_dir(repo_root)
        .output()
        .map_err(|e| IntegrateError::MergeFailed {
            target: target.to_string(),
            message: format!("failed to run git update-ref: {e}"),
        })?;
    if !update.status.success() {
        let _ = restore_checkout(repo_root, &original);
        return Err(IntegrateError::MergeFailed {
            target: target.to_string(),
            message: String::from_utf8_lossy(&update.stderr).trim().to_string(),
        });
    }

    // Stage 5: report. The branch ref already points at the rebased tip;
    // the target now shares it.
    let new_target_oid = resolve_branch(&repo, target)?;
    let commits_landed = count_commits(&repo, target_oid, new_target_oid);

    // HEAD is still on `branch` (Stage 2). Return the caller to their original
    // checkout; when that is the now-advanced target (integrate run from the
    // target's own worktree), this updates its working tree to the landed tip.
    let mut warnings = refresh_target_worktrees(repo_root, target, target_oid, tested_oid);
    if let Err(message) = restore_checkout(repo_root, &original) {
        warnings.push(format!(
            "target ref advanced, but checkout restoration failed: {message}"
        ));
    }
    let final_checkout = current_checkout(repo_root).unwrap_or(original);

    Ok(IntegrateOutcome {
        branch: branch.to_string(),
        target: target.to_string(),
        target_protected,
        old_target: old_target_short,
        new_target: short_id(&repo, new_target_oid),
        commits_landed,
        test_gate,
        final_checkout,
        warnings,
    })
}

/// Run the full-mode test gate; restores `original` checkout on failure.
fn run_test_stage(repo_root: &Path, original: &str) -> Result<TestGateSummary, IntegrateError> {
    match run_gate(repo_root, "full") {
        Ok(GateOutcome::NoTargets { reason }) => Ok(TestGateSummary::SkippedNoTargets { reason }),
        Ok(GateOutcome::Completed { results, passed, .. }) => {
            if passed {
                return Ok(TestGateSummary::Passed {
                    targets: results.into_iter().map(|r| r.name).collect(),
                });
            }
            let _ = restore_checkout(repo_root, original);
            let summary = results
                .iter()
                .map(|r| {
                    format!(
                        "  {}: exit {}{}",
                        r.name,
                        r.exit_code,
                        r.error
                            .as_deref()
                            .map(|e| format!(" ({e})"))
                            .unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            Err(IntegrateError::TestGateFailed { summary })
        }
        Err(e) => {
            let _ = restore_checkout(repo_root, original);
            Err(IntegrateError::Testing(e))
        }
    }
}

fn acquire_lock(repo: &git2::Repository) -> Result<PathLock, IntegrateError> {
    let lock_path: PathBuf = repo.commondir().join(INTEGRATE_LOCK);
    lock_path_exclusive(&lock_path).map_err(IntegrateError::Lock)
}

fn resolve_branch(repo: &git2::Repository, name: &str) -> Result<git2::Oid, IntegrateError> {
    repo.find_branch(name, git2::BranchType::Local)
        .map_err(|_| {
            IntegrateError::Preflight(format!("branch '{name}' not found in this repository"))
        })
        .and_then(|b| {
            b.get().target().ok_or_else(|| {
                IntegrateError::Preflight(format!("branch '{name}' has no commit"))
            })
        })
}

/// Tracked-file modifications that block integrate (untracked files are
/// permitted — they survive checkout/rebase untouched).
fn dirty_files(repo_root: &Path) -> Result<Vec<String>, IntegrateError> {
    let output = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .current_dir(repo_root)
        .output()
        .map_err(|e| IntegrateError::Preflight(format!("failed to run git status: {e}")))?;
    if !output.status.success() {
        return Err(IntegrateError::Preflight(format!(
            "git status failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(ToString::to_string)
        .collect())
}

fn checkout(repo_root: &Path, name: &str) -> Result<(), IntegrateError> {
    let output = Command::new("git")
        .args(["checkout", name])
        .current_dir(repo_root)
        .output()
        .map_err(|e| IntegrateError::CheckoutFailed {
            branch: name.to_string(),
            message: format!("failed to run git checkout: {e}"),
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(IntegrateError::CheckoutFailed {
            branch: name.to_string(),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }
}

fn restore_checkout(repo_root: &Path, name: &str) -> Result<(), String> {
    let output = Command::new("git")
        .args(["checkout", name])
        .current_dir(repo_root)
        .output()
        .map_err(|e| format!("failed to run git checkout: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn current_checkout(repo_root: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["symbolic-ref", "--short", "HEAD"])
        .current_dir(repo_root)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn refresh_target_worktrees(
    repo_root: &Path,
    target: &str,
    old_target_oid: git2::Oid,
    tested_oid: git2::Oid,
) -> Vec<String> {
    let mut warnings = Vec::new();
    let Ok(output) = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_root)
        .output()
    else {
        warnings.push("could not enumerate linked worktrees after landing".to_string());
        return warnings;
    };
    if !output.status.success() {
        warnings.push(format!(
            "could not enumerate linked worktrees after landing: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
        return warnings;
    }

    let current = repo_root.canonicalize().unwrap_or_else(|_| repo_root.to_path_buf());
    for record in String::from_utf8_lossy(&output.stdout).split("\n\n") {
        let path = record
            .lines()
            .find_map(|line| line.strip_prefix("worktree "))
            .map(PathBuf::from);
        let branch = record
            .lines()
            .find_map(|line| line.strip_prefix("branch "));
        if branch != Some(&format!("refs/heads/{target}")) {
            continue;
        }
        let Some(path) = path else { continue };
        if path.canonicalize().unwrap_or_else(|_| path.clone()) == current {
            continue;
        }
        match worktree_clean_at(&path, old_target_oid) {
            Ok(true) => {
                let reset = Command::new("git")
                    .args([
                        "-C",
                        path.to_string_lossy().as_ref(),
                        "reset",
                        "--hard",
                        &tested_oid.to_string(),
                    ])
                    .output();
                if !reset.as_ref().is_ok_and(|result| result.status.success()) {
                    warnings.push(format!(
                        "target worktree '{}' could not be refreshed to the landed tip",
                        path.display()
                    ));
                }
            }
            Ok(false) => warnings.push(format!(
                "target worktree '{}' has local changes and was not refreshed",
                path.display()
            )),
            _ => warnings.push(format!(
                "target worktree '{}' could not be checked before refresh",
                path.display()
            )),
        }
    }
    warnings
}

fn worktree_clean_at(path: &Path, old_target_oid: git2::Oid) -> Result<bool, ()> {
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .map_err(|_| ())
    };
    let unstaged = git(&["diff-files", "--quiet"])?;
    let staged = git(&[
        "diff-index",
        "--cached",
        "--quiet",
        &old_target_oid.to_string(),
        "--",
    ])?;
    let untracked = git(&["ls-files", "--others", "--exclude-standard"])?;
    if !unstaged.status.success() && unstaged.status.code() != Some(1) {
        return Err(());
    }
    if !staged.status.success() && staged.status.code() != Some(1) {
        return Err(());
    }
    if !untracked.status.success() {
        return Err(());
    }
    Ok(unstaged.status.success() && staged.status.success() && untracked.stdout.is_empty())
}

fn short_id(repo: &git2::Repository, oid: git2::Oid) -> String {
    repo.find_object(oid, None)
        .ok()
        .and_then(|o| o.short_id().ok())
        .and_then(|b| b.as_str().map(ToString::to_string))
        .unwrap_or_else(|| oid.to_string())
}

fn count_commits(repo: &git2::Repository, old: git2::Oid, new: git2::Oid) -> usize {
    if old == new {
        return 0;
    }
    let Ok(mut walk) = repo.revwalk() else {
        return 0;
    };
    if walk.push(new).is_err() || walk.hide(old).is_err() {
        return 0;
    }
    walk.count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn git(dir: &Path, args: &[&str]) -> std::process::Output {
        let output = Command::new("git")
            .args(args)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .current_dir(dir)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn commit_file(dir: &Path, name: &str, content: &str, message: &str) {
        fs::write(dir.join(name), content).unwrap();
        git(dir, &["add", name]);
        git(dir, &["commit", "-m", message]);
    }

    /// A repo with `main` (one commit) and `feat/x` (one extra commit).
    fn repo_with_feature_branch() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-b", "main"]);
        commit_file(dir.path(), "base.txt", "base\n", "chore: initial commit");
        git(dir.path(), &["checkout", "-b", "feat/x"]);
        commit_file(dir.path(), "feature.txt", "feature\n", "feat: add feature");
        git(dir.path(), &["checkout", "main"]);
        dir
    }

    fn write_test_config(dir: &Path, command: &str) {
        let codeflow = dir.join(".codeflow");
        fs::create_dir_all(&codeflow).unwrap();
        fs::write(
            codeflow.join("test-config.json"),
            format!(
                r#"{{"schema_version": "1.0", "targets": [{{"name": "gate", "runner": "custom", "modes": {{"full": {{"command": "{command}"}}}}}}]}}"#
            ),
        )
        .unwrap();
    }

    fn branch_oid(dir: &Path, name: &str) -> String {
        let out = git(dir, &["rev-parse", name]);
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[test]
    fn gate_token_env_name_is_the_coordinated_constant() {
        // f-hooks reads the same constant; the name is contract.
        assert_eq!(GATE_TOKEN_ENV, "CODEFLOW_INTEGRATE_TOKEN");
    }

    #[test]
    fn happy_path_lands_branch_with_test_gate() {
        let dir = repo_with_feature_branch();
        write_test_config(dir.path(), "exit 0");

        let before = branch_oid(dir.path(), "main");
        let outcome = integrate(dir.path(), "feat/x", "main").expect("integrate succeeds");

        let after = branch_oid(dir.path(), "main");
        assert_ne!(before, after, "main must advance");
        assert_eq!(
            after,
            branch_oid(dir.path(), "feat/x"),
            "main must equal the branch tip (ff merge)"
        );
        assert_eq!(outcome.commits_landed, 1);
        assert!(outcome.target_protected, "main is protected by default");
        assert!(
            matches!(outcome.test_gate, TestGateSummary::Passed { ref targets } if targets == &["gate"]),
            "test gate must have run: {:?}",
            outcome.test_gate
        );
        // Report renders.
        let report = outcome.to_string();
        assert!(report.contains("integrated 'feat/x' into 'main'"));
        assert!(report.contains("test gate: passed"));
    }

    // codex round-2 (CF-11): the mandated topology keeps `main` checked out at
    // the repo root and does feature work in a linked worktree. The old
    // checkout-based merge could not land here — git refuses a second checkout
    // of `main` — so integrate was unusable as documented. `update-ref` lands the
    // fast-forward in place without touching the root worktree's checkout.
    #[test]
    fn lands_from_a_linked_worktree_while_target_checked_out_at_root() {
        let dir = repo_with_feature_branch(); // main @ root, feat/x exists
        let wt = tempfile::tempdir().unwrap();
        let wt_path = wt.path().join("feat");
        git(
            dir.path(),
            &["worktree", "add", wt_path.to_str().unwrap(), "feat/x"],
        );

        let before = branch_oid(dir.path(), "main");
        let outcome =
            integrate(&wt_path, "feat/x", "main").expect("integrate from a linked worktree lands");
        let after = branch_oid(dir.path(), "main");

        assert_ne!(before, after, "main must advance");
        assert_eq!(
            after,
            branch_oid(dir.path(), "feat/x"),
            "main == feat/x tip (fast-forward)"
        );
        assert_eq!(outcome.commits_landed, 1);
        // The root worktree is never checked out away from main by integrate.
        let root_head = String::from_utf8_lossy(
            &git(dir.path(), &["symbolic-ref", "--short", "HEAD"]).stdout,
        )
        .trim()
        .to_string();
        assert_eq!(root_head, "main", "root worktree stays on main");
        assert_eq!(
            fs::read_to_string(dir.path().join("feature.txt")).unwrap(),
            "feature\n",
            "target worktree files must match the landed tree"
        );
        let status = git(dir.path(), &["status", "--porcelain"]);
        assert!(status.stdout.is_empty(), "target worktree must be clean");
        assert!(outcome.warnings.is_empty(), "{:?}", outcome.warnings);
    }

    #[test]
    fn successful_ref_update_warns_when_checkout_restore_fails() {
        let dir = repo_with_feature_branch();
        write_test_config(
            dir.path(),
            "mkdir -p .git/hooks; printf '#!/bin/sh\\nexit 1\\n' > .git/hooks/post-checkout; chmod +x .git/hooks/post-checkout",
        );

        let outcome = integrate(dir.path(), "feat/x", "main")
            .expect("the landed ref is a partial success, not a failed integration");
        assert_eq!(branch_oid(dir.path(), "main"), branch_oid(dir.path(), "feat/x"));
        assert!(
            outcome
                .warnings
                .iter()
                .any(|warning| warning.contains("checkout restoration failed")),
            "warnings: {:?}",
            outcome.warnings
        );
    }

    // codex round-3: if the branch moves during the test gate, the tested tip is
    // no longer the branch tip — integrate must refuse to land untested content.
    #[test]
    fn rejects_a_branch_that_moved_during_the_test_gate() {
        let dir = repo_with_feature_branch();
        // The gate command force-advances feat/x mid-run, then exits 0 (gate passes).
        write_test_config(dir.path(), "git commit --allow-empty -m moved; true");

        let before = branch_oid(dir.path(), "main");
        let err = integrate(dir.path(), "feat/x", "main").expect_err("must refuse");
        assert!(
            matches!(err, IntegrateError::MergeFailed { .. }),
            "expected a moved-branch refusal, got {err:?}"
        );
        assert_eq!(
            branch_oid(dir.path(), "main"),
            before,
            "main must not advance when the tested tip changed"
        );
    }

    #[test]
    fn happy_path_without_test_targets_warns_loudly() {
        let dir = repo_with_feature_branch();

        let outcome = integrate(dir.path(), "feat/x", "main").expect("integrate succeeds");
        assert!(matches!(
            outcome.test_gate,
            TestGateSummary::SkippedNoTargets { .. }
        ));
        let report = outcome.to_string();
        assert!(report.contains("SKIPPED"));
        assert!(report.contains("NOT test-verified"));
    }

    #[test]
    fn rebase_conflict_aborts_cleanly_with_report() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-b", "main"]);
        commit_file(dir.path(), "file.txt", "base\n", "chore: initial commit");
        git(dir.path(), &["checkout", "-b", "feat/x"]);
        commit_file(dir.path(), "file.txt", "branch change\n", "feat: branch edit");
        git(dir.path(), &["checkout", "main"]);
        commit_file(dir.path(), "file.txt", "main change\n", "fix: main edit");

        let before = branch_oid(dir.path(), "main");
        let branch_before = branch_oid(dir.path(), "feat/x");

        let err = integrate(dir.path(), "feat/x", "main").unwrap_err();
        match &err {
            IntegrateError::RebaseConflict {
                conflicting_files, ..
            } => {
                assert!(
                    conflicting_files.contains(&"file.txt".to_string()),
                    "conflict report must name the file: {conflicting_files:?}"
                );
            }
            other => panic!("expected RebaseConflict, got {other}"),
        }
        // Legible report names branch, target, file, and the next step.
        let message = err.to_string();
        assert!(message.contains("feat/x"));
        assert!(message.contains("main"));
        assert!(message.contains("file.txt"));
        assert!(message.contains("re-run integrate"));

        // Nothing mutated: target unchanged, branch unchanged, no rebase
        // in progress, original checkout restored.
        assert_eq!(branch_oid(dir.path(), "main"), before);
        assert_eq!(branch_oid(dir.path(), "feat/x"), branch_before);
        assert!(!dir.path().join(".git/rebase-merge").exists());
        assert!(!dir.path().join(".git/rebase-apply").exists());
        let head = git(dir.path(), &["rev-parse", "--abbrev-ref", "HEAD"]);
        assert_eq!(String::from_utf8_lossy(&head.stdout).trim(), "main");
    }

    #[test]
    fn dirty_tree_blocks_integrate() {
        let dir = repo_with_feature_branch();
        fs::write(dir.path().join("base.txt"), "uncommitted edit\n").unwrap();

        let before = branch_oid(dir.path(), "main");
        let err = integrate(dir.path(), "feat/x", "main").unwrap_err();
        assert!(
            matches!(err, IntegrateError::DirtyTree { .. }),
            "expected DirtyTree, got {err}"
        );
        assert!(err.to_string().contains("base.txt"));
        assert_eq!(branch_oid(dir.path(), "main"), before, "main untouched");
    }

    #[test]
    fn failing_test_gate_blocks_merge() {
        let dir = repo_with_feature_branch();
        write_test_config(dir.path(), "exit 1");

        let before = branch_oid(dir.path(), "main");
        let err = integrate(dir.path(), "feat/x", "main").unwrap_err();
        assert!(
            matches!(err, IntegrateError::TestGateFailed { .. }),
            "expected TestGateFailed, got {err}"
        );
        assert!(err.to_string().contains("nothing was merged"));
        assert_eq!(branch_oid(dir.path(), "main"), before, "main untouched");
        // Restored to the original checkout.
        let head = git(dir.path(), &["rev-parse", "--abbrev-ref", "HEAD"]);
        assert_eq!(String::from_utf8_lossy(&head.stdout).trim(), "main");
    }

    #[test]
    fn unknown_branch_fails_preflight() {
        let dir = repo_with_feature_branch();
        let err = integrate(dir.path(), "feat/nope", "main").unwrap_err();
        assert!(matches!(err, IntegrateError::Preflight(_)));
        assert!(err.to_string().contains("feat/nope"));
    }

    #[test]
    fn unknown_target_fails_preflight() {
        let dir = repo_with_feature_branch();
        let err = integrate(dir.path(), "feat/x", "release/zz").unwrap_err();
        assert!(matches!(err, IntegrateError::Preflight(_)));
    }

    #[test]
    fn branch_equal_target_fails_preflight() {
        let dir = repo_with_feature_branch();
        let err = integrate(dir.path(), "main", "main").unwrap_err();
        assert!(matches!(err, IntegrateError::Preflight(_)));
    }

    #[test]
    fn unprotected_target_is_reported_not_blocked() {
        let dir = repo_with_feature_branch();
        git(dir.path(), &["branch", "develop", "main"]);

        let outcome = integrate(dir.path(), "feat/x", "develop").expect("integrate succeeds");
        assert!(!outcome.target_protected);
        assert!(outcome.to_string().contains("protected: no"));
    }

    #[test]
    fn lock_file_lives_in_git_common_dir() {
        let dir = repo_with_feature_branch();
        let _ = integrate(dir.path(), "feat/x", "main");
        assert!(dir.path().join(".git/codeflow/integrate.lock").exists());
    }
}
