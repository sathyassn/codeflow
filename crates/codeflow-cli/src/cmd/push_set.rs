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
//!   leaves out only history known to be on the destination: the sha it
//!   advertised for the branch, or for a new branch the branch and tag tips
//!   it advertises now (`git ls-remote`), falling back to its protected
//!   branches' tracking refs when it cannot be asked. When nothing gives a
//!   base, the range is reported unresolved and left to CI, never compared
//!   with a local branch.
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

/// Run the push set for `refs` into `report`. `remote` is the pre-push
/// hook's first argument: a configured remote name, or the URL or path
/// pushed to directly.
pub(super) fn run(
    root: &Path,
    policy: &GitPolicy,
    refs: &[PushRef],
    remote: Option<&str>,
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
    let namespace = remote.and_then(|name| tracking_namespace(root, name));
    // Asked once, and only when a pushed branch is new to the destination.
    let advertised: OnceCell<Option<Vec<String>>> = OnceCell::new();
    let destination = Destination {
        remote,
        advertised: &advertised,
        namespace: namespace.as_deref(),
        protected: &policy.protected_branches,
    };
    let mut steps: Vec<PushStep> = Vec::new();

    for r in &pushed {
        let branch = r.remote_branch().unwrap_or_default();
        match range_base(root, r, &destination) {
            Some(RangeBase { base, note }) => {
                report.notes.extend(note);
                let args = [
                    "ci",
                    "--base",
                    &base,
                    "--head",
                    &r.local_sha,
                    "--branch",
                    branch,
                ];
                run_check(&exe, root, &args, policy, report, &mut steps);
            }
            None => report.notes.push(format!(
                "`codeflow ci` did not run for '{branch}': range unresolved (a new branch, \
                 and none of the destination's advertised tips or tracked protected \
                 branches is here); CI checks it"
            )),
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

/// Run one built-in check through this binary and time it.
fn run_check(
    exe: &Path,
    root: &Path,
    args: &[&str],
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
        .output();
    steps.push(PushStep {
        name: shown.clone(),
        duration: started.elapsed(),
        configurable: false,
    });
    match output {
        Ok(out) if out.status.success() => {
            // A passing check can still have degraded; keep that legible.
            let stderr = String::from_utf8_lossy(&out.stderr);
            report.notes.extend(
                stderr
                    .lines()
                    .filter(|line| line.contains("warning:") && !line.contains("registry"))
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
    /// The hook's remote argument: a configured name, a URL or a path.
    remote: Option<&'a str>,
    /// The commits the destination advertises now that exist here; `None`
    /// when it could not be asked. Filled on first use.
    advertised: &'a OnceCell<Option<Vec<String>>>,
    /// The remote's tracking namespace, for the protected-branch fallback.
    namespace: Option<&'a str>,
    protected: &'a [String],
}

impl Destination<'_> {
    fn advertised(&self, root: &Path) -> Option<&[String]> {
        self.advertised
            .get_or_init(|| {
                self.remote
                    .and_then(|remote| advertised_commits(root, remote))
            })
            .as_deref()
    }
}

/// The branch and tag tips `remote` advertises now (tags peeled), kept when
/// the commit exists in this repository. `None` when it cannot be asked: no
/// network, no credentials (never prompted for), or no such remote.
fn advertised_commits(root: &Path, remote: &str) -> Option<Vec<String>> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-remote", "--heads", "--tags", remote])
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .ok()
        .filter(|out| out.status.success())?;
    let listed = String::from_utf8_lossy(&out.stdout);
    let shas: Vec<&str> = listed
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|sha| sha.len() >= 40 && sha.chars().all(|c| c.is_ascii_hexdigit()))
        .collect();
    if shas.is_empty() {
        return Some(Vec::new());
    }
    let mut input = shas.join("\n");
    input.push('\n');
    let types = git_input(
        root,
        &["cat-file", "--batch-check=%(objectname) %(objecttype)"],
        &input,
    )?;
    let mut commits: Vec<String> = types
        .lines()
        .filter_map(|line| line.strip_suffix(" commit"))
        .map(ToString::to_string)
        .collect();
    commits.sort();
    commits.dedup();
    Some(commits)
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
/// 1. an existing destination branch: its current sha, which the destination
///    advertised for this push. For a fast-forward the range is exactly the
///    push. For a rewrite (a rebase or amend) it also holds any commits the
///    rewrite brought in from other branches; they are checked again, and the
///    note says so, because a cached tracking ref cannot prove they are still
///    on the destination (a feature or integration branch may have been
///    rewritten since the last fetch). With no shared history, `codeflow ci`
///    reports the unrelated base;
/// 2. a new branch: a boundary of the commits not reachable from the branch
///    and tag tips the destination advertises now (`git ls-remote`). Those
///    are exactly the commits it has, so a stale local tracking ref neither
///    hides nor adds anything, and a branch cut from any integration line is
///    checked for its own commits only. The pushed sha itself when nothing
///    is new;
/// 3. a new branch when the destination cannot be asked, or none of its
///    tips is here: the same boundary against its protected branches'
///    tracking refs. Policy forbids rewriting a protected branch, so even an
///    old cached tip is history the destination keeps. A failed ask is noted;
/// 4. else `None`: the range is unresolved and CI checks it. A local branch
///    is never substituted: it may be stale or not the destination's base.
fn range_base(root: &Path, r: &PushRef, destination: &Destination<'_>) -> Option<RangeBase> {
    let zero = r.remote_sha.is_empty() || r.remote_sha.chars().all(|c| c == '0');
    if !zero && is_commit(root, &r.remote_sha) {
        let extends = git(
            root,
            &["merge-base", "--is-ancestor", &r.remote_sha, &r.local_sha],
        )
        .is_some();
        let related = git(root, &["merge-base", &r.remote_sha, &r.local_sha]).is_some();
        let note = (!extends && related).then(|| {
            let range = format!("{}..{}", r.remote_sha, r.local_sha);
            let count = git(root, &["rev-list", "--count", &range])
                .map_or_else(|| "?".to_string(), |n| n.trim().to_string());
            format!(
                "'{}' rewrites the destination's {}: `codeflow ci` checks all {count} \
                 commit(s) not on it, including any the rewrite brought in from other \
                 branches",
                r.remote_branch().unwrap_or_default(),
                short(&r.remote_sha)
            )
        });
        return Some(RangeBase {
            base: r.remote_sha.clone(),
            note,
        });
    }
    let advertised = destination.advertised(root);
    if let Some(tips) = advertised.filter(|tips| !tips.is_empty()) {
        let mut input = format!("{}\n", r.local_sha);
        for tip in tips {
            input.push('^');
            input.push_str(tip);
            input.push('\n');
        }
        let listed = git_input(root, &["rev-list", "--boundary", "--stdin"], &input)?;
        return boundary(&listed, &r.local_sha, None);
    }
    let note = advertised.is_none().then(|| {
        format!(
            "`git ls-remote {}` failed: the range of new branch '{}' is bounded by the \
             destination's tracked protected branches instead",
            destination.remote.unwrap_or("?"),
            r.remote_branch().unwrap_or_default()
        )
    });
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

fn git(root: &Path, args: &[&str]) -> Option<String> {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
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
