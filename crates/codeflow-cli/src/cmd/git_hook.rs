//! `codeflow git-hook
//! <pre-commit|commit-msg|pre-merge-commit|reference-transaction|pre-push>` —
//! the target the `.git/hooks` shims exec (charter §6.1 plane 1). Exit 1
//! blocks the git operation; warn-level findings are printed and let it
//! proceed.

use std::io::Read;
use std::path::{Path, PathBuf};

use clap::Args;
use codeflow_core::hooks::{git_hook, policy::Policy, policy_schema};

/// Which git client hook stage to run.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum StageName {
    PreCommit,
    CommitMsg,
    PreMergeCommit,
    ReferenceTransaction,
    PrePush,
    /// Print this binary's hook capability for the shims (SPC-013 R-85).
    Capabilities,
}

/// The capability the current hook shims need. A shim whose probe does not
/// print exactly this line warns that the binary is older than the shims.
pub const HOOK_CAPABILITY: &str = "hooks 3";
/// The stage names the dispatcher accepts, as git and the shims spell them.
/// The generated policy reference is checked against exactly this set; the
/// shims' capability probe is not a git hook stage.
pub fn stage_names() -> Vec<String> {
    use clap::ValueEnum;
    StageName::value_variants()
        .iter()
        .filter(|stage| !matches!(stage, StageName::Capabilities))
        .filter_map(clap::ValueEnum::to_possible_value)
        .map(|value| value.get_name().to_string())
        .collect()
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
    if let StageName::Capabilities = args.stage {
        println!("{HOOK_CAPABILITY}");
        return 0;
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let root = super::project_root(&cwd);
    // reference-transaction fires on every ref update, including the hundreds
    // of remote-tracking refs a `git fetch` touches. Short-circuit before any
    // policy load or full stdin parse when it cannot apply (charter §6.1
    // performance note; ADR-0007).
    if let StageName::ReferenceTransaction = args.stage {
        return run_reference_transaction(&root, &args.args);
    }

    // The judging binary's identity, for every stage a person reads: the
    // reference-transaction stage above stays silent and cheap.
    for line in git_hook::judging_identity(
        &root,
        env!("CARGO_PKG_VERSION"),
        env!("CODEFLOW_SOURCE_REVISION"),
        env!("CODEFLOW_SOURCE_DIRTY"),
        env!("CODEFLOW_HOOK_INPUT_DIGEST"),
    ) {
        eprintln!("{line}");
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
        for warning in policy_schema::deprecation_warnings(&root) {
            eprintln!("{}", warning.line("codeflow commit-msg", "warning"));
        }
    }

    let (policy, _armed) = Policy::load_effective(&root);
    let token = super::integrate_token_present();

    let (plane, result) = match args.stage {
        StageName::PreCommit => (
            "pre-commit",
            git_hook::pre_commit(&root, &policy.git, token, super::human_override_present()),
        ),
        StageName::CommitMsg => ("commit-msg", commit_msg(&root, &policy, &args.args)),
        StageName::PreMergeCommit => (
            "pre-merge-commit",
            git_hook::pre_merge_commit(&root, &policy.git, token, super::human_override_present()),
        ),
        StageName::ReferenceTransaction | StageName::Capabilities => {
            unreachable!("handled above")
        }
        StageName::PrePush => {
            let stdin = match read_hook_input(std::io::stdin()) {
                Ok(stdin) => stdin,
                Err(error) => {
                    eprintln!("{}", degraded_hook_input_note("pre-push", &error));
                    String::new()
                }
            };
            let refs = git_hook::parse_push_refs(&stdin);
            let mut result = git_hook::pre_push(
                &root,
                &policy.git,
                &refs,
                token,
                super::human_override_present(),
            );
            if let Ok(report) = result.as_mut() {
                let remote = args.args.first().map(String::as_str);
                let url = args.args.get(1).map(String::as_str);
                super::push_set::run(&root, &policy.git, &refs, remote, url, report);
                sync_pending_ids(&root, remote, &refs, report);
            }
            ("pre-push", result)
        }
    };

    match result {
        Ok(report) => super::render_stage(plane, &root, &report, 1),
        Err(e) => {
            // A hook that cannot evaluate must not block work invisibly:
            // report and pass (CI remains the hard line, charter D19).
            let finding = codeflow_core::remedy::Finding::new(
                format!("{e}; check skipped"),
                codeflow_core::remedy::HOOK_UNEVALUATED.remedy(),
            );
            eprintln!("{}", finding.line(&format!("codeflow {plane}"), "warning"));
            0
        }
    }
}

/// `ids sync` in pre-push (SPC-013 R-15): publish pending reservations
/// before code that may carry their records reaches the authority. It warns
/// and continues when the authority is unreachable or is not the push
/// target, blocks only on a number held by a different `uid`, and never runs
/// for a push of the registry itself, so it cannot recurse (R-6).
fn sync_pending_ids(
    root: &Path,
    remote: Option<&str>,
    refs: &[git_hook::PushRef],
    report: &mut git_hook::StageReport,
) {
    use codeflow_core::ids::{issue, IdsError, AUTHORITY, REGISTRY_BRANCH};
    let pushes_code = refs.iter().any(|r| {
        r.remote_branch()
            .is_some_and(|branch| branch != REGISTRY_BRANCH)
            && !r.is_delete()
    });
    if !pushes_code || !issue::has_pending(root) {
        return;
    }
    if remote != Some(AUTHORITY) {
        report.notes.push(codeflow_core::remedy::Finding::new(
            format!(
                "pending id reservations stay local: this push goes to {}, not the authority `{AUTHORITY}`",
                remote.unwrap_or("an unnamed remote")
            ),
            codeflow_core::remedy::IDS_PENDING_LOCAL.remedy(),
        ));
        return;
    }
    match issue::sync(root) {
        Ok(synced) if !synced.published.is_empty() => report.status.push(format!(
            "published pending id reservations: {}",
            synced
                .published
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )),
        Ok(_) => {}
        Err(IdsError::Clash(message)) => {
            report
                .violations
                .push(codeflow_core::hooks::Violation::always_blocking(
                "registry.sync",
                message,
                "renumber the unmerged record with `codeflow ids retarget <id>`, then push again",
            ));
        }
        Err(error) => report.notes.push(codeflow_core::remedy::Finding::new(
            format!("ids sync skipped, reservations stay pending: {error}"),
            codeflow_core::remedy::IDS_SYNC_FAILED.remedy(),
        )),
    }
}

/// Handle `git-hook reference-transaction <state>`. Exit 1 on the `prepared`
/// state cancels the transaction; every other path exits 0.
fn run_reference_transaction(root: &std::path::Path, args: &[String]) -> i32 {
    run_reference_transaction_with_reader(root, args, std::io::stdin())
}

fn run_reference_transaction_with_reader(
    root: &std::path::Path,
    args: &[String],
    reader: impl Read,
) -> i32 {
    // Only the `prepared` phase can cancel a transaction; `committed`/`aborted`
    // are notifications. Bail before touching stdin or policy.
    if args.first().map(String::as_str) != Some("prepared") {
        return 0;
    }
    let stdin = match read_hook_input(reader) {
        Ok(stdin) => stdin,
        Err(error) => {
            // An explicitly inactive policy has nothing to protect. Otherwise
            // unreadable prepared input leaves the transaction unclassifiable,
            // so Git must cancel it instead of silently moving a protected ref.
            let (policy, _armed) = Policy::load_effective(root);
            if !policy.git.local_ref_protection.is_active()
                && !policy.git.delete_protected.is_active()
            {
                return 0;
            }
            eprintln!(
                "codeflow reference-transaction: could not read reference-transaction input ({error}) — operation blocked while ref protection is active"
            );
            return 1;
        }
    };
    // Fast path: no local-branch update in this transaction (e.g. a fetch that
    // only moved remote-tracking refs) — allow without loading policy.
    if !stdin.lines().any(git_hook::ref_line_touches_local_branch) {
        return 0;
    }

    let (policy, _armed) = Policy::load_effective(root);
    let token = super::integrate_token_present();
    let human = super::human_override_present();
    match git_hook::reference_transaction(root, &policy.git, &stdin, token, human) {
        Ok(report) => super::render_stage("reference-transaction", root, &report, 1),
        Err(e) => {
            eprintln!(
                "codeflow reference-transaction: could not evaluate protected-ref transaction ({e}) — operation blocked"
            );
            1
        }
    }
}

// OS text rule (issue 79, `docs/architecture.md`): kept strict, by a recorded
// decision that `remedy_clearing` pins (`HOOK_STDIN_UNREAD`). The refs git
// passes name the branches whose protection is judged, and a ref name that is
// not valid UTF-8 is reported with the rename that clears it, never judged
// under a lossy name.
fn read_hook_input(mut reader: impl Read) -> std::io::Result<String> {
    let mut input = String::new();
    reader.read_to_string(&mut input)?;
    Ok(input)
}

fn degraded_hook_input_note(stage: &str, error: &std::io::Error) -> String {
    codeflow_core::remedy::Finding::new(
        format!("could not read hook stdin ({error}); ref checks degraded"),
        codeflow_core::remedy::HOOK_STDIN_UNREAD.remedy(),
    )
    .line(&format!("codeflow {stage}"), "warning")
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
    let message = std::fs::read_to_string(&msg_file).map_err(|e| {
        codeflow_core::error::HookError::Config(format!(
            "commit-msg: cannot read the message file {}: {e}",
            msg_file.display()
        ))
    })?;
    // The contract-surface tripwire needs the files this commit stages
    // (ADR-0020); empty on any error, so it simply does not fire.
    let mut report = git_hook::commit_msg_with_files(
        &policy.git,
        &message,
        &staged_files(root),
        merge_in_progress(root),
        &git_hook::MessageSource::Pending(pending_cleanup(root)),
    );
    // A commit has no pull request body to settle it with (TSK-147 AC-4).
    git_hook::note_watched_paths(&mut report);
    Ok(report)
}

/// The hook's best-effort inference of Git's cleanup of the commit-msg
/// message file, for early feedback only.
/// Git runs the hook before its cleanup and exports `GIT_EDITOR=:` to it when
/// no editor runs (`-m`, `-F`, `--no-edit`), which keeps `#` lines by
/// default. `commit.cleanup` and the comment prefix come from Git's effective
/// config, `git -c` included, read byte for byte. A command-line `--cleanup`,
/// `-v` or `--no-verbose` is not visible to a hook, so the inference can be
/// wrong in either direction; `codeflow ci` scans the stored message and
/// stays the authority.
fn pending_cleanup(root: &Path) -> git_hook::GitCleanup {
    let editor_used = std::env::var_os("GIT_EDITOR").is_none_or(|e| e != ":");
    let mode = git_config_values(root, &["--get", "commit.cleanup"]).pop();
    // core.commentString and core.commentChar set one value; the last wins.
    let comment = git_config_values(root, &["--get-regexp", r"^core\.comment(char|string)$"])
        .pop()
        .map(|entry| {
            entry
                .split_once('\n')
                .map_or(String::new(), |(_, v)| v.to_string())
        });
    git_hook::GitCleanup::resolve(mode.as_deref(), editor_used, comment.as_deref())
}

/// The NUL-separated entries of one `git config -z` read in `root`, exact
/// bytes, value text unchanged; empty when unset or unreadable.
fn git_config_values(root: &Path, args: &[&str]) -> Vec<String> {
    codeflow_core::git::command()
        .arg("-C")
        .arg(root)
        .args(["config", "-z"])
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .split_terminator('\0')
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// True while git is creating a real merge commit — `MERGE_HEAD` exists in the
/// git dir. The `Merge ` subject exemption keys off THIS structural fact, not
/// the subject text, so a normal one-parent commit named `Merge ...` is still
/// format- and body-checked.
fn merge_in_progress(root: &Path) -> bool {
    let git_dir = codeflow_core::git::command()
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
    codeflow_core::git::command()
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
    fn pre_push_stdin_failure_note_remains_degraded_and_nonblocking() {
        let error = read_hook_input(FailingReader).unwrap_err();
        let note = degraded_hook_input_note("pre-push", &error);
        assert!(note.contains("pre-push"), "{note}");
        assert!(note.contains("ref checks degraded"), "{note}");
        assert!(note.contains("clear it: rerun `git push`"), "{note}");
    }

    #[test]
    fn prepared_reference_transaction_stdin_failure_blocks_when_policy_is_active() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            1,
            run_reference_transaction_with_reader(
                dir.path(),
                &["prepared".to_string()],
                FailingReader,
            )
        );
    }

    #[test]
    fn prepared_reference_transaction_stdin_failure_passes_when_policy_is_inactive() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
        std::fs::write(
            dir.path().join(".codeflow/policy.json"),
            r#"{"git":{"local_ref_protection":"off","delete_protected":"off"}}"#,
        )
        .unwrap();
        assert_eq!(
            0,
            run_reference_transaction_with_reader(
                dir.path(),
                &["prepared".to_string()],
                FailingReader,
            )
        );
    }

    struct PanicReader;

    impl Read for PanicReader {
        fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
            panic!("notification phase must not read stdin")
        }
    }

    #[test]
    fn reference_transaction_notification_phases_do_not_read_stdin() {
        let dir = tempfile::tempdir().unwrap();
        for phase in ["committed", "aborted"] {
            assert_eq!(
                0,
                run_reference_transaction_with_reader(
                    dir.path(),
                    &[phase.to_string()],
                    PanicReader,
                ),
                "{phase}"
            );
        }
    }
}
