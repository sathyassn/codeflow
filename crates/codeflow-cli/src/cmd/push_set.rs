//! The pre-push push set (TSK-132), bound to what is actually pushed.
//!
//! - `codeflow ci` runs once per pushed branch on git data: the range from
//!   the destination's own history to the pushed sha.
//! - The tree checks (`codeflow validate --docs` and the test-config targets
//!   with a `quick` mode) read the working tree, so they run only when a
//!   pushed sha is the checked-out commit, no tracked file differs from it,
//!   and the checkout is complete (no sparse checkout, every submodule
//!   initialized at its recorded commit). Otherwise the hook notes that they
//!   did not run and CI runs them; they are never reported as passed for a
//!   tree they did not see. Untracked files are part of the working checkout
//!   and can influence a quick target, which the pass line says.
//! - The hook blocks on what it can see and never claims more: the range
//!   leaves out only history known to be on the destination: the branch and
//!   tag tips the push location advertises now (one `git ls-remote`, never
//!   interactive, bounded in time and size, fetching nothing, passed on to
//!   each `codeflow ci` it runs), plus the sha it advertised for an
//!   existing branch. A destination that cannot be asked cannot say whether
//!   the release rules apply, so the push is refused (SPC-013 R-120). With
//!   no destination given, an existing branch falls back to its advertised
//!   sha alone, and a new branch to its protected branches' tracking refs
//!   when those refs describe the same location. When nothing gives a base,
//!   the range is reported unresolved and left to CI, never compared with a
//!   local branch.
//! - For a push to an existing branch, the work-record check reads the
//!   governing `work_records_baseline` from the destination's current tip of
//!   that branch, the push's target, not from the range's base, which can be
//!   another line's tip. A new branch keeps the base. CI judging the pull
//!   request stays the authority.
//! - The whole set is timed; over [`git_hook::PUSH_SET_BUDGET`] the hook names
//!   the slowest step.
//!
//! Every check runs through this same binary, so the hook checks what CI will
//! check. A failure is a `git.test_gate_on_push` violation at the policy's
//! level, with the check's own output printed above it.

use std::cell::OnceCell;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

use codeflow_core::hooks::git_hook::{self, PushRef, PushStep, StageReport};
use codeflow_core::hooks::policy::{GitPolicy, INTEGRATION_BRANCH_PREFIX};
use codeflow_core::hooks::Violation;
use codeflow_core::remedy::{self, Finding};
use codeflow_core::workgraph::release_line;

#[cfg(test)]
mod tests;

/// Run the push set for `refs` into `report`. `remote` and `url` are the
/// pre-push hook's arguments: a configured remote name (or the URL or path
/// pushed to directly), and the location actually pushed to, which honours
/// `pushurl`.
pub(super) fn run(
    root: &Path,
    policy: &GitPolicy,
    refs: &[PushRef],
    remote: Option<&str>,
    url: Option<&str>,
    report: &mut StageReport,
) {
    if let Err(error) = run_checked(root, policy, refs, remote, url, report) {
        report.violations.push(Violation::always_blocking(
            "git.test_gate_on_push",
            format!("cannot prove push inputs: {error}"),
            "repair the unreadable input and retry the push",
        ));
    }
}

/// Whether the project adopted the local release check; an unreadable
/// release configuration refuses the push rather than skipping the preflight.
fn release_adopted(root: &Path) -> Result<bool, String> {
    codeflow_core::release_local::adopted(root).map_err(|error| {
        format!(
            "release preflight unavailable: {error}; repair release configuration before pushing"
        )
    })
}

fn run_checked(
    root: &Path,
    policy: &GitPolicy,
    refs: &[PushRef],
    remote: Option<&str>,
    url: Option<&str>,
    report: &mut StageReport,
) -> Result<(), String> {
    // A push of the id registry carries no code: it is judged by its own
    // rules (SPC-013 R-6), and checking it here would recurse through the
    // registry sync the hook runs.
    let pushed: Vec<&PushRef> = refs
        .iter()
        .filter(|r| {
            r.remote_branch()
                .is_some_and(|branch| branch != codeflow_core::ids::REGISTRY_BRANCH)
                && !r.is_delete()
        })
        .collect();
    // The working copy's gate decides the tree checks; each branch's
    // `codeflow ci` is gated by the policy at the destination's default
    // branch (see [`judged_by`]), so a head cannot turn that check off.
    if pushed.is_empty() {
        return Ok(());
    }
    let started = Instant::now();
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => {
            report.violations.push(violation(
                policy,
                format!("push set could not locate the codeflow binary: {error}"),
                codeflow_core::remedy::PUSH_SET_BY_HAND.remedy(),
            ));
            return Ok(());
        }
    };
    let url = url.or(remote);
    // Tracking refs describe the fetch location; they stand in for the push
    // location only when the two are the same.
    let namespace = match (remote, url) {
        (Some(name), Some(url)) if fetches_from(root, name, url)? => {
            tracking_namespace(root, name)?
        }
        _ => None,
    };
    // Asked once, on first use, for every pushed branch and each
    // `codeflow ci` the push set runs.
    let listing: OnceCell<Result<String, String>> = OnceCell::new();
    let advertised: OnceCell<Advertised> = OnceCell::new();
    let answer: OnceCell<Result<release_line::Destination, String>> = OnceCell::new();
    let anchor: OnceCell<Result<Anchor, String>> = OnceCell::new();
    let fetch_failed: OnceCell<String> = OnceCell::new();
    let destination = Destination {
        url,
        listing: &listing,
        advertised: &advertised,
        answer: &answer,
        anchor: &anchor,
        fetch_failed: &fetch_failed,
        fork: fork(root, url)?,
        namespace: namespace.as_deref(),
        policy,
    };
    let mut steps: Vec<PushStep> = Vec::new();

    run_ci_ranges(&exe, root, &pushed, &destination, report, &mut steps)?;

    if !policy.test_gate_on_push.is_active() {
        return Ok(());
    }
    if release_adopted(root)? {
        let remote = remote.unwrap_or("origin");
        for r in &pushed {
            release_preflight(&exe, root, r, remote, policy, report, &mut steps);
        }
    }

    let head = rev_parse(root, "HEAD")?;
    let incomplete = incomplete_checkout(root)?;
    let clean = incomplete.is_none() && tracked_tree_clean(root)?;
    let at_head = |r: &&&PushRef| clean && head.as_deref() == Some(r.local_sha.as_str());
    if pushed.iter().any(|r| at_head(&r)) {
        run_check(
            &exe,
            root,
            &["validate", "--docs"],
            policy,
            report,
            &mut steps,
        );
        steps.extend(git_hook::run_push_targets(root, policy, report));
    }
    for r in pushed.iter().filter(|r| !at_head(r)) {
        let why = if let Some(reason) = &incomplete {
            reason.clone()
        } else if clean {
            format!("{} is not the checked-out commit", short(&r.local_sha))
        } else {
            "tracked files differ from the checked-out commit".to_string()
        };
        report.notes.push(Finding::new(
            format!(
                "tree checks (`codeflow validate --docs`, quick targets) did not run for '{}': \
                 {why}; CI runs them",
                r.remote_branch().unwrap_or_default()
            ),
            remedy::PUSH_TREE_UNCHECKED.remedy(),
        ));
    }

    let total = started.elapsed();
    if let Some(note) = git_hook::over_budget_note(&steps, total, git_hook::PUSH_SET_BUDGET) {
        report.notes.push(note);
    }
    report
        .status
        .push(format!("push set finished in {:.1}s", total.as_secs_f64()));
    Ok(())
}

fn violation(
    policy: &GitPolicy,
    message: String,
    remedy: codeflow_core::remedy::Remedy,
) -> Violation {
    Violation::new(
        "git.test_gate_on_push",
        policy.test_gate_on_push,
        message,
        remedy,
    )
}

/// How to clear a push-set check: rerun it by hand after fixing its findings.
fn check_remedy(args: &[&str]) -> codeflow_core::remedy::Remedy {
    let (clearing, rest) = match args.split_first() {
        Some((&"validate", rest)) => (&codeflow_core::remedy::PUSH_SET_VALIDATE, rest),
        Some((_, rest)) => (&codeflow_core::remedy::PUSH_SET_CI, rest),
        None => (&codeflow_core::remedy::PUSH_SET_CI, args),
    };
    clearing.with(&[("args", &rest.join(" "))])
}

/// Run `codeflow ci` over each pushed ref's range. A release branch is
/// judged from the default target's tip, as its pull request is; another
/// branch from the boundary of what the destination holds, or noted when
/// that is unresolved; a push whose scope cannot be read is refused (see
/// [`release_base`]). The range's boundary bounds the checks other than
/// the commit checks; those run from the candidate authority's tip for
/// every branch that shares history with it, so no history the destination
/// holds, and no target a task record declares, hides a commit from them
/// (see [`commits_from`] for an unrelated deployment branch). The policy, and whether
/// and at what level the check gates the push, come from that authority
/// (see [`judged_by`]). An authority whose policy cannot be read or
/// validated refuses every pushed branch, range or not.
fn run_ci_ranges(
    exe: &Path,
    root: &Path,
    pushed: &[&PushRef],
    destination: &Destination<'_>,
    report: &mut StageReport,
    steps: &mut Vec<PushStep>,
) -> Result<(), String> {
    if let Some(why) = &destination.fork {
        report.status.push(format!("note: {why}"));
    }
    for r in pushed {
        let branch = r.remote_branch().unwrap_or_default();
        let judged = match judged_by(root, destination, steps) {
            Ok(judged) => judged,
            Err(error) => {
                refuse_authority(root, r, branch, destination, report, &error);
                continue;
            }
        };
        let (policy, judging) = match &judged {
            Judged::Target {
                target,
                tip,
                policy,
            } => {
                if !policy.test_gate_on_push.is_active() {
                    report.status.push(format!(
                        "`codeflow ci` did not run for '{branch}': the policy at {target} {}, the destination's default branch and the candidate authority, turns the push check off; the hosted job on the pull request's target is the enforcement",
                        short(tip)
                    ));
                    continue;
                }
                (
                    (**policy).clone(),
                    format!(
                        "`codeflow ci` judges '{branch}' with the policy at {target} {}, the destination's default branch: a candidate authority, not a known pull request target; the hosted job on the real target is the enforcement",
                        short(tip)
                    ),
                )
            }
            Judged::Invalid { target, tip, why } => {
                report.violations.push(Violation::always_blocking(
                    "git.policy_authority",
                    format!(
                        "the policy at {target} {}, the destination's default branch, {why}, so '{branch}' is refused",
                        short(tip)
                    ),
                    "if a newer codeflow wrote that policy, upgrade this codeflow and push again; if the policy itself is malformed, repair it on the destination's default branch through a reviewed pull request",
                ));
                continue;
            }
            Judged::Unverified(why) => {
                report
                    .status
                    .push(format!("no candidate authority for '{branch}': {why}"));
                (
                    blocking(destination.policy),
                    format!(
                        "`codeflow ci` checks '{branch}' with the policy at its range's base, at block level whatever the working copy's gate says; this is a best-effort check, not the hosted verdict"
                    ),
                )
            }
        };
        let policy = &policy;
        let base = ci_base(root, r, branch, destination, report)?;
        if let Some(base) = base {
            report.status.push(judging);
            let running = policy.test_gate_on_push.to_string();
            let mut args = vec![
                "ci",
                "--base",
                &base,
                "--head",
                &r.local_sha,
                "--branch",
                branch,
                "--run-level",
                &running,
            ];
            // The push's target is the branch itself: its current tip's
            // baseline list governs the record check, not the boundary's,
            // which can be another line's tip. A new branch keeps the base.
            if let Some(tip) = existing_tip(root, r)? {
                args.extend(["--baseline-from", tip]);
            }
            // The commit checks run from the candidate authority's tip (see
            // [`commits_from`]); a declared target bounds only the others.
            let from;
            if let Judged::Target { target, tip, .. } = &judged {
                from = commits_from(root, r, branch, target, tip, &base, report)?;
                args.extend(["--policy-from", tip, "--commits-from", &from]);
            }
            // The release scope reads the policy at this destination's
            // default target (SPC-013 R-120), from the advertisement the
            // hook already holds.
            let mut input = None;
            if let Some(url) = destination.url {
                args.extend(["--destination", url]);
                if let Ok(listed) = destination.listing(root) {
                    args.push("--advertisement-stdin");
                    input = Some(listed);
                }
            }
            run_check_with(exe, root, &args, input, policy, report, steps);
        }
    }
    Ok(())
}

fn refuse_authority(
    root: &Path,
    pushed: &PushRef,
    branch: &str,
    destination: &Destination<'_>,
    report: &mut StageReport,
    error: &str,
) {
    // The scope reader retains the precise destination/fetch remedy.
    // Both readers share cached acquisition results, so this never retries.
    let first = report.violations.len();
    if matches!(
        release_base(root, pushed, branch, destination, report),
        Scoped::Refused
    ) {
        for violation in &mut report.violations[first..] {
            violation.message = format!("cannot prove push inputs: {}", violation.message);
        }
    } else {
        report.violations.push(Violation::always_blocking(
            "git.policy_authority",
            format!("cannot prove push inputs: cannot prove the authority for '{branch}': {error}"),
            "repair the destination or policy named above, then push again",
        ));
    }
}

/// Where a pushed branch's commit checks start, under the candidate
/// authority `target` at `tip`. Whenever the head shares history with the
/// tip, everything it adds to the tip, as the hosted job's range from the
/// target tip: history the destination already holds under another name,
/// commits an earlier push carried that the tip now forbids, and commits
/// a declared target's line carries are not left out, and no commit is
/// filtered by first parent or ancestry path. A head whose history is
/// proven to share nothing with the tip, such as a deployment branch, is
/// never diffed against it: a branch the destination already has, that
/// this push fast-forwards, keeps its own new commits from its old tip,
/// and a new branch keeps the commits the destination does not hold yet,
/// from `base`, under the same policy; the hook says this is not
/// default-target parity, since that history was never a pull request into
/// the default branch. Unrelatedness is proven only from recorded history:
/// a shallow clone, a graft or replace ref (which can drop the parents
/// that join a branch to the tip) or an unreadable history proves nothing,
/// so it keeps the tip and says so.
fn commits_from(
    root: &Path,
    r: &PushRef,
    branch: &str,
    target: &str,
    tip: &str,
    base: &str,
    report: &mut StageReport,
) -> Result<String, String> {
    let keep = tip.to_string();
    if base == tip {
        return Ok(keep);
    }
    let related = codeflow_core::git::command()
        .arg("-C")
        .arg(root)
        .args(["merge-base", tip, &r.local_sha])
        .env("GIT_NO_LAZY_FETCH", "1")
        .output();
    let unknown = |why: &str, report: &mut StageReport| {
        report.status.push(format!(
            "note: whether '{branch}' shares history with {target} {} cannot be read ({why}), so push checks refuse",
            short(tip)
        ));
        Err(format!("cannot prove history: {why}"))
    };
    let out = match related {
        Ok(out) => out,
        Err(error) => return unknown(&error.to_string(), report),
    };
    match out.status.code() {
        Some(0) => return Ok(keep),
        Some(1) => {}
        _ => return unknown(String::from_utf8_lossy(&out.stderr).trim(), report),
    }
    let shallow = git(root, &["rev-parse", "--is-shallow-repository"])?;
    if shallow
        .as_deref()
        .map(|value| value.strip_suffix('\n').unwrap_or(value))
        != Some("false")
    {
        return unknown("this clone is shallow", report);
    }
    match release_line::history_overlay_at(root) {
        Ok(None) => {}
        Ok(Some(overlay)) => {
            return unknown(
                &format!("this clone overlays its recorded history with {overlay}"),
                report,
            )
        }
        Err(why) => return unknown(&why, report),
    }
    let (from, what) = match existing_tip(root, r)? {
        Some(old) => {
            if git(root, &["merge-base", "--is-ancestor", old, &r.local_sha])?.is_none() {
                return Ok(keep);
            }
            (old, "its own new commits")
        }
        None => (base, "the commits the destination does not hold yet"),
    };
    report.status.push(format!(
        "note: '{branch}' shares no history with {target} {}, so its commit checks run over {what}, from {}, under that policy; this is not default-target parity",
        short(tip),
        short(from)
    ));
    Ok(from.to_string())
}

/// The base of a pushed branch's `codeflow ci` range: a release branch's
/// default tip, else the boundary of what the destination holds, which a
/// declared target may select. `None` when the push is refused or the
/// range is unresolved, both reported.
fn ci_base(
    root: &Path,
    r: &PushRef,
    branch: &str,
    destination: &Destination<'_>,
    report: &mut StageReport,
) -> Result<Option<String>, String> {
    Ok(match release_base(root, r, branch, destination, report) {
        Scoped::Refused => None,
        Scoped::Release(tip) => Some(tip),
        Scoped::Ordinary => {
            let mut notices = Vec::new();
            let range = range_base(root, r, destination, &mut notices)?;
            report
                .status
                .extend(notices.into_iter().map(|text| format!("note: {text}")));
            if let Some(RangeBase { base, note }) = range {
                report.notes.extend(note);
                Some(base)
            } else {
                report.notes.push(unresolved(branch, destination));
                None
            }
        }
    })
}

/// The local release preflight of a project that adopted `CodeFlow`'s
/// calculator (SPC-013 R-93): it reads git data only, so it runs for every
/// pushed branch. A missing entry is a note; only a push that breaks a
/// release tree its base kept valid is a violation.
fn release_preflight(
    exe: &Path,
    root: &Path,
    pushed: &PushRef,
    remote: &str,
    policy: &GitPolicy,
    report: &mut StageReport,
    steps: &mut Vec<PushStep>,
) {
    let branch = pushed.remote_branch().unwrap_or_default();
    let started = Instant::now();
    let result =
        codeflow_core::release_local::preflight(root, &pushed.local_sha, branch, remote, exe);
    steps.push(PushStep {
        name: format!("release preflight ({branch})"),
        duration: started.elapsed(),
        configurable: false,
    });
    match result {
        Ok(outcome) => {
            for note in &outcome.notes {
                let line = format!("release preflight ({branch}): {note}");
                // A valid tree is a result, not a finding: nothing to clear.
                if note.starts_with(codeflow_core::release_local::TREE_VALID) {
                    report.status.push(line);
                } else {
                    report.notes.push(Finding::new(
                        line,
                        remedy::RELEASE_PREFLIGHT_NOTE.with(&[
                            ("script", codeflow_core::release_local::SCRIPT),
                            ("branch", branch),
                        ]),
                    ));
                }
            }
            if outcome.blocked() {
                report.violations.push(violation(
                    policy,
                    format!("release preflight ({branch}): this push breaks the release tree"),
                    codeflow_core::remedy::RELEASE_PREFLIGHT.with(&[
                        ("script", codeflow_core::release_local::SCRIPT),
                        ("branch", branch),
                    ]),
                ));
            }
        }
        Err(error) => report.notes.push(Finding::new(
            format!(
                "release preflight did not run for '{branch}': {error}; the pull request job checks it"
            ),
            remedy::RELEASE_PREFLIGHT_UNRUN.with(&[("branch", branch)]),
        )),
    }
}

/// Run one built-in check through this binary and time it.
fn run_check(
    exe: &Path,
    root: &Path,
    args: &[&str],
    policy: &GitPolicy,
    report: &mut StageReport,
    steps: &mut Vec<PushStep>,
) {
    run_check_with(exe, root, args, None, policy, report, steps);
}

/// As [`run_check`], with `input` on the check's stdin.
fn run_check_with(
    exe: &Path,
    root: &Path,
    args: &[&str],
    input: Option<&str>,
    policy: &GitPolicy,
    report: &mut StageReport,
    steps: &mut Vec<PushStep>,
) {
    // The command a person reruns: the check at its own levels, as CI runs
    // it, without the push gate's level, and asking the destination itself
    // instead of reading the hook's hand-off.
    let run_level = args.iter().position(|arg| *arg == "--run-level");
    let rerun: Vec<&str> = args
        .iter()
        .enumerate()
        .filter(|(at, arg)| {
            **arg != "--advertisement-stdin"
                && run_level.is_none_or(|level| *at != level && *at != level + 1)
        })
        .map(|(_, arg)| *arg)
        .collect();
    let shown = format!("codeflow {}", rerun.join(" "));
    // `codeflow ci` names its blocking rules in a file of its own, for the
    // refusal record; its printed findings can quote operation content.
    let rules_out = (args.first() == Some(&"ci"))
        .then(RulesOut::create)
        .flatten();
    let mut command = Command::new(exe);
    command.args(args);
    if let Some(out) = &rules_out {
        command.arg("--blocking-rules-out").arg(&out.0);
    }
    let started = Instant::now();
    let output = codeflow_core::git::output_with_input(
        command.current_dir(root).env_remove("CODEFLOW_PR_BODY"),
        input.unwrap_or_default().as_bytes(),
    );
    steps.push(PushStep {
        name: shown.clone(),
        duration: started.elapsed(),
        configurable: false,
    });
    match output {
        Ok(out) if out.status.success() => {
            // A passing check can still have degraded, or name what a human
            // reviews (a baseline list it introduces); keep that legible.
            // Validate prints its notes to stdout, ci to stderr.
            let printed = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            report.relayed.extend(
                relayed_findings(&printed)
                    .into_iter()
                    .map(|finding| format!("`{shown}`: {finding}")),
            );
        }
        Ok(out) => {
            eprint!("{}", String::from_utf8_lossy(&out.stdout));
            eprint!("{}", String::from_utf8_lossy(&out.stderr));
            if let Some(rules) = &rules_out {
                report.refused_by.extend(rules.read());
            }
            // A check run at the push gate's level (R-80) that still fails
            // holds a finding that keeps its block: it stops the push.
            let kept_block = run_level.is_some() && out.status.code() == Some(1);
            let mut failed = violation(
                policy,
                format!("push set check failed: `{shown}` (output above)"),
                check_remedy(&rerun),
            );
            if kept_block && policy.test_gate_on_push != codeflow_core::hooks::PolicyLevel::Block {
                failed.level = codeflow_core::hooks::PolicyLevel::Block;
                let _ = write!(
                    failed.message,
                    "; a finding there keeps its block level, so the push stops although \
                     git.test_gate_on_push is {}",
                    policy.test_gate_on_push
                );
            }
            report.violations.push(failed);
        }
        Err(error) => report.violations.push(violation(
            policy,
            format!("push set check could not run: `{shown}`: {error}"),
            codeflow_core::remedy::PUSH_SET_BY_HAND.remedy(),
        )),
    }
}

/// The warnings, notes and notices a passing check printed, each with the
/// lines under it that name its remedy. The per-user registry warning is
/// left out: this hook's own process reports it.
fn relayed_findings(stderr: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut open = false;
    for line in stderr.split_terminator('\n') {
        if line.starts_with("  ") {
            if open {
                if let Some(last) = found.last_mut() {
                    last.push('\n');
                    last.push_str(line);
                }
            }
            continue;
        }
        open = is_finding_header(line);
        if open {
            found.push(line.trim().to_string());
        }
    }
    found
}

/// A private file `codeflow ci` writes its blocking rule ids to
/// (`--blocking-rules-out`), removed when dropped.
struct RulesOut(std::path::PathBuf);

impl RulesOut {
    /// A new empty file in the temporary directory, created exclusively so
    /// no other file is written through it; `None` when it cannot be made.
    fn create() -> Option<Self> {
        let path = std::env::temp_dir().join(format!(
            "codeflow-blocking-rules-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .ok()?;
        Some(Self(path))
    }

    /// The rule ids the check wrote. Only text with a rule id's form is
    /// kept, so nothing else can reach the refusal record.
    fn read(&self) -> Vec<String> {
        std::fs::read_to_string(&self.0)
            .unwrap_or_default()
            .split_terminator('\n')
            .filter(|line| codeflow_core::hooks::is_rule_id(line))
            .map(str::to_string)
            .collect()
    }
}

impl Drop for RulesOut {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Whether `line` opens a finding: `<plane>: warning`, `note` or `notice`,
/// followed by `:` (a finding) or ` —` (a policy rule).
fn is_finding_header(line: &str) -> bool {
    let Some((plane, rest)) = line.split_once(": ") else {
        return false;
    };
    plane != "codeflow"
        && ["warning", "note", "notice"].iter().any(|kind| {
            rest.strip_prefix(kind)
                .is_some_and(|after| after.starts_with(':') || after.starts_with(" —"))
        })
}

/// The remote-tracking prefix a configured remote fetches into (for example
/// `refs/remotes/origin/`), read from its fetch refspecs. `None` for a URL or
/// path pushed to directly, or a remote with no glob fetch refspec: that
/// destination has no local history of its own to compare with.
fn tracking_namespace(root: &Path, remote: &str) -> Result<Option<String>, String> {
    let key = format!("remote.{remote}.fetch");
    let Some(out) = git(root, &["config", "-z", "--get-all", &key])? else {
        return Ok(None);
    };
    Ok(out.split('\0').find_map(|spec| {
        let (_, dst) = spec.strip_prefix('+').unwrap_or(spec).split_once(':')?;
        let prefix = dst.strip_suffix('*')?;
        prefix.starts_with("refs/").then(|| prefix.to_string())
    }))
}

/// What judges a pushed branch's `codeflow ci` (sathyassn/codeflow#22).
/// The authority is always a candidate: the destination's default branch,
/// which is the pull request's target only when the request goes there.
/// The hosted job, run on the real target, is the enforcement.
enum Judged {
    /// The validated policy at the destination default branch's tip: it
    /// decides the rules, whether the check gates the push and at what
    /// level.
    Target {
        target: String,
        tip: String,
        policy: Box<GitPolicy>,
    },
    /// The default branch's tip is here, and its policy cannot be read or
    /// fails strict validation: the push is refused, whether or not its
    /// range resolves. `why` says which, for this codeflow; a newer one
    /// may accept a policy this one rejects.
    Invalid {
        target: String,
        tip: String,
        why: String,
    },
    /// No candidate authority could be read: the check runs where a range
    /// resolves, at block level, and says it is not the hosted verdict.
    Unverified(String),
}

/// The destination default branch's advertised tip and its policy: the
/// one candidate authority, so nothing the pushed branch, another branch
/// at the destination or the working copy says chooses whose policy
/// judges it.
struct Anchor {
    name: String,
    tip: String,
    policy: TargetPolicy,
}

/// A default tip's policy, read as git data and strictly validated.
enum TargetPolicy {
    Valid(Box<GitPolicy>),
    /// The tip carries no policy yet (the change that adopts `CodeFlow`).
    Missing,
    /// Why the policy at a tip that is here cannot be used: it cannot be
    /// read (bytes that are not UTF-8, an unreadable tree or blob) or it
    /// fails strict validation. Content, never acquisition: the tip is
    /// already here, so this refuses the push.
    Malformed(String),
}

/// Malformed is unproven; `judged_by` carries it into `Judged::Invalid` and
/// `run_ci_ranges` refuses regardless of the candidate policy level.
fn target_policy(root: &Path, tip: &str) -> TargetPolicy {
    let version = env!("CARGO_PKG_VERSION");
    let text = match codeflow_core::hooks::landed_policy::policy_text_at(root, tip) {
        Ok(Some(text)) => text,
        Ok(None) => return TargetPolicy::Missing,
        Err(why) => {
            return TargetPolicy::Malformed(format!(
                "cannot be read by this codeflow {version} ({why})"
            ))
        }
    };
    let invalid = || TargetPolicy::Malformed(format!("is not valid for this codeflow {version}"));
    if codeflow_core::hooks::policy_schema::validate_policy_str(&text).is_err() {
        return invalid();
    }
    serde_json::from_str::<codeflow_core::hooks::policy::Policy>(&text).map_or_else(
        |_| invalid(),
        |policy| TargetPolicy::Valid(Box::new(policy.git)),
    )
}

/// The working copy's git policy with the push check at block level: the
/// fallback cannot be lowered or turned off by the head.
fn blocking(policy: &GitPolicy) -> GitPolicy {
    GitPolicy {
        test_gate_on_push: codeflow_core::hooks::PolicyLevel::Block,
        ..policy.clone()
    }
}

/// A note for a push that does not go to the configured `upstream`: its
/// pull request may target the upstream, whose policy can differ from the
/// candidate authority read here.
fn fork(root: &Path, url: Option<&str>) -> Result<Option<String>, String> {
    let Some(upstream) = git(root, &["config", "--get", "remote.upstream.url"])? else {
        return Ok(None);
    };
    let upstream = upstream.strip_suffix('\n').unwrap_or(&upstream);
    Ok((url != Some(upstream)).then(|| format!("the push goes to {}, not the configured upstream {upstream}; a pull request may target the upstream", url.unwrap_or("an unnamed destination"))))
}

/// What judges every pushed branch: the policy at the destination default
/// branch's advertised tip, fetched once per push when this clone lacks it
/// (the fetch is timed in the push set's steps, and a failure is not
/// retried). No other branch is an authority, protected or not: a branch
/// the destination advertises proves it exists, not that its policy
/// landed through review. An unanswered destination, a failed fetch or a
/// default branch without a policy leave no candidate authority.
fn judged_by(
    root: &Path,
    destination: &Destination<'_>,
    steps: &mut Vec<PushStep>,
) -> Result<Judged, String> {
    let asked = destination
        .answer(root)
        .map_err(str::to_string)?
        .as_ref()
        .map_err(Clone::clone)?;
    if asked.default.is_none() {
        return Ok(Judged::Unverified(
            "the destination has no default branch yet".into(),
        ));
    }
    let anchor = match destination.anchor(root, steps) {
        Ok(anchor) => anchor,
        Err(why) => return Err(why.clone()),
    };
    let (target, tip) = (anchor.name.clone(), anchor.tip.clone());
    Ok(match &anchor.policy {
        TargetPolicy::Valid(policy) => Judged::Target {
            target,
            tip,
            policy: policy.clone(),
        },
        TargetPolicy::Malformed(why) => Judged::Invalid {
            target,
            tip,
            why: why.clone(),
        },
        TargetPolicy::Missing => Judged::Unverified(format!(
            "the destination's default branch {target} has no policy yet"
        )),
    })
}

/// What is known of the destination's history.
struct Destination<'a> {
    /// Where the push goes: the hook's URL argument, else its remote name.
    url: Option<&'a str>,
    /// The destination's one advertisement (see
    /// [`release_line::advertisement`]), or why it could not be asked.
    /// Filled on first use.
    listing: &'a OnceCell<Result<String, String>>,
    /// The advertised commits that exist here, from `listing`.
    advertised: &'a OnceCell<Advertised>,
    /// The destination's default target and branches, from `listing`.
    answer: &'a OnceCell<Result<release_line::Destination, String>>,
    /// The default branch's tip and policy, acquired once per push.
    anchor: &'a OnceCell<Result<Anchor, String>>,
    /// Why fetching the default branch's tip failed, so the release scope
    /// does not fetch it again.
    fetch_failed: &'a OnceCell<String>,
    /// Why the destination is not the upstream, for a fork.
    fork: Option<String>,
    /// The tracking namespace of a remote that fetches from `url`, for the
    /// protected-branch fallback; `None` when no tracking refs describe it.
    namespace: Option<&'a str>,
    policy: &'a GitPolicy,
}

struct AdvertisedTips {
    /// Advertised commits available locally, sorted for membership checks.
    commits: Vec<String>,
    /// All advertised branch tips, including those not fetched here.
    branches: BTreeMap<String, String>,
}

/// The destination's answer to `git ls-remote`.
enum Advertised {
    /// The advertised commits that exist here.
    Tips(AdvertisedTips),
    /// Why it could not be asked.
    Failed(String),
}

impl Destination<'_> {
    /// The advertisement, asked on first use.
    fn listing(&self, root: &Path) -> Result<&str, &str> {
        self.listing
            .get_or_init(|| {
                self.url.map_or_else(
                    || Err("no destination given".to_string()),
                    |url| release_line::advertisement(root, url),
                )
            })
            .as_deref()
            .map_err(String::as_str)
    }

    fn advertised(&self, root: &Path) -> &Advertised {
        self.advertised.get_or_init(|| match self.listing(root) {
            Ok(listed) => advertised_commits(root, listed),
            Err(why) => Advertised::Failed(why.to_string()),
        })
    }

    /// The destination's default target and branches: `Err` when it did
    /// not answer, `Ok(Err)` when its answer names no usable default
    /// target. With no destination given it is empty.
    fn answer(&self, root: &Path) -> Result<&Result<release_line::Destination, String>, &str> {
        let Some(url) = self.url else {
            return Ok(self
                .answer
                .get_or_init(|| Ok(release_line::Destination::default())));
        };
        let listed = self.listing(root)?;
        Ok(self
            .answer
            .get_or_init(|| release_line::from_advertisement(url, listed)))
    }

    /// The default branch's advertised tip, fetched when this clone lacks
    /// it, and its policy: acquired once per push, the acquisition timed as
    /// a push-set step.
    fn anchor(&self, root: &Path, steps: &mut Vec<PushStep>) -> Result<&Anchor, &String> {
        self.anchor
            .get_or_init(|| {
                let started = Instant::now();
                let anchor = (|| {
                    let asked = match self.answer(root) {
                        Ok(Ok(asked)) => asked,
                        Ok(Err(why)) => return Err(why.clone()),
                        Err(why) => return Err(format!("the destination did not answer ({why})")),
                    };
                    let Some((name, _)) = &asked.default else {
                        return Err("the destination has no default branch yet".to_string());
                    };
                    let tip = release_line::advertised_tip_here(root, asked, name).inspect_err(
                        |why| {
                            let _ = self.fetch_failed.set(why.clone());
                        },
                    )?;
                    let tip = tip.to_string();
                    Ok(Anchor {
                        name: name.clone(),
                        policy: target_policy(root, &tip),
                        tip,
                    })
                })();
                steps.push(PushStep {
                    name: "target policy".to_string(),
                    duration: started.elapsed(),
                    configurable: false,
                });
                anchor
            })
            .as_ref()
    }

    /// Why the destination could not be asked, once it was tried.
    fn failure(&self) -> Option<&str> {
        self.listing.get()?.as_ref().err().map(String::as_str)
    }
}

/// Whether remote `name` fetches from `url`, so that its tracking refs
/// describe the location pushed to. A `pushurl` elsewhere does not.
fn fetches_from(root: &Path, name: &str, url: &str) -> Result<bool, String> {
    Ok(git(root, &["remote", "get-url", name])?
        .is_some_and(|fetch| fetch.strip_suffix('\n').unwrap_or(&fetch) == url))
}

/// How a pushed branch's range is chosen, by its scope at the destination.
enum Scoped {
    /// A release branch, judged from the default target's tip.
    Release(String),
    /// Any other branch, judged from what the destination holds.
    Ordinary,
    /// Not judged and refused; the violation is reported.
    Refused,
}

/// The scope of a pushed branch under the policy at the destination's
/// default target (SPC-013 R-120). A release branch's range is everything
/// it adds to the default target's tip, the range its pull request is
/// judged on, however the destination already holds its commits: history
/// published under another name was never judged as release work, so no
/// advertised boundary may hide it. The release rules are acceptance
/// rules, so they apply where durable work tracking is on at the checkout,
/// the pushed commit or the destination's default target, the same places
/// `codeflow ci` looks. Each is read, never assumed: the default target's
/// tip is fetched when this clone lacks it, and a push is ordinary only
/// when all three are read and none tracks durable work. A destination
/// that does not answer, an answer with no usable default target, or a
/// state that cannot be read refuses the push whatever its name: a failed
/// query does not prove the push itself fails. A release branch pushed
/// where no default target exists yet is refused.
fn release_base(
    root: &Path,
    pushed: &PushRef,
    branch: &str,
    destination: &Destination<'_>,
    report: &mut StageReport,
) -> Scoped {
    use codeflow_core::workgraph::{
        durable_work_tracking_enabled, durable_work_tracking_enabled_at,
    };
    // Not judging a release push is never a warning (R-80): the refusal
    // keeps its block whatever `git.test_gate_on_push` is, as CI's own
    // refusal of an unreadable release scope does.
    let mut refuse = |message: String, sanctioned: &str| {
        report.violations.push(Violation::always_blocking(
            codeflow_core::workgraph::acceptance::FROZEN_RULE,
            message,
            sanctioned,
        ));
        Scoped::Refused
    };
    let asked = match destination.answer(root) {
        Ok(Ok(asked)) => asked,
        Ok(Err(why)) => {
            return refuse(
                format!("whether '{branch}' is a release branch cannot be decided, so it is not pushed unjudged (SPC-013 R-120): {why}"),
                "fix what the message names at the destination, then push again",
            )
        }
        Err(why) => {
            // Never a warning (R-80), with the catalogued step that clears it.
            let mut silent = Violation::new(
                codeflow_core::workgraph::acceptance::FROZEN_RULE,
                codeflow_core::hooks::PolicyLevel::Block,
                format!("the destination did not answer, so whether '{branch}' is a release branch cannot be read; it is not pushed unjudged (SPC-013 R-120): asking the destination for its branches failed ({why})"),
                remedy::PUSH_DESTINATION_SILENT.remedy(),
            );
            silent.level_fixed = true;
            report.violations.push(silent);
            return Scoped::Refused;
        }
    };
    // Both release-scope reads below need the default target's tip; when
    // the authority's fetch of it already failed, neither fetches again.
    if let Some(why) = destination.fetch_failed.get() {
        return refuse(
            format!("whether the release rules apply to '{branch}' cannot be read, so it is not pushed unjudged (SPC-013 R-120): {why}"),
            "fix what the message names, then push again",
        );
    }
    let tracked = match (
        durable_work_tracking_enabled(root).map_err(|error| error.to_string()),
        durable_work_tracking_enabled_at(root, &pushed.local_sha),
    ) {
        (Ok(here), Ok(at_head)) => here || at_head,
        (Err(error), _) | (_, Err(error)) => {
            return refuse(
                format!("cannot read push tracking state: {error}"),
                "repair the unreadable work state and retry",
            )
        }
    };
    let tracked = tracked
        || match release_line::default_tracks_work(root, asked) {
            Ok(on) => on,
            Err(why) => {
                return refuse(
                    format!("whether the release rules apply to '{branch}' cannot be read, so it is not pushed unjudged (SPC-013 R-120): {why}"),
                    "fix what the message names, then push again",
                )
            }
        };
    if !tracked {
        return Scoped::Ordinary;
    }
    match release_line::scope(root, asked, branch, None) {
        Ok(scope) if !scope.release() => Scoped::Ordinary,
        Ok(_) => match &asked.default {
            Some((name, tip)) => {
                let tip = tip.to_string();
                // A result, not a finding: nothing to clear.
                report.status.push(format!(
                    "'{branch}' is a release branch: `codeflow ci` judges everything it adds to {name} at {}, as its pull request is",
                    short(&tip)
                ));
                Scoped::Release(tip)
            }
            None => refuse(
                format!("'{branch}' is a release branch, and the destination has no default target yet to judge it against (SPC-013 R-120)"),
                "push the default branch first, then the release branch",
            ),
        },
        Err(why) => refuse(
            format!("whether '{branch}' is a release branch cannot be decided, so it is not pushed unjudged (SPC-013 R-120): {why}"),
            "fix what the message names at the destination, then push again",
        ),
    }
}

/// Why `codeflow ci` did not run for a pushed branch whose range has no base.
fn unresolved(branch: &str, destination: &Destination<'_>) -> Finding {
    let why = destination
        .failure()
        .map(|why| format!("; asking it: {why}"))
        .unwrap_or_default();
    Finding::new(
        format!(
            "`codeflow ci` did not run for '{branch}': range unresolved (a new \
             branch, and neither the destination's advertised tips nor tracking \
             refs bound to it give a base{why}); CI checks it"
        ),
        remedy::PUSH_RANGE_UNRESOLVED.remedy(),
    )
}

/// The branch and tag tips the destination advertises now (tags peeled),
/// from its advertisement `listed`, kept when the commit already exists
/// here; nothing is fetched, even in a partial clone.
fn advertised_commits(root: &Path, listed: &str) -> Advertised {
    let shas: Vec<&str> = listed
        .split_terminator('\n')
        .filter_map(|line| line.split_once('\t').map(|(sha, _)| sha))
        .filter(|sha| sha.len() >= 40 && sha.chars().all(|c| c.is_ascii_hexdigit()))
        .collect();
    if shas.is_empty() {
        return Advertised::Tips(AdvertisedTips {
            commits: Vec::new(),
            branches: BTreeMap::new(),
        });
    }
    let mut input = shas.join("\n");
    input.push('\n');
    let Ok(Some(types)) = git_input(
        root,
        &["cat-file", "--batch-check=%(objectname) %(objecttype)"],
        &input,
    ) else {
        return Advertised::Failed("its tips could not be looked up here".to_string());
    };
    let mut commits: Vec<String> = types
        .split_terminator('\n')
        .filter_map(|line| line.strip_suffix(" commit"))
        .map(ToString::to_string)
        .collect();
    commits.sort();
    commits.dedup();
    let branches = listed
        .split_terminator('\n')
        .filter_map(|line| line.split_once('\t'))
        .filter_map(|(sha, reference)| {
            let branch = reference.strip_prefix("refs/heads/")?;
            (sha.len() >= 40 && sha.chars().all(|c| c.is_ascii_hexdigit()))
                .then(|| (branch.to_string(), sha.to_string()))
        })
        .collect();
    Advertised::Tips(AdvertisedTips { commits, branches })
}

/// The exclusive base of a pushed branch's range, and a note when the range
/// is wider than the push itself.
struct RangeBase {
    base: String,
    note: Option<Finding>,
}

/// Select a range without excluding any destination-missing commit:
/// 1. An existing protected/integration fast-forward uses its advertised old
///    tip when the destination can be asked.
/// 2. A branch with a task target at its pushed commit uses the merge base
///    with that target's advertised tip when the tip is available locally.
///    An unfetched target is noted, even if the fallback finds no base.
/// 3. An existing branch otherwise uses the historical boundary against all
///    locally available advertised tips and its own old sha. A rewrite is
///    noted; unrelated history uses the old sha so CI reports it unrelated.
/// 4. If the destination cannot be asked or its answer is unreadable, refuse.
/// 5. A new branch uses the historical boundary against available advertised
///    tips, or its pushed sha when the destination already holds all its history.
/// 6. With no locally available advertised tips, a new branch uses protected
///    tracking refs only from the same destination.
/// 7. Otherwise return `None` and leave the range unresolved for CI. Never
///    substitute a policy default or a local branch for an undeclared target.
fn range_base(
    root: &Path,
    r: &PushRef,
    destination: &Destination<'_>,
    notices: &mut Vec<String>,
) -> Result<Option<RangeBase>, String> {
    if let Advertised::Tips(tips) = destination.advertised(root) {
        if let Some(base) = target_base(root, r, destination.policy, tips, notices)? {
            return Ok(Some(base));
        }
    }
    if existing_tip(root, r)?.is_some() {
        return Ok(Some(existing_base(root, r, destination)?));
    }
    let note = match destination.advertised(root) {
        Advertised::Tips(tips) if !tips.commits.is_empty() => {
            let line = own_line_tip(root, r, destination)?;
            return Ok(bounded_by(
                root,
                &r.local_sha,
                tips.commits.iter(),
                None,
                line.as_deref(),
            )?
            .map(|base| RangeBase { base, note: None }));
        }
        Advertised::Tips(_) => None,
        Advertised::Failed(why) => return Err(why.clone()),
    };
    let Some(ns) = destination.namespace else {
        return Ok(None);
    };
    let mut known: Vec<String> = Vec::new();
    for branch in &destination.policy.protected_branches {
        let reference = format!("{ns}{branch}");
        if branch.contains(['*', '?', '[']) {
            let any = git(
                root,
                &[
                    "for-each-ref",
                    "--count=1",
                    "--format=%(refname)",
                    &reference,
                ],
            )?
            .is_some_and(|out| !out.is_empty());
            if any {
                known.push(format!("--glob={reference}"));
            }
        } else if is_commit(root, &reference)? {
            known.push(reference);
        }
    }
    if known.is_empty() {
        return Ok(None);
    }
    let mut args = vec!["rev-list", "--boundary", r.local_sha.as_str(), "--not"];
    args.extend(known.iter().map(String::as_str));
    let listed = git(root, &args)?.ok_or("cannot read range boundaries")?;
    Ok(boundary(&listed, &r.local_sha, note))
}

/// Match the pull request's base without excluding any destination-missing
/// commit: the selected base is either an advertised tip or its ancestor.
/// Line rewrites and unavailable targets retain the historical fallback.
fn target_base(
    root: &Path,
    r: &PushRef,
    policy: &GitPolicy,
    tips: &AdvertisedTips,
    notices: &mut Vec<String>,
) -> Result<Option<RangeBase>, String> {
    let Some(branch) = r.remote_branch() else {
        return Ok(None);
    };
    let line = policy.branch_is_protected(branch) || branch.starts_with(INTEGRATION_BRANCH_PREFIX);
    let (base, target) = if line {
        let Some(old) = existing_tip(root, r)? else {
            return Ok(None);
        };
        if git(root, &["merge-base", "--is-ancestor", old, &r.local_sha])?.is_none() {
            return Ok(None);
        }
        (old.to_string(), branch.to_string())
    } else {
        let target = codeflow_core::workgraph::work_start::declared_work_target_at_revision(
            root,
            branch,
            &r.local_sha,
        )
        .map_err(|error| error.to_string())?;
        let Some(target) = target else {
            return Ok(None);
        };
        if !codeflow_core::workgraph::is_stable_work_target(&target) {
            return Ok(None);
        }
        let target = target.as_str();
        let Some(tip) = tips.branches.get(target) else {
            return Ok(None);
        };
        if tips.commits.binary_search(tip).is_err() {
            notices.push(format!(
                "declared target '{target}' advertised at {tip} is not fetched here; \
                     falling back to advertised-history range selection for '{branch}'"
            ));
            return Ok(None);
        }
        let Some(base) = git(root, &["merge-base", &r.local_sha, tip])? else {
            return Ok(None);
        };
        (
            base.strip_suffix('\n').unwrap_or(&base).to_string(),
            target.to_string(),
        )
    };
    notices.push(format!(
        "range of '{branch}' uses advertised target '{target}': `codeflow ci --base {base} --head {}`",
        r.local_sha
    ));
    let mut note = None;
    if let Some(old) = existing_tip(root, r)? {
        if git(root, &["merge-base", "--is-ancestor", old, &r.local_sha])?.is_none()
            && git(root, &["merge-base", old, &r.local_sha])?.is_some()
        {
            let count = git(
                root,
                &["rev-list", "--count", &format!("{base}..{}", r.local_sha)],
            )?
            .ok_or("cannot read rewritten commit count")?;
            note = Some(Finding::new(
                format!(
                    "'{branch}' rewrites the destination's {}: `codeflow ci` checks {} commit(s), \
                     leaving out history on the target",
                    short(old),
                    count.trim()
                ),
                remedy::PUSH_REWRITE.remedy(),
            ));
        }
    }
    Ok(Some(RangeBase { base, note }))
}

/// The destination's advertised sha for a branch it already has, when that
/// commit is here: the push updates an existing branch.
fn existing_tip<'a>(root: &Path, r: &'a PushRef) -> Result<Option<&'a str>, String> {
    let zero = r.remote_sha.is_empty() || r.remote_sha.chars().all(|c| c == '0');
    Ok((!zero && is_commit(root, &r.remote_sha)?).then_some(r.remote_sha.as_str()))
}

/// The historical advertised-history fallback for an existing branch.
fn existing_base(
    root: &Path,
    r: &PushRef,
    destination: &Destination<'_>,
) -> Result<RangeBase, String> {
    let branch = r.remote_branch().unwrap_or_default();
    let old = &r.remote_sha;
    let line = own_line_tip(root, r, destination)?;
    let (base, failed): (String, Option<&String>) = match destination.advertised(root) {
        Advertised::Tips(tips) => (
            bounded_by(
                root,
                &r.local_sha,
                std::iter::once(old).chain(&tips.commits),
                Some(old),
                line.as_deref(),
            )?
            .unwrap_or_else(|| old.clone()),
            None,
        ),
        Advertised::Failed(why) => return Err(why.clone()),
    };
    let extends = git(root, &["merge-base", "--is-ancestor", old, &r.local_sha])?.is_some();
    let related = git(root, &["merge-base", old, &r.local_sha])?.is_some();
    let count = if !extends && related {
        let range = format!("{base}..{}", r.local_sha);
        Some(
            git(root, &["rev-list", "--count", &range])?
                .ok_or("cannot read rewrite count")?
                .trim()
                .to_string(),
        )
    } else {
        None
    };
    let rewrite = format!("'{branch}' rewrites the destination's {}", short(old));
    let note = match (failed, count) {
        (None, None) => None,
        (None, Some(count)) => Some(Finding::new(
            format!(
                "{rewrite}: `codeflow ci` checks {count} commit(s), leaving out history the \
                 destination's branches and tags hold"
            ),
            remedy::PUSH_REWRITE.remedy(),
        )),
        (Some(why), count) => {
            let fallback = format!(
                "asking the destination for its branches failed ({why}): the range of \
                 '{branch}' is bounded by its advertised {} alone",
                short(old)
            );
            Some(Finding::new(
                match count {
                    Some(count) => format!(
                        "{fallback}; {rewrite}: `codeflow ci` checks all {count} commit(s) not \
                         on it, including any the rewrite brought in from other branches"
                    ),
                    None => fallback,
                },
                remedy::PUSH_DESTINATION_SILENT.remedy(),
            ))
        }
    };
    Ok(RangeBase { base, note })
}

/// The exclusive base of `local_sha` against the `known` commits the
/// destination holds: the boundary of what is new, or the pushed sha itself
/// when nothing is. When several commits bound it, the newest of those on
/// the pushed task's own integration line (`line`, its advertised tip) is
/// the base: what the branch took from its line by a merge is the line's,
/// already judged there, not the task's, so the range is the one its pull
/// request into the line is judged on. Without such a line, [`narrowest`]
/// picks among the boundaries, where `own` (the branch's own advertised
/// sha) is not another ref. The base is always a commit the destination
/// holds, so nothing new is left out. `None` means no boundary exists
/// (no shared history); Git obtaining errors propagate to the push refusal.
fn bounded_by<'a>(
    root: &Path,
    local_sha: &str,
    known: impl Iterator<Item = &'a String>,
    own: Option<&str>,
    line: Option<&str>,
) -> Result<Option<String>, String> {
    let known: Vec<&String> = known.collect();
    let mut input = format!("{local_sha}\n");
    for sha in &known {
        let _ = writeln!(input, "^{sha}");
    }
    let listed = git_input(root, &["rev-list", "--boundary", "--stdin"], &input)?
        .ok_or("cannot read range boundaries")?;
    if listed.is_empty() {
        return Ok(Some(local_sha.to_string()));
    }
    let bounds: Vec<&str> = listed
        .split_terminator('\n')
        .filter_map(|l| l.strip_prefix('-'))
        .collect();
    if let (Some(line), [_, _, ..]) = (line, bounds.as_slice()) {
        let mut on_line = Vec::new();
        for &bound in &bounds {
            if git(root, &["merge-base", "--is-ancestor", bound, line])?.is_some() {
                on_line.push(bound);
            }
        }
        if !on_line.is_empty() {
            let mut args = vec!["merge-base", "--independent"];
            args.extend(&on_line);
            if let Some(newest) = git(root, &args)? {
                if let [only] = newest
                    .strip_suffix('\n')
                    .unwrap_or(&newest)
                    .split('\n')
                    .collect::<Vec<_>>()
                    .as_slice()
                {
                    return Ok(Some((*only).to_string()));
                }
            }
        }
    }
    let others: Vec<&String> = known
        .into_iter()
        .filter(|sha| Some(sha.as_str()) != own)
        .collect();
    Ok(narrowest(root, local_sha, &bounds, own, &others)?.map(str::to_string))
}

/// The boundary to take as the range base. One base cannot leave out every
/// known tip, so take the one whose range holds the fewest foreign commits,
/// then the fewest commits. A foreign commit is one another destination ref
/// holds and the branch's own history (`own`) does not: the branch's own
/// earlier commits may be checked again, another line's history never is
/// when a boundary avoids it. A branch that merged its moved line twice
/// borders both line tips, and the older one would bring in the newer
/// one's commits (TSK-165).
fn narrowest<'b>(
    root: &Path,
    local_sha: &str,
    boundaries: &[&'b str],
    own: Option<&str>,
    others: &[&String],
) -> Result<Option<&'b str>, String> {
    if boundaries.len() < 2 {
        return Ok(boundaries.first().copied());
    }
    let count = |input: &str| -> Result<usize, String> {
        let text = git_input(root, &["rev-list", "--count", "--stdin"], input)?
            .ok_or("cannot read range count")?;
        text.strip_suffix('\n')
            .unwrap_or(&text)
            .parse()
            .map_err(|error| format!("cannot parse range count: {error}"))
    };
    let mut scored = Vec::new();
    for &base in boundaries {
        let range = format!("{local_sha}\n^{base}\n");
        let all = count(&range)?;
        let mut not_own = range;
        if let Some(own) = own {
            let _ = writeln!(not_own, "^{own}");
        }
        let mut fresh = not_own.clone();
        for sha in others {
            let _ = writeln!(fresh, "^{sha}");
        }
        scored.push((base, (count(&not_own)?.saturating_sub(count(&fresh)?), all)));
    }
    Ok(scored
        .into_iter()
        .min_by_key(|(_, score)| *score)
        .map(|(base, _)| base))
}

/// The advertised tip of the integration line the pushed branch's task
/// declares, when the branch carries a task and the destination has that
/// line.
fn own_line_tip(
    root: &Path,
    r: &PushRef,
    destination: &Destination<'_>,
) -> Result<Option<String>, String> {
    use codeflow_core::workgraph::{declared_work_target, task_id_from_branch};
    let Some(branch) = r.remote_branch() else {
        return Ok(None);
    };
    let Some(id) = task_id_from_branch(root, branch).map_err(|error| error.to_string())? else {
        return Ok(None);
    };
    let Some(target) = declared_work_target(root, &id).map_err(|error| error.to_string())? else {
        return Ok(None);
    };
    let asked = destination
        .answer(root)
        .map_err(str::to_string)?
        .as_ref()
        .map_err(Clone::clone)?;
    Ok(asked
        .heads
        .iter()
        .find(|(name, _)| *name == target)
        .map(|(_, tip)| tip.to_string()))
}

/// The base from `rev-list --boundary` output: its first boundary commit, or
/// the pushed sha itself when no commit is new.
fn boundary(listed: &str, local_sha: &str, note: Option<Finding>) -> Option<RangeBase> {
    if listed.is_empty() {
        return Some(RangeBase {
            base: local_sha.to_string(),
            note,
        });
    }
    listed
        .split_terminator('\n')
        .find_map(|line| line.strip_prefix('-'))
        .map(|base| RangeBase {
            base: base.to_string(),
            note,
        })
}

/// Why the working checkout is not the whole committed tree, if it is not:
/// a sparse checkout, or a submodule that is uninitialized, at another
/// commit or conflicted.
fn incomplete_checkout(root: &Path) -> Result<Option<String>, String> {
    let sparse = git(root, &["config", "--bool", "core.sparseCheckout"])?;
    if sparse.as_deref() == Some("true\n") {
        return Ok(Some("the checkout is sparse".to_string()));
    }
    match std::fs::metadata(root.join(".gitmodules")) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot read submodule state: {error}")),
    }
    let status = git(root, &["submodule", "status", "--recursive"])?
        .ok_or("cannot read submodule status")?;
    Ok(status.split_terminator('\n').find_map(|line| {
        let path = line.get(1..)?.split_whitespace().nth(1).unwrap_or("?");
        match line.chars().next()? {
            '-' => Some(format!("submodule {path} is not initialized")),
            '+' => Some(format!("submodule {path} is not at its recorded commit")),
            'U' => Some(format!("submodule {path} has merge conflicts")),
            _ => None,
        }
    }))
}

/// Git for the hook's own queries: never fetches a missing object, even in
/// a partial clone.
///
/// OS text rule (issue 79): the answer is read as text (shas, config values,
/// URLs, ref and submodule names the callers compare), so an answer that is
/// not valid UTF-8 returns an error through `run_checked` to `run`, which refuses.
/// Lookup/predicate exit 1, and `remote get-url` exit 2, mean an absent answer.
fn git(root: &Path, args: &[&str]) -> Result<Option<String>, String> {
    let out = codeflow_core::git::command()
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_NO_LAZY_FETCH", "1")
        .output()
        .map_err(|error| format!("cannot read Git answer: {error}"))?;
    git_answer(args, out)
}

/// [`git`] with `input` on stdin.
fn git_input(root: &Path, args: &[&str], input: &str) -> Result<Option<String>, String> {
    let out = codeflow_core::git::output_with_input(
        codeflow_core::git::command()
            .arg("-C")
            .arg(root)
            .args(args)
            .env("GIT_NO_LAZY_FETCH", "1"),
        input.as_bytes(),
    )
    .map_err(|error| format!("cannot read Git answer: {error}"))?;
    git_answer(args, out)
}

fn git_answer(args: &[&str], out: std::process::Output) -> Result<Option<String>, String> {
    if !out.status.success() {
        if out.status.code() == Some(2) && args.starts_with(&["remote", "get-url"]) {
            return Ok(None);
        }
        if out.status.code() == Some(1) && missing_git_answer(args) {
            return Ok(None);
        }
        return Err(format!(
            "cannot read Git {} answer: {}",
            args.first().copied().unwrap_or("command"),
            out.status
        ));
    }
    let text = String::from_utf8(out.stdout)
        .map_err(|error| format!("cannot decode Git answer: {error}"))?;
    if args.starts_with(&["rev-parse", "--verify", "--quiet"]) {
        let oid = text.strip_suffix('\n').unwrap_or(&text);
        if !matches!(oid.len(), 40 | 64) || !oid.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("cannot read Git revision id".into());
        }
    }
    Ok(Some(text))
}

/// Only these lookup and predicate forms define exit 1 as absence/false.
/// Other commands, including `merge-base --independent`, must succeed.
fn missing_git_answer(args: &[&str]) -> bool {
    match args {
        ["rev-parse", "--verify", "--quiet", _]
        | ["config", "--get" | "--bool", _]
        | ["config", "-z", "--get-all", _]
        | ["merge-base", "--is-ancestor", _, _] => true,
        ["merge-base", first, second] => !first.starts_with('-') && !second.starts_with('-'),
        _ => false,
    }
}

fn rev_parse(root: &Path, rev: &str) -> Result<Option<String>, String> {
    Ok(git(
        root,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{rev}^{{commit}}"),
        ],
    )?
    .map(|s| s.strip_suffix('\n').unwrap_or(&s).to_string())
    .filter(|s| !s.is_empty()))
}

fn is_commit(root: &Path, sha: &str) -> Result<bool, String> {
    Ok(rev_parse(root, sha)?.is_some())
}

/// `true` when no tracked file differs from the checked-out commit.
fn tracked_tree_clean(root: &Path) -> Result<bool, String> {
    Ok(
        git(root, &["status", "--porcelain", "--untracked-files=no"])?
            .ok_or("cannot read worktree status")?
            .is_empty(),
    )
}

fn short(sha: &str) -> &str {
    sha.get(..9).unwrap_or(sha)
}

#[cfg(test)]
mod name_text_tests {
    use super::*;

    /// Obtaining errors are explicit failures at the push refusal boundary.
    #[test]
    fn r16_a_git_answer_that_is_not_utf8_refuses() {
        use std::io::Write as _;
        let dir = tempfile::tempdir().unwrap();
        let init = codeflow_core::git::command()
            .args(["init", "--quiet"])
            .arg(dir.path())
            .status()
            .unwrap();
        assert!(init.success());
        let mut config = std::fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join(".git").join("config"))
            .unwrap();
        config
            .write_all(b"[demo]\n\tword = caf\xe9\n\tplain = ok\n")
            .unwrap();
        assert_eq!(
            git(dir.path(), &["config", "--get", "demo.plain"])
                .unwrap()
                .as_deref(),
            Some("ok\n")
        );
        assert!(git(dir.path(), &["config", "--get", "demo.word"]).is_err());
    }
}
