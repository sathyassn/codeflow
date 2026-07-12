//! `codeflow ci` — the CI-portable, binary-sourced verification of a commit
//! range and branch name against `.codeflow/policy.json` (charter §6.4, §6.1
//! plane 4). This is the SINGLE SOURCE OF TRUTH the CI plane calls: it reuses
//! the very same `standards`/`policy` check functions the git-client hooks and
//! the Claude git-guard use, so the CI checks can never drift from the hooks
//! (ADR-0017). Per-platform CI files (GitHub/GitLab/Bitbucket/generic) are thin
//! wrappers that install codeflow and shell out to this command.
//!
//! Exit 1 when any block-level violation is found; 0 when clean or only
//! warnings (which are printed, then the gate proceeds).

use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Args;
use codeflow_core::hooks::policy::Policy;
use codeflow_core::hooks::{
    any_blocking, git_hook, repo, standards, GitPolicy, PolicyLevel, Violation,
};

#[derive(Debug, Args)]
pub struct CiArgs {
    /// Base ref of the range (exclusive). Auto-detected from CI env when omitted.
    #[arg(long, value_name = "REF")]
    pub base: Option<String>,

    /// Head ref of the range (inclusive). Auto-detected from CI env when omitted.
    #[arg(long, value_name = "REF")]
    pub head: Option<String>,

    /// Branch name to check against the naming policy (default: the CI-provided
    /// or current HEAD branch).
    #[arg(long, value_name = "NAME")]
    pub branch: Option<String>,

    /// PR/MR body text to scan for AI attribution and emoji.
    #[arg(long, value_name = "TEXT")]
    pub pr_body: Option<String>,

    /// Read the PR/MR body from a file (scanned like `--pr-body`).
    #[arg(long, value_name = "FILE")]
    pub pr_body_file: Option<PathBuf>,
}

/// Environment variable holding the PR/MR body, consulted when neither
/// `--pr-body` nor `--pr-body-file` is given (the GitHub workflow sets it from
/// `github.event.pull_request.body`).
const PR_BODY_ENV: &str = "CODEFLOW_PR_BODY";

/// A violation tagged with the commit it came from (`None` for branch-name and
/// PR-body findings, which are not per-commit).
struct TaggedViolation {
    sha: Option<String>,
    violation: Violation,
}

/// One commit in the range: its full sha and full message (subject + body).
struct CommitRecord {
    sha: String,
    message: String,
}

/// How the base..head range was resolved, for an honest one-line report.
struct DetectedRange {
    /// Base ref candidates, tried in order (first that resolves wins).
    base_candidates: Vec<String>,
    /// Head ref.
    head: String,
    /// Human description of how the range was derived.
    source: String,
}

pub fn run(args: &CiArgs) -> i32 {
    let root = super::repo_root();
    // Reuse the exact loader the hooks use (charter D7). Bootstrap grace is
    // moot here: the pre-first-commit window cannot occur in CI, which always
    // has history — CI is the authoritative, always-armed perimeter.
    let (policy, _armed) = Policy::load_effective(&root);
    let git = &policy.git;

    // --- resolve the range ------------------------------------------------
    let detected = detect_range(|k| std::env::var(k).ok().filter(|v| !v.is_empty()));
    let base_spec = args
        .base
        .clone()
        .map(|b| (vec![b], "explicit --base flag".to_string()));
    let head = args
        .head
        .clone()
        .unwrap_or_else(|| detected.head.clone());

    let (base_candidates, range_source) = match base_spec {
        Some((cands, src)) => (cands, src),
        None => (detected.base_candidates.clone(), detected.source.clone()),
    };

    let branch = args
        .branch
        .clone()
        .or_else(|| detect_branch(|k| std::env::var(k).ok().filter(|v| !v.is_empty())))
        .or_else(|| repo::open(&root).map(|r| repo::current_branch(&r)))
        .unwrap_or_default();

    let pr_body = resolve_pr_body(args);

    println!("codeflow ci: verifying against .codeflow/policy.json");

    // --- commit-range checks ---------------------------------------------
    let mut tagged: Vec<TaggedViolation> = Vec::new();
    match resolve_base(&root, &base_candidates) {
        Some(base_sha) => match enumerate_commits(&root, &base_sha, &head) {
            Ok(commits) => {
                println!(
                    "codeflow ci: range {}..{} ({}) — {} non-merge commit(s)",
                    short(&base_sha),
                    head,
                    range_source,
                    commits.len()
                );
                tagged.extend(evaluate_commits(git, &commits));
            }
            Err(e) => {
                eprintln!("codeflow ci: warning: could not enumerate commits ({e}) — commit checks skipped");
            }
        },
        None => {
            eprintln!(
                "codeflow ci: warning: could not resolve a base ref (tried: {}) — commit checks skipped. Pass --base/--head explicitly.",
                base_candidates.join(", ")
            );
        }
    }

    // --- branch-naming check ---------------------------------------------
    if branch.is_empty() {
        println!("codeflow ci: no branch name resolved — branch-naming check skipped");
    } else {
        println!("codeflow ci: branch '{branch}'");
        if let Some(v) = evaluate_branch(git, &branch) {
            tagged.push(v);
        }
    }

    // --- PR-body check ----------------------------------------------------
    if let Some(body) = &pr_body {
        tagged.extend(
            evaluate_pr_body(git, body)
                .into_iter()
                .map(|violation| TaggedViolation { sha: None, violation }),
        );
    }

    report(&tagged)
}

/// Print every violation and a summary; return the process exit code
/// (1 when any violation blocks, else 0).
fn report(tagged: &[TaggedViolation]) -> i32 {
    for t in tagged {
        let plane = match &t.sha {
            Some(sha) => format!("ci commit {}", short(sha)),
            None => "ci".to_string(),
        };
        eprintln!("{}", t.violation.render(&plane));
    }

    let violations: Vec<Violation> = tagged.iter().map(|t| t.violation.clone()).collect();
    let blocking = any_blocking(&violations);
    let warnings = violations
        .iter()
        .filter(|v| v.level == PolicyLevel::Warn)
        .count();
    let blocks = violations
        .iter()
        .filter(|v| v.level == PolicyLevel::Block)
        .count();

    if violations.is_empty() {
        println!("codeflow ci: clean — commit, branch, and PR-body checks passed");
    } else if blocking {
        eprintln!("codeflow ci: FAILED — {blocks} blocking, {warnings} warning(s)");
    } else {
        eprintln!("codeflow ci: {warnings} warning(s) only — proceeding");
    }

    i32::from(blocking)
}

// ---------------------------------------------------------------------------
// pure evaluators (git-free, env-free — unit-tested directly)
// ---------------------------------------------------------------------------

/// Run every commit through the same `git_hook::commit_msg` the commit-msg
/// hook runs (format, breaking-footer, AI attribution, emoji), tagging each
/// finding with its commit sha. Reusing that function is what guarantees the
/// CI checks cannot drift from the hook (ADR-0017).
fn evaluate_commits(git: &GitPolicy, commits: &[CommitRecord]) -> Vec<TaggedViolation> {
    let mut out = Vec::new();
    for c in commits {
        let stage = git_hook::commit_msg(git, &c.message);
        for violation in stage.violations {
            out.push(TaggedViolation {
                sha: Some(c.sha.clone()),
                violation,
            });
        }
    }
    out
}

/// Check the head branch name against the naming policy. Mirrors the pre-push
/// hook's condition exactly (protected branches are exempt — governed by the
/// protection rules, not naming).
fn evaluate_branch(git: &GitPolicy, branch: &str) -> Option<TaggedViolation> {
    if branch.is_empty()
        || git.branch_is_protected(branch)
        || !git.branch_naming.is_active()
        || git.branch_name_ok(branch)
    {
        return None;
    }
    Some(TaggedViolation {
        sha: None,
        violation: Violation::new(
            "git.branch_naming",
            git.branch_naming,
            format!("branch '{branch}' does not match `{{prefix}}/{{kebab-name}}`"),
            format!(
                "rename with a sanctioned prefix: {}",
                git.branch_prefixes.join(" ")
            ),
        ),
    })
}

/// Scan a PR/MR body for AI attribution and emoji — the same public
/// `standards` functions the git-guard's PR-body scan uses (identical rules,
/// so the two planes flag identical content).
fn evaluate_pr_body(git: &GitPolicy, body: &str) -> Vec<Violation> {
    let mut out = Vec::new();
    if git.ai_attribution.is_active() {
        if let Some(which) = standards::find_attribution(body) {
            out.push(Violation::new(
                "git.ai_attribution",
                git.ai_attribution,
                format!("PR body contains AI attribution ({which})"),
                "remove the attribution — project policy forbids AI attribution in commits and PR bodies (charter §6.4)".to_string(),
            ));
        }
    }
    if git.commit_emoji.is_active() {
        if let Some(c) = standards::find_emoji(body) {
            out.push(Violation::new(
                "git.commit_emoji",
                git.commit_emoji,
                format!("PR body contains emoji ('{c}')"),
                "remove emoji from the PR body (charter §6.4)".to_string(),
            ));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// CI-platform range / branch detection (env-injected — unit-tested directly)
// ---------------------------------------------------------------------------

/// Auto-detect the commit range from CI-platform environment variables. The
/// `env` accessor returns non-empty values only. Verified variable names:
/// GitHub `GITHUB_BASE_REF`/`GITHUB_HEAD_REF`; GitLab
/// `CI_MERGE_REQUEST_DIFF_BASE_SHA`/`CI_COMMIT_SHA`; Bitbucket
/// `BITBUCKET_PR_DESTINATION_COMMIT`/`BITBUCKET_COMMIT`.
fn detect_range<F: Fn(&str) -> Option<String>>(env: F) -> DetectedRange {
    // GitLab merge-request pipeline: an exact base sha is provided.
    if let Some(base) = env("CI_MERGE_REQUEST_DIFF_BASE_SHA") {
        return DetectedRange {
            base_candidates: vec![base],
            head: env("CI_COMMIT_SHA").unwrap_or_else(|| "HEAD".to_string()),
            source: "GitLab CI_MERGE_REQUEST_DIFF_BASE_SHA".to_string(),
        };
    }
    // Bitbucket pull-request pipeline: destination-branch commit hash.
    if let Some(base) = env("BITBUCKET_PR_DESTINATION_COMMIT") {
        return DetectedRange {
            base_candidates: vec![base],
            head: env("BITBUCKET_COMMIT").unwrap_or_else(|| "HEAD".to_string()),
            source: "Bitbucket BITBUCKET_PR_DESTINATION_COMMIT".to_string(),
        };
    }
    // GitHub Actions pull_request: base is a branch NAME — prefer the
    // remote-tracking ref (present with fetch-depth: 0), fall back to the bare
    // name. Head stays HEAD (the checked-out PR head).
    if let Some(base_ref) = env("GITHUB_BASE_REF") {
        return DetectedRange {
            base_candidates: vec![format!("origin/{base_ref}"), base_ref],
            head: "HEAD".to_string(),
            source: "GitHub GITHUB_BASE_REF".to_string(),
        };
    }
    // Fallback: diff against the default protected branch. `origin/<b>` first
    // (the usual CI checkout), then the bare local name.
    if let Some(base) = env("CODEFLOW_DEFAULT_BRANCH") {
        return DetectedRange {
            base_candidates: vec![format!("origin/{base}"), base.clone()],
            head: "HEAD".to_string(),
            source: format!("default branch '{base}' (fallback)"),
        };
    }
    DetectedRange {
        base_candidates: vec!["origin/main".to_string(), "main".to_string()],
        head: "HEAD".to_string(),
        source: "default branch 'main' (fallback)".to_string(),
    }
}

/// Auto-detect the head branch NAME to name-check. Verified variable names:
/// GitHub `GITHUB_HEAD_REF`; GitLab `CI_MERGE_REQUEST_SOURCE_BRANCH_NAME` then
/// `CI_COMMIT_REF_NAME`; Bitbucket `BITBUCKET_BRANCH`.
fn detect_branch<F: Fn(&str) -> Option<String>>(env: F) -> Option<String> {
    env("GITHUB_HEAD_REF")
        .or_else(|| env("CI_MERGE_REQUEST_SOURCE_BRANCH_NAME"))
        .or_else(|| env("CI_COMMIT_REF_NAME"))
        .or_else(|| env("BITBUCKET_BRANCH"))
}

// ---------------------------------------------------------------------------
// git / env plumbing (the thin, side-effecting shell around the pure fns)
// ---------------------------------------------------------------------------

/// Read the PR body from `--pr-body`, then `--pr-body-file`, then the
/// `CODEFLOW_PR_BODY` env var. Returns `None` (skip the check) when none is set.
fn resolve_pr_body(args: &CiArgs) -> Option<String> {
    if let Some(body) = &args.pr_body {
        return Some(body.clone());
    }
    if let Some(path) = &args.pr_body_file {
        match std::fs::read_to_string(path) {
            Ok(body) => return Some(body),
            Err(e) => eprintln!(
                "codeflow ci: warning: could not read --pr-body-file {}: {e} — PR-body check skipped",
                path.display()
            ),
        }
    }
    std::env::var(PR_BODY_ENV).ok().filter(|v| !v.is_empty())
}

/// Resolve the first base candidate that names a real commit, returning its sha.
fn resolve_base(root: &Path, candidates: &[String]) -> Option<String> {
    candidates.iter().find_map(|c| rev_parse(root, c))
}

/// `git rev-parse --verify --quiet <rev>^{commit}` — returns the resolved sha,
/// or `None` when the rev does not name a commit.
fn rev_parse(root: &Path, rev: &str) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--verify", "--quiet"])
        .arg(format!("{rev}^{{commit}}"))
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!sha.is_empty()).then_some(sha)
}

/// Enumerate the non-merge commits in `base..head`, newest first, as
/// (sha, full-message) records. Uses a NUL-delimited `git log` so multi-line
/// bodies parse unambiguously.
fn enumerate_commits(root: &Path, base: &str, head: &str) -> Result<Vec<CommitRecord>, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["log", "--no-merges", "-z", "--format=%H%n%B"])
        .arg(format!("{base}..{head}"))
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(parse_log(&String::from_utf8_lossy(&out.stdout)))
}

/// Parse the NUL-delimited `git log --format=%H%n%B` output into records.
fn parse_log(stdout: &str) -> Vec<CommitRecord> {
    stdout
        .split('\0')
        .filter(|rec| !rec.trim().is_empty())
        .filter_map(|rec| {
            let (sha, message) = rec.split_once('\n')?;
            Some(CommitRecord {
                sha: sha.trim().to_string(),
                message: message.to_string(),
            })
        })
        .collect()
}

/// First 8 chars of a sha for display; the full string when shorter.
fn short(sha: &str) -> &str {
    sha.get(..8).unwrap_or(sha)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git() -> GitPolicy {
        GitPolicy::default()
    }

    fn commit(sha: &str, message: &str) -> CommitRecord {
        CommitRecord {
            sha: sha.to_string(),
            message: message.to_string(),
        }
    }

    // -- commit-range evaluation -----------------------------------------

    #[test]
    fn clean_range_passes() {
        let commits = vec![
            commit("aaaa1111", "feat(ci): add codeflow ci command"),
            commit("bbbb2222", "fix: handle unborn head\n\nA clean body."),
        ];
        let v = evaluate_commits(&git(), &commits);
        assert!(v.is_empty(), "clean commits produce no violations: {v:?}", v = v.len());
    }

    #[test]
    fn malformed_subject_blocks() {
        let commits = vec![commit("aaaa1111", "Added some stuff")];
        let v = evaluate_commits(&git(), &commits);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].violation.rule, "git.commit_format");
        assert_eq!(v[0].violation.level, PolicyLevel::Block);
        assert_eq!(v[0].sha.as_deref(), Some("aaaa1111"));
    }

    #[test]
    fn ai_attribution_trailer_blocks() {
        let msg = "feat: add thing\n\nCo-Authored-By: Claude Fable 5 <noreply@anthropic.com>";
        let v = evaluate_commits(&git(), &[commit("cccc3333", msg)]);
        assert!(v.iter().any(|t| t.violation.rule == "git.ai_attribution"));
        assert!(any_blocking(
            &v.iter().map(|t| t.violation.clone()).collect::<Vec<_>>()
        ));
    }

    #[test]
    fn emoji_in_subject_blocks() {
        let v = evaluate_commits(&git(), &[commit("dddd4444", "feat: ship it \u{1F680}")]);
        assert!(v.iter().any(|t| t.violation.rule == "git.commit_emoji"));
    }

    #[test]
    fn warn_level_does_not_block() {
        let g = GitPolicy {
            commit_format: PolicyLevel::Warn,
            ..GitPolicy::default()
        };
        let v = evaluate_commits(&g, &[commit("eeee5555", "Added some stuff")]);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].violation.level, PolicyLevel::Warn);
        let flat: Vec<Violation> = v.into_iter().map(|t| t.violation).collect();
        assert!(!any_blocking(&flat), "a warn-level violation must not block");
    }

    #[test]
    fn off_level_skips() {
        let g = GitPolicy {
            commit_format: PolicyLevel::Off,
            commit_emoji: PolicyLevel::Off,
            ai_attribution: PolicyLevel::Off,
            ..GitPolicy::default()
        };
        let v = evaluate_commits(&g, &[commit("ffff6666", "Added some stuff")]);
        assert!(v.is_empty(), "off-level checks produce nothing");
    }

    // -- branch-naming ----------------------------------------------------

    #[test]
    fn bad_branch_name_blocks() {
        let t = evaluate_branch(&git(), "my-cool-branch").expect("violation");
        assert_eq!(t.violation.rule, "git.branch_naming");
        assert_eq!(t.violation.level, PolicyLevel::Block);
    }

    #[test]
    fn good_branch_name_passes() {
        assert!(evaluate_branch(&git(), "feat/codeflow-ci").is_none());
    }

    #[test]
    fn protected_branch_is_exempt_from_naming() {
        assert!(evaluate_branch(&git(), "main").is_none());
        assert!(evaluate_branch(&git(), "master").is_none());
    }

    #[test]
    fn branch_naming_off_skips() {
        let g = GitPolicy {
            branch_naming: PolicyLevel::Off,
            ..GitPolicy::default()
        };
        assert!(evaluate_branch(&g, "my-cool-branch").is_none());
    }

    // -- PR body ----------------------------------------------------------

    #[test]
    fn pr_body_attribution_and_emoji_block() {
        let v = evaluate_pr_body(&git(), "Summary\n\nGenerated with [Claude Code](x) \u{2728}");
        assert!(v.iter().any(|x| x.rule == "git.ai_attribution"));
        assert!(v.iter().any(|x| x.rule == "git.commit_emoji"));
    }

    #[test]
    fn pr_body_clean_passes() {
        assert!(evaluate_pr_body(&git(), "Summary of a clean PR body.").is_empty());
    }

    // -- range detection --------------------------------------------------

    fn env_from<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            pairs
                .iter()
                .find(|(key, _)| *key == k)
                .map(|(_, v)| (*v).to_string())
        }
    }

    #[test]
    fn detect_range_gitlab() {
        let r = detect_range(env_from(&[
            ("CI_MERGE_REQUEST_DIFF_BASE_SHA", "abc123"),
            ("CI_COMMIT_SHA", "def456"),
        ]));
        assert_eq!(r.base_candidates, vec!["abc123"]);
        assert_eq!(r.head, "def456");
        assert!(r.source.contains("GitLab"));
    }

    #[test]
    fn detect_range_bitbucket() {
        let r = detect_range(env_from(&[
            ("BITBUCKET_PR_DESTINATION_COMMIT", "dest99"),
            ("BITBUCKET_COMMIT", "head99"),
        ]));
        assert_eq!(r.base_candidates, vec!["dest99"]);
        assert_eq!(r.head, "head99");
        assert!(r.source.contains("Bitbucket"));
    }

    #[test]
    fn detect_range_github() {
        let r = detect_range(env_from(&[("GITHUB_BASE_REF", "main")]));
        assert_eq!(r.base_candidates, vec!["origin/main", "main"]);
        assert_eq!(r.head, "HEAD");
        assert!(r.source.contains("GitHub"));
    }

    #[test]
    fn detect_range_fallback_default() {
        let r = detect_range(env_from(&[]));
        assert_eq!(r.base_candidates, vec!["origin/main", "main"]);
        assert_eq!(r.head, "HEAD");
        assert!(r.source.contains("fallback"));
    }

    #[test]
    fn detect_range_precedence_gitlab_over_github() {
        // A GitLab MR pipeline can also carry GITHUB_* if mirrored; the exact
        // base sha wins over the branch-name heuristic.
        let r = detect_range(env_from(&[
            ("CI_MERGE_REQUEST_DIFF_BASE_SHA", "gl-base"),
            ("GITHUB_BASE_REF", "main"),
        ]));
        assert_eq!(r.base_candidates, vec!["gl-base"]);
    }

    // -- branch detection -------------------------------------------------

    #[test]
    fn detect_branch_github_then_gitlab_then_bitbucket() {
        assert_eq!(
            detect_branch(env_from(&[("GITHUB_HEAD_REF", "feat/gh")])),
            Some("feat/gh".to_string())
        );
        assert_eq!(
            detect_branch(env_from(&[("CI_MERGE_REQUEST_SOURCE_BRANCH_NAME", "feat/gl")])),
            Some("feat/gl".to_string())
        );
        assert_eq!(
            detect_branch(env_from(&[("CI_COMMIT_REF_NAME", "feat/gl-branch")])),
            Some("feat/gl-branch".to_string())
        );
        assert_eq!(
            detect_branch(env_from(&[("BITBUCKET_BRANCH", "feat/bb")])),
            Some("feat/bb".to_string())
        );
        assert_eq!(detect_branch(env_from(&[])), None);
    }

    // -- log parsing ------------------------------------------------------

    #[test]
    fn parse_log_splits_nul_records() {
        let stdout = "aaaa1111\nfeat: one\n\nbody one\n\0bbbb2222\nfix: two\n\0";
        let recs = parse_log(stdout);
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].sha, "aaaa1111");
        assert!(recs[0].message.starts_with("feat: one"));
        assert_eq!(recs[1].sha, "bbbb2222");
        assert!(recs[1].message.starts_with("fix: two"));
    }

    #[test]
    fn parse_log_empty_is_empty() {
        assert!(parse_log("").is_empty());
        assert!(parse_log("\0").is_empty());
    }

    #[test]
    fn short_sha_truncates() {
        assert_eq!(short("0123456789abcdef"), "01234567");
        assert_eq!(short("abc"), "abc");
    }
}
