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
use crate::security::SecurityPolicy;
use crate::security::git::is_on_protected_branch;
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
        write!(f, "  checkout:  {}", self.final_checkout)
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

    let policy = SecurityPolicy::load(&repo_root.join(".codeflow/policy.json"));
    let target_protected = is_on_protected_branch(target, &policy);

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
            restore_checkout(repo_root, &original);
            return Err(IntegrateError::RebaseConflict {
                branch: branch.to_string(),
                target: target.to_string(),
                conflicting_files,
            });
        }
    }

    // Stage 3: test gate (full mode).
    let test_gate = run_test_stage(repo_root, &original)?;

    // Stage 4: fast-forward merge under the gate-context token.
    checkout(repo_root, target)?;
    let merge = Command::new("git")
        .args(["merge", "--ff-only", branch])
        .env(GATE_TOKEN_ENV, ulid::Ulid::new().to_string())
        .current_dir(repo_root)
        .output()
        .map_err(|e| IntegrateError::MergeFailed {
            target: target.to_string(),
            message: format!("failed to run git merge: {e}"),
        })?;
    if !merge.status.success() {
        restore_checkout(repo_root, &original);
        return Err(IntegrateError::MergeFailed {
            target: target.to_string(),
            message: String::from_utf8_lossy(&merge.stderr).trim().to_string(),
        });
    }

    // Stage 5: report. The branch ref already points at the rebased tip;
    // the target now shares it.
    let new_target_oid = resolve_branch(&repo, target)?;
    let commits_landed = count_commits(&repo, target_oid, new_target_oid);

    let final_checkout = if original == target {
        target.to_string()
    } else {
        restore_checkout(repo_root, &original);
        original
    };

    Ok(IntegrateOutcome {
        branch: branch.to_string(),
        target: target.to_string(),
        target_protected,
        old_target: old_target_short,
        new_target: short_id(&repo, new_target_oid),
        commits_landed,
        test_gate,
        final_checkout,
    })
}

/// Run the full-mode test gate; restores `original` checkout on failure.
fn run_test_stage(repo_root: &Path, original: &str) -> Result<TestGateSummary, IntegrateError> {
    match run_gate(repo_root, "full") {
        Ok(GateOutcome::NoTargets { reason }) => Ok(TestGateSummary::SkippedNoTargets { reason }),
        Ok(GateOutcome::Completed { results, passed }) => {
            if passed {
                return Ok(TestGateSummary::Passed {
                    targets: results.into_iter().map(|r| r.name).collect(),
                });
            }
            restore_checkout(repo_root, original);
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
            restore_checkout(repo_root, original);
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

/// Best-effort restore of the original checkout on failure paths; the
/// integrate verdict (the error) matters more than the final HEAD.
fn restore_checkout(repo_root: &Path, name: &str) {
    let _ = Command::new("git")
        .args(["checkout", name])
        .current_dir(repo_root)
        .output();
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
