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
//!   tag tips the push location advertises now (`git ls-remote`, never
//!   interactive, bounded in time and size, fetching nothing), plus the sha
//!   it advertised for an existing branch. When it cannot be asked, an
//!   existing branch falls back to that sha alone, and a new branch to its
//!   protected branches' tracking refs when those refs describe the same
//!   location. When nothing gives a base, the range is reported unresolved
//!   and left to CI, never compared with a local branch.
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
use std::io::{Read as _, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use codeflow_core::hooks::git_hook::{self, PushRef, PushStep, StageReport};
use codeflow_core::hooks::policy::GitPolicy;
use codeflow_core::hooks::Violation;

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
    // Asked once, on first use.
    let advertised: OnceCell<Advertised> = OnceCell::new();
    let destination = Destination {
        url,
        advertised: &advertised,
        namespace: namespace.as_deref(),
        protected: &policy.protected_branches,
    };
    let mut steps: Vec<PushStep> = Vec::new();

    for r in &pushed {
        let branch = r.remote_branch().unwrap_or_default();
        if let Some(RangeBase { base, note }) = range_base(root, r, &destination) {
            report.notes.extend(note);
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
            run_check(&exe, root, &args, policy, report, &mut steps);
        } else {
            let why = destination
                .failure()
                .map(|why| format!("; asking it: {why}"))
                .unwrap_or_default();
            report.notes.push(format!(
                "`codeflow ci` did not run for '{branch}': range unresolved (a new \
                     branch, and neither the destination's advertised tips nor tracking \
                     refs bound to it give a base{why}); CI checks it"
            ));
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
    /// What the destination advertises now. Filled on first use.
    advertised: &'a OnceCell<Advertised>,
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
    fn advertised(&self, root: &Path) -> &Advertised {
        self.advertised.get_or_init(|| {
            self.url.map_or_else(
                || Advertised::Failed("no destination given".to_string()),
                |url| advertised_commits(root, url),
            )
        })
    }

    /// Why the destination could not be asked, once it was tried.
    fn failure(&self) -> Option<&str> {
        match self.advertised.get()? {
            Advertised::Failed(why) => Some(why),
            Advertised::Tips(_) => None,
        }
    }
}

/// Whether remote `name` fetches from `url`, so that its tracking refs
/// describe the location pushed to. A `pushurl` elsewhere does not.
fn fetches_from(root: &Path, name: &str, url: &str) -> bool {
    git(root, &["remote", "get-url", name]).is_some_and(|fetch| fetch.trim() == url)
}

/// How long the destination may take to answer, and how much it may say.
const LS_REMOTE_DEADLINE: Duration = Duration::from_secs(10);
const LS_REMOTE_MAX_BYTES: usize = 16 << 20;

/// The branch and tag tips `url` advertises now (tags peeled), kept when the
/// commit already exists here; nothing is fetched, even in a partial clone.
fn advertised_commits(root: &Path, url: &str) -> Advertised {
    let listed = match ls_remote(root, url) {
        Ok(listed) => listed,
        Err(why) => return Advertised::Failed(why),
    };
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

/// `git ls-remote --heads --tags <url>`, never interactive and bounded: no
/// terminal, askpass or SSH password prompt, at most [`LS_REMOTE_DEADLINE`]
/// (then the whole process group is killed) and [`LS_REMOTE_MAX_BYTES`].
fn ls_remote(root: &Path, url: &str) -> Result<String, String> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(root)
        .args(["ls-remote", "--heads", "--tags", url])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "false")
        .env("SSH_ASKPASS", "false")
        .env("SSH_ASKPASS_REQUIRE", "never")
        .env("GIT_SSH_COMMAND", batch_ssh_command(root))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start git: {error}"))?;
    let Some(stdout) = child.stdout.take() else {
        kill_group(&mut child);
        return Err("could not read its answer".to_string());
    };
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let read = stdout
            .take(LS_REMOTE_MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes);
        let _ = sender.send(read.map(|_| bytes));
    });
    let deadline = Instant::now() + LS_REMOTE_DEADLINE;
    let timed_out = || format!("no answer within {}s", LS_REMOTE_DEADLINE.as_secs());
    let bytes = loop {
        match receiver.recv_timeout(Duration::from_millis(20)) {
            Ok(Ok(bytes)) => break bytes,
            Ok(Err(error)) => {
                kill_group(&mut child);
                return Err(format!("reading its answer failed: {error}"));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) if Instant::now() < deadline => {}
            Err(_) => {
                kill_group(&mut child);
                return Err(timed_out());
            }
        }
    };
    if bytes.len() > LS_REMOTE_MAX_BYTES {
        kill_group(&mut child);
        return Err(format!(
            "its answer is over {} MiB",
            LS_REMOTE_MAX_BYTES >> 20
        ));
    }
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                kill_group(&mut child);
                return Err(timed_out());
            }
        }
    };
    if !status.success() {
        return Err(
            "`git ls-remote` failed (unreachable, no credentials, or no such repository)"
                .to_string(),
        );
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// The SSH command git would use, with password and host-key prompts off.
/// A configured command is kept: `GIT_SSH_COMMAND`, then `core.sshCommand`,
/// then `GIT_SSH`, then `ssh`.
fn batch_ssh_command(root: &Path) -> String {
    let configured = std::env::var("GIT_SSH_COMMAND")
        .ok()
        .filter(|command| !command.trim().is_empty())
        .or_else(|| {
            git(root, &["config", "--get", "core.sshCommand"])
                .map(|command| command.trim().to_string())
                .filter(|command| !command.is_empty())
        })
        .or_else(|| {
            std::env::var("GIT_SSH")
                .ok()
                .filter(|program| !program.is_empty())
                .map(|program| format!("'{}'", program.replace('\'', "'\\''")))
        })
        .unwrap_or_else(|| "ssh".to_string());
    format!("{configured} -o BatchMode=yes")
}

/// Kill a child and, on Unix, the process group it leads (an SSH or
/// credential helper it started), then reap it.
fn kill_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    if let Ok(group) = i32::try_from(child.id()) {
        // SAFETY: the child leads its own process group (`process_group(0)`),
        // whose id is its pid; SIGKILL to it is best-effort.
        unsafe {
            libc::killpg(group, libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
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
