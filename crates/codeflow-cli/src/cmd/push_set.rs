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
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};
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
    // `codeflow ci` is gated by the policy at its target (see
    // [`judged_by`]), so a head cannot turn that check off.
    if pushed.is_empty() {
        return;
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
            return;
        }
    };
    let url = url.or(remote);
    // Tracking refs describe the fetch location; they stand in for the push
    // location only when the two are the same.
    let namespace = remote
        .filter(|name| url.is_some_and(|url| fetches_from(root, name, url)))
        .and_then(|name| tracking_namespace(root, name));
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
        fork: fork(root, url),
        namespace: namespace.as_deref(),
        policy,
    };
    let mut steps: Vec<PushStep> = Vec::new();

    run_ci_ranges(&exe, root, &pushed, &destination, report, &mut steps);

    if !policy.test_gate_on_push.is_active() {
        return;
    }
    if codeflow_core::release_local::adopted(root) {
        let remote = remote.unwrap_or("origin");
        for r in &pushed {
            release_preflight(&exe, root, r, remote, policy, report, &mut steps);
        }
    }

    let head = rev_parse(root, "HEAD");
    let incomplete = incomplete_checkout(root);
    let clean = incomplete.is_none() && tracked_tree_clean(root);
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
/// [`release_base`]). The range's boundary only bounds the commits: the
/// policy that judges them, and whether and at what level the check gates
/// the push, come from the branch's target (see [`judged_by`]).
fn run_ci_ranges(
    exe: &Path,
    root: &Path,
    pushed: &[&PushRef],
    destination: &Destination<'_>,
    report: &mut StageReport,
    steps: &mut Vec<PushStep>,
) {
    for r in pushed {
        let branch = r.remote_branch().unwrap_or_default();
        let judged = judged_by(root, r, branch, destination, steps);
        let policy = match &judged {
            Judged::Target {
                target,
                tip,
                policy,
            } => {
                if !policy.test_gate_on_push.is_active() {
                    report.status.push(format!(
                        "`codeflow ci` did not run for '{branch}': the policy at {target} {} turns the push check off",
                        short(tip)
                    ));
                    continue;
                }
                report.status.push(format!(
                    "`codeflow ci` judges '{branch}' with the policy at {target} {}, the branch its pull request is judged against",
                    short(tip)
                ));
                (**policy).clone()
            }
            Judged::Invalid { target, tip } => {
                report.status.push(format!(
                    "the policy at {target} {} is not valid for this codeflow; `codeflow ci` refuses '{branch}' as the hosted job would",
                    short(tip)
                ));
                blocking(destination.policy)
            }
            Judged::Unverified(why) => {
                report.status.push(format!(
                    "not hosted parity for '{branch}': {why}; `codeflow ci` still judges it, with the policy at its range's base and at block level, whatever the working copy's gate says"
                ));
                blocking(destination.policy)
            }
        };
        let policy = &policy;
        let base = match release_base(root, r, branch, destination, report) {
            Scoped::Refused => None,
            Scoped::Release(tip) => Some(tip),
            Scoped::Ordinary => {
                let mut notices = Vec::new();
                let range = range_base(root, r, destination, &mut notices);
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
        };
        if let Some(base) = base {
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
            if let Some(tip) = existing_tip(root, r) {
                args.extend(["--baseline-from", tip]);
            }
            if let Judged::Target { tip, .. } | Judged::Invalid { tip, .. } = &judged {
                args.extend(["--policy-from", tip]);
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
    let output = command
        .current_dir(root)
        .env_remove("CODEFLOW_PR_BODY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            let stdin = child.stdin.take();
            let input = input.unwrap_or_default().to_string();
            // Written from its own thread so a large output cannot
            // deadlock the pipes; closing stdin ends the input.
            let writer = std::thread::spawn(move || {
                stdin.map_or(Ok(()), |mut stdin| stdin.write_all(input.as_bytes()))
            });
            let out = child.wait_with_output();
            let _ = writer.join();
            out
        });
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
    for line in stderr.lines() {
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
            .lines()
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
fn tracking_namespace(root: &Path, remote: &str) -> Option<String> {
    let key = format!("remote.{remote}.fetch");
    let out = git(root, &["config", "--get-all", &key])?;
    out.lines().find_map(|spec| {
        let (_, dst) = spec.trim().trim_start_matches('+').split_once(':')?;
        let prefix = dst.strip_suffix('*')?;
        prefix.starts_with("refs/").then(|| prefix.to_string())
    })
}

/// What judges a pushed branch's `codeflow ci` (sathyassn/codeflow#22).
enum Judged {
    /// The validated policy at the tip of the branch its pull request is
    /// judged against: it decides the rules, whether the check gates the
    /// push and at what level.
    Target {
        target: String,
        tip: String,
        policy: Box<GitPolicy>,
    },
    /// The target's policy fails strict validation: `codeflow ci` refuses
    /// the range at block level, as the hosted job does.
    Invalid { target: String, tip: String },
    /// No target could be established: the check still runs, at block
    /// level, and says it is not the hosted verdict.
    Unverified(String),
}

/// The destination default branch's advertised tip and its policy: the
/// anchor every other authority decision is taken from, so nothing the
/// pushed branch or the working copy says chooses whose policy judges it.
struct Anchor {
    name: String,
    tip: String,
    policy: TargetPolicy,
}

/// A target tip's policy, read as git data and strictly validated.
#[derive(Clone)]
enum TargetPolicy {
    Valid(Box<GitPolicy>),
    /// The tip carries no policy yet (the change that adopts `CodeFlow`).
    Missing,
    Malformed,
}

fn target_policy(root: &Path, tip: &str) -> Result<TargetPolicy, String> {
    let Some(text) = codeflow_core::hooks::landed_policy::policy_text_at(root, tip)? else {
        return Ok(TargetPolicy::Missing);
    };
    if codeflow_core::hooks::policy_schema::validate_policy_str(&text).is_err() {
        return Ok(TargetPolicy::Malformed);
    }
    Ok(
        serde_json::from_str::<codeflow_core::hooks::policy::Policy>(&text)
            .map_or(TargetPolicy::Malformed, |policy| {
                TargetPolicy::Valid(Box::new(policy.git))
            }),
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

/// Why the push is not to the upstream repository, when it is not: a
/// configured `upstream` remote at another URL makes this a fork, whose
/// default branch need not be the pull request's target.
fn fork(root: &Path, url: Option<&str>) -> Option<String> {
    let upstream = git(root, &["remote", "get-url", "upstream"])?;
    let upstream = upstream.trim();
    (url != Some(upstream)).then(|| {
        format!(
            "the push goes to {}, not the configured upstream {upstream}, so its pull request's target is not known here",
            url.unwrap_or("an unnamed destination")
        )
    })
}

/// What judges a pushed branch. The anchor is the destination default
/// branch's advertised tip, fetched once per push when this clone lacks it
/// (the fetch is timed in the push set's steps, and a failure is not
/// retried). Another branch is trusted as the target only when the
/// anchor's policy protects it and the destination already has it: the
/// pushed branch itself, then the target its task record declares. A
/// branch the push creates, a declared target the anchor does not
/// protect, and every hint from the working copy are ignored. A fork, an
/// unanswered destination, a failed fetch or a default branch without a
/// policy leave the result unverified.
fn judged_by(
    root: &Path,
    r: &PushRef,
    branch: &str,
    destination: &Destination<'_>,
    steps: &mut Vec<PushStep>,
) -> Judged {
    if let Some(why) = &destination.fork {
        return Judged::Unverified(why.clone());
    }
    let anchor = match destination.anchor(root, steps) {
        Ok(anchor) => anchor,
        Err(why) => return Judged::Unverified(why.clone()),
    };
    let anchor_policy = match &anchor.policy {
        TargetPolicy::Valid(policy) => policy,
        TargetPolicy::Malformed => {
            return Judged::Invalid {
                target: anchor.name.clone(),
                tip: anchor.tip.clone(),
            }
        }
        TargetPolicy::Missing => {
            return Judged::Unverified(format!(
                "the destination's default branch {} has no policy yet",
                anchor.name
            ))
        }
    };
    let Ok(Ok(asked)) = destination.answer(root) else {
        return Judged::Unverified("the destination did not answer".to_string());
    };
    let trusted = |name: &str| {
        name != anchor.name
            && anchor_policy.branch_is_protected(name)
            && asked.heads.iter().any(|(head, _)| head == name)
    };
    let declared = codeflow_core::workgraph::work_start::declared_work_target_at_revision(
        root,
        branch,
        &r.local_sha,
    )
    .ok()
    .flatten()
    .map(|target| target.trim().to_string());
    let target = [Some(branch.to_string()), declared]
        .into_iter()
        .flatten()
        .find(|name| trusted(name));
    let Some(target) = target else {
        return Judged::Target {
            target: anchor.name.clone(),
            tip: anchor.tip.clone(),
            policy: anchor_policy.clone(),
        };
    };
    let tip = match release_line::advertised_tip_here(root, asked, &target) {
        Ok(tip) => tip.to_string(),
        Err(why) => return Judged::Unverified(why),
    };
    match target_policy(root, &tip) {
        Ok(TargetPolicy::Valid(policy)) => Judged::Target {
            target,
            tip,
            policy,
        },
        Ok(TargetPolicy::Malformed) => Judged::Invalid { target, tip },
        Ok(TargetPolicy::Missing) => Judged::Unverified(format!("{target} has no policy")),
        Err(why) => Judged::Unverified(why),
    }
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
                        policy: target_policy(root, &tip)?,
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
fn fetches_from(root: &Path, name: &str, url: &str) -> bool {
    git(root, &["remote", "get-url", name]).is_some_and(|fetch| fetch.trim() == url)
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
    let tracked = durable_work_tracking_enabled(root).unwrap_or(true)
        || durable_work_tracking_enabled_at(root, &pushed.local_sha).unwrap_or(true);
    if let (false, Some(why)) = (tracked, destination.fetch_failed.get()) {
        return refuse(
            format!("whether the release rules apply to '{branch}' cannot be read, so it is not pushed unjudged (SPC-013 R-120): {why}"),
            "fix what the message names, then push again",
        );
    }
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
        .lines()
        .filter_map(|line| line.split_whitespace().next())
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
    let Some(types) = git_input(
        root,
        &["cat-file", "--batch-check=%(objectname) %(objecttype)"],
        &input,
    ) else {
        return Advertised::Failed("its tips could not be looked up here".to_string());
    };
    let mut commits: Vec<String> = types
        .lines()
        .filter_map(|line| line.strip_suffix(" commit"))
        .map(ToString::to_string)
        .collect();
    commits.sort();
    commits.dedup();
    let branches = listed
        .lines()
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
/// 4. If the destination cannot be asked, an existing branch uses its old
///    sha alone, noted.
/// 5. A new branch uses the historical boundary against available advertised
///    tips, or its pushed sha when the destination already holds all its history.
/// 6. With no locally available advertised tips or a failed query, a new branch
///    uses protected tracking refs only from the same destination, noting failure.
/// 7. Otherwise return `None` and leave the range unresolved for CI. Never
///    substitute a policy default or a local branch for an undeclared target.
fn range_base(
    root: &Path,
    r: &PushRef,
    destination: &Destination<'_>,
    notices: &mut Vec<String>,
) -> Option<RangeBase> {
    if let Advertised::Tips(tips) = destination.advertised(root) {
        if let Some(base) = target_base(root, r, destination.policy, tips, notices) {
            return Some(base);
        }
    }
    if existing_tip(root, r).is_some() {
        return Some(existing_base(root, r, destination));
    }
    let note = match destination.advertised(root) {
        Advertised::Tips(tips) if !tips.commits.is_empty() => {
            let line = own_line_tip(root, r, destination);
            return bounded_by(
                root,
                &r.local_sha,
                tips.commits.iter(),
                None,
                line.as_deref(),
            )
            .map(|base| RangeBase { base, note: None });
        }
        Advertised::Tips(_) => None,
        Advertised::Failed(why) => Some(Finding::new(
            format!(
                "asking the destination for its branches failed ({why}): the range of new \
                 branch '{}' is bounded by its tracked protected branches instead",
                r.remote_branch().unwrap_or_default()
            ),
            remedy::PUSH_DESTINATION_SILENT.remedy(),
        )),
    };
    let ns = destination.namespace?;
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
            )
            .is_some_and(|out| !out.trim().is_empty());
            if any {
                known.push(format!("--glob={reference}"));
            }
        } else if is_commit(root, &reference) {
            known.push(reference);
        }
    }
    if known.is_empty() {
        return None;
    }
    let mut args = vec!["rev-list", "--boundary", r.local_sha.as_str(), "--not"];
    args.extend(known.iter().map(String::as_str));
    let listed = git(root, &args)?;
    boundary(&listed, &r.local_sha, note)
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
) -> Option<RangeBase> {
    let branch = r.remote_branch()?;
    let (base, target) =
        if policy.branch_is_protected(branch) || branch.starts_with(INTEGRATION_BRANCH_PREFIX) {
            let old = existing_tip(root, r)?;
            git(root, &["merge-base", "--is-ancestor", old, &r.local_sha])?;
            (old.to_string(), branch.to_string())
        } else {
            let target = codeflow_core::workgraph::work_start::declared_work_target_at_revision(
                root,
                branch,
                &r.local_sha,
            )
            .ok()??;
            if !codeflow_core::workgraph::is_stable_work_target(&target) {
                return None;
            }
            let target = target.trim();
            let tip = tips.branches.get(target)?;
            if tips.commits.binary_search(tip).is_err() {
                notices.push(format!(
                    "declared target '{target}' advertised at {tip} is not fetched here; \
                     falling back to advertised-history range selection for '{branch}'"
                ));
                return None;
            }
            let base = git(root, &["merge-base", &r.local_sha, tip])?;
            (base.trim().to_string(), target.to_string())
        };
    notices.push(format!(
        "range of '{branch}' uses advertised target '{target}': `codeflow ci --base {base} --head {}`",
        r.local_sha
    ));
    let mut note = None;
    if let Some(old) = existing_tip(root, r) {
        if git(root, &["merge-base", "--is-ancestor", old, &r.local_sha]).is_none()
            && git(root, &["merge-base", old, &r.local_sha]).is_some()
        {
            let count = git(
                root,
                &["rev-list", "--count", &format!("{base}..{}", r.local_sha)],
            )?;
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
    Some(RangeBase { base, note })
}

/// The destination's advertised sha for a branch it already has, when that
/// commit is here: the push updates an existing branch.
fn existing_tip<'a>(root: &Path, r: &'a PushRef) -> Option<&'a str> {
    let zero = r.remote_sha.is_empty() || r.remote_sha.chars().all(|c| c == '0');
    (!zero && is_commit(root, &r.remote_sha)).then_some(r.remote_sha.as_str())
}

/// The historical advertised-history fallback for an existing branch.
fn existing_base(root: &Path, r: &PushRef, destination: &Destination<'_>) -> RangeBase {
    let branch = r.remote_branch().unwrap_or_default();
    let old = &r.remote_sha;
    let line = own_line_tip(root, r, destination);
    let (base, failed) = match destination.advertised(root) {
        Advertised::Tips(tips) => (
            bounded_by(
                root,
                &r.local_sha,
                std::iter::once(old).chain(&tips.commits),
                Some(old),
                line.as_deref(),
            )
            .unwrap_or_else(|| old.clone()),
            None,
        ),
        Advertised::Failed(why) => (old.clone(), Some(why)),
    };
    let extends = git(root, &["merge-base", "--is-ancestor", old, &r.local_sha]).is_some();
    let related = git(root, &["merge-base", old, &r.local_sha]).is_some();
    let count = (!extends && related).then(|| {
        let range = format!("{base}..{}", r.local_sha);
        git(root, &["rev-list", "--count", &range])
            .map_or_else(|| "?".to_string(), |n| n.trim().to_string())
    });
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
    RangeBase { base, note }
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
/// holds, so nothing new is left out. `None` when git fails or no boundary
/// exists (no shared history).
fn bounded_by<'a>(
    root: &Path,
    local_sha: &str,
    known: impl Iterator<Item = &'a String>,
    own: Option<&str>,
    line: Option<&str>,
) -> Option<String> {
    let known: Vec<&String> = known.collect();
    let mut input = format!("{local_sha}\n");
    for sha in &known {
        input.push('^');
        input.push_str(sha);
        input.push('\n');
    }
    let listed = git_input(root, &["rev-list", "--boundary", "--stdin"], &input)?;
    if listed.trim().is_empty() {
        return Some(local_sha.to_string());
    }
    let bounds: Vec<&str> = listed.lines().filter_map(|l| l.strip_prefix('-')).collect();
    if let (Some(line), [_, _, ..]) = (line, bounds.as_slice()) {
        let on_line: Vec<&str> = bounds
            .iter()
            .copied()
            .filter(|bound| git(root, &["merge-base", "--is-ancestor", bound, line]).is_some())
            .collect();
        if !on_line.is_empty() {
            let mut args = vec!["merge-base", "--independent"];
            args.extend(&on_line);
            if let Some(newest) = git(root, &args) {
                if let [only] = newest.split_whitespace().collect::<Vec<_>>().as_slice() {
                    return Some((*only).to_string());
                }
            }
        }
    }
    let others: Vec<&String> = known
        .into_iter()
        .filter(|sha| Some(sha.as_str()) != own)
        .collect();
    narrowest(root, local_sha, &bounds, own, &others).map(str::to_string)
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
) -> Option<&'b str> {
    if boundaries.len() < 2 {
        return boundaries.first().copied();
    }
    let count = |input: &str| {
        git_input(root, &["rev-list", "--count", "--stdin"], input)
            .and_then(|n| n.trim().parse::<usize>().ok())
            .unwrap_or(usize::MAX)
    };
    boundaries.iter().copied().min_by_key(|base| {
        let range = format!("{local_sha}\n^{base}\n");
        let all = count(&range);
        let mut not_own = range;
        if let Some(own) = own {
            not_own.push('^');
            not_own.push_str(own);
            not_own.push('\n');
        }
        let mut fresh = not_own.clone();
        for sha in others {
            fresh.push('^');
            fresh.push_str(sha);
            fresh.push('\n');
        }
        (count(&not_own).saturating_sub(count(&fresh)), all)
    })
}

/// The advertised tip of the integration line the pushed branch's task
/// declares, when the branch carries a task and the destination has that
/// line.
fn own_line_tip(root: &Path, r: &PushRef, destination: &Destination<'_>) -> Option<String> {
    use codeflow_core::workgraph::{declared_work_target, task_id_from_branch};
    let id = task_id_from_branch(root, r.remote_branch()?)?;
    let target = declared_work_target(root, &id)?;
    let Ok(Ok(asked)) = destination.answer(root) else {
        return None;
    };
    asked
        .heads
        .iter()
        .find(|(name, _)| *name == target)
        .map(|(_, tip)| tip.to_string())
}

/// The base from `rev-list --boundary` output: its first boundary commit, or
/// the pushed sha itself when no commit is new.
fn boundary(listed: &str, local_sha: &str, note: Option<Finding>) -> Option<RangeBase> {
    if listed.trim().is_empty() {
        return Some(RangeBase {
            base: local_sha.to_string(),
            note,
        });
    }
    listed
        .lines()
        .find_map(|line| line.strip_prefix('-'))
        .map(|base| RangeBase {
            base: base.to_string(),
            note,
        })
}

/// Why the working checkout is not the whole committed tree, if it is not:
/// a sparse checkout, or a submodule that is uninitialized, at another
/// commit or conflicted.
fn incomplete_checkout(root: &Path) -> Option<String> {
    let sparse =
        git(root, &["config", "--bool", "core.sparseCheckout"]).is_some_and(|v| v.trim() == "true");
    if sparse {
        return Some("the checkout is sparse".to_string());
    }
    if !root.join(".gitmodules").exists() {
        return None;
    }
    let Some(status) = git(root, &["submodule", "status", "--recursive"]) else {
        return Some("submodule state could not be read".to_string());
    };
    status.lines().find_map(|line| {
        let path = line.get(1..)?.split_whitespace().nth(1).unwrap_or("?");
        match line.chars().next()? {
            '-' => Some(format!("submodule {path} is not initialized")),
            '+' => Some(format!("submodule {path} is not at its recorded commit")),
            'U' => Some(format!("submodule {path} has merge conflicts")),
            _ => None,
        }
    })
}

/// Git for the hook's own queries: never fetches a missing object, even in
/// a partial clone.
fn git(root: &Path, args: &[&str]) -> Option<String> {
    codeflow_core::git::command()
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_NO_LAZY_FETCH", "1")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
}

/// [`git`] with `input` on stdin.
fn git_input(root: &Path, args: &[&str], input: &str) -> Option<String> {
    let mut child = codeflow_core::git::command()
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_NO_LAZY_FETCH", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // Written from a thread so a large answer cannot fill the pipe first.
    let mut stdin = child.stdin.take()?;
    let input = input.to_string();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let out = child.wait_with_output().ok()?;
    writer.join().ok()?.ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

fn rev_parse(root: &Path, rev: &str) -> Option<String> {
    git(
        root,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{rev}^{{commit}}"),
        ],
    )
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
}

fn is_commit(root: &Path, sha: &str) -> bool {
    rev_parse(root, sha).is_some()
}

/// `true` when no tracked file differs from the checked-out commit.
fn tracked_tree_clean(root: &Path) -> bool {
    git(root, &["status", "--porcelain", "--untracked-files=no"])
        .is_some_and(|out| out.trim().is_empty())
}

fn short(sha: &str) -> &str {
    sha.get(..9).unwrap_or(sha)
}
