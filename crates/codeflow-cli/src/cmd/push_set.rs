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
//! - The hook blocks on what it can see and never claims more: when neither
//!   the destination's current sha nor its own tracking refs give a base,
//!   the range is reported unresolved and left to CI, never compared with a
//!   local branch.
//! - The whole set is timed; over [`git_hook::PUSH_SET_BUDGET`] the hook names
//!   the slowest step.
//!
//! Every check runs through this same binary, so the hook checks what CI will
//! check. A failure is a `git.test_gate_on_push` violation at the policy's
//! level, with the check's own output printed above it.

use std::path::Path;
use std::process::Command;
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
    let pushed: Vec<&PushRef> = refs
        .iter()
        .filter(|r| r.remote_branch().is_some() && !r.is_delete())
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
    let mut steps: Vec<PushStep> = Vec::new();

    for r in &pushed {
        let branch = r.remote_branch().unwrap_or_default();
        match range_base(root, r, namespace.as_deref()) {
            Some(base) => {
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
                "`codeflow ci` did not run for '{branch}': range unresolved (no destination \
                 sha or tracking history for this destination); CI checks it"
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

/// The exclusive base of a pushed branch's range, from the destination's
/// own history only:
/// 1. the destination's current sha, when it names a known commit that the
///    pushed sha extends, or one with no shared history (`codeflow ci` then
///    reports the unrelated base);
/// 2. else, including a rewrite such as a rebase, the newest commit the
///    pushed sha shares with the destination's own tracking refs (the pushed
///    sha itself when nothing is new). From the old sha, a rebased branch's
///    range would hold every upstream commit the rebase brought in;
/// 3. else `None`: the range is unresolved and CI checks it. A local branch
///    is never substituted: it may be stale or not the destination's base.
fn range_base(root: &Path, r: &PushRef, namespace: Option<&str>) -> Option<String> {
    let zero = r.remote_sha.is_empty() || r.remote_sha.chars().all(|c| c == '0');
    if !zero && is_commit(root, &r.remote_sha) {
        let extends = git(
            root,
            &["merge-base", "--is-ancestor", &r.remote_sha, &r.local_sha],
        )
        .is_some();
        let related = git(root, &["merge-base", &r.remote_sha, &r.local_sha]).is_some();
        if extends || !related {
            return Some(r.remote_sha.clone());
        }
    }
    let ns = namespace?;
    let glob = format!("--glob={ns}*");
    let listed = git(
        root,
        &["rev-list", "--boundary", &r.local_sha, "--not", &glob],
    )?;
    if listed.trim().is_empty() {
        return Some(r.local_sha.clone());
    }
    listed
        .lines()
        .find_map(|line| line.strip_prefix('-'))
        .map(str::to_string)
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
