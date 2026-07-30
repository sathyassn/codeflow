//! `codeflow git-hook
//! <pre-commit|commit-msg|pre-merge-commit|reference-transaction|pre-push>` —
//! the target the `.git/hooks` shims exec (charter §6.1 plane 1). Exit 1
//! blocks the git operation; warn-level findings are printed and let it
//! proceed.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Args;
use codeflow_core::hooks::{git_hook, policy::Policy, policy_schema};
use codeflow_core::validate::validate_workgraph;
use codeflow_core::workgraph::{
    check_work_start, declared_work_target, durable_work_tracking_enabled, resolve_work_target,
    task_id_from_branch,
};

/// Which git client hook stage to run.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum StageName {
    PreCommit,
    CommitMsg,
    PreMergeCommit,
    ReferenceTransaction,
    PrePush,
}

#[derive(Debug, Args)]
pub struct GitHookArgs {
    /// Hook stage (receives git's standard hook arguments).
    #[arg(value_enum)]
    pub stage: StageName,
    /// Arguments passed through by git (e.g. the commit-msg file path,
    /// the pre-push remote name and URL).
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

/// Run the stage; returns the process exit code.
#[must_use]
pub fn run(args: &GitHookArgs) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let root = super::project_root(&cwd);

    // reference-transaction fires on every ref update, including the hundreds
    // of remote-tracking refs a `git fetch` touches. Short-circuit before any
    // policy load or full stdin parse when it cannot apply (charter §6.1
    // performance note; ADR-0007).
    if let StageName::ReferenceTransaction = args.stage {
        return run_reference_transaction(&root, &args.args);
    }

    // A policy file that does not validate cannot express the project's intent
    // — fail the commit loudly, naming each offending key, rather than silently
    // enforce the built-in defaults (which could weaken a hardened gate). The
    // commit-msg stage is the loud surface: every commit passes through it,
    // and blocking here never strands a fetch or push mid-flight.
    if matches!(args.stage, StageName::CommitMsg) {
        if let Err(errors) = policy_schema::validate_policy(&root) {
            for e in &errors {
                eprintln!("codeflow commit-msg: policy error: {e}");
            }
            eprintln!(
                "codeflow commit-msg: .codeflow/policy.json is invalid — fix the key(s) above (see `codeflow policy explain`) or remove the file to use the built-in defaults"
            );
            return 1;
        }
    }

    if matches!(args.stage, StageName::PreCommit) {
        if let Some(exit) = durable_work_preflight(&root) {
            return exit;
        }
    }

    let (policy, _armed) = Policy::load_effective(&root);
    let token = super::integrate_token_present();

    let (plane, result) = match args.stage {
        StageName::PreCommit => (
            "pre-commit",
            git_hook::pre_commit(&root, &policy.git, token),
        ),
        StageName::CommitMsg => ("commit-msg", commit_msg(&root, &policy, &args.args)),
        StageName::PreMergeCommit => (
            "pre-merge-commit",
            git_hook::pre_merge_commit(
                &root,
                &policy.git,
                token,
                // Out-of-band human-authorization check-point (ADR-0009): today
                // `none` passes the env override through unchanged; a future
                // adapter would require its factor here.
                policy
                    .human_authorization
                    .authorizes_override(super::human_override_present()),
            ),
        ),
        StageName::ReferenceTransaction => unreachable!("handled above"),
        StageName::PrePush => {
            let stdin = match read_hook_input(std::io::stdin(), "pre-push") {
                Ok(stdin) => stdin,
                Err(note) => {
                    eprintln!("{note}");
                    String::new()
                }
            };
            let refs = git_hook::parse_push_refs(&stdin);
            (
                "pre-push",
                git_hook::pre_push(&root, &policy.git, &refs, token),
            )
        }
    };

    match result {
        Ok(report) => super::render_outcome(plane, &report.violations, &report.notes, 1),
        Err(e) => {
            // A hook that cannot evaluate must not block work invisibly:
            // report and pass (CI remains the hard line, charter D19).
            eprintln!("codeflow {plane}: warning: {e} — check skipped");
            0
        }
    }
}

/// Enforce a valid visible workgraph and stable planning anchor on task branches.
///
/// Planning belongs on a `plan/` branch. Allowing task-local planning commits
/// would let an implementation surface propose its own authorization and would
/// disagree with the authoritative CI check.
fn durable_work_preflight(root: &Path) -> Option<i32> {
    let branch = current_branch(root)?;
    if !branch.starts_with("task/") || !durable_work_tracking_enabled(root) {
        return None;
    }
    let staged = staged_files(root);
    if staged.is_empty() {
        return None;
    }
    let workgraph = validate_workgraph(root);
    if !workgraph.is_clean() {
        for issue in workgraph.issues {
            eprintln!("codeflow pre-commit: workgraph error: {issue}");
        }
        eprintln!(
            "codeflow pre-commit: task work cannot proceed until `codeflow validate --docs` passes"
        );
        return Some(1);
    }
    let Some(task_id) = task_id_from_branch(root, &branch) else {
        eprintln!(
            "codeflow pre-commit: task implementation is not ready: task branch \
             '{branch}' does not identify a visible durable task record\n\
             create and merge the task record into its stable integration target \
             before implementation"
        );
        return Some(1);
    };
    let declared = declared_work_target(root, &task_id);
    let target =
        resolve_work_target(root, declared.as_deref()).unwrap_or_else(|| "main".to_string());
    match check_work_start(root, &task_id, &target) {
        Ok(_) => None,
        Err(error) => {
            eprintln!(
                "codeflow pre-commit: task implementation is not ready: {error}\n\
                 run `codeflow work start {task_id}` after the planning record is merged into '{target}'"
            );
            Some(1)
        }
    }
}

fn current_branch(root: &Path) -> Option<String> {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["branch", "--show-current"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|branch| !branch.is_empty())
}

/// Handle `git-hook reference-transaction <state>`. Exit 1 on the `prepared`
/// state cancels the transaction; every other path exits 0.
fn run_reference_transaction(root: &std::path::Path, args: &[String]) -> i32 {
    // Only the `prepared` phase can cancel a transaction; `committed`/`aborted`
    // are notifications. Bail before touching stdin or policy.
    if args.first().map(String::as_str) != Some("prepared") {
        return 0;
    }
    let stdin = match read_hook_input(std::io::stdin(), "reference-transaction") {
        Ok(stdin) => stdin,
        Err(note) => {
            eprintln!("{note}");
            return 0;
        }
    };
    // Fast path: no local-branch update in this transaction (e.g. a fetch that
    // only moved remote-tracking refs) — allow without loading policy.
    if !stdin.lines().any(git_hook::ref_line_touches_local_branch) {
        return 0;
    }

    let (policy, _armed) = Policy::load_effective(root);
    let token = super::integrate_token_present();
    // Out-of-band human-authorization check-point (ADR-0009): `none` is a no-op.
    let human = policy
        .human_authorization
        .authorizes_override(super::human_override_present());
    match git_hook::reference_transaction(root, &policy.git, &stdin, token, human) {
        Ok(report) => super::render_outcome(
            "reference-transaction",
            &report.violations,
            &report.notes,
            1,
        ),
        Err(e) => {
            eprintln!("codeflow reference-transaction: warning: {e} — check skipped");
            0
        }
    }
}

fn read_hook_input(mut reader: impl Read, stage: &str) -> Result<String, String> {
    let mut input = String::new();
    reader.read_to_string(&mut input).map_err(|error| {
        format!(
            "codeflow {stage}: warning: could not read hook stdin ({error}) — \
             ref checks degraded; server-side CI remains authoritative"
        )
    })?;
    Ok(input)
}

fn commit_msg(
    root: &Path,
    policy: &Policy,
    args: &[String],
) -> Result<git_hook::StageReport, codeflow_core::error::HookError> {
    // Git invokes commit-msg with the message file path relative to its own
    // cwd (the worktree top level), which is also our cwd — read it as-is.
    let msg_file = args.first().map(PathBuf::from).ok_or_else(|| {
        codeflow_core::error::HookError::Config(
            "commit-msg: missing message file argument".to_string(),
        )
    })?;
    let message = std::fs::read_to_string(&msg_file)?;
    // The contract-surface tripwire needs the files this commit stages
    // (ADR-0020); empty on any error, so it simply does not fire.
    Ok(git_hook::commit_msg_with_files(
        &policy.git,
        &message,
        &staged_files(root),
        merge_in_progress(root),
    ))
}

/// True while git is creating a real merge commit — `MERGE_HEAD` exists in the
/// git dir. The `Merge ` subject exemption keys off THIS structural fact, not
/// the subject text, so a normal one-parent commit named `Merge ...` is still
/// format- and body-checked.
fn merge_in_progress(root: &Path) -> bool {
    let git_dir = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--git-dir"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    match git_dir {
        Some(dir) => root.join(dir).join("MERGE_HEAD").exists(),
        None => false,
    }
}

/// Files staged for the pending commit (`git diff --cached --name-only`), for
/// the contract-surface tripwire. Empty on any error — the tripwire is advisory,
/// so an unavailable file list simply means no nudge.
fn staged_files(root: &Path) -> Vec<String> {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["diff", "--cached", "--name-only"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FailingReader;

    impl Read for FailingReader {
        fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("stdin unavailable"))
        }
    }

    #[test]
    fn stdin_failure_note_names_degraded_stage() {
        for stage in ["pre-push", "reference-transaction"] {
            let note = read_hook_input(FailingReader, stage).unwrap_err();
            assert!(note.contains(stage), "{note}");
            assert!(note.contains("ref checks degraded"), "{note}");
        }
    }
}
