//! `codeflow ci` — the CI-portable, binary-sourced verification of a commit
//! range and branch name against `.codeflow/policy.json` (charter §6.4, §6.1
//! plane 4). This is the SINGLE SOURCE OF TRUTH the CI plane calls: it reuses
//! the very same `standards`/`policy` check functions the git-client hooks and
//! the Claude git-guard use, so the CI checks can never drift from the hooks
//! (ADR-0017). Per-platform CI files (GitHub/GitLab/Bitbucket/generic) are thin
//! wrappers that install codeflow and shell out to this command.
//!
//! Exit 1 when any block-level violation is found; 0 when clean or only
//! warnings (which are printed, then the gate proceeds). Exit 2 when the
//! verification could not run in full — the commit checks were skipped (no
//! base ref resolved, or the range could not be enumerated) or an explicit
//! `--pr-body-file` was unreadable: an unverified range is not a pass. The
//! scaffolded wrappers avoid the skip state entirely (full-depth checkout
//! plus explicit `--base`/`--head`).

use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Args;
use codeflow_core::hooks::policy::{Policy, PolicySource};
use codeflow_core::hooks::{
    any_blocking, git_hook, policy_schema, repo, standards, GitPolicy, PolicyLevel, Violation,
};
use codeflow_core::validate::validate_workgraph;
use codeflow_core::workgraph::{
    check_work_start_for_branch, declared_work_target, durable_work_tracking_enabled,
    resolve_work_target, task_id_from_branch,
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

    /// PR/MR body text to scan for AI attribution, emoji, and the required
    /// section structure.
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

/// One commit in the range: its full sha, full message (subject + body), and
/// the files it touches (for the contract-surface tripwire, ADR-0020).
struct CommitRecord {
    sha: String,
    message: String,
    files: Vec<String>,
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

struct CommitRangeEvaluation {
    files: Option<Vec<String>>,
    violations: Vec<TaggedViolation>,
    ran: bool,
}

pub fn run(args: &CiArgs) -> i32 {
    let root = super::repo_root();
    // An invalid policy cannot verify the consumer's intent — fail loudly,
    // naming each offending key, rather than silently verify against the
    // built-in defaults, which could pass a range the real (mistyped) policy
    // meant to block. Exit 2, the existing could-not-verify-in-full code.
    if let Err(errors) = policy_schema::validate_policy(&root) {
        for e in &errors {
            eprintln!("codeflow ci: policy error: {e}");
        }
        eprintln!(
            "codeflow ci: error: .codeflow/policy.json is invalid — nothing was verified (see `codeflow policy explain`)"
        );
        return 2;
    }
    // Reuse the exact loader the hooks use (charter D7). Bootstrap grace is
    // moot here: the pre-first-commit window cannot occur in CI, which always
    // has history — CI is the authoritative, always-armed perimeter.
    let (policy, _armed) = Policy::load_effective(&root);
    let git = &policy.git;

    // --- resolve the range ------------------------------------------------
    let detected = detect_range(
        |k| std::env::var(k).ok().filter(|v| !v.is_empty()),
        &git.protected_branches,
    );
    let base_spec = args
        .base
        .clone()
        .map(|b| (vec![b], "explicit --base flag".to_string()));
    let head = args.head.clone().unwrap_or_else(|| detected.head.clone());

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

    let pr_body = match resolve_pr_body(args) {
        Ok(body) => body,
        Err(e) => {
            // The flag explicitly requested the check — failing open would
            // silently skip what the caller asked to verify.
            eprintln!("codeflow ci: error: {e}");
            return 2;
        }
    };

    print_source_banner(&root);

    // Track what actually executed — the summary must not claim more.
    let mut ran: Vec<&str> = Vec::new();
    let mut skipped: Vec<&str> = Vec::new();

    // --- commit-range checks ---------------------------------------------
    let mut tagged: Vec<TaggedViolation> = Vec::new();
    // Every path the range touches, for the PR-structure docs-only test.
    // `None` = the range could not be resolved (unknown = code, conservative).
    let range = evaluate_commit_range(&root, &base_candidates, &head, &range_source, git);
    tagged.extend(range.violations);
    if range.ran {
        ran.push("commit");
    } else {
        skipped.push("commit");
    }

    // --- branch-naming check ---------------------------------------------
    if branch.is_empty() {
        println!("codeflow ci: no branch name resolved — branch-naming check skipped");
        skipped.push("branch-naming");
    } else {
        println!("codeflow ci: branch '{branch}'");
        if let Some(v) = evaluate_branch(git, &branch) {
            tagged.push(v);
        }
        ran.push("branch-naming");
    }

    // A durable task branch may contain implementation only after its planning
    // record is present on the declared integration target. This uses the same
    // read-only merge-base preflight as `codeflow work start` and pre-commit.
    if branch.starts_with("task/") {
        match durable_work_tracking_enabled(&root) {
            Ok(true) => {
                evaluate_work_start(&root, &branch, &mut tagged);
                ran.push("work-start");
            }
            Ok(false) => {}
            Err(error) => {
                tagged.push(TaggedViolation {
                    sha: None,
                    violation: Violation::new(
                        "work.tracking_state",
                        PolicyLevel::Block,
                        format!("cannot determine durable-work tracking: {error}"),
                        "repair CodeFlow state or task-home access before task work".to_string(),
                    ),
                });
                ran.push("work-start");
            }
        }
    }

    // --- PR-body check ----------------------------------------------------
    if let Some(body) = &pr_body {
        tagged.extend(
            evaluate_pr_body(git, body)
                .into_iter()
                .chain(evaluate_pr_structure(git, body, range.files.as_deref()))
                .map(|violation| TaggedViolation {
                    sha: None,
                    violation,
                }),
        );
        ran.push("PR-body");
    }

    report(&tagged, &ran, &skipped)
}

fn evaluate_commit_range(
    root: &Path,
    base_candidates: &[String],
    head: &str,
    range_source: &str,
    git: &codeflow_core::hooks::policy::GitPolicy,
) -> CommitRangeEvaluation {
    let Some(base_sha) = resolve_base(root, base_candidates) else {
        eprintln!(
            "codeflow ci: warning: could not resolve a base ref (tried: {}) — commit checks skipped. Pass --base/--head explicitly.",
            base_candidates.join(", ")
        );
        return CommitRangeEvaluation {
            files: None,
            violations: Vec::new(),
            ran: false,
        };
    };
    match enumerate_commits(root, &base_sha, head) {
        Ok(commits) => {
            println!(
                "codeflow ci: range {}..{} ({}) — {} non-merge commit(s)",
                short(&base_sha),
                head,
                range_source,
                commits.len()
            );
            CommitRangeEvaluation {
                files: Some(
                    commits
                        .iter()
                        .flat_map(|commit| commit.files.clone())
                        .collect(),
                ),
                violations: evaluate_commits(git, &commits),
                ran: true,
            }
        }
        Err(error) => {
            eprintln!(
                "codeflow ci: warning: could not enumerate commits ({error}) — commit checks skipped"
            );
            CommitRangeEvaluation {
                files: None,
                violations: Vec::new(),
                ran: false,
            }
        }
    }
}

fn evaluate_work_start(root: &Path, branch: &str, tagged: &mut Vec<TaggedViolation>) {
    let workgraph = validate_workgraph(root);
    if !workgraph.is_clean() {
        let findings = workgraph
            .issues
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("; ");
        tagged.push(TaggedViolation {
            sha: None,
            violation: Violation::new(
                "work.valid_graph",
                PolicyLevel::Block,
                format!("visible durable workgraph is invalid: {findings}"),
                "repair the workgraph until `codeflow validate --docs` passes".to_string(),
            ),
        });
    }
    if let Some(task_id) = task_id_from_branch(root, branch) {
        let declared = declared_work_target(root, &task_id);
        let target =
            resolve_work_target(root, declared.as_deref()).unwrap_or_else(|| "main".to_string());
        if let Err(error) = check_work_start_for_branch(root, &task_id, &target, branch) {
            tagged.push(TaggedViolation {
                sha: None,
                violation: Violation::new(
                    "work.stable_planning_anchor",
                    PolicyLevel::Block,
                    error.to_string(),
                    format!(
                        "merge the validated planning record into '{target}', then run `codeflow work start {task_id}`"
                    ),
                ),
            });
        }
    } else {
        tagged.push(TaggedViolation {
            sha: None,
            violation: Violation::new(
                "work.task_record",
                PolicyLevel::Block,
                format!("task branch '{branch}' does not identify a visible task record"),
                "create and merge the durable task record before implementation".to_string(),
            ),
        });
    }
}

/// Report the ruleset actually enforced: the loader falls back to the
/// built-in charter defaults silently (fail-safe), so the banner must say
/// which source is in effect rather than assert the project file blindly.
fn print_source_banner(root: &Path) {
    match Policy::source(root) {
        PolicySource::ProjectFile => {
            println!("codeflow ci: verifying against .codeflow/policy.json");
        }
        PolicySource::MalformedFile => println!(
            "codeflow ci: .codeflow/policy.json is malformed — verifying against built-in charter defaults"
        ),
        PolicySource::Absent => println!(
            "codeflow ci: no .codeflow/policy.json — verifying against built-in charter defaults"
        ),
    }
}

/// Print every violation and an honest summary naming what ran vs what was
/// skipped; return the process exit code: 1 when any violation blocks, 2 when
/// the commit checks were skipped (nothing in the range was verified — not a
/// pass), else 0. A skipped branch-naming check is reported but not fatal: a
/// detached-head run without a CI branch variable is a legitimate state.
fn report(tagged: &[TaggedViolation], ran: &[&str], skipped: &[&str]) -> i32 {
    for t in tagged {
        let plane = match &t.sha {
            Some(sha) => format!("ci commit {}", short(sha)),
            None => "ci".to_string(),
        };
        if t.violation.rule.starts_with("work.") {
            eprintln!(
                "{}",
                t.violation.render_invariant(
                    &plane,
                    ".codeflow/project.toml full tier or an existing durable task layout"
                )
            );
        } else {
            eprintln!("{}", t.violation.render(&plane));
        }
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
        if skipped.is_empty() {
            println!("codeflow ci: clean — {} check(s) passed", ran.join(", "));
        } else {
            let ran_desc = if ran.is_empty() {
                "none".to_string()
            } else {
                ran.join(", ")
            };
            eprintln!(
                "codeflow ci: no violations in the checks that ran ({ran_desc}) — skipped: {}",
                skipped.join(", ")
            );
        }
    } else if blocking {
        eprintln!("codeflow ci: FAILED — {blocks} blocking, {warnings} warning(s)");
    } else if skipped.is_empty() {
        eprintln!("codeflow ci: {warnings} warning(s) only — proceeding");
    } else {
        eprintln!(
            "codeflow ci: {warnings} warning(s); skipped: {}",
            skipped.join(", ")
        );
    }

    if blocking {
        1
    } else if skipped.contains(&"commit") {
        2
    } else {
        0
    }
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
        let stage = git_hook::commit_msg_with_files(git, &c.message, &c.files, false);
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

/// How a required PR-body section was (or was not) found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SectionState {
    /// A matching heading exists with real content under it.
    Present,
    /// A matching heading exists but carries no content — only HTML comments,
    /// blank lines, and bare `-` bullets before the next heading.
    Empty,
    /// No matching heading at all.
    Missing,
}

/// Enforce the PR-body structure policy (`git.pr_sections`): the
/// `pr_required_sections` headings must be present and non-empty in every PR
/// body, the `pr_code_sections` headings additionally when the commit range
/// touches non-docs files, and template remnants (leftover placeholders from
/// the shipped PR template) draw a WARN — always warn-only, never a block,
/// whatever the level says. `range_files` is every path the range touches;
/// `None` means the range could not be resolved — treated as a code change
/// (conservative: unknown = code).
fn evaluate_pr_structure(
    git: &GitPolicy,
    body: &str,
    range_files: Option<&[String]>,
) -> Vec<Violation> {
    if !git.pr_sections.is_active() {
        return Vec::new();
    }
    let mut out = Vec::new();

    // (section, why it is required) — code sections carry the reason so the
    // finding explains itself; dedupe so a heading in both lists reports once.
    let mut required: Vec<(&str, &str)> = git
        .pr_required_sections
        .iter()
        .map(|s| (s.as_str(), ""))
        .collect();
    if !docs_only(range_files) {
        for s in &git.pr_code_sections {
            if !required
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case(s))
            {
                required.push((s.as_str(), " (the range touches code)"));
            }
        }
    }

    for (section, why) in required {
        let (found, detail) = match find_section(body, section) {
            SectionState::Present => continue,
            SectionState::Empty => (
                format!("PR body section '## {section}' is present but empty{why}"),
                "fill the section in — HTML comments and bare '-' bullets do not count as content",
            ),
            SectionState::Missing => (
                format!("PR body is missing required section '## {section}'{why}"),
                "add the section with real content — the shipped PR template carries the required structure",
            ),
        };
        out.push(Violation::new(
            "git.pr_sections",
            git.pr_sections,
            found,
            detail.to_string(),
        ));
    }

    // Template remnants: always warn-only — a nudge to finish the body, never
    // a block (mirrors the breaking_watch_paths tripwire convention).
    for (line_no, line, what) in find_placeholders(body) {
        out.push(Violation::new(
            "git.pr_sections",
            PolicyLevel::Warn,
            format!("PR body line {line_no} is a template remnant ({what}): '{line}'"),
            "replace the placeholder with real content, or delete the line".to_string(),
        ));
    }
    out
}

/// `true` when the range is known and every touched path is documentation:
/// Recognized prose, inert documentation images, license text, or GitHub issue
/// forms. A docs directory alone does not make executable content documentation.
/// `None` (unresolved range) and an empty file list are both treated
/// as code — the conservative direction, so a range whose files could not be
/// listed still requires the code sections.
fn docs_only(range_files: Option<&[String]>) -> bool {
    range_files.is_some_and(|files| !files.is_empty() && files.iter().all(|f| is_docs_path(f)))
}

/// Whether one changed path counts as documentation for [`docs_only`].
/// Everything unrecognized — code, config, CI yml, `Cargo.*`, `src/` — is a
/// code change; in particular `.github/workflows/**` is CI config, not docs.
fn is_docs_path(path: &str) -> bool {
    let p = path.trim();
    let name = p.rsplit('/').next().unwrap_or(p);
    // Instructions and shipped assets can change runtime/agent behavior even
    // when their serialization is Markdown. Prefer extra evidence to a prose
    // exemption for these contract surfaces.
    if matches!(name, "AGENTS.md" | "CLAUDE.md" | "SKILL.md")
        || p.starts_with("assets/")
        || p.split('/').any(|part| {
            matches!(
                part,
                ".agents" | ".claude" | ".codex" | ".grok" | ".codeflow"
            )
        })
    {
        return false;
    }
    let extension = Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if name.to_ascii_uppercase().starts_with("LICENSE") && extension.is_empty() {
        return true;
    }
    if p.starts_with(".github/ISSUE_TEMPLATE/") && matches!(extension.as_str(), "yml" | "yaml") {
        return true;
    }
    matches!(extension.as_str(), "md" | "txt" | "rst" | "adoc")
        || (p.starts_with("docs/")
            && matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp"))
}

/// Find a required section `name` in the body: a `##`/`###` heading whose
/// trimmed text equals `name` case-insensitively. Content runs to the next
/// heading of any depth; when every matching heading is content-free the
/// section is [`SectionState::Empty`].
fn find_section(body: &str, name: &str) -> SectionState {
    let lines: Vec<&str> = body.lines().collect();
    let mut state = SectionState::Missing;
    let mut i = 0;
    while i < lines.len() {
        let matched = heading(lines[i]).is_some_and(|(depth, text)| {
            (2..=3).contains(&depth) && text.eq_ignore_ascii_case(name)
        });
        if !matched {
            i += 1;
            continue;
        }
        let mut content = String::new();
        i += 1;
        while i < lines.len() && heading(lines[i]).is_none() {
            content.push_str(lines[i]);
            content.push('\n');
            i += 1;
        }
        if section_has_content(&content) {
            return SectionState::Present;
        }
        state = SectionState::Empty;
    }
    state
}

/// Parse a markdown ATX heading line into (depth, text). Leading whitespace is
/// tolerated; a closing `##` sequence is stripped (`## Summary ##` → `Summary`).
fn heading(line: &str) -> Option<(usize, &str)> {
    let t = line.trim();
    let depth = t.bytes().take_while(|b| *b == b'#').count();
    if depth == 0 || depth > 6 {
        return None;
    }
    let rest = &t[depth..];
    if !rest.is_empty() && !rest.starts_with(' ') && !rest.starts_with('\t') {
        return None;
    }
    Some((depth, rest.trim().trim_end_matches('#').trim_end()))
}

/// `true` when section text carries real content: anything beyond blank
/// lines, HTML comments, and bare `-` bullets (the template's empty stubs).
fn section_has_content(text: &str) -> bool {
    strip_html_comments(text).lines().any(|l| {
        let t = l.trim();
        !t.is_empty() && t != "-"
    })
}

/// Remove every `<!-- … -->` span (multi-line included). An unclosed comment
/// swallows the rest of the text — exactly how a markdown renderer treats it.
fn strip_html_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        match rest[start..].find("-->") {
            Some(end) => rest = &rest[start + end + 3..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Scan the body for leftovers of the shipped PR template, each reported as
/// (1-based line number, the trimmed line, what it is): the paste-your-output
/// placeholder, a table row of empty cells (`|  |  |`), and a bare `- CAP-` /
/// `- EPC-` linked-work bullet with nothing after the dash-prefix.
fn find_placeholders(body: &str) -> Vec<(usize, String, &'static str)> {
    let mut out = Vec::new();
    for (idx, line) in body.lines().enumerate() {
        let t = line.trim();
        let what = if t.contains("(paste the real test summary output here)") {
            "the template's paste-your-output placeholder"
        } else if is_empty_table_row(t) {
            "a table row of empty cells"
        } else if t == "- CAP-" || t == "- EPC-" {
            "a bare linked-work bullet"
        } else {
            continue;
        };
        out.push((idx + 1, t.to_string(), what));
    }
    out
}

/// A table row whose every cell is whitespace, e.g. `|  |  |` — at least two
/// cells, so a lone `|` or `| |` spacer is not flagged.
fn is_empty_table_row(t: &str) -> bool {
    let Some(inner) = t.strip_prefix('|').and_then(|rest| rest.strip_suffix('|')) else {
        return false;
    };
    let cells: Vec<&str> = inner.split('|').collect();
    cells.len() >= 2 && cells.iter().all(|c| c.trim().is_empty())
}

// ---------------------------------------------------------------------------
// CI-platform range / branch detection (env-injected — unit-tested directly)
// ---------------------------------------------------------------------------

/// Auto-detect the commit range from CI-platform environment variables. The
/// `env` accessor returns non-empty values only. Verified variable names:
/// GitHub `GITHUB_BASE_REF`/`GITHUB_HEAD_REF`; GitLab
/// `CI_MERGE_REQUEST_DIFF_BASE_SHA`/`CI_COMMIT_SHA`; Bitbucket
/// `BITBUCKET_PR_DESTINATION_COMMIT`/`BITBUCKET_COMMIT`. Outside those,
/// `CODEFLOW_DEFAULT_BRANCH` names the base branch explicitly (documented in
/// `assets/base/ci/ci-generic.sh`); the terminal fallback tries the policy's
/// protected branches (`git.protected_branches`, e.g. `main` then `master`).
fn detect_range<F: Fn(&str) -> Option<String>>(env: F, protected: &[String]) -> DetectedRange {
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
    // Explicit escape hatch for hosts with none of the variables above:
    // CODEFLOW_DEFAULT_BRANCH names the base branch. `origin/<b>` first (the
    // usual CI checkout), then the bare local name.
    if let Some(base) = env("CODEFLOW_DEFAULT_BRANCH") {
        return DetectedRange {
            base_candidates: vec![format!("origin/{base}"), base.clone()],
            head: "HEAD".to_string(),
            source: format!("CODEFLOW_DEFAULT_BRANCH '{base}' (fallback)"),
        };
    }
    // Terminal fallback: the policy's protected branches — the default branch
    // is virtually always among them, so a master-default repo resolves too.
    // Globs cannot name a ref and are skipped; per branch, `origin/<b>` then
    // the bare local name (the first candidate that resolves wins).
    let mut base_candidates: Vec<String> = Vec::new();
    for b in protected {
        if b.contains(['*', '?', '[']) {
            continue;
        }
        base_candidates.push(format!("origin/{b}"));
        base_candidates.push(b.clone());
    }
    if base_candidates.is_empty() {
        base_candidates = vec!["origin/main".to_string(), "main".to_string()];
    }
    DetectedRange {
        base_candidates,
        head: "HEAD".to_string(),
        source: "policy protected branches (fallback)".to_string(),
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
/// `CODEFLOW_PR_BODY` env var. Returns `Ok(None)` (check not requested) when
/// none is set. An unreadable `--pr-body-file` is a hard error — the flag
/// explicitly requested the check, and falling through to the env var would
/// scan a different body than the one named (or silently skip the check).
fn resolve_pr_body(args: &CiArgs) -> Result<Option<String>, String> {
    if let Some(body) = &args.pr_body {
        return Ok(Some(body.clone()));
    }
    if let Some(path) = &args.pr_body_file {
        return match std::fs::read_to_string(path) {
            Ok(body) => Ok(Some(body)),
            Err(e) => Err(format!(
                "could not read --pr-body-file {}: {e}",
                path.display()
            )),
        };
    }
    Ok(std::env::var(PR_BODY_ENV).ok().filter(|v| !v.is_empty()))
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
    let mut records = parse_log(&String::from_utf8_lossy(&out.stdout));
    // Populate each commit's touched files for the contract-surface tripwire
    // (ADR-0020); a per-commit call keeps the -z log parse unambiguous.
    for rec in &mut records {
        rec.files = commit_files(root, &rec.sha);
    }
    Ok(records)
}

/// Files a single commit touches (`git diff-tree --no-commit-id --name-only -r`).
/// Empty on any error — the tripwire is advisory, so an unavailable list means
/// no nudge.
fn commit_files(root: &Path, sha: &str) -> Vec<String> {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["diff-tree", "--no-commit-id", "--name-only", "-r", sha])
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
                files: Vec::new(),
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
            files: Vec::new(),
        }
    }

    fn commit_with_files(sha: &str, message: &str, files: &[&str]) -> CommitRecord {
        CommitRecord {
            sha: sha.to_string(),
            message: message.to_string(),
            files: files.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    // -- commit-range evaluation -----------------------------------------

    #[test]
    fn clean_range_passes() {
        let commits = vec![
            commit("aaaa1111", "feat(ci): add codeflow ci command"),
            commit(
                "bbbb2222",
                "fix: handle unborn head\n\n- guard the unborn head case",
            ),
        ];
        let v = evaluate_commits(&git(), &commits);
        assert!(
            v.is_empty(),
            "clean commits produce no violations: {v:?}",
            v = v.len()
        );
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
    fn contract_surface_tripwire_warns_in_ci() {
        // ADR-0020: the CI commit-range path inherits the tripwire (it reuses
        // commit_msg_with_files) — an unmarked commit touching a watched surface
        // warns but never blocks.
        let g = GitPolicy {
            breaking_watch_paths: vec!["crates/**/policy.rs".to_string()],
            ..GitPolicy::default()
        };
        let touch = commit_with_files(
            "aaaa0001",
            "feat: add a policy field",
            &["crates/codeflow-core/src/hooks/policy.rs"],
        );
        let v = evaluate_commits(&g, &[touch]);
        let warn = v
            .iter()
            .find(|t| t.violation.rule == "git.breaking_watch_paths")
            .expect("a tripwire warn");
        assert_eq!(warn.violation.level, PolicyLevel::Warn);
        let flat: Vec<Violation> = v.into_iter().map(|t| t.violation).collect();
        assert!(!any_blocking(&flat), "the tripwire must never block");

        // A marked commit on the same surface does not warn.
        let marked = commit_with_files(
            "aaaa0002",
            "feat!: change a policy field\n\nBREAKING CHANGE: renamed key",
            &["crates/codeflow-core/src/hooks/policy.rs"],
        );
        let v = evaluate_commits(&g, &[marked]);
        assert!(!v
            .iter()
            .any(|t| t.violation.rule == "git.breaking_watch_paths"));
    }

    #[test]
    fn story_body_and_long_description_block() {
        // ADR-0020: the CI commit-range path inherits the restored standard
        // because it reuses git_hook::commit_msg — a story body blocks at
        // commit_body, an over-long description at commit_format.
        let story = "feat: add a thing\n\nA prose paragraph explaining the whole story here.";
        let v = evaluate_commits(&git(), &[commit("eeee7777", story)]);
        assert!(v.iter().any(|t| t.violation.rule == "git.commit_body"));

        let long = format!("fix: {}", "y".repeat(60));
        let v = evaluate_commits(&git(), &[commit("ffff8888", &long)]);
        assert!(v.iter().any(|t| t.violation.rule == "git.commit_format"
            && t.violation.message.contains("description")));
    }

    #[test]
    fn trailers_strict_by_default_and_opt_in_in_ci() {
        // ADR-0020 amendment: the CI commit path reuses git_hook::commit_msg, so
        // the strict default (a `Refs:` blocks) and the opt-in (once whitelisted,
        // it passes) behave identically to the hook — no drift.
        let strict = evaluate_commits(&git(), &[commit("aaaa1111", "feat: x\n\nRefs: PROJ-1")]);
        assert!(strict.iter().any(|t| t.violation.rule == "git.commit_body"));
        let g = GitPolicy {
            commit_footer_tokens: vec!["Refs".into()],
            ..GitPolicy::default()
        };
        let opted = evaluate_commits(&g, &[commit("bbbb2222", "feat: x\n\nRefs: PROJ-1")]);
        assert!(
            opted.is_empty(),
            "opted-in Refs must pass: {opted:?}",
            opted = opted.len()
        );
    }

    #[test]
    fn ticket_requirement_inherits_in_ci() {
        // keys + required=block → a ticket-less commit blocks in CI too.
        let g = GitPolicy {
            commit_ticket_keys: vec!["Refs".into()],
            commit_ticket_required: PolicyLevel::Block,
            ..GitPolicy::default()
        };
        let missing = evaluate_commits(&g, &[commit("cccc3333", "feat: x\n\n- no ticket")]);
        assert!(missing
            .iter()
            .any(|t| t.violation.rule == "git.commit_ticket"));
        let present = evaluate_commits(&g, &[commit("dddd4444", "feat: x\n\nRefs: PROJ-1")]);
        assert!(!present
            .iter()
            .any(|t| t.violation.rule == "git.commit_ticket"));
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
        assert!(
            !any_blocking(&flat),
            "a warn-level violation must not block"
        );
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
        let v = evaluate_pr_body(
            &git(),
            "Summary\n\nGenerated with [Claude Code](x) \u{2728}",
        );
        assert!(v.iter().any(|x| x.rule == "git.ai_attribution"));
        assert!(v.iter().any(|x| x.rule == "git.commit_emoji"));
    }

    #[test]
    fn pr_body_clean_passes() {
        assert!(evaluate_pr_body(&git(), "Summary of a clean PR body.").is_empty());
    }

    // -- PR-body structure --------------------------------------------------

    /// A body carrying every default-required section with real content.
    const FULL_BODY: &str = "## Summary\n\n- adds a thing\n\n## Changes\n\n- one change\n\n\
                             ## Testing\n\n- cargo test: 12 passed\n";

    fn code_files() -> Vec<String> {
        vec!["src/main.rs".to_string()]
    }

    #[test]
    fn pr_structure_full_body_passes() {
        let v = evaluate_pr_structure(&git(), FULL_BODY, Some(&code_files()));
        assert!(v.is_empty(), "a complete body is clean: {v:?}");
    }

    #[test]
    fn pr_structure_missing_summary_blocks() {
        let body = "## Changes\n\n- one change\n\n## Testing\n\n- ran the tests\n";
        let v = evaluate_pr_structure(&git(), body, Some(&code_files()));
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].rule, "git.pr_sections");
        assert_eq!(v[0].level, PolicyLevel::Block);
        assert!(v[0].message.contains("'## Summary'"), "{}", v[0].message);
        assert!(v[0].message.contains("missing"), "{}", v[0].message);
    }

    #[test]
    fn pr_structure_warn_level_warns_not_blocks() {
        let g = GitPolicy {
            pr_sections: PolicyLevel::Warn,
            ..GitPolicy::default()
        };
        let v = evaluate_pr_structure(
            &g,
            "## Changes\n\n- x\n\n## Testing\n\n- y\n",
            Some(&code_files()),
        );
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].level, PolicyLevel::Warn);
        assert!(!any_blocking(&v), "warn-level structure must not block");
    }

    #[test]
    fn pr_structure_off_skips_everything() {
        let g = GitPolicy {
            pr_sections: PolicyLevel::Off,
            ..GitPolicy::default()
        };
        let bare = "no sections at all\n\n|  |  |\n";
        assert!(evaluate_pr_structure(&g, bare, Some(&code_files())).is_empty());
    }

    #[test]
    fn pr_structure_testing_required_only_for_code_ranges() {
        let body = "## Summary\n\n- docs fix\n\n## Changes\n\n- reword a guide\n";
        // Code in the range → Testing is required, and the finding says why.
        let v = evaluate_pr_structure(&git(), body, Some(&code_files()));
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("'## Testing'"), "{}", v[0].message);
        assert!(v[0].message.contains("touches code"), "{}", v[0].message);
        // Docs-only range → Testing is not required.
        let docs = vec!["docs/guide.md".to_string(), "README.md".to_string()];
        assert!(evaluate_pr_structure(&git(), body, Some(&docs)).is_empty());
        // Unknown range (unresolved) is conservatively code.
        let v = evaluate_pr_structure(&git(), body, None);
        assert_eq!(v.len(), 1, "unknown range must require the code sections");
    }

    #[test]
    fn pr_structure_empty_section_counts_as_missing() {
        // Summary exists but holds only the template's comment and bare bullet.
        let body = "## Summary\n\n<!-- 2-4 bullets, plain words -->\n\n-\n\n\
                    ## Changes\n\n- one change\n\n## Testing\n\n- ran it\n";
        let v = evaluate_pr_structure(&git(), body, Some(&code_files()));
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(
            v[0].message.contains("present but empty"),
            "{}",
            v[0].message
        );
        assert!(v[0].message.contains("'## Summary'"), "{}", v[0].message);
    }

    #[test]
    fn pr_structure_headings_match_case_insensitive_at_depth_2_or_3() {
        let body = "### summary\n\n- x\n\n## CHANGES\n\n- y\n\n## Testing\n\n- z\n";
        assert!(evaluate_pr_structure(&git(), body, Some(&code_files())).is_empty());
        // Depth 4 is not a section heading; depth 1 is a title, not a section.
        let body = "#### Summary\n\n- x\n\n# Changes\n\n- y\n\n## Testing\n\n- z\n";
        let v = evaluate_pr_structure(&git(), body, Some(&code_files()));
        assert_eq!(v.len(), 2, "{v:?}");
    }

    #[test]
    fn pr_structure_placeholders_warn_never_block() {
        let body = format!(
            "{FULL_BODY}\n```text\n(paste the real test summary output here)\n```\n\n\
             | Metric | This PR |\n|---|---|\n|  |  |\n\n- CAP-\n- EPC-\n"
        );
        let v = evaluate_pr_structure(&git(), &body, Some(&code_files()));
        assert_eq!(v.len(), 4, "{v:?}");
        assert!(v.iter().all(|x| x.level == PolicyLevel::Warn));
        assert!(!any_blocking(&v), "placeholders must never block");
        assert!(v.iter().all(|x| x.rule == "git.pr_sections"));
        // Each finding names its line and what the remnant is.
        assert!(
            v.iter().any(|x| x.message.contains("paste-your-output")),
            "{v:?}"
        );
        assert!(v.iter().any(|x| x.message.contains("empty cells")), "{v:?}");
        assert!(
            v.iter()
                .filter(|x| x.message.contains("linked-work"))
                .count()
                == 2,
            "{v:?}"
        );
        assert!(v.iter().all(|x| x.message.contains("line ")), "{v:?}");
    }

    #[test]
    fn pr_structure_table_separator_and_filled_rows_are_not_remnants() {
        let body = format!("{FULL_BODY}\n| Test | What it pins |\n|---|---|\n| a | b |\n");
        assert!(evaluate_pr_structure(&git(), &body, Some(&code_files())).is_empty());
    }

    #[test]
    fn docs_only_path_classification() {
        let docs = |p: &str| is_docs_path(p);
        assert!(docs("README.md"));
        assert!(docs("notes.txt"));
        assert!(docs("LICENSE-MIT"));
        assert!(docs("docs/img/arch.png"));
        assert!(docs(".github/ISSUE_TEMPLATE/bug.yml"));
        assert!(docs(".github/pull_request_template.md"));
        // Code, config, CI yml, Cargo.*, src — all code.
        assert!(!docs("src/lib.rs"));
        assert!(!docs("Cargo.toml"));
        assert!(!docs(".github/workflows/ci.yml"));
        assert!(!docs(".github/dependabot.yml"));
        for executable in [
            "docs/examples/install.sh",
            "docs/site.config.ts",
            "docs/view.html",
            "docs/diagram.svg",
            "docs/.claude/settings.json",
            "LICENSE.rs",
            ".github/pull_request_template.sh",
            ".github/ISSUE_TEMPLATE/helper.py",
        ] {
            assert!(!docs(executable), "executable path: {executable}");
        }
        // Unknown or empty ranges are conservatively code.
        assert!(!docs_only(None));
        assert!(!docs_only(Some(&[])));
        assert!(docs_only(Some(&["docs/a.md".to_string()])));
    }

    #[test]
    fn instruction_and_shipped_markdown_requires_code_evidence() {
        for path in [
            "assets/base/agents/skills/cf-plan/SKILL.md",
            "assets/base/README.md",
            ".agents/skills/cf-plan/SKILL.md",
            ".claude/agents/reviewer.md",
            ".codeflow/.baseline/AGENTS.md",
            "AGENTS.md",
            "CLAUDE.md",
            "packages/web/AGENTS.md",
            "packages/web/.claude/agents/review.md",
            ".codex/instructions.md",
            ".grok/instructions.md",
            "custom-agent/skills/explain/SKILL.md",
        ] {
            let files = vec![path.to_string()];
            assert!(!docs_only(Some(&files)), "behavioral contract: {path}");
            let violations =
                evaluate_pr_structure(&git(), "## Summary\nChange guidance\n", Some(&files));
            assert!(
                violations.iter().any(|v| v.message.contains("Testing")),
                "missing evidence requirement: {path}"
            );
        }
        assert!(is_docs_path("docs/assets/overview.md"));
        assert!(is_docs_path("assets-guide.md"));
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

    /// The default policy's protected branches, as `run()` passes them.
    fn protected() -> Vec<String> {
        GitPolicy::default().protected_branches
    }

    #[test]
    fn detect_range_gitlab() {
        let r = detect_range(
            env_from(&[
                ("CI_MERGE_REQUEST_DIFF_BASE_SHA", "abc123"),
                ("CI_COMMIT_SHA", "def456"),
            ]),
            &protected(),
        );
        assert_eq!(r.base_candidates, vec!["abc123"]);
        assert_eq!(r.head, "def456");
        assert!(r.source.contains("GitLab"));
    }

    #[test]
    fn detect_range_bitbucket() {
        let r = detect_range(
            env_from(&[
                ("BITBUCKET_PR_DESTINATION_COMMIT", "dest99"),
                ("BITBUCKET_COMMIT", "head99"),
            ]),
            &protected(),
        );
        assert_eq!(r.base_candidates, vec!["dest99"]);
        assert_eq!(r.head, "head99");
        assert!(r.source.contains("Bitbucket"));
    }

    #[test]
    fn detect_range_github() {
        let r = detect_range(env_from(&[("GITHUB_BASE_REF", "main")]), &protected());
        assert_eq!(r.base_candidates, vec!["origin/main", "main"]);
        assert_eq!(r.head, "HEAD");
        assert!(r.source.contains("GitHub"));
    }

    #[test]
    fn detect_range_env_default_branch() {
        // CODEFLOW_DEFAULT_BRANCH is the explicit escape hatch for hosts with
        // no recognized CI variables (documented in ci-generic.sh).
        let r = detect_range(
            env_from(&[("CODEFLOW_DEFAULT_BRANCH", "trunk")]),
            &protected(),
        );
        assert_eq!(r.base_candidates, vec!["origin/trunk", "trunk"]);
        assert_eq!(r.head, "HEAD");
        assert!(r.source.contains("CODEFLOW_DEFAULT_BRANCH"));
    }

    #[test]
    fn detect_range_fallback_tries_policy_protected_branches() {
        // The terminal fallback covers a master-default repo too: every
        // literal protected branch is a candidate, not just main.
        let r = detect_range(env_from(&[]), &protected());
        assert_eq!(
            r.base_candidates,
            vec!["origin/main", "main", "origin/master", "master"]
        );
        assert_eq!(r.head, "HEAD");
        assert!(r.source.contains("fallback"));
    }

    #[test]
    fn detect_range_fallback_skips_protected_globs() {
        // A glob cannot name a ref; only literal branches become candidates.
        let prot = vec!["main".to_string(), "release/*".to_string()];
        let r = detect_range(env_from(&[]), &prot);
        assert_eq!(r.base_candidates, vec!["origin/main", "main"]);
    }

    #[test]
    fn detect_range_precedence_gitlab_over_github() {
        // A GitLab MR pipeline can also carry GITHUB_* if mirrored; the exact
        // base sha wins over the branch-name heuristic.
        let r = detect_range(
            env_from(&[
                ("CI_MERGE_REQUEST_DIFF_BASE_SHA", "gl-base"),
                ("GITHUB_BASE_REF", "main"),
            ]),
            &protected(),
        );
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
            detect_branch(env_from(&[(
                "CI_MERGE_REQUEST_SOURCE_BRANCH_NAME",
                "feat/gl"
            )])),
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

    // -- report: honest summary + exit codes -------------------------------

    fn block_violation() -> TaggedViolation {
        TaggedViolation {
            sha: Some("aaaa1111".to_string()),
            violation: Violation::new(
                "git.commit_format",
                PolicyLevel::Block,
                "bad subject".to_string(),
                "fix it".to_string(),
            ),
        }
    }

    #[test]
    fn report_clean_all_ran_exits_zero() {
        assert_eq!(report(&[], &["commit", "branch-naming"], &[]), 0);
    }

    #[test]
    fn report_skipped_commit_checks_exits_two() {
        // A skipped commit check verified nothing — never a clean exit 0.
        assert_eq!(report(&[], &["branch-naming"], &["commit"]), 2);
    }

    #[test]
    fn report_skipped_branch_naming_alone_exits_zero() {
        // A detached-head run without a CI branch variable is legitimate; the
        // skip is named in the summary but is not fatal.
        assert_eq!(report(&[], &["commit"], &["branch-naming"]), 0);
    }

    #[test]
    fn report_blocking_violation_exits_one_even_when_skipped() {
        // A found violation outranks the incomplete-run signal.
        assert_eq!(
            report(&[block_violation()], &["branch-naming"], &["commit"]),
            1
        );
    }
}
