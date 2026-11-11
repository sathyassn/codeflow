//! The pre-push push set (TSK-132), bound to what is actually pushed.
//!
//! - `codeflow ci` runs once per pushed branch on git data: the range from
//!   the destination's own history to the pushed sha.
//! - The push set runs no tree checks (TSK-449): `codeflow validate --docs`
//!   is `codeflow ci`'s own `work.valid_graph` check, and the test-config
//!   targets belong to `codeflow test`, cited when the pull request is made
//!   ready. A push is a backup; it neither runs nor waits for a test target.
//!   The conflict-marker check runs inside each `codeflow ci`; staged
//!   secrets are refused by the pre-commit hook and by the hosted gitleaks
//!   scan, not here.
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
//! - Cheap checks first, and the first refusal stops the set (TSK-1306):
//!   the hook's own ref rules run before any `codeflow ci`, and a refusal
//!   skips the later branches' checks and the release preflight. Each
//!   `codeflow ci` runs every check, so the hook reports the findings CI
//!   reports for the same range (SPC-013 R-120). A push that passes runs
//!   every check, as before. Stopping never turns a refusal into a pass.
//! - The whole set is timed; over `git.push_budget_seconds` (default 60 s)
//!   the hook names the slowest step and, for a `codeflow ci` step, its
//!   slowest checks. The budget only reports: it never refuses a push.
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

use codeflow_core::git::{display_output, display_text, remote_query};
use codeflow_core::hooks::git_hook::{self, PushRef, PushStep, StageReport};
use codeflow_core::hooks::policy::{GitPolicy, INTEGRATION_BRANCH_PREFIX};
use codeflow_core::hooks::Violation;
use codeflow_core::remedy::{self, Finding};
use codeflow_core::scaffold::too_old;
use codeflow_core::workgraph::{landing_candidate, release_line};

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
    run_checked_with_binary(
        root,
        policy,
        refs,
        remote,
        url,
        report,
        std::env::current_exe,
    )
}

fn run_checked_with_binary(
    root: &Path,
    policy: &GitPolicy,
    refs: &[PushRef],
    remote: Option<&str>,
    url: Option<&str>,
    report: &mut StageReport,
    current_exe: impl FnOnce() -> std::io::Result<std::path::PathBuf>,
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
    // Working, committed and destination policy bound the tree checks; each branch's
    // `codeflow ci` is gated by the policy at the destination's default
    // branch (see [`judged_by`]), so a head cannot turn that check off.
    if pushed.is_empty() {
        return Ok(());
    }
    let started = Instant::now();
    let exe = match current_exe() {
        Ok(exe) => exe,
        Err(error) => {
            report.violations.push(Violation::always_blocking(
                "git.test_gate_on_push",
                format!("push set could not locate the codeflow binary: {error}"),
                &codeflow_core::remedy::PUSH_SET_BY_HAND.remedy(),
            ));
            return Ok(());
        }
    };
    let url = url.or(remote);
    // Tracking refs describe the fetch location; they stand in for the push
    // location only when the two are the same.
    let tracking = match (remote, url) {
        (Some(name), Some(url)) if fetches_from(root, name, url)? => Some(name),
        _ => None,
    };
    let namespace = match tracking {
        Some(name) => tracking_namespace(root, name)?,
        None => None,
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
        limits: probe_limits(root, report),
        listing: &listing,
        advertised: &advertised,
        answer: &answer,
        anchor: &anchor,
        fetch_failed: &fetch_failed,
        fork: fork(root, url)?,
        namespace: namespace.as_deref(),
        tracking,
        policy,
    };
    let mut steps: Vec<PushStep> = Vec::new();

    run_ci_ranges(&exe, root, &pushed, &destination, report, &mut steps)?;

    let active = policy.test_gate_on_push.is_active();
    if active && release_adopted(root)? {
        // Cheap checks first (TSK-1306): once a check above refuses the
        // push, the preflight could add findings but never lift that one.
        if codeflow_core::hooks::any_blocking(&report.violations) {
            report.status.push(
                "the release preflight did not run: a check above already refuses the push"
                    .to_string(),
            );
        } else {
            let remote = remote.unwrap_or("origin");
            for r in &pushed {
                release_preflight(&exe, root, r, remote, policy, report, &mut steps);
            }
        }
    }

    let total = started.elapsed();
    let budget = push_budget(root);
    if let Some(note) = git_hook::over_budget_note(&steps, total, budget) {
        report.notes.push(note);
    }
    report
        .status
        .push(format!("push set finished in {:.1}s", total.as_secs_f64()));
    Ok(())
}

/// The push set's time budget, `git.push_budget_seconds` (TSK-1306), read
/// from the local policy authority as the lookup limits are: the remote
/// default branch's committed policy as last fetched, never the working
/// copy or the pushed head. With no such branch to read, the shipped
/// default applies. The budget only decides whether the hook reports an
/// overrun; it never refuses a push, so no reading of it can weaken a check.
fn push_budget(root: &Path) -> std::time::Duration {
    let seconds = codeflow_core::hooks::landed_policy::load_local(root)
        .ok()
        .filter(|landed| landed.authority.is_some())
        .map_or(
            codeflow_core::hooks::limits::PUSH_BUDGET.default,
            |landed| landed.policy.git.push_budget_seconds,
        );
    std::time::Duration::from_secs(seconds)
}

/// The destination lookup's timeout and attempts (TSK-1301): read from the
/// local policy authority, the remote default branch's committed policy as
/// last fetched, never from the working copy or the pushed head, which the
/// push itself can change. With no such branch to read, the shipped limits
/// apply, noted; the lookup still refuses a destination that never answers.
fn probe_limits(root: &Path, report: &mut StageReport) -> remote_query::QueryLimits {
    remote_query::QueryLimits::from_authority(root).unwrap_or_else(|why| {
        let limits = remote_query::QueryLimits::default();
        report.status.push(format!(
            "note: the destination lookup uses the shipped limits ({}s per attempt, {} attempts), since no default branch supplies them: {why}",
            limits.timeout().as_secs(),
            limits.attempts()
        ));
        limits
    })
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
    let rest = args.split_first().map_or(args, |(_, rest)| rest);
    codeflow_core::remedy::PUSH_SET_CI.with(&[("args", &rest.join(" "))])
}

/// The refusal of a push whose destination holds a policy this binary cannot
/// use. The tip is the authority, so its pin says whether the binary is too
/// old (TSK-1285).
fn invalid_target_refusal(
    root: &Path,
    target: &str,
    tip: &str,
    why: &str,
    shown_branch: &str,
) -> Violation {
    let age = too_old::BinaryAge::at_commit(root, tip, env!("CARGO_PKG_VERSION"));
    if let Some(refusal) = age.too_old("the policy on the destination's default branch") {
        return Violation::always_blocking(too_old::RULE, refusal.message, &refusal.remedy);
    }
    let mut message = format!(
        "the policy at {} {}, the destination's default branch, {why}, so '{shown_branch}' is refused",
        display_text(target),
        short(tip)
    );
    if let Some(line) = age.may_be_older() {
        let _ = write!(message, "; {line}");
    }
    Violation::always_blocking(
        "git.policy_authority",
        message,
        "if a newer codeflow wrote that policy, upgrade this codeflow and push again; if the policy itself is malformed, repair it on the destination's default branch through a reviewed pull request",
    )
}

/// What judges a push under [`Judged::Target`]: the policy at the
/// destination default branch's tip, or the shipped defaults when that
/// tip holds no policy file. The destination names its default branch, so
/// the name is shown escaped.
fn judging_source(target: &str, tip: &str, defaults: bool) -> String {
    let target = display_text(target);
    if defaults {
        format!(
            "the shipped defaults, since {target} {} holds no .codeflow/policy.json",
            short(tip)
        )
    } else {
        format!("the policy at {target} {}", short(tip))
    }
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
/// Whether a check above already refuses the push, noted for `branch`: the
/// first refusal stops the set (TSK-1306), since the push is refused
/// whatever the later branches' checks find.
fn refused_already(report: &mut StageReport, branch: &str) -> bool {
    let refused = codeflow_core::hooks::any_blocking(&report.violations);
    if refused {
        report.status.push(format!(
            "`codeflow ci` did not run for '{branch}': a check above already refuses the push"
        ));
    }
    refused
}

/// What a pushed branch is judged as, by the landing candidate binding
/// (TSK-1351).
enum Landing {
    /// Neither a landing candidate nor a line push that lands one: judged
    /// exactly as before.
    Ordinary,
    /// A landing candidate bound to its line: judged as the line's next tip.
    Candidate(landing_candidate::BoundLine),
    /// A line push that lands a bound candidate by a fast-forward: judged
    /// as any line push is.
    Lands,
}

/// Bind a pushed landing candidate, or a line push whose new tip is a
/// landing candidate's tip, under the policy at the default branch's tip
/// (TSK-1351 AC-1 to AC-3, AC-5). Any other push is [`Landing::Ordinary`].
/// `None` when it is refused: a condition that does not hold, or cannot be
/// read, is reported with its cause, so a candidate is never judged
/// against the default branch. `pushed` is the whole push set: a line
/// update is checked against every candidate destination in it before it
/// is judged as an ordinary line push.
fn landing(
    root: &Path,
    (r, pushed): (&PushRef, &[&PushRef]),
    branch: &str,
    judged: &Judged,
    destination: &Destination<'_>,
    report: &mut StageReport,
) -> Result<Option<Landing>, String> {
    let Judged::Target { tip, policy, .. } = judged else {
        return Ok(Some(Landing::Ordinary));
    };
    if policy.is_candidate_branch(branch) {
        let bound = pushed_candidate(root, r, branch, (tip, policy), destination, report);
        return Ok(bound.map(Landing::Candidate));
    }
    if codeflow_core::workgraph::line_adoption::epic_id(branch).is_err() {
        return Ok(Some(Landing::Ordinary));
    }
    line_landing(root, (r, pushed), branch, (tip, policy), destination, report)
}

/// The branches the destination advertises in this push, or why they
/// cannot be read.
fn advertised_branches<'a>(
    root: &Path,
    destination: &'a Destination<'_>,
) -> Result<&'a BTreeMap<String, String>, String> {
    match destination.advertised(root) {
        Advertised::Tips(tips) => Ok(&tips.branches),
        Advertised::Failed(why) => {
            Err(format!("the destination's branches cannot be read ({why})"))
        }
    }
}

/// Report a [`landing_candidate::RULE`] refusal: `what` was refused, and
/// `cause` says why.
fn refuse_landing(report: &mut StageReport, what: &str, cause: &str) {
    report.violations.push(Violation::always_blocking(
        landing_candidate::RULE,
        format!("{what}: {cause}"),
        landing_candidate::REMEDY,
    ));
}

/// Bind the pushed landing candidate `branch` to its line; `None` when the
/// binding is refused (reported).
fn pushed_candidate(
    root: &Path,
    r: &PushRef,
    branch: &str,
    (tip, policy): (&str, &GitPolicy),
    destination: &Destination<'_>,
    report: &mut StageReport,
) -> Option<landing_candidate::BoundLine> {
    let shown = display_text(branch);
    let what = format!("'{shown}' is not judged as a landing candidate of a line");
    let bound = advertised_branches(root, destination).and_then(|advertised| {
        landing_candidate::bind(
            root,
            &landing_candidate::Inputs {
                branch,
                head: &r.local_sha,
                policy,
                default_tip: tip,
                advertised,
                line: None,
            },
        )
    });
    match bound {
        Ok(bound) => {
            report.status.push(format!(
                "'{shown}' is a landing candidate of '{}': {} member merge(s) on its tip {}, so `codeflow ci` judges it as the line's next tip",
                display_text(&bound.line),
                bound.merges,
                short(&bound.tip)
            ));
            Some(bound)
        }
        Err(cause) => {
            refuse_landing(report, &what, &cause);
            None
        }
    }
}

/// Judge a push of the line `branch`: one whose new tip is a landing
/// candidate's tip must bind that candidate to the line and fast-forward
/// the line to it; every other line push is [`Landing::Ordinary`], judged
/// exactly as before. `None` when it is refused (reported).
fn line_landing(
    root: &Path,
    (r, pushed): (&PushRef, &[&PushRef]),
    branch: &str,
    (tip, policy): (&str, &GitPolicy),
    destination: &Destination<'_>,
    report: &mut StageReport,
) -> Result<Option<Landing>, String> {
    let advertised = advertised_branches(root, destination);
    let at = (r, pushed);
    let Some(name) = candidate_at(root, at, policy, advertised.as_ref().ok().copied())? else {
        return Ok(Some(Landing::Ordinary));
    };
    let what = format!(
        "the push of '{}' lands the candidate '{}', which is not a bound landing candidate of the line",
        display_text(branch),
        display_text(&name)
    );
    let bound = advertised.and_then(|advertised| {
        landing_candidate::bind(
            root,
            &landing_candidate::Inputs {
                branch: &name,
                head: &r.local_sha,
                policy,
                default_tip: tip,
                advertised,
                line: Some(branch),
            },
        )
    });
    let bound = match bound {
        Ok(bound) if r.remote_sha.eq_ignore_ascii_case(&bound.tip) => bound,
        Ok(bound) => {
            let old = if r.remote_sha.is_empty() || r.remote_sha.chars().all(|c| c == '0') {
                "the destination has no such line yet".to_string()
            } else {
                format!("the destination's line is at {}", short(&r.remote_sha))
            };
            let cause = format!(
                "the push is not a fast-forward of the line to the candidate: the candidate is cut from {}, and {old}",
                short(&bound.tip)
            );
            refuse_landing(report, &what, &cause);
            return Ok(None);
        }
        Err(cause) => {
            refuse_landing(report, &what, &cause);
            return Ok(None);
        }
    };
    report.status.push(format!(
        "the push of '{}' lands the landing candidate '{}': a fast-forward from {} by {} member merge(s)",
        display_text(branch),
        display_text(&name),
        short(&bound.tip),
        bound.merges
    ));
    Ok(Some(Landing::Lands))
}

/// The landing candidate whose tip a line push's new tip is: the local ref
/// pushed, a candidate destination another update of the same push sets to
/// that commit (a new candidate ref the destination does not hold yet and
/// that need not exist here), a local branch at that commit, or a branch
/// the destination advertises there, named with `git.candidate_prefix`.
/// `None` when no candidate is at it.
fn candidate_at(
    root: &Path,
    (r, pushed): (&PushRef, &[&PushRef]),
    policy: &GitPolicy,
    advertised: Option<&BTreeMap<String, String>>,
) -> Result<Option<String>, String> {
    if let Some(name) = r.local_ref.strip_prefix("refs/heads/") {
        if policy.is_candidate_branch(name) {
            return Ok(Some(name.to_string()));
        }
    }
    if let Some(name) = pushed
        .iter()
        .filter(|other| other.local_sha.eq_ignore_ascii_case(&r.local_sha))
        .filter_map(|other| other.remote_branch())
        .find(|name| policy.is_candidate_branch(name))
    {
        return Ok(Some(name.to_string()));
    }
    let local = git(
        root,
        &[
            "for-each-ref",
            "--format=%(refname:lstrip=2)",
            "--points-at",
            &r.local_sha,
            "refs/heads/",
        ],
    )?
    .unwrap_or_default();
    // One ref name per line: only `\n` separates them, as a name may hold
    // any other character.
    if let Some(name) = local
        .split_terminator('\n')
        .find(|name| policy.is_candidate_branch(name))
    {
        return Ok(Some(name.to_string()));
    }
    Ok(advertised.and_then(|branches| {
        branches
            .iter()
            .find(|(name, sha)| {
                policy.is_candidate_branch(name) && sha.eq_ignore_ascii_case(&r.local_sha)
            })
            .map(|(name, _)| name.clone())
    }))
}

#[allow(clippy::too_many_lines)] // one linear hand-off per pushed ref; each step is its own helper
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
    if !destination_available(root, destination, report) {
        return Ok(());
    }
    for r in pushed {
        let branch = r.remote_branch().unwrap_or_default();
        let shown_branch = display_text(branch);
        if refused_already(report, &shown_branch) {
            continue;
        }
        let judged = match judged_by(root, destination, steps) {
            Ok(judged) => judged,
            Err(error) => {
                refuse_authority(root, r, branch, destination, report, &error);
                continue;
            }
        };
        let Some(landing) = landing(root, (r, pushed), branch, &judged, destination, report)?
        else {
            continue;
        };
        let (policy, judging) = match &judged {
            Judged::Target {
                target,
                tip,
                policy,
                defaults,
            } => {
                let which = judging_source(target, tip, *defaults);
                if !policy.test_gate_on_push.is_active() {
                    report.status.push(format!(
                        "`codeflow ci` did not run for '{shown_branch}': {which}, the destination's default branch and the candidate authority, turns the push check off; the hosted job on the pull request's target is the enforcement"
                    ));
                    continue;
                }
                (
                    (**policy).clone(),
                    format!(
                        "`codeflow ci` judges '{shown_branch}' with {which}, the destination's default branch: a candidate authority, not a known pull request target; the hosted job on the real target is the enforcement"
                    ),
                )
            }
            Judged::Invalid { target, tip, why } => {
                report.violations.push(invalid_target_refusal(
                    root,
                    target,
                    tip,
                    why,
                    &shown_branch,
                ));
                continue;
            }
            Judged::Initialization => {
                // A release branch is still refused where no default target
                // exists (see [`release_base`]); nothing else is judged.
                if matches!(
                    release_base(root, r, branch, destination, report),
                    Scoped::Refused
                ) {
                    continue;
                }
                report.status.push(format!(
                    "initialization: the destination advertises no branches, so '{shown_branch}' creates its first; creating the default branch is not an authority claim, and no `codeflow ci` runs until it exists"
                ));
                continue;
            }
        };
        let policy = &policy;
        // The declared target and the task's own line are found under the
        // judging policy's work prefixes, as `codeflow ci` judges the range
        // with that policy (TSK-1257).
        let prefixes = &policy.branch_prefixes;
        let base = ci_base(root, r, (branch, &landing), destination, prefixes, report)?;
        if let Some(base) = base {
            report.status.push(judging);
            let running = policy.test_gate_on_push.to_string();
            let mut args = vec!["ci", "--base", &base, "--head", &r.local_sha];
            args.extend(["--branch", branch, "--run-level", &running]);
            // The hook judges a push to a line itself (`work.epic_line`). A
            // landing candidate's push is judged as its line's next tip, and
            // `codeflow ci` binds the candidate again from its own reads.
            args.extend(["--pushed-ref", &r.remote_ref]);
            // Range boundaries can be unnamed merge bases. Pass destination
            // intent separately so CI does not infer it from coincident refs.
            let judged_as = (&judged, &landing);
            let into = ci_target(root, r, branch, judged_as, (destination, prefixes), report)?;
            let into_arg = into.map(|target| format!("--into={target}"));
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
                    // --into alone denotes a landing check. Supply the
                    // hook target only with the push-context hand-off.
                    if base != r.local_sha {
                        if let Some(into) = &into_arg {
                            args.push(into);
                        }
                    }
                    input = Some(listed);
                }
            }
            run_check_with(exe, root, &args, input, policy, report, steps);
        }
    }
    Ok(())
}

/// The target the hook hands `codeflow ci`: the branch's declared target
/// when it may bound the push's range ([`declared_bounds`]), otherwise the
/// destination's default. The range and every check that reads the
/// target, the Outcome marker among them, use the same one, so a
/// declaration the range rejected never names the target the change is
/// judged against. A bound landing candidate's target is its line
/// (TSK-1351).
fn ci_target(
    root: &Path,
    pushed: &PushRef,
    branch: &str,
    (judged, landing): (&Judged, &Landing),
    (destination, prefixes): (&Destination<'_>, &[String]),
    report: &mut StageReport,
) -> Result<Option<String>, String> {
    use super::ci::Declared;
    if let Landing::Candidate(bound) = landing {
        return Ok(Some(bound.line.clone()));
    }
    let policy = destination.policy;
    let mut dropped = false;
    let declared = match super::ci::judged_declared_target(
        root,
        branch,
        &pushed.local_sha,
        &policy.protected_branches,
        prefixes,
    )? {
        Some(Declared::Eligible(target)) if declared_bounds(root, destination, &target) => {
            Some(target)
        }
        Some(Declared::Eligible(target)) => {
            dropped = true;
            report.status.push(format!(
                "declared target '{}' is a protected branch other than the destination's default; the check uses the destination's default target",
                display_text(&target)
            ));
            None
        }
        Some(Declared::Rejected { target, reason }) => {
            report.status.push(format!(
                "declared target '{target}' {reason}; the check uses the destination's default target"
            ));
            None
        }
        None => None,
    };
    let target = declared.or_else(|| match judged {
        // A protected declaration the range set aside used to name the
        // target itself; where the destination's default is no stable line,
        // the hook hands none, and `codeflow ci` reads that default from
        // the advertisement, as the range did.
        Judged::Target { target, .. }
            if dropped && !codeflow_core::workgraph::is_stable_work_target(target) =>
        {
            None
        }
        Judged::Target { target, .. } => Some(target.clone()),
        _ => None,
    });
    if let Some(target) = &target {
        if !codeflow_core::workgraph::is_stable_work_target(target) {
            return Err(format!(
                "target '{}' must name a stable non-task branch",
                display_text(target)
            ));
        }
        codeflow_core::hooks::git_guard::read_branch_name(root, None, target)?;
    }
    Ok(target)
}

/// Refuse the shared acquisition error once before any ref check starts.
fn destination_available(
    root: &Path,
    destination: &Destination<'_>,
    report: &mut StageReport,
) -> bool {
    let unavailable = match destination.answer(root) {
        Ok(Ok(_)) => None,
        Ok(Err(why)) => Some(why.as_str()),
        Err(why) => Some(why),
    };
    if let Some(why) = unavailable {
        report.violations.push(Violation::always_blocking(
            release_line::DESTINATION_RULE,
            format!("cannot prove push inputs: the destination cannot be read, so no pushed range is judged: {why}"),
            &remedy::PUSH_DESTINATION_SILENT.remedy(),
        ));
        return false;
    }
    true
}

fn refuse_authority(
    root: &Path,
    pushed: &PushRef,
    branch: &str,
    destination: &Destination<'_>,
    report: &mut StageReport,
    error: &str,
) {
    let shown_branch = display_text(branch);
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
            format!("cannot prove push inputs: cannot prove the authority for '{shown_branch}': {error}"),
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
    let shown_branch = display_text(branch);
    codeflow_core::workgraph::history::check_command(root)?;
    // The destination names its default branch: shown escaped.
    let target = display_text(target);
    let keep = tip.to_string();
    if base == tip {
        return Ok(keep);
    }
    let related = codeflow_core::git::judging_command()
        .args(["-c", "core.commitGraph=false"])
        .arg("-C")
        .arg(root)
        .args(["merge-base", tip, &r.local_sha])
        .env("GIT_NO_LAZY_FETCH", "1")
        .output();
    let unknown = |why: &str, report: &mut StageReport| {
        report.status.push(format!(
            "note: whether '{shown_branch}' shares history with {target} {} cannot be read ({why}), so push checks refuse",
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
        "note: '{shown_branch}' shares no history with {target} {}, so its commit checks run over {what}, from {}, under that policy; this is not default-target parity",
        short(tip),
        short(from)
    ));
    Ok(from.to_string())
}

/// The base of a pushed branch's `codeflow ci` range: a release branch's
/// default tip, else the boundary of what the destination holds, which a
/// declared target may select. `None` when the push is refused or the
/// range is unresolved, both reported. A bound landing candidate's base is
/// its line's tip, never the narrowest boundary (TSK-1351).
fn ci_base(
    root: &Path,
    r: &PushRef,
    (branch, landing): (&str, &Landing),
    destination: &Destination<'_>,
    prefixes: &[String],
    report: &mut StageReport,
) -> Result<Option<String>, String> {
    if let Landing::Candidate(bound) = landing {
        return Ok(Some(bound.tip.clone()));
    }
    Ok(match release_base(root, r, branch, destination, report) {
        Scoped::Refused => None,
        Scoped::Release(tip) => Some(tip),
        Scoped::Ordinary => {
            let mut notices = Vec::new();
            let range = range_base(root, r, destination, prefixes, &mut notices)?;
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
    let shown_branch = display_text(branch);
    let started = Instant::now();
    let result =
        codeflow_core::release_local::preflight(root, &pushed.local_sha, branch, remote, exe);
    steps.push(PushStep {
        name: format!("release preflight ({shown_branch})"),
        duration: started.elapsed(),
        parts: Vec::new(),
    });
    match result {
        Ok(outcome) => {
            for note in &outcome.notes {
                let line = format!("release preflight ({shown_branch}): {note}");
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
                    format!("release preflight ({shown_branch}): this push breaks the release tree"),
                    codeflow_core::remedy::RELEASE_PREFLIGHT.with(&[
                        ("script", codeflow_core::release_local::SCRIPT),
                        ("branch", branch),
                    ]),
                ));
            }
        }
        Err(error) => report.notes.push(Finding::new(
            format!(
                "release preflight could not complete for '{shown_branch}': {error}; the pull request job checks it"
            ),
            remedy::RELEASE_PREFLIGHT_UNRUN.with(&[("branch", branch)]),
        )),
    }
}

/// Run one built-in check through this binary, with `input` on its stdin,
/// and time it.
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
    // instead of reading the hook's hand-off. A destination URL is shown
    // without its credentials; the check itself receives the real one.
    let run_level = args.iter().position(|arg| *arg == "--run-level");
    let displayed: Vec<std::borrow::Cow<'_, str>> = args
        .iter()
        .enumerate()
        .filter(|(at, arg)| {
            **arg != "--advertisement-stdin"
                && run_level.is_none_or(|level| *at != level && *at != level + 1)
        })
        .map(|(_, arg)| match arg.strip_prefix("--into=") {
            // The destination's default branch, which it names itself.
            Some(into) => std::borrow::Cow::Owned(format!("--into={}", display_text(into))),
            None => remote_query::display_url(arg),
        })
        .collect();
    let rerun: Vec<&str> = displayed.iter().map(AsRef::as_ref).collect();
    let shown = format!("codeflow {}", rerun.join(" "));
    // `codeflow ci` names its blocking rules in a file of its own, for the
    // refusal record; its printed findings can quote operation content.
    let is_ci = args.first() == Some(&"ci");
    let rules_out = is_ci.then(RulesOut::create).flatten();
    // Its check times, for the over-budget note (TSK-1306).
    let timings_out = is_ci
        .then(|| RulesOut::create_named("codeflow-ci-timings"))
        .flatten();
    let mut command = Command::new(exe);
    command.args(args);
    if let Some(out) = &rules_out {
        command.arg("--blocking-rules-out").arg(&out.0);
    }
    if let Some(out) = &timings_out {
        command.arg("--timings-out").arg(&out.0);
    }
    let started = Instant::now();
    let output = codeflow_core::git::output_with_input(
        command.current_dir(root).env_remove("CODEFLOW_PR_BODY"),
        input.unwrap_or_default().as_bytes(),
    );
    steps.push(PushStep {
        name: shown.clone(),
        duration: started.elapsed(),
        parts: timings_out
            .as_ref()
            .map(|out| out.timings(3))
            .unwrap_or_default(),
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
                relayed_findings(&relayed_text(&printed))
                    .into_iter()
                    .map(|finding| format!("`{shown}`: {finding}")),
            );
        }
        Ok(out) => {
            // A failing check's whole output, every line indented, so no
            // line of it can pass as one of the hook's own (TSK-388).
            let stdout = remote_query::display_urls(&String::from_utf8_lossy(&out.stdout));
            eprint!("{}", codeflow_core::git::display_relay(&stdout));
            let stderr = remote_query::display_urls(&String::from_utf8_lossy(&out.stderr));
            eprint!("{}", codeflow_core::git::display_relay(&stderr));
            if let Some(rules) = &rules_out {
                report.refused_by.extend(rules.read());
            }
            // A check run at the push gate's level (R-80) that still fails
            // holds a finding that keeps its block: it stops the push.
            let kept_block = run_level.is_some() && matches!(out.status.code(), Some(1 | 2));
            let too_old = report.refused_by.iter().any(|rule| rule == too_old::RULE);
            let mut failed = violation(
                policy,
                format!("push set check failed: `{shown}` (output above)"),
                if too_old {
                    remedy::PUSH_SET_TOO_OLD.remedy()
                } else {
                    check_remedy(&rerun)
                },
            );
            if kept_block && policy.test_gate_on_push != codeflow_core::hooks::PolicyLevel::Block {
                failed.level = codeflow_core::hooks::PolicyLevel::Block;
                let reason = if out.status.code() == Some(2) {
                    "the CI context could not be verified"
                } else {
                    "a finding there keeps its block level"
                };
                let _ = write!(
                    failed.message,
                    "; {reason}, so the push stops although \
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

/// A check's printed output as the hook shows it: URL credentials hidden,
/// and each control, format and separator character other than a line
/// break or tab escaped ([`display_output`]). The check can quote a name
/// the destination supplied, such as its default branch, which the hook
/// hands it as `--into`; escaped, that name cannot write a terminal escape
/// sequence into the push's output or reorder its line (TSK-388).
fn relayed_text(printed: &str) -> String {
    display_output(&remote_query::display_urls(printed))
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

/// A private file `codeflow ci` writes to for the hook: its blocking rule
/// ids (`--blocking-rules-out`) or its check times (`--timings-out`),
/// removed when dropped.
struct RulesOut(std::path::PathBuf);

impl RulesOut {
    /// A new empty file in the temporary directory, created exclusively so
    /// no other file is written through it; `None` when it cannot be made.
    fn create() -> Option<Self> {
        Self::create_named("codeflow-blocking-rules")
    }

    fn create_named(prefix: &str) -> Option<Self> {
        let path = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::new_v4().simple()));
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

    /// The check times the check wrote, slowest first, at most `keep`.
    /// Only `<check>\t<microseconds>` lines whose check name is a plain
    /// word of letters, digits and hyphens are kept, and the run's total is
    /// left out; anything else in the file is ignored, never shown. The
    /// times only report, so a missing or unreadable file costs the
    /// breakdown and nothing else.
    fn timings(&self, keep: usize) -> Vec<(String, std::time::Duration)> {
        let text = std::fs::read_to_string(&self.0).unwrap_or_default();
        let mut parts: Vec<(String, std::time::Duration)> = text
            .split_terminator('\n')
            .filter_map(|line| {
                let (check, micros) = line.split_once('\t')?;
                let plain = !check.is_empty()
                    && check.len() <= 40
                    && check.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
                (plain && check != "total").then_some(())?;
                let micros: u64 = micros.parse().ok()?;
                Some((check.to_string(), std::time::Duration::from_micros(micros)))
            })
            .collect();
        parts.sort_by(|a, b| b.1.cmp(&a.1));
        parts.truncate(keep);
        parts
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

/// The first tracking ref this clone holds for `remote`, under
/// `refs/remotes/<remote>/` or any destination of its fetch refspecs,
/// exact or glob (a glob is read up to its last `/` before the `*`, a
/// superset). A destination that is not a full ref name cannot be checked,
/// so it refuses (TSK-1077 review round 01 F2).
fn held_tracking_ref(root: &Path, remote: &str) -> Result<Option<String>, String> {
    let key = format!("remote.{remote}.fetch");
    let mut patterns = vec![format!("refs/remotes/{remote}/")];
    if let Some(out) = git(root, &["config", "-z", "--get-all", &key])? {
        for spec in out.split('\0').filter(|spec| !spec.is_empty()) {
            if spec.starts_with('^') {
                continue;
            }
            let spec = spec.strip_prefix('+').unwrap_or(spec);
            let Some((_, dst)) = spec.split_once(':') else {
                continue;
            };
            if dst.is_empty() {
                continue;
            }
            let pattern = match dst.find('*') {
                Some(star) => dst[..star]
                    .rfind('/')
                    .map_or(&dst[..star], |slash| &dst[..=slash]),
                None => dst,
            };
            if !pattern.starts_with("refs/") {
                return Err(format!(
                    "{key} stores into {}, which is not a full ref name, so whether this clone still tracks the destination cannot be read; run git fetch --prune {}",
                    display_text(dst),
                    display_text(remote)
                ));
            }
            patterns.push(pattern.to_string());
        }
    }
    let mut args = vec!["for-each-ref", "--count=1", "--format=%(refname)"];
    args.extend(patterns.iter().map(String::as_str));
    Ok(git(root, &args)?.and_then(|out| {
        out.split('\n')
            .find(|name| !name.is_empty())
            .map(str::to_string)
    }))
}

/// What judges a pushed branch's `codeflow ci` (sathyassn/codeflow#22).
/// The authority is always a candidate: the destination's default branch,
/// which is the pull request's target only when the request goes there.
/// The hosted job, run on the real target, is the enforcement.
enum Judged {
    /// The validated policy at the destination default branch's tip, or
    /// the shipped defaults when that tip proves the policy file absent
    /// (`defaults`): it decides the rules, whether the check gates the push
    /// and at what level.
    Target {
        target: String,
        tip: String,
        policy: Box<GitPolicy>,
        defaults: bool,
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
    /// The destination advertises no branches and this clone holds no
    /// tracking refs for it: the push creates the first branch, which is
    /// not an authority claim, so no `codeflow ci` runs (TSK-1077).
    Initialization,
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

/// A note for a push that does not go to the configured `upstream`: its
/// pull request may target the upstream, whose policy can differ from the
/// candidate authority read here.
fn fork(root: &Path, url: Option<&str>) -> Result<Option<String>, String> {
    let Some(upstream) = git(root, &["config", "--get", "remote.upstream.url"])? else {
        return Ok(None);
    };
    let upstream = upstream.strip_suffix('\n').unwrap_or(&upstream);
    Ok((url != Some(upstream)).then(|| {
        format!(
            "the push goes to {}, not the configured upstream {}; a pull request may target the upstream",
            url.map_or(std::borrow::Cow::Borrowed("an unnamed destination"), remote_query::display_url),
            remote_query::display_url(upstream)
        )
    }))
}

/// What judges every pushed branch: the policy at the destination default
/// branch's advertised tip, fetched once per push when this clone lacks it
/// (the fetch is timed in the push set's steps, and a failure is not
/// retried). No other branch is an authority, protected or not: a branch
/// the destination advertises proves it exists, not that its policy
/// landed through review. An unanswered destination or a failed fetch
/// refuses; a default branch without a policy file is judged by the
/// shipped defaults. A destination that advertises no branches is
/// initialization only while this clone holds no tracking refs for it;
/// otherwise the tracking refs and the destination disagree and the push
/// is refused with the fetch route, never judged by the working copy.
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
        // Only a destination that answered with no branches is
        // initialization; one that was not asked, or could not be, refuses.
        destination.listing(root).map_err(str::to_string)?;
        let Some(remote) = destination.tracking else {
            return Ok(Judged::Initialization);
        };
        let Some(held) = held_tracking_ref(root, remote)? else {
            return Ok(Judged::Initialization);
        };
        return Err(format!(
            "the destination advertises no branches while this clone still holds its tracking ref {}, so no default branch can judge the push; run git fetch --prune {}",
            display_text(&held),
            display_text(remote)
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
            defaults: false,
        },
        TargetPolicy::Malformed(why) => Judged::Invalid {
            target,
            tip,
            why: why.clone(),
        },
        // A proven absence at the tip: the shipped defaults, never the
        // working copy (TSK-1077 AC-5).
        TargetPolicy::Missing => Judged::Target {
            target,
            tip,
            policy: Box::new(GitPolicy::default()),
            defaults: true,
        },
    })
}

/// What is known of the destination's history.
struct Destination<'a> {
    /// Where the push goes: the hook's URL argument, else its remote name.
    url: Option<&'a str>,
    /// How long the destination lookup may take, from the local policy
    /// authority (see [`probe_limits`]).
    limits: remote_query::QueryLimits,
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
    /// The configured remote that fetches from `url`, whose tracking refs
    /// this clone may hold; `None` for a URL or path pushed to directly.
    tracking: Option<&'a str>,
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
                    |url| release_line::advertisement(root, url, self.limits),
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
        Ok(self.answer.get_or_init(|| {
            release_line::from_advertisement(url, listed)
                .map(|destination| destination.with_limits(self.limits))
        }))
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
                    parts: Vec::new(),
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
    let shown_branch = display_text(branch);
    // Not judging a release push is never a warning (R-80): the refusal
    // keeps its block whatever `git.test_gate_on_push` is, as CI's own
    // refusal of an unreadable release scope does.
    let asked = match destination
        .answer(root)
        .and_then(|answer| answer.as_ref().map_err(String::as_str))
    {
        Ok(asked) => asked,
        Err(why) => {
            report.violations.push(Violation::always_blocking(
                release_line::DESTINATION_RULE,
                format!("cannot prove push inputs: the destination cannot be read: {why}"),
                &remedy::PUSH_DESTINATION_SILENT.remedy(),
            ));
            return Scoped::Refused;
        }
    };
    let mut refuse = |message: String, sanctioned: &str| {
        report.violations.push(Violation::always_blocking(
            codeflow_core::workgraph::acceptance::FROZEN_RULE,
            message,
            sanctioned,
        ));
        Scoped::Refused
    };
    // Both release-scope reads below need the default target's tip; when
    // the authority's fetch of it already failed, neither fetches again.
    if let Some(why) = destination.fetch_failed.get() {
        return refuse(
            format!("whether the release rules apply to '{shown_branch}' cannot be read, so it is not pushed unjudged (SPC-013 R-120): {why}"),
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
                    format!("whether the release rules apply to '{shown_branch}' cannot be read, so it is not pushed unjudged (SPC-013 R-120): {why}"),
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
                    "'{shown_branch}' is a release branch: `codeflow ci` judges everything it adds to {} at {}, as its pull request is",
                    display_text(name),
                    short(&tip)
                ));
                Scoped::Release(tip)
            }
            None => refuse(
                format!("'{shown_branch}' is a release branch, and the destination has no default target yet to judge it against (SPC-013 R-120)"),
                "push the default branch first, then the release branch",
            ),
        },
        Err(why) => refuse(
            format!("whether '{shown_branch}' is a release branch cannot be decided, so it is not pushed unjudged (SPC-013 R-120): {why}"),
            "fix what the message names at the destination, then push again",
        ),
    }
}

/// Why `codeflow ci` did not run for a pushed branch whose range has no base.
fn unresolved(branch: &str, destination: &Destination<'_>) -> Finding {
    let shown_branch = display_text(branch);
    let why = destination
        .failure()
        .map(|why| format!("; asking it: {why}"))
        .unwrap_or_default();
    Finding::new(
        format!(
            "`codeflow ci` did not run for '{shown_branch}': range unresolved (a new \
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
/// 2. A branch with an eligible declared target at its pushed commit uses
///    that target's advertised tip when available locally. Protected names
///    must share the destination default's tip. Rejected declarations and
///    differing protected tips use the destination default instead.
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
    prefixes: &[String],
    notices: &mut Vec<String>,
) -> Result<Option<RangeBase>, String> {
    if let Advertised::Tips(tips) = destination.advertised(root) {
        if let Some(base) = target_base(root, r, destination, prefixes, tips, notices)? {
            return Ok(Some(base));
        }
    }
    if existing_tip(root, r)?.is_some() {
        return Ok(Some(existing_base(root, r, destination, prefixes)?));
    }
    let note = match destination.advertised(root) {
        Advertised::Tips(tips) if !tips.commits.is_empty() => {
            let line = own_line_tip(root, r, destination, prefixes)?;
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
        } else if let Some(tip) = rev_parse(root, &reference)? {
            // The verified commit, never the name: a missing tracking ref
            // would let `refs/heads/<reference>` answer for it.
            known.push(tip);
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
/// Line rewrites and unavailable eligible targets retain the historical fallback.
/// Whether an eligible declared target may bound a push's range and name
/// the target its checks read: an unprotected line may; a protected branch
/// only when the destination advertises it as its default branch's tip.
/// Both the range ([`target_base`]) and the target handed to `codeflow ci`
/// ([`ci_target`]) ask this, so they never disagree.
fn declared_bounds(root: &Path, destination: &Destination<'_>, target: &str) -> bool {
    if !destination.policy.branch_is_protected(target) {
        return true;
    }
    let Advertised::Tips(tips) = destination.advertised(root) else {
        return false;
    };
    matches!(destination.answer(root), Ok(Ok(answer)) if answer
        .default
        .as_ref()
        .is_some_and(|(_, tip)| tips.branches.get(target) == Some(&tip.to_string())))
}

fn target_base(
    root: &Path,
    r: &PushRef,
    destination: &Destination<'_>,
    prefixes: &[String],
    tips: &AdvertisedTips,
    notices: &mut Vec<String>,
) -> Result<Option<RangeBase>, String> {
    let Some(branch) = r.remote_branch() else {
        return Ok(None);
    };
    let policy = destination.policy;
    let shown_branch = display_text(branch);
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
        let declared = super::ci::judged_declared_target(
            root,
            branch,
            &r.local_sha,
            &policy.protected_branches,
            prefixes,
        )?;
        let Some(declared) = declared else {
            return Ok(None);
        };
        let default = match destination.answer(root) {
            Ok(Ok(answer)) => answer.default.as_ref(),
            _ => None,
        };
        let target = match declared {
            super::ci::Declared::Eligible(target)
                if declared_bounds(root, destination, &target) =>
            {
                target
            }
            super::ci::Declared::Eligible(target)
            | super::ci::Declared::Rejected { target, .. } => {
                let Some((name, _)) = default else {
                    return Err(format!(
                        "declared target '{target}' cannot bound the range and the destination has no default target"
                    ));
                };
                notices.push(format!(
                    "declared target '{target}' cannot bound the range; using the destination's default target '{}'",
                    display_text(name)
                ));
                name.clone()
            }
        };
        let target = target.as_str();
        let Some(tip) = tips.branches.get(target) else {
            return Ok(None);
        };
        if tips.commits.binary_search(tip).is_err() {
            notices.push(format!(
                "declared target '{}' advertised at {tip} is not fetched here; \
                     falling back to advertised-history range selection for '{shown_branch}'",
                display_text(target)
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
        "range of '{shown_branch}' uses advertised target '{}': `codeflow ci --base {base} --head {}`",
        display_text(&target),
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
                    "'{shown_branch}' rewrites the destination's {}: `codeflow ci` checks {} commit(s), \
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
    prefixes: &[String],
) -> Result<RangeBase, String> {
    let branch = r.remote_branch().unwrap_or_default();
    let shown_branch = display_text(branch);
    let old = &r.remote_sha;
    let line = own_line_tip(root, r, destination, prefixes)?;
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
    let rewrite = format!("'{shown_branch}' rewrites the destination's {}", short(old));
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
                 '{shown_branch}' is bounded by its advertised {} alone",
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
/// line. The task and its target are read from the records at the pushed
/// commit under `prefixes`, the work prefixes of the policy that judges the
/// push (the destination default branch's), never from the pushing
/// checkout's records or policy (TSK-1257), so a push from a root checkout
/// that lacks the line's records bounds its range as a push from the task's
/// worktree does.
fn own_line_tip(
    root: &Path,
    r: &PushRef,
    destination: &Destination<'_>,
    prefixes: &[String],
) -> Result<Option<String>, String> {
    use codeflow_core::workgraph::{judged_task_id, task_target_at_revision};
    let Some(branch) = r.remote_branch() else {
        return Ok(None);
    };
    let Some(id) =
        judged_task_id(root, prefixes, branch, &r.local_sha).map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let Some(target) =
        task_target_at_revision(root, &id, &r.local_sha).map_err(|error| error.to_string())?
    else {
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

/// Git for the hook's own queries: never fetches a missing object, even in
/// a partial clone.
///
/// OS text rule (issue 79): the answer is read as text (shas, config values,
/// URLs, ref and submodule names the callers compare), so an answer that is
/// not valid UTF-8 returns an error through `run_checked` to `run`, which refuses.
/// Lookup/predicate exit 1, and `remote get-url` exit 2, mean an absent answer.
fn git(root: &Path, args: &[&str]) -> Result<Option<String>, String> {
    if matches!(args.first(), Some(&"merge-base" | &"rev-list" | &"cherry")) {
        codeflow_core::workgraph::history::check_command(root)?;
    }
    let out = codeflow_core::git::judging_command()
        .args(["-c", "core.commitGraph=false"])
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
    if matches!(args.first(), Some(&"merge-base" | &"rev-list" | &"cherry")) {
        codeflow_core::workgraph::history::check_command(root)?;
    }
    let out = codeflow_core::git::output_with_input(
        codeflow_core::git::judging_command()
            .args(["-c", "core.commitGraph=false"])
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

/// The commit `rev` names. A full ref must exist exactly: when it does not,
/// Git would match `refs/heads/<rev>` or `refs/tags/<rev>` instead.
fn rev_parse(root: &Path, rev: &str) -> Result<Option<String>, String> {
    if super::ci::is_full_ref(rev) && !super::ci::exact_ref_exists(root, rev)? {
        return Ok(None);
    }
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
        let init = codeflow_core::git::judging_command()
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
