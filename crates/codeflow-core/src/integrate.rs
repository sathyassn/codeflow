//! The `integrate` primitive: lock, prepare a candidate, gate, advance the target.
//!
//! The local sanctioned path for landing work on a protected branch
//! (charter §6.2, D9): rebase by default, or create a merge commit without
//! rewriting the branch, then gate and advance the target under a lock on
//! `.git/codeflow/integrate.lock`. Failure at any stage leaves the target
//! ref untouched.
//!
//! The merge subprocess runs with [`GATE_TOKEN_ENV`] set so the hook plane
//! (git hooks + git-guard, workstream f-hooks) recognizes the sanctioned
//! path and permits the protected-branch merge. The token is a discipline
//! aid for agents, not a security boundary — CI + remote protection are
//! the hard line (charter §6.5).

use crate::workgraph::history;
use crate::workgraph::landing_class::{classify_landing, LandingClass};

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::error::GitError;
use crate::file_lock::{lock_path_exclusive, PathLock};
use crate::git::conflict::{attempt_rebase, RebaseResult};
use crate::hooks::policy::Policy;
use crate::testing::error::TestingError;
use crate::testing::gate::{run_gate_with_options, GateOptions, GateOutcome};

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

    #[error("merge of '{branch}' into '{target}' failed; nothing was merged:\n{message}")]
    CandidateMergeFailed {
        branch: String,
        target: String,
        message: String,
    },

    #[error("test gate failed — nothing was merged:\n{summary}")]
    TestGateFailed { summary: String },

    #[error("advance of '{target}' failed: {message}")]
    MergeFailed { target: String, message: String },

    #[error(transparent)]
    Git(#[from] GitError),

    #[error("test gate: {0}")]
    Testing(#[from] TestingError),
}

/// How to prepare the commit that the integration gate tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrateMode {
    /// Rebase the source branch onto the target, then fast-forward (default).
    Rebase,
    /// Create a no-fast-forward merge commit, preserving the source head.
    Merge,
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
    /// Class selected from the pinned target-to-candidate diff.
    pub landing_class: LandingClass,
    /// Checks successfully executed for this landing.
    pub checks: Vec<String>,
}

impl std::fmt::Display for IntegrateOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.block().as_str())
    }
}

impl IntegrateOutcome {
    /// The text as a block for a terminal: each `writeln!` is one line
    /// under the block rule, so a quoted value cannot start a line of its
    /// own (TSK-388).
    #[must_use]
    pub fn block(&self) -> crate::git::TextBlock {
        let mut block = crate::git::TextBlock::new();
        let _ = self.write_block(&mut block);
        block
    }

    fn write_block(&self, f: &mut crate::git::TextBlock) -> std::fmt::Result {
        // Branch and target names, test target names, reasons and the
        // checkout are fields: each stays on its line (TSK-388).
        use crate::git::{display_line, display_text};
        writeln!(
            f,
            "integrated '{}' into '{}'",
            display_text(&self.branch),
            display_text(&self.target)
        )?;
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
                writeln!(
                    f,
                    "  test gate: SKIPPED (executed 0; skipped 0; not complete): {}",
                    display_line(reason)
                )?;
                writeln!(
                    f,
                    "             WARNING: no tests ran; this merge is NOT test-verified"
                )?;
            }
            TestGateSummary::Passed { targets } => {
                writeln!(
                    f,
                    "  test gate: passed (executed {}; skipped 0; {})",
                    targets.len(),
                    targets
                        .iter()
                        .map(|name| display_text(name))
                        .collect::<Vec<_>>()
                        .join(", ")
                )?;
            }
        }
        writeln!(f, "  landing class: {}", self.landing_class.name())?;
        writeln!(f, "  checks ran: {}", self.checks.join(", "))?;
        writeln!(f, "  checkout:  {}", display_text(&self.final_checkout))?;
        for warning in &self.warnings {
            writeln!(f, "  WARNING:   {}", display_line(warning))?;
        }
        Ok(())
    }
}

/// The branch to restore after a failed landing.
///
/// OS text rule (issue 79): a failed landing puts the checkout back on this
/// branch by name. A name that is not valid UTF-8 cannot be given back
/// exactly, and falling back to another name would restore the wrong
/// checkout, so integrate refuses before it changes anything. A HEAD that
/// cannot be read is refused the same way; only an unborn branch restores
/// the target.
fn checked_out_branch(repo: &git2::Repository, target: &str) -> Result<String, IntegrateError> {
    match repo.head() {
        Ok(head) => match crate::git::name::reference_shorthand(&head).rule_text() {
            Ok(name) => Ok(name.to_string()),
            Err(_) => Err(IntegrateError::Preflight(
                "the checked-out branch name is not valid UTF-8, so integrate could not \
                 restore it after a failure; switch to another branch first"
                    .to_string(),
            )),
        },
        // An unborn branch has nothing to restore but the target; any other
        // failure is a checkout integrate cannot name, so it refuses.
        Err(error) if error.code() == git2::ErrorCode::UnbornBranch => Ok(target.to_string()),
        Err(error) => Err(IntegrateError::Preflight(format!(
            "cannot read the checked-out branch, so integrate could not restore it after a \
             failure: {error}"
        ))),
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
    integrate_with_mode(repo_root, branch, target, IntegrateMode::Rebase)
}

/// Integrate using a rebase or a no-fast-forward merge candidate.
/// The merge candidate is detached so neither branch moves before the gate.
/// Merge mode requires a linked worktree so commit hooks retain their normal
/// root-checkout enforcement.
///
/// # Errors
///
/// Returns the failed stage; the target is unchanged until its final atomic
/// update. A failed merge is aborted and the original checkout restored.
pub fn integrate_with_mode(
    repo_root: &Path,
    branch: &str,
    target: &str,
    mode: IntegrateMode,
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
    if mode == IntegrateMode::Merge && crate::root_checkout::is_root_checkout(&repo) {
        return Err(IntegrateError::Preflight(
            "--merge cannot run from a root checkout; run codeflow integrate --merge \
             from a linked worktree instead"
                .to_string(),
        ));
    }
    let branch_oid = resolve_branch(&repo, branch)?;
    let target_oid = resolve_branch(&repo, target)?;
    let dirty = dirty_files(repo_root)?;
    if !dirty.is_empty() {
        return Err(IntegrateError::DirtyTree { files: dirty });
    }

    // Canonical policy (git.protected_branches with glob support), not the
    // obsolete flat SecurityPolicy — so a custom protected target is honored.
    let (policy, _) = Policy::load_effective(repo_root).map_err(IntegrateError::Preflight)?;
    let target_protected = policy.git.branch_is_protected(target);

    let original = checked_out_branch(&repo, target)?;
    let old_target_short = short_id(&repo, target_oid);

    // Stage 2: prepare and pin the candidate before the gate runs.
    let (source_oid, tested_oid) = prepare_candidate(
        repo_root, &repo, branch, target, branch_oid, target_oid, &original, mode,
    )?;

    // Stage 3: selected landing checks, then the release structure of a
    // project that adopted CodeFlow's calculator (SPC-013 R-94).
    let (class, test_gate, checks) =
        run_landing_checks(repo_root, &original, branch, target, target_oid, tested_oid)?;

    // The source must still match the head used to prepare the candidate,
    // and the old target must be an ancestor of the tested candidate.
    let current_tip = resolve_branch(&repo, branch).inspect_err(|_| {
        let _ = restore_checkout(repo_root, &original);
    })?;
    if current_tip != source_oid {
        let _ = restore_checkout(repo_root, &original);
        return Err(IntegrateError::MergeFailed {
            target: target.to_string(),
            message: format!(
                "branch '{branch}' moved during integrate (tested {}, now {}) — \
                 refusing to land untested content",
                short_id(&repo, source_oid),
                short_id(&repo, current_tip)
            ),
        });
    }
    if mode == IntegrateMode::Merge {
        verify_merge_head(repo_root, &repo, target, &original, tested_oid)?;
    }
    let is_fast_forward = tested_oid == target_oid
        || history::descendant_of(&repo, tested_oid, target_oid).map_err(|error| {
            let _ = restore_checkout(repo_root, &original);
            IntegrateError::MergeFailed {
                target: target.to_string(),
                message: error.to_string(),
            }
        })?;
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
    let update = crate::git::command()
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

    finish_integration(
        &repo,
        repo_root,
        branch,
        target,
        target_oid,
        tested_oid,
        target_protected,
        old_target_short,
        original,
        test_gate,
        class,
        checks,
    )
}

fn verify_merge_head(
    repo_root: &Path,
    repo: &git2::Repository,
    target: &str,
    original: &str,
    tested_oid: git2::Oid,
) -> Result<(), IntegrateError> {
    let head = repo.head().map_err(|error| {
        let _ = restore_checkout(repo_root, original);
        IntegrateError::MergeFailed {
            target: target.to_string(),
            message: format!("cannot read merge candidate HEAD after the gate: {error}"),
        }
    })?;
    if head.target() != Some(tested_oid) {
        let _ = restore_checkout(repo_root, original);
        return Err(IntegrateError::MergeFailed {
            target: target.to_string(),
            message:
                "merge candidate HEAD moved during integrate; refusing to land untested content"
                    .to_string(),
        });
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn prepare_candidate(
    repo_root: &Path,
    repo: &git2::Repository,
    branch: &str,
    target: &str,
    branch_oid: git2::Oid,
    target_oid: git2::Oid,
    original: &str,
    mode: IntegrateMode,
) -> Result<(git2::Oid, git2::Oid), IntegrateError> {
    match mode {
        IntegrateMode::Rebase => {
            checkout(repo_root, branch)?;
            match attempt_rebase(repo_root, target)? {
                RebaseResult::Success => {}
                RebaseResult::ConflictAborted { conflicting_files } => {
                    let _ = restore_checkout(repo_root, original);
                    return Err(IntegrateError::RebaseConflict {
                        branch: branch.to_string(),
                        target: target.to_string(),
                        conflicting_files,
                    });
                }
            }
            let rebased = resolve_branch(repo, branch)?;
            Ok((rebased, rebased))
        }
        IntegrateMode::Merge => Ok((
            branch_oid,
            prepare_merge(
                repo_root, repo, branch, target, branch_oid, target_oid, original,
            )?,
        )),
    }
}

/// Build a merge on detached HEAD. The source and target refs stay untouched,
/// including when Git refuses the merge or leaves conflicts in the index.
#[allow(clippy::too_many_arguments)]
fn prepare_merge(
    repo_root: &Path,
    repo: &git2::Repository,
    branch: &str,
    target: &str,
    branch_oid: git2::Oid,
    target_oid: git2::Oid,
    original: &str,
) -> Result<git2::Oid, IntegrateError> {
    let result = (|| {
        checkout_detached(repo_root, &target_oid.to_string())?;
        let output = crate::git::command()
            .args([
                "merge",
                "--no-ff",
                "--no-edit",
                "-m",
                &format!("Merge branch '{branch}' into {target}"),
                &branch_oid.to_string(),
            ])
            .current_dir(repo_root)
            .output()
            .map_err(|error| IntegrateError::CandidateMergeFailed {
                branch: branch.to_string(),
                target: target.to_string(),
                message: format!("failed to run git merge: {error}"),
            })?;
        if !output.status.success() {
            let remedy = if repo.index().is_ok_and(|index| index.has_conflicts()) {
                "\nresolve on the source branch and re-run integrate"
            } else {
                ""
            };
            return Err(IntegrateError::CandidateMergeFailed {
                branch: branch.to_string(),
                target: target.to_string(),
                message: format!(
                    "{}{}{remedy}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr),
                ),
            });
        }
        repo.head()
            .and_then(|head| head.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|error| {
                IntegrateError::Preflight(format!("cannot read merge candidate: {error}"))
            })
    })();
    if result.is_err() {
        if repo.state() == git2::RepositoryState::Merge {
            let abort = crate::git::command()
                .args(["merge", "--abort"])
                .current_dir(repo_root)
                .output();
            if !matches!(abort, Ok(ref output) if output.status.success()) {
                return Err(IntegrateError::CandidateMergeFailed {
                    branch: branch.to_string(),
                    target: target.to_string(),
                    message: format!(
                        "merge failed and could not be aborted: {abort:?}; run git merge --abort"
                    ),
                });
            }
        }
        let _ = restore_checkout(repo_root, original);
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn finish_integration(
    repo: &git2::Repository,
    repo_root: &Path,
    branch: &str,
    target: &str,
    old_target_oid: git2::Oid,
    tested_oid: git2::Oid,
    target_protected: bool,
    old_target: String,
    original: String,
    test_gate: TestGateSummary,
    landing_class: LandingClass,
    checks: Vec<String>,
) -> Result<IntegrateOutcome, IntegrateError> {
    let new_target_oid = resolve_branch(repo, target)?;
    let mut warnings = refresh_target_worktrees(repo_root, target, old_target_oid, tested_oid);
    let record = serde_json::json!({
        "base": old_target_oid.to_string(), "head": tested_oid.to_string(),
        "target": target, "class": landing_class, "checks_ran": checks,
    });
    let directory = repo.commondir().join("codeflow/landings");
    if let Err(error) = std::fs::create_dir_all(&directory).and_then(|()| {
        std::fs::write(
            directory.join(format!("{tested_oid}.json")),
            record.to_string(),
        )
    }) {
        warnings.push(format!(
            "landing succeeded but its local check record could not be written: {error}"
        ));
    }
    if let Err(message) = restore_checkout(repo_root, &original) {
        warnings.push(format!(
            "target ref advanced, but checkout restoration failed: {message}"
        ));
    }
    Ok(IntegrateOutcome {
        branch: branch.to_string(),
        target: target.to_string(),
        target_protected,
        old_target,
        new_target: short_id(repo, new_target_oid),
        commits_landed: count_commits(repo, old_target_oid, new_target_oid),
        test_gate,
        final_checkout: current_checkout(repo_root).unwrap_or(original),
        warnings,
        landing_class,
        checks,
    })
}

/// The structural release check of a project that adopted `CodeFlow`'s
/// calculator, on the tested commit; any other project runs nothing.
fn run_release_structure(
    repo_root: &Path,
    original: &str,
    tested: &str,
) -> Result<(), IntegrateError> {
    let adopted = crate::release_local::adopted(repo_root).map_err(|message| {
        let _ = restore_checkout(repo_root, original);
        IntegrateError::TestGateFailed {
            summary: format!(
                "  release state: {message}; repair release configuration before integrating"
            ),
        }
    })?;
    if !adopted {
        return Ok(());
    }
    crate::release_local::structural(repo_root, tested).map_err(|message| {
        let _ = restore_checkout(repo_root, original);
        IntegrateError::TestGateFailed {
            summary: format!(
                "  release state (structural, not checked against the host): {message}"
            ),
        }
    })
}

fn run_landing_checks(
    repo_root: &Path,
    original: &str,
    branch: &str,
    target: &str,
    target_oid: git2::Oid,
    tested_oid: git2::Oid,
) -> Result<(LandingClass, TestGateSummary, Vec<String>), IntegrateError> {
    let class = if target.starts_with("integration/") {
        classify_landing(repo_root, &target_oid.to_string(), &tested_oid.to_string()).class
    } else {
        LandingClass::Code
    };
    let mut checks = Vec::new();
    if class == LandingClass::RecordsOnly {
        run_record_checks(repo_root, original, branch, target, target_oid, tested_oid)?;
        checks.extend(["validate --docs", "ci range", "built-in secret scan"].map(str::to_string));
    }
    let test_gate = run_test_stage(repo_root, original, class.gate_mode())?;
    if matches!(test_gate, TestGateSummary::Passed { .. }) {
        checks.push(format!("test --mode {} --all", class.gate_mode()));
    } else if class == LandingClass::RecordsOnly {
        let _ = restore_checkout(repo_root, original);
        return Err(IntegrateError::TestGateFailed {
            summary: "records-only landing requires a completed quick gate".into(),
        });
    }
    run_release_structure(repo_root, original, &tested_oid.to_string())?;
    Ok((class, test_gate, checks))
}

fn run_record_checks(
    root: &Path,
    original: &str,
    branch: &str,
    target: &str,
    base: git2::Oid,
    head: git2::Oid,
) -> Result<(), IntegrateError> {
    let checked = (|| {
        let base = base.to_string();
        let head = head.to_string();
        for arguments in [
            vec!["validate", "--docs"],
            vec![
                "ci", "--base", &base, "--head", &head, "--branch", branch, "--into", target,
            ],
        ] {
            let output = crate::git::codeflow_command()
                .args(&arguments)
                .current_dir(root)
                .output()
                .map_err(|error| error.to_string())?;
            if !output.status.success() {
                return Err(format!(
                    "{} failed ({}): {}{}",
                    arguments.join(" "),
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
        }
        scan_record_range(root, &base, &head)
    })();
    checked.map_err(|summary| {
        let _ = restore_checkout(root, original);
        IntegrateError::TestGateFailed { summary }
    })
}

/// Scan every introduced commit, including changes later removed, with the
/// same credential patterns as pre-commit. Never print matched content.
fn scan_record_range(root: &Path, base: &str, head: &str) -> Result<(), String> {
    let repo = git2::Repository::discover(root).map_err(|error| error.to_string())?;
    let mut walk = super::workgraph::history::revwalk(&repo).map_err(|error| error.to_string())?;
    walk.push(git2::Oid::from_str(head).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    walk.hide(git2::Oid::from_str(base).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    for id in walk {
        let commit = repo
            .find_commit(id.map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
        let tree = commit.tree().map_err(|error| error.to_string())?;
        let parent = history::parent(&repo, &commit, 0)
            .and_then(|parent| parent.tree())
            .map_err(|error| error.to_string())?;
        let diff = repo
            .diff_tree_to_tree(Some(&parent), Some(&tree), None)
            .map_err(|error| error.to_string())?;
        for delta in diff.deltas() {
            if delta.new_file().id().is_zero() {
                continue;
            }
            let blob = repo
                .find_blob(delta.new_file().id())
                .map_err(|error| error.to_string())?;
            let text = std::str::from_utf8(blob.content())
                .map_err(|_| "secret scan cannot read non-UTF-8 content")?;
            if text
                .lines()
                .any(|line| crate::hooks::scan::scan_line(line).is_some())
            {
                return Err(format!(
                    "secret scan found a credential pattern in commit {}",
                    commit.id()
                ));
            }
        }
    }
    Ok(())
}

fn run_test_stage(
    repo_root: &Path,
    original: &str,
    mode: &str,
) -> Result<TestGateSummary, IntegrateError> {
    match run_gate_with_options(
        repo_root,
        mode,
        &GateOptions {
            all: true,
            ..GateOptions::default()
        },
    ) {
        Ok(GateOutcome::NoTargets { reason }) => Ok(TestGateSummary::SkippedNoTargets { reason }),
        Ok(GateOutcome::Completed {
            results,
            passed,
            skipped,
            complete,
            evidence_skipped,
            ..
        }) => {
            // `complete` denotes a full-mode run. The records path asks for
            // every quick target (`all: true` above), with none omitted or
            // satisfied by earlier candidate evidence.
            if passed
                && (complete || mode == "quick")
                && evidence_skipped == 0
                && skipped.is_empty()
                && !results.is_empty()
            {
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
            let summary = format!(
                "executed {}; skipped {}{}\n{summary}",
                results.len(),
                skipped.len(),
                if complete { "" } else { "; not complete" }
            );
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
            b.get()
                .target()
                .ok_or_else(|| IntegrateError::Preflight(format!("branch '{name}' has no commit")))
        })
}

/// Tracked-file modifications that block integrate (untracked files are
/// permitted — they survive checkout/rebase untouched).
fn dirty_files(repo_root: &Path) -> Result<Vec<String>, IntegrateError> {
    let output = crate::git::command()
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
    run_checkout(repo_root, name, false)
}

fn checkout_detached(repo_root: &Path, oid: &str) -> Result<(), IntegrateError> {
    run_checkout(repo_root, oid, true)
}

fn run_checkout(repo_root: &Path, name: &str, detach: bool) -> Result<(), IntegrateError> {
    let mut command = crate::git::command();
    command.arg("checkout");
    if detach {
        command.arg("--detach");
    }
    let output = command
        .arg(name)
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
    let output = crate::git::command()
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
    let output = crate::git::command()
        .args(["symbolic-ref", "--short", "HEAD"])
        .current_dir(repo_root)
        .output()
        .ok()?;
    // Shown only: the display form keeps a name that is not valid UTF-8 apart.
    output.status.success().then(|| {
        crate::git::GitName::from_bytes(output.stdout.trim_ascii())
            .display()
            .to_string()
    })
}

fn refresh_target_worktrees(
    repo_root: &Path,
    target: &str,
    old_target_oid: git2::Oid,
    tested_oid: git2::Oid,
) -> Vec<String> {
    let mut warnings = Vec::new();
    let Ok(repo) = git2::Repository::open(repo_root) else {
        warnings.push("could not enumerate linked worktrees after landing".to_string());
        return warnings;
    };
    let current = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    // OS text rule (issue 79): the path below is handed to `reset --hard`, so
    // it comes from git's own files as exact bytes. A text listing could end a
    // path at a newline or spell it lossily, and then name another checkout.
    let target_ref = format!("refs/heads/{target}");
    let heads = match crate::git::checkout_heads(&repo) {
        Ok(heads) => heads,
        Err(error) => {
            warnings.push(format!(
                "cannot read linked worktrees after landing: {error}"
            ));
            return warnings;
        }
    };
    for (path, head) in heads {
        if head.as_ref().map(crate::git::GitName::bytes) != Some(target_ref.as_bytes()) {
            continue;
        }
        if path.canonicalize().unwrap_or_else(|_| path.clone()) == current {
            continue;
        }
        match worktree_clean_at(&path, old_target_oid) {
            Ok(true) => {
                let reset = crate::git::command()
                    .arg("-C")
                    .arg(&path)
                    .args(["reset", "--hard", &tested_oid.to_string()])
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
        crate::git::command()
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
        .and_then(|b| b.as_str().ok().map(ToString::to_string))
        .unwrap_or_else(|| oid.to_string())
}

fn count_commits(repo: &git2::Repository, old: git2::Oid, new: git2::Oid) -> usize {
    if old == new {
        return 0;
    }
    let Ok(mut walk) = history::revwalk(repo) else {
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
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) -> std::process::Output {
        let output = crate::git::command()
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

    /// Review finding on issue 79: a checked-out branch whose name is not
    /// valid UTF-8 was replaced by the target name, so a failed landing
    /// restored the wrong checkout. Integrate refuses before changing anything.
    /// Windows cannot hold the name as a ref path, so libgit2 fails to read
    /// the branch there and integrate refuses on that read instead.
    #[test]
    fn a_checked_out_branch_that_is_not_utf8_is_refused_before_any_change() {
        let dir = repo_with_feature_branch();
        let tip = branch_oid(dir.path(), "main");
        crate::git::write_packed_refs(
            &dir.path().join(".git"),
            &[
                (tip.clone(), b"refs/heads/main".to_vec()),
                (tip, b"refs/heads/caf\xe9".to_vec()),
            ],
        );
        fs::write(dir.path().join(".git/HEAD"), b"ref: refs/heads/caf\xe9\n").unwrap();
        let before = branch_oid(dir.path(), "feat/x");
        let error = integrate(dir.path(), "feat/x", "main").unwrap_err();
        let reason = if cfg!(windows) {
            "cannot read the checked-out branch"
        } else {
            "not valid UTF-8"
        };
        assert!(
            matches!(error, IntegrateError::Preflight(ref why) if why.contains(reason)),
            "{error}"
        );
        assert_eq!(before, branch_oid(dir.path(), "feat/x"));
    }

    /// A checked-out branch that cannot be read is no branch to restore:
    /// integrate refuses before any change instead of restoring the target.
    /// An unborn branch still restores the target, as nothing else exists.
    #[test]
    fn r23_an_unreadable_head_is_refused_before_any_change() {
        let dir = repo_with_feature_branch();
        write_test_config(dir.path(), "true");
        // Nothing staged, so git status, which reads the broken HEAD as
        // unborn, sees a clean tree and only the HEAD read can refuse.
        git(dir.path(), &["rm", "-q", "-r", "--cached", "base.txt"]);
        fs::remove_file(dir.path().join("base.txt")).unwrap();
        fs::write(
            dir.path().join(".git/refs/heads/broken"),
            b"not an object id\n",
        )
        .unwrap();
        fs::write(dir.path().join(".git/HEAD"), b"ref: refs/heads/broken\n").unwrap();
        let before = branch_oid(dir.path(), "feat/x");
        let error = integrate(dir.path(), "feat/x", "main").unwrap_err();
        assert!(
            matches!(error, IntegrateError::Preflight(ref why) if why.contains("cannot read the checked-out branch")),
            "{error}"
        );
        assert_eq!(before, branch_oid(dir.path(), "feat/x"));

        // The control: an unborn branch with nothing staged lands and puts
        // the checkout on the target.
        let dir = repo_with_feature_branch();
        write_test_config(dir.path(), "true");
        git(dir.path(), &["rm", "-q", "-r", "--cached", "base.txt"]);
        fs::remove_file(dir.path().join("base.txt")).unwrap();
        fs::write(dir.path().join(".git/HEAD"), b"ref: refs/heads/orphan\n").unwrap();
        let tested = branch_oid(dir.path(), "feat/x");
        let outcome = integrate(dir.path(), "feat/x", "main");
        assert!(
            outcome.is_ok(),
            "{:?}",
            outcome.err().map(|e| e.to_string())
        );
        assert_eq!(branch_oid(dir.path(), "main"), tested);
    }

    /// Review finding on issue 79: the path of another worktree that holds the
    /// target was read lossily and handed to `reset --hard`. It is the exact
    /// path now. Runs where the file system accepts the name, as on Linux.
    #[cfg(unix)]
    #[test]
    fn a_target_worktree_whose_path_is_not_utf8_is_refreshed_at_that_path() {
        use std::os::unix::ffi::OsStrExt as _;
        let dir = repo_with_feature_branch();
        let old = branch_oid(dir.path(), "main");
        let tested = branch_oid(dir.path(), "feat/x");
        git(dir.path(), &["branch", "land"]);
        let parent = tempfile::tempdir().unwrap();
        let wt = parent.path().join(std::ffi::OsStr::from_bytes(b"wt\xff"));
        if fs::create_dir(&wt).is_err() {
            return;
        }
        let added = crate::git::command()
            .arg("-C")
            .arg(dir.path())
            .args(["worktree", "add", "--force"])
            .arg(&wt)
            .arg("land")
            .output()
            .unwrap();
        assert!(added.status.success(), "{added:?}");
        let warnings = refresh_target_worktrees(
            dir.path(),
            "land",
            git2::Oid::from_str(&old).unwrap(),
            git2::Oid::from_str(&tested).unwrap(),
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        let head = crate::git::command()
            .arg("-C")
            .arg(&wt)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&head.stdout).trim(), tested);
    }

    /// Review finding on issue 79: a text listing ended a worktree path at a
    /// newline, so a checkout named `wt\nother` was read as `wt`, and the
    /// landing reset the wrong checkout. The path now comes from git's files.
    #[cfg(unix)]
    #[test]
    fn a_target_worktree_path_with_a_newline_never_names_another_checkout() {
        let dir = repo_with_feature_branch();
        let old = branch_oid(dir.path(), "main");
        let tested = branch_oid(dir.path(), "feat/x");
        git(dir.path(), &["branch", "land"]);
        git(dir.path(), &["branch", "other"]);
        let parent = tempfile::tempdir().unwrap();
        let plain = parent.path().join("wt");
        let tricky = parent.path().join("wt\nother");
        for (path, branch) in [(&plain, "other"), (&tricky, "land")] {
            let added = crate::git::command()
                .arg("-C")
                .arg(dir.path())
                .args(["worktree", "add"])
                .arg(path)
                .arg(branch)
                .output()
                .unwrap();
            assert!(added.status.success(), "{added:?}");
        }
        let warnings = refresh_target_worktrees(
            dir.path(),
            "land",
            git2::Oid::from_str(&old).unwrap(),
            git2::Oid::from_str(&tested).unwrap(),
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        let head = |path: &Path| {
            let out = crate::git::command()
                .arg("-C")
                .arg(path)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        assert_eq!(head(&tricky), tested, "the target worktree is refreshed");
        assert_eq!(head(&plain), old, "another checkout is left alone");
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

        write_test_config(&wt_path, "exit 0");
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
        let root_head =
            String::from_utf8_lossy(&git(dir.path(), &["symbolic-ref", "--short", "HEAD"]).stdout)
                .trim()
                .to_string();
        assert_eq!(root_head, "main", "root worktree stays on main");
        let landed = fs::read_to_string(dir.path().join("feature.txt")).unwrap();
        assert_eq!(
            landed.lines().collect::<Vec<_>>(),
            ["feature"],
            "target worktree files must match the landed tree"
        );
        let status = git(dir.path(), &["status", "--porcelain"]);
        assert!(status.stdout.is_empty(), "target worktree must be clean");
        assert!(outcome.warnings.is_empty(), "{:?}", outcome.warnings);
    }

    #[test]
    fn successful_ref_update_warns_when_checkout_restore_fails() {
        let dir = repo_with_feature_branch();
        let hooks = dir.path().join(".codeflow/test-hooks");
        fs::create_dir_all(&hooks).unwrap();
        let post_checkout = hooks.join("post-checkout");
        fs::write(&post_checkout, "#!/bin/sh\nexit 1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;

            let mut permissions = fs::metadata(&post_checkout).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&post_checkout, permissions).unwrap();
        }
        write_test_config(dir.path(), "git config core.hooksPath .codeflow/test-hooks");

        let outcome = integrate(dir.path(), "feat/x", "main")
            .expect("the landed ref is a partial success, not a failed integration");
        assert_eq!(
            branch_oid(dir.path(), "main"),
            branch_oid(dir.path(), "feat/x")
        );
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
        // Prepare an untested descendant, then make the passing gate advance
        // feat/x to it. update-ref needs no ambient Git identity and returns a
        // failure if the intended mutation does not happen.
        git(dir.path(), &["checkout", "-b", "untested", "feat/x"]);
        commit_file(
            dir.path(),
            "untested.txt",
            "untested\n",
            "test: add untested change",
        );
        git(dir.path(), &["checkout", "main"]);
        write_test_config(
            dir.path(),
            "git update-ref refs/heads/feat/x refs/heads/untested",
        );

        let before = branch_oid(dir.path(), "main");
        let err = integrate(dir.path(), "feat/x", "main").expect_err("must refuse");
        assert!(
            matches!(err, IntegrateError::MergeFailed { .. }),
            "expected a moved-branch refusal, got {err:?}"
        );
        assert!(
            err.to_string().contains("moved during integrate"),
            "refusal must identify the tested-tip race: {err}"
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
        commit_file(
            dir.path(),
            "file.txt",
            "branch change\n",
            "feat: branch edit",
        );
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

    /// A project that adopted `CodeFlow`'s release calculator, with a stub
    /// `release.py` that records its arguments and exits with `code`.
    fn adopt_release_calculator(dir: &Path, code: i32) {
        fs::write(
            dir.join(".codeflow/project.toml"),
            "scaffold_version = \"3.0.0\"\n\n[release]\nbackend = \"codeflow\"\n",
        )
        .unwrap();
        fs::create_dir_all(dir.join("scripts")).unwrap();
        fs::write(
            dir.join("scripts/release.py"),
            format!(
                "import sys\nopen('release-args.txt', 'w').write(' '.join(sys.argv[1:]))\n\
                 sys.stderr.write('release error: coupled stamps disagree')\nsys.exit({code})\n"
            ),
        )
        .unwrap();
    }

    /// SPC-013 R-94: `codeflow integrate` runs the release structure check in
    /// its test stage, on the exact commit it would land.
    #[test]
    fn integrate_runs_the_structural_release_check_in_its_test_stage() {
        if Command::new("python3").arg("--version").output().is_err() {
            return;
        }
        let dir = repo_with_feature_branch();
        write_test_config(dir.path(), "exit 0");
        adopt_release_calculator(dir.path(), 2);
        let before = branch_oid(dir.path(), "main");
        let tested = branch_oid(dir.path(), "feat/x");
        let err = integrate(dir.path(), "feat/x", "main").unwrap_err();
        assert!(
            matches!(err, IntegrateError::TestGateFailed { .. }),
            "expected TestGateFailed, got {err}"
        );
        let text = err.to_string();
        assert!(
            text.contains("release state (structural, not checked against the host)"),
            "{text}"
        );
        assert!(text.contains("coupled stamps disagree"), "{text}");
        assert_eq!(branch_oid(dir.path(), "main"), before, "main untouched");
        let args = fs::read_to_string(dir.path().join("release-args.txt")).unwrap();
        assert_eq!(args, format!("check-state --structural --ref {tested}"));

        adopt_release_calculator(dir.path(), 0);
        integrate(dir.path(), "feat/x", "main").expect("a valid release tree lands");
        assert_eq!(branch_oid(dir.path(), "main"), tested);
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
        write_test_config(dir.path(), "exit 0");

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

#[cfg(test)]
mod r22_regressions {
    use super::*;

    #[test]
    fn r22_unreadable_release_adoption_refuses_integration() {
        let dir = crate::fixture_tests::project_dir();
        assert!(run_release_structure(dir.path(), "main", "HEAD").is_ok());
        std::fs::create_dir(dir.path().join(".codeflow")).unwrap();
        std::fs::write(dir.path().join(".codeflow/project.toml"), "invalid = [").unwrap();
        let error = run_release_structure(dir.path(), "main", "HEAD").unwrap_err();
        assert!(error.to_string().contains("repair release configuration"));
    }
}
