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
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use codeflow_core::hooks::git_hook::{self, PushRef, PushStep, StageReport};
use codeflow_core::hooks::policy::GitPolicy;
use codeflow_core::hooks::Violation;
use codeflow_core::workgraph::release_line;

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
    if pushed.is_empty() || !policy.test_gate_on_push.is_active() {
        return;
    }
    let started = Instant::now();
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => {
            report.violations.push(violation(
                policy,
                format!("push set could not locate the codeflow binary: {error}"),
                "run `codeflow ci` and `codeflow validate --docs` by hand".to_string(),
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
    let destination = Destination {
        url,
        listing: &listing,
        advertised: &advertised,
        answer: &answer,
        namespace: namespace.as_deref(),
        protected: &policy.protected_branches,
    };
    let mut steps: Vec<PushStep> = Vec::new();

    run_ci_ranges(
        &exe,
        root,
        &pushed,
        &destination,
        policy,
        report,
        &mut steps,
    );

    if codeflow_core::release_local::adopted(root) {
        let remote = remote.unwrap_or("origin");
        for r in &pushed {
            release_preflight(root, r, remote, policy, report, &mut steps);
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
        report.notes.push(format!(
            "tree checks (`codeflow validate --docs`, quick targets) did not run for '{}': \
             {why}; CI runs them",
            r.remote_branch().unwrap_or_default()
        ));
    }

    let total = started.elapsed();
    if let Some(note) = git_hook::over_budget_note(&steps, total, git_hook::PUSH_SET_BUDGET) {
        report.notes.push(note);
    }
    report
        .notes
        .push(format!("push set finished in {:.1}s", total.as_secs_f64()));
}

fn violation(policy: &GitPolicy, message: String, remedy: String) -> Violation {
    Violation::new(
        "git.test_gate_on_push",
        policy.test_gate_on_push,
        message,
        remedy,
    )
}

/// Run `codeflow ci` over each pushed ref's range. A release branch is
/// judged from the default target's tip, as its pull request is; another
/// branch from the boundary of what the destination holds, or noted when
/// that is unresolved; a push whose scope cannot be read is refused (see
/// [`release_base`]).
fn run_ci_ranges(
    exe: &Path,
    root: &Path,
    pushed: &[&PushRef],
    destination: &Destination<'_>,
    policy: &GitPolicy,
    report: &mut StageReport,
    steps: &mut Vec<PushStep>,
) {
    for r in pushed {
        let branch = r.remote_branch().unwrap_or_default();
        let base = match release_base(root, r, branch, destination, policy, report) {
            Scoped::Refused => None,
            Scoped::Release(tip) => Some(tip),
            Scoped::Ordinary => {
                if let Some(RangeBase { base, note }) = range_base(root, r, destination) {
                    report.notes.extend(note);
                    Some(base)
                } else {
                    report.notes.push(unresolved(branch, destination));
                    None
                }
            }
        };
        if let Some(base) = base {
            let mut args = vec![
                "ci",
                "--base",
                &base,
                "--head",
                &r.local_sha,
                "--branch",
                branch,
            ];
            // The push's target is the branch itself: its current tip's
            // baseline list governs the record check, not the boundary's,
            // which can be another line's tip. A new branch keeps the base.
            if let Some(tip) = existing_tip(root, r) {
                args.extend(["--baseline-from", tip]);
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
    root: &Path,
    pushed: &PushRef,
    remote: &str,
    policy: &GitPolicy,
    report: &mut StageReport,
    steps: &mut Vec<PushStep>,
) {
    let branch = pushed.remote_branch().unwrap_or_default();
    let started = Instant::now();
    let result = codeflow_core::release_local::preflight(root, &pushed.local_sha, branch, remote);
    steps.push(PushStep {
        name: format!("release preflight ({branch})"),
        duration: started.elapsed(),
        configurable: false,
    });
    match result {
        Ok(outcome) => {
            report.notes.extend(
                outcome
                    .notes
                    .iter()
                    .map(|note| format!("release preflight ({branch}): {note}")),
            );
            if outcome.blocked() {
                report.violations.push(violation(
                    policy,
                    format!("release preflight ({branch}): this push breaks the release tree"),
                    format!(
                        "fix the release state named above, then rerun `python3 {} preflight --branch {branch}`",
                        codeflow_core::release_local::SCRIPT
                    ),
                ));
            }
        }
        Err(error) => report.notes.push(format!(
            "release preflight did not run for '{branch}': {error}; the pull request job checks it"
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
    let shown = format!("codeflow {}", args.join(" "));
    let started = Instant::now();
    let output = Command::new(exe)
        .args(args)
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
            let stderr = String::from_utf8_lossy(&out.stderr);
            report.notes.extend(
                stderr
                    .lines()
                    .filter(|line| {
                        (line.contains("warning:") && !line.contains("registry"))
                            || line.contains("notice:")
                    })
                    .map(|line| format!("`{shown}`: {}", line.trim())),
            );
        }
        Ok(out) => {
            eprint!("{}", String::from_utf8_lossy(&out.stdout));
            eprint!("{}", String::from_utf8_lossy(&out.stderr));
            report.violations.push(violation(
                policy,
                format!("push set check failed: `{shown}` (output above)"),
                format!("fix the findings, then rerun `{shown}`"),
            ));
        }
        Err(error) => report.violations.push(violation(
            policy,
            format!("push set check could not run: `{shown}`: {error}"),
            format!("run `{shown}` by hand"),
        )),
    }
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
    /// The tracking namespace of a remote that fetches from `url`, for the
    /// protected-branch fallback; `None` when no tracking refs describe it.
    namespace: Option<&'a str>,
    protected: &'a [String],
}

/// The destination's answer to `git ls-remote`.
enum Advertised {
    /// The advertised commits that exist here.
    Tips(Vec<String>),
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
    policy: &GitPolicy,
    report: &mut StageReport,
) -> Scoped {
    use codeflow_core::workgraph::{
        durable_work_tracking_enabled, durable_work_tracking_enabled_at,
    };
    let mut refuse = |message: String, remedy: &str| {
        report
            .violations
            .push(violation(policy, message, remedy.to_string()));
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
            return refuse(
                format!("the destination did not answer, so whether '{branch}' is a release branch cannot be read; it is not pushed unjudged (SPC-013 R-120): {why}"),
                "push again when the destination answers",
            )
        }
    };
    let tracked = durable_work_tracking_enabled(root).unwrap_or(true)
        || durable_work_tracking_enabled_at(root, &pushed.local_sha).unwrap_or(true);
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
                report.notes.push(format!(
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
fn unresolved(branch: &str, destination: &Destination<'_>) -> String {
    let why = destination
        .failure()
        .map(|why| format!("; asking it: {why}"))
        .unwrap_or_default();
    format!(
        "`codeflow ci` did not run for '{branch}': range unresolved (a new \
             branch, and neither the destination's advertised tips nor tracking \
             refs bound to it give a base{why}); CI checks it"
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
        return Advertised::Tips(Vec::new());
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
    Advertised::Tips(commits)
}

/// The exclusive base of a pushed branch's range, and a note when the range
/// is wider than the push itself.
struct RangeBase {
    base: String,
    note: Option<String>,
}

/// The base of a pushed branch's range. Only history known to be on the
/// destination is left out, and a cached tracking ref proves that only for a
/// branch the destination may not rewrite:
/// 1. an existing destination branch: a boundary of the commits not
///    reachable from the branch and tag tips the destination advertises now
///    (`git ls-remote`) or from the branch's own advertised sha. A
///    fast-forward is checked for the push itself, and a rebase onto an
///    integration line for the branch's own commits: the line's commits the
///    destination holds through other refs are not checked again. A rewrite
///    notes how many commits are checked. With no shared history, the
///    advertised sha is the base and `codeflow ci` reports it unrelated;
/// 2. an existing branch when the destination cannot be asked: its
///    advertised sha alone, noted. A rewrite then also checks the commits it
///    brought in from other branches;
/// 3. a new branch: the same boundary against the advertised tips alone.
///    Those are exactly the commits the destination has, so a stale local
///    tracking ref neither hides nor adds anything. The pushed sha itself
///    when nothing is new;
/// 4. a new branch when the destination cannot be asked, or none of its
///    tips is here: the same boundary against its protected branches'
///    tracking refs, when the remote fetches from the location pushed to.
///    Policy forbids rewriting a protected branch, so even an old cached tip
///    is history the destination keeps. A failed ask is noted;
/// 5. else `None`: the range is unresolved and CI checks it. A local branch
///    is never substituted: it may be stale or not the destination's base.
fn range_base(root: &Path, r: &PushRef, destination: &Destination<'_>) -> Option<RangeBase> {
    if existing_tip(root, r).is_some() {
        return Some(existing_base(root, r, destination));
    }
    let note = match destination.advertised(root) {
        Advertised::Tips(tips) if !tips.is_empty() => {
            return bounded_by(root, &r.local_sha, tips.iter())
                .map(|base| RangeBase { base, note: None });
        }
        Advertised::Tips(_) => None,
        Advertised::Failed(why) => Some(format!(
            "asking the destination for its branches failed ({why}): the range of new \
             branch '{}' is bounded by its tracked protected branches instead",
            r.remote_branch().unwrap_or_default()
        )),
    };
    let ns = destination.namespace?;
    let mut known: Vec<String> = Vec::new();
    for branch in destination.protected {
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

/// The destination's advertised sha for a branch it already has, when that
/// commit is here: the push updates an existing branch.
fn existing_tip<'a>(root: &Path, r: &'a PushRef) -> Option<&'a str> {
    let zero = r.remote_sha.is_empty() || r.remote_sha.chars().all(|c| c == '0');
    (!zero && is_commit(root, &r.remote_sha)).then_some(r.remote_sha.as_str())
}

/// The base of an existing destination branch's range (cases 1 and 2 of
/// [`range_base`]).
fn existing_base(root: &Path, r: &PushRef, destination: &Destination<'_>) -> RangeBase {
    let branch = r.remote_branch().unwrap_or_default();
    let old = &r.remote_sha;
    let (base, failed) = match destination.advertised(root) {
        Advertised::Tips(tips) => (
            bounded_by(root, &r.local_sha, std::iter::once(old).chain(tips))
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
        (None, Some(count)) => Some(format!(
            "{rewrite}: `codeflow ci` checks {count} commit(s), leaving out history the \
             destination's branches and tags hold"
        )),
        (Some(why), count) => {
            let fallback = format!(
                "asking the destination for its branches failed ({why}): the range of \
                 '{branch}' is bounded by its advertised {} alone",
                short(old)
            );
            Some(match count {
                Some(count) => format!(
                    "{fallback}; {rewrite}: `codeflow ci` checks all {count} commit(s) not \
                     on it, including any the rewrite brought in from other branches"
                ),
                None => fallback,
            })
        }
    };
    RangeBase { base, note }
}

/// The base of the commits in `local_sha` not reachable from `known`: see
/// [`boundary`]. `None` when git fails or no boundary exists (no shared
/// history).
fn bounded_by<'a>(
    root: &Path,
    local_sha: &str,
    known: impl Iterator<Item = &'a String>,
) -> Option<String> {
    let mut input = format!("{local_sha}\n");
    for sha in known {
        input.push('^');
        input.push_str(sha);
        input.push('\n');
    }
    let listed = git_input(root, &["rev-list", "--boundary", "--stdin"], &input)?;
    boundary(&listed, local_sha, None).map(|found| found.base)
}

/// The base from `rev-list --boundary` output: its first boundary commit, or
/// the pushed sha itself when no commit is new.
fn boundary(listed: &str, local_sha: &str, note: Option<String>) -> Option<RangeBase> {
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
    Command::new("git")
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
    let mut child = Command::new("git")
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
