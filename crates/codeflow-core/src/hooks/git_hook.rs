//! Git client hook stages (charter §6.1 plane 1): pre-commit, commit-msg,
//! pre-merge-commit, reference-transaction, pre-push. Harness-agnostic — they
//! work for any agent or human.
//!
//! The shims in `.git/hooks/` exec `codeflow git-hook <stage>`; ALL levels
//! and lists come from `.codeflow/policy.json` (D7/D8 — nothing hardcoded).

use std::path::Path;

use git2::Repository;

use crate::error::HookError;
use crate::testing::gate::{run_gate_exact, GateOutcome, GateTargetResult};

use super::policy::{GitPolicy, PolicyLevel};
use super::repo::current_branch;
use super::scan;
use super::{standards, Violation};

/// Outcome of running one hook stage.
#[derive(Debug, Default)]
pub struct StageReport {
    /// Policy violations (block and warn level).
    pub violations: Vec<Violation>,
    /// Non-violation diagnostics (skipped gates, degraded checks) — printed
    /// so degradation stays legible (charter principle 8).
    pub notes: Vec<String>,
}

const SANCTIONED: &str =
    "land work via PR (gh pr create → merge on evidenced-green checks) or `codeflow integrate <branch> --into <target>`";

// ---------------------------------------------------------------------------
// pre-commit
// ---------------------------------------------------------------------------

/// The pre-commit stage: protected-branch commit check, staged-.env block,
/// and the secret content scan over the staged diff (charter §6.1, §6.3).
///
/// # Errors
///
/// Returns [`HookError::Config`] when `root` is not inside a git repository.
pub fn pre_commit(
    root: &Path,
    policy: &GitPolicy,
    integrate_token: bool,
) -> Result<StageReport, HookError> {
    let repo = Repository::discover(root)
        .map_err(|e| HookError::Config(format!("not a git repository: {e}")))?;
    let mut report = StageReport::default();

    let branch = current_branch(&repo);
    if policy.commit_to_protected.is_active()
        && policy.branch_is_protected(&branch)
        && !integrate_token
    {
        report.violations.push(Violation::new(
            "git.commit_to_protected",
            policy.commit_to_protected,
            format!("commit on protected branch '{branch}'"),
            SANCTIONED.to_string(),
        ));
    }

    if policy.secret_scan.is_active() {
        scan_staged(&repo, policy, &mut report, false);
    }

    Ok(report)
}

/// Scan the staged diff (index vs HEAD) for .env files and secret content.
fn scan_staged(
    repo: &Repository,
    policy: &GitPolicy,
    report: &mut StageReport,
    abort_traversal_for_test: bool,
) {
    let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
    let Ok(diff) = repo.diff_tree_to_index(head_tree.as_ref(), None, None) else {
        report
            .notes
            .push("secret scan skipped: could not read the staged diff".to_string());
        return;
    };

    for delta in diff.deltas() {
        // A staged DELETION is the remediation the policy prescribes (getting
        // a tracked env file OUT of git) — never a leak. Only content entering
        // the repo is scanned; the content scan below is already '+'-lines-only.
        if delta.status() == git2::Delta::Deleted {
            continue;
        }
        let Some(path) = delta.new_file().path() else {
            continue;
        };
        let path_str = path.to_string_lossy();
        if scan::is_env_file(&path_str) {
            report.violations.push(Violation::new(
                "git.secret_scan",
                policy.secret_scan,
                format!("dotenv file staged for commit: {path_str}"),
                "keep env files out of git (.gitignore covers them); commit a .env.example instead"
                    .to_string(),
            ));
        }
    }

    let mut hits: Vec<scan::SecretHit> = Vec::new();
    let traversal = diff.foreach(
        &mut |_, _| true,
        None,
        None,
        Some(&mut |delta, _hunk, line| {
            if abort_traversal_for_test {
                return false;
            }
            if line.origin() == '+' {
                let content = String::from_utf8_lossy(line.content());
                if let Some(pattern) = scan::scan_line(&content) {
                    hits.push(scan::SecretHit {
                        file: delta
                            .new_file()
                            .path()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_default(),
                        line: line.new_lineno().unwrap_or(0),
                        pattern,
                    });
                }
            }
            true
        }),
    );
    if let Err(error) = traversal {
        report.violations.push(Violation::new(
            "git.secret_scan",
            policy.secret_scan,
            format!("staged secret scan incomplete: {error}"),
            "retry the commit after the staged diff can be scanned completely".to_string(),
        ));
        return;
    }
    for hit in hits {
        report.violations.push(Violation::new(
            "git.secret_scan",
            policy.secret_scan,
            format!(
                "possible secret ({}) staged at {}:{}",
                hit.pattern, hit.file, hit.line
            ),
            "remove the secret from the staged content; rotate it if it was ever committed"
                .to_string(),
        ));
    }
}

// ---------------------------------------------------------------------------
// commit-msg
// ---------------------------------------------------------------------------

/// The `commit_format` sub-checks — conventional shape, the subject-length
/// budget (ADR-0020), and the breaking-footer casing — each pushed at the
/// `commit_format` level. Split out to keep [`commit_msg`] readable; only called
/// when `commit_format` is active.
fn push_format_violations(
    policy: &GitPolicy,
    subject: &str,
    cleaned: &str,
    report: &mut StageReport,
) {
    if let Some(reason) = standards::check_commit_format(subject, &policy.commit_types) {
        report.violations.push(Violation::new(
            "git.commit_format",
            policy.commit_format,
            reason,
            format!(
                "use `type(scope): description` with type one of: {}",
                policy.commit_types.join(", ")
            ),
        ));
    }
    if let Some(reason) = standards::check_subject_length(
        subject,
        policy.commit_desc_max_len,
        policy.commit_subject_max_len,
    ) {
        report.violations.push(Violation::new(
            "git.commit_format",
            policy.commit_format,
            reason,
            format!(
                "keep the description ≤ {} chars and the whole subject line ≤ {} chars",
                policy.commit_desc_max_len, policy.commit_subject_max_len
            ),
        ));
    }
    if let Some(reason) = standards::check_breaking_footer(subject, cleaned) {
        report.violations.push(Violation::new(
            "git.commit_format",
            policy.commit_format,
            reason,
            "signal a breaking change with `type!: description` or the exact footer \
             `BREAKING CHANGE:` (uppercase)"
                .to_string(),
        ));
    }
}

/// Where a commit message comes from. It decides what the policy-character
/// rule (ADR-0067) scans. Git hands the commit-msg hook the message file
/// before its own cleanup, so the live hook scans what that cleanup will
/// retain; a message read back from history is committed content, every byte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageSource {
    /// The live commit-msg hook's message file, with the cleanup Git will
    /// still apply to it.
    Pending(GitCleanup),
    /// A message already recorded in history, as `codeflow ci` reads it.
    Committed,
}

/// The live hook's best-effort model of Git's commit-message cleanup, for
/// early feedback. From config and environment it infers whether Git strips
/// comment lines, and it treats Git's full cut signature as the start of a
/// verbose or scissors preview. Both are inferences: a command-line cleanup
/// override can make the stripping wrong, and a message that carries the
/// signature as content is cut although Git keeps the text below it.
/// `codeflow ci` scans the stored message and stays the authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitCleanup {
    /// The exact comment prefix, `None` when Git picks one per process
    /// (`core.commentChar=auto`), so no line is known to be a comment.
    comment: Option<String>,
    /// Git drops every line starting with `comment` (the `strip` mode).
    strip_comments: bool,
}

/// The lines `wt_status_append_cut_line` writes (Git 2.53.0 `wt-status.c`
/// lines 1117 to 1123): the `cut_line` constant (line 42), then the
/// explanation, each line as the comment prefix, one space, the text. Git
/// writes them for a verbose or scissors session, which truncates there; the
/// hook takes a match as that case, a heuristic that a message holding the
/// same lines as content also triggers. The explanation is translated under
/// a non-English locale, where this match fails and the preview is scanned.
const CUT_SIGNATURE: [&str; 3] = [
    "------------------------ >8 ------------------------",
    "Do not modify or remove the line above.",
    "Everything below it will be ignored.",
];

impl GitCleanup {
    /// Infer the cleanup from config and environment. `mode` is the raw
    /// `commit.cleanup` value;
    /// `editor_used` is false when Git told the hook no editor runs
    /// (`GIT_EDITOR=:`); `comment` is the raw value of the last
    /// `core.commentChar` or `core.commentString` setting, `None` when unset.
    ///
    /// Git strips comments for `strip`, and for the default mode only with an
    /// editor (Git 2.53.0 `sequencer.c` `get_cleanup_mode`). Values are used
    /// byte for byte, as Git uses them; `auto` (any case) leaves the prefix
    /// unknown. A command-line `--cleanup` is not visible to the hook and
    /// can make the inferred stripping wrong, with or without an editor.
    #[must_use]
    pub fn resolve(mode: Option<&str>, editor_used: bool, comment: Option<&str>) -> Self {
        let comment = match comment {
            None => Some("#".to_string()),
            Some(value) if value.eq_ignore_ascii_case("auto") || value.is_empty() => None,
            Some(value) => Some(value.to_string()),
        };
        let strip_comments = match mode {
            None | Some("default") => editor_used,
            Some(mode) => mode == "strip",
        };
        Self {
            comment,
            strip_comments,
        }
    }

    /// The cleanup of an interactive `git commit` under default settings:
    /// `#` comment lines are dropped, and so is a verbose diff preview below
    /// Git's cut signature.
    #[must_use]
    pub fn editor_default() -> Self {
        Self::resolve(None, true, None)
    }

    /// The lines of `message` that Git may record under this cleanup.
    #[must_use]
    pub fn retained(&self, message: &str) -> String {
        let Some(prefix) = self.comment.as_deref() else {
            return message.to_string();
        };
        let signature: Vec<String> = CUT_SIGNATURE
            .iter()
            .map(|text| format!("{prefix} {text}"))
            .collect();
        let lines: Vec<&str> = message.split('\n').collect();
        let mut out = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            let rest = &lines[i..];
            if rest.len() >= signature.len() && rest.iter().zip(&signature).all(|(l, s)| l == s) {
                break;
            }
            if self.strip_comments && line.starts_with(prefix) {
                continue;
            }
            out.push(*line);
        }
        out.join("\n")
    }
}

/// The commit-msg stage: conventional format (whitelisted types), the restored
/// v1 subject-length budget and body-shape rule (ADR-0020), standard git-trailer
/// footers with an optional ticket requirement, the no-AI-attribution rule, the
/// no-emoji rule (charter §6.4, AC #13), and the policy-character rule over the
/// subject and body (ADR-0067). The message is an interactive editor template
/// under Git's default cleanup; the live hook passes its resolved cleanup
/// through [`commit_msg_with_files`].
#[must_use]
pub fn commit_msg(policy: &GitPolicy, message: &str, is_merge: bool) -> StageReport {
    commit_msg_from(
        policy,
        message,
        is_merge,
        &MessageSource::Pending(GitCleanup::editor_default()),
    )
}

/// [`commit_msg`] for a message from `source`. The policy-character rule runs
/// before the merge exemption: ADR-0067 names commit subjects and bodies with
/// no merge carve-out, so only the conventional-format rules skip a merge.
#[must_use]
pub fn commit_msg_from(
    policy: &GitPolicy,
    message: &str,
    is_merge: bool,
    source: &MessageSource,
) -> StageReport {
    let mut report = StageReport::default();
    report
        .violations
        .extend(policy_characters_in_message(policy, message, source));
    let cleaned = strip_commit_comments(message);
    let Some(subject) = cleaned.lines().find(|l| !l.trim().is_empty()) else {
        return report; // git rejects empty messages itself
    };

    // A genuine merge commit carries a git-authored subject/body — exempt it
    // ONCE, structurally (is_merge is decided from MERGE_HEAD, not the subject
    // text), so a one-parent commit named `Merge ...` is still fully checked.
    if is_merge && subject.starts_with("Merge ") {
        return report;
    }

    if policy.commit_format.is_active() {
        push_format_violations(policy, subject, &cleaned, &mut report);
    }

    if policy.commit_body.is_active() {
        if let Some(reason) = standards::check_commit_body(
            subject,
            &cleaned,
            policy.commit_body_max_bullets,
            policy.commit_body_bullet_max_len,
            &policy.allowed_footer_tokens(),
        ) {
            report.violations.push(Violation::new(
                "git.commit_body",
                policy.commit_body,
                reason,
                format!(
                    "the body is `- ` bullets (max {}, each ≤ {} chars) and an optional \
                     `BREAKING CHANGE:` footer; blank lines are fine, prose paragraphs are not. \
                     Other trailers are allowed only when opted in via git.commit_footer_tokens",
                    policy.commit_body_max_bullets, policy.commit_body_bullet_max_len
                ),
            ));
        }
        if let Some(reason) =
            standards::check_required_footers(subject, &cleaned, &policy.commit_required_footers)
        {
            report.violations.push(Violation::new(
                "git.commit_body",
                policy.commit_body,
                reason,
                format!(
                    "every commit must carry these footer trailers: {}",
                    policy.commit_required_footers.join(", ")
                ),
            ));
        }
    }

    // Ticket allow-and-require (ADR-0020). Active only when the project has
    // configured ticket keys; the returned level distinguishes a required-but-
    // missing ticket (`commit_ticket_required`) from a malformed present one
    // (block).
    if !policy.commit_ticket_keys.is_empty() {
        if let Some((level, reason)) = standards::check_commit_ticket(
            subject,
            &cleaned,
            &policy.commit_ticket_keys,
            policy.commit_ticket_required,
            &policy.commit_ticket_pattern,
        ) {
            report.violations.push(Violation::new(
                "git.commit_ticket",
                level,
                reason,
                format!(
                    "add a ticket-reference footer trailer (key one of: {})",
                    policy.commit_ticket_keys.join(", ")
                ),
            ));
        }
    }

    if policy.ai_attribution.is_active() {
        if let Some(which) = standards::find_attribution(&cleaned) {
            report.violations.push(Violation::new(
                "git.ai_attribution",
                policy.ai_attribution,
                format!("commit message contains AI attribution ({which})"),
                "remove it — project policy forbids AI attribution in commits and PR bodies (charter §6.4)"
                    .to_string(),
            ));
        }
    }

    if policy.commit_emoji.is_active() {
        if let Some(c) = standards::find_emoji(subject) {
            report.violations.push(Violation::new(
                "git.commit_emoji",
                policy.commit_emoji,
                format!("commit subject contains emoji ('{c}')"),
                "remove emoji from the commit subject (charter §6.4)".to_string(),
            ));
        }
    }

    report
}

/// The policy-character rule (ADR-0067) over one commit message, `None` when
/// the rule is off or the message is clean. A pending message is scanned as
/// Git's cleanup will record it; a committed message is scanned as stored,
/// so a retained `#` line or scissors-like text is still examined.
#[must_use]
pub fn policy_characters_in_message(
    policy: &GitPolicy,
    message: &str,
    source: &MessageSource,
) -> Option<Violation> {
    if !policy.policy_characters.is_active() {
        return None;
    }
    match source {
        MessageSource::Pending(cleanup) => {
            policy_character_violation(policy, &cleanup.retained(message))
        }
        MessageSource::Committed => policy_character_violation(policy, message),
    }
}

/// The policy-character rule over the scanned text: the finding says whether
/// the subject or which message line carries the character.
fn policy_character_violation(policy: &GitPolicy, scanned: &str) -> Option<Violation> {
    let (line, c) = standards::find_policy_character(scanned)?;
    let name = standards::policy_character_name(c);
    let subject_line = scanned
        .lines()
        .position(|l| !l.trim().is_empty())
        .map(|i| i + 1);
    let place = if subject_line == Some(line) {
        "commit subject".to_string()
    } else {
        format!("commit message line {line}")
    };
    Some(Violation::new(
        "git.policy_characters",
        policy.policy_characters,
        format!("{place} contains an {name}"),
        standards::POLICY_CHARACTER_FIX.to_string(),
    ))
}

/// The commit-msg stage plus the file-aware contract-surface tripwire
/// (ADR-0020). `changed_files` is the set of paths the commit touches — the
/// staged files at the git hook, a commit's own diff in `codeflow ci`. When
/// `git.breaking_watch_paths` is non-empty and one of those files matches a
/// watched glob while the message carries no breaking marker (`type!:` or a
/// `BREAKING CHANGE:` footer), a WARN is added — never a block, because
/// breaking-ness is semantic and unprovable; the warn only nudges.
///
/// [`commit_msg`] remains the message-only entry (callers with no file context
/// use it); this is the richer entry the enforcement planes call.
#[must_use]
pub fn commit_msg_with_files(
    policy: &GitPolicy,
    message: &str,
    changed_files: &[String],
    is_merge: bool,
    source: &MessageSource,
) -> StageReport {
    let mut report = commit_msg_from(policy, message, is_merge, source);
    if policy.breaking_watch_paths.is_empty() {
        return report;
    }
    let cleaned = strip_commit_comments(message);
    let Some(subject) = cleaned.lines().find(|l| !l.trim().is_empty()) else {
        return report;
    };
    if standards::breaking_marker_present(subject, &cleaned) {
        return report; // already marked breaking — no nudge needed
    }
    if let Some(path) = standards::first_watched_path(changed_files, &policy.breaking_watch_paths) {
        report.violations.push(Violation::new(
            "git.breaking_watch_paths",
            PolicyLevel::Warn,
            format!("commit touches a declared contract surface ({path})"),
            "confirm it is not a breaking change, or mark it with `type!:` and a \
             `BREAKING CHANGE:` footer with the migration path"
                .to_string(),
        ));
    }
    report
}

/// Drop the verbose-commit scissors section and `#` comment lines.
fn strip_commit_comments(message: &str) -> String {
    let mut out = Vec::new();
    for line in message.lines() {
        if line.starts_with('#') {
            if line.contains(">8") {
                break; // scissors: everything below is the diff preview
            }
            continue;
        }
        out.push(line);
    }
    out.join("\n")
}

// ---------------------------------------------------------------------------
// pre-merge-commit
// ---------------------------------------------------------------------------

/// The pre-merge-commit stage (charter §6.1 plane 1): git fires this hook
/// just before recording a **non-fast-forward** merge commit. When the branch
/// being merged into is protected, block per `merge_to_protected` — unless the
/// `codeflow integrate` gate token or a human's
/// [`HUMAN_OVERRIDE_ENV`](super::HUMAN_OVERRIDE_ENV) is
/// present (both sanctioned paths, ADR-0007).
///
/// HONEST BOUNDARY: git fires **no** client hook for a fast-forward merge
/// (no merge commit is created), so a ff-merge onto a protected branch cannot
/// be caught here — the Claude-layer `git-guard` catches it in-session, and the
/// push gate + remote protection remain the perimeter for other harnesses
/// (charter D19). See `tests::ff_merge_fires_no_client_hook` and ADR-0007.
///
/// # Errors
///
/// Returns [`HookError::Config`] when `root` is not inside a git repository.
pub fn pre_merge_commit(
    root: &Path,
    policy: &GitPolicy,
    integrate_token: bool,
    human_override: bool,
) -> Result<StageReport, HookError> {
    let repo = Repository::discover(root)
        .map_err(|e| HookError::Config(format!("not a git repository: {e}")))?;
    let mut report = StageReport::default();

    let branch = current_branch(&repo);
    if policy.merge_to_protected.is_active()
        && policy.branch_is_protected(&branch)
        && !integrate_token
        && !human_override
    {
        report.violations.push(Violation::new(
            "git.merge_to_protected",
            policy.merge_to_protected,
            format!("merge commit on protected branch '{branch}'"),
            SANCTIONED.to_string(),
        ));
    }

    Ok(report)
}

// ---------------------------------------------------------------------------
// reference-transaction
// ---------------------------------------------------------------------------

/// `true` when a `reference-transaction` stdin line updates a local branch
/// (`refs/heads/…`). The cheap first-pass filter: remote-tracking refs, tags,
/// stash, and `HEAD` are out of scope, so a fetch touching hundreds of
/// `refs/remotes/…` never reaches policy evaluation.
#[must_use]
pub fn ref_line_touches_local_branch(line: &str) -> bool {
    line.split_whitespace()
        .nth(2)
        .is_some_and(|r| r.starts_with("refs/heads/"))
}

/// The reference-transaction stage (charter §6.1 plane 1; ADR-0007): the
/// harness-agnostic backstop that classic client hooks miss. Git calls it
/// during ref updates; on the `prepared` state a non-zero exit cancels the
/// whole transaction.
///
/// For each `refs/heads/<protected>` update it enforces `local_ref_protection`
/// (and `delete_protected` for deletions). Only a move to EXACTLY the
/// `refs/remotes/origin/<branch>` head passes — the `git pull` fast-forward,
/// the one legitimate sync; any other local move (a fast-forward merge,
/// `reset --hard`, a move merely behind origin, a local commit not yet on the
/// remote) blocks. The remote-tracking ref is agent-writable, so it is not a
/// full authorization oracle (ADR-0009): the git-guard forbids writing it, and
/// remote+CI remain the boundary. The integrate gate token and the human
/// override pass.
///
/// Git < 2.28 never invokes this hook — the protection then degrades to the
/// other planes, gracefully and silently (charter principle 8).
///
/// # Errors
///
/// Returns [`HookError::Config`] when `root` is not inside a git repository.
pub fn reference_transaction(
    root: &Path,
    policy: &GitPolicy,
    stdin: &str,
    integrate_token: bool,
    human_override: bool,
) -> Result<StageReport, HookError> {
    let mut report = StageReport::default();
    if !policy.local_ref_protection.is_active() && !policy.delete_protected.is_active() {
        return Ok(report); // nothing this stage enforces is on
    }

    let repo = Repository::discover(root)
        .map_err(|e| HookError::Config(format!("not a git repository: {e}")))?;
    // The integrate token and a human's override sanction the transaction
    // (git layer only — the git-guard never trusts the human override).
    let sanctioned = integrate_token || human_override;

    for line in stdin.lines() {
        let Some((_old_oid, new_oid, refname)) = parse_ref_line(line) else {
            continue;
        };
        let Some(branch) = refname.strip_prefix("refs/heads/") else {
            continue;
        };
        if !policy.branch_is_protected(branch) {
            continue; // feature branches stay fully free (rebase, force, etc.)
        }

        // Deletion (new-oid all zeros): governed by delete_protected, which
        // never had a client-hook reach before (`git branch -D` fired nothing).
        if is_zero_sha(new_oid) {
            if policy.delete_protected.is_active() && !sanctioned {
                report.violations.push(Violation::new(
                    "git.delete_protected",
                    policy.delete_protected,
                    format!("deleting protected branch '{branch}' via a local ref update"),
                    "protected branches are never deleted; remove the entry from git.protected_branches first if truly intended".to_string(),
                ));
            }
            continue;
        }

        if !policy.local_ref_protection.is_active() || sanctioned {
            continue;
        }
        // Allow the one legitimate sync: the new tip is EXACTLY the
        // remote-tracking head (a `git pull` fast-forward lands there). Any
        // other local move — including one merely *behind* origin — is not a
        // sync and blocks. The narrower rule (exact match, no ancestor
        // allowance) shrinks reliance on the agent-writable `refs/remotes` ref,
        // which the git-guard additionally forbids writing (ADR-0009); the
        // residual (an off-Claude agent that both writes refs/remotes and
        // fast-forwards onto it) is why remote+CI is the boundary (charter D19).
        if new_matches_remote_head(&repo, branch, new_oid) {
            continue;
        }
        report.violations.push(Violation::new(
            "git.local_ref_protection",
            policy.local_ref_protection,
            format!("local update of protected branch '{branch}' that is not a sync from origin"),
            SANCTIONED.to_string(),
        ));
    }
    Ok(report)
}

/// Parse one `<old-oid> <new-oid> <ref-name>` reference-transaction line.
fn parse_ref_line(line: &str) -> Option<(&str, &str, &str)> {
    let mut parts = line.split_whitespace();
    let old = parts.next()?;
    let new = parts.next()?;
    let refname = parts.next()?;
    Some((old, new, refname))
}

/// `true` when `new_oid` equals the branch's remote-tracking head exactly — the
/// state a `git pull` fast-forward produces. The prior "or an ancestor of it"
/// allowance is dropped (ADR-0009): it was not needed for `git pull` (which
/// lands on the tip, not behind it) and only widened trust in the
/// agent-writable `refs/remotes` ref. The candidate ref is the branch's
/// *configured* upstream when set (so `git pull upstream main` on a
/// non-origin-tracked branch is not a false positive), falling back to
/// `refs/remotes/origin/<branch>`. Absent ref or an unparsable oid means
/// "cannot prove a sync" — the caller then blocks.
fn new_matches_remote_head(repo: &Repository, branch: &str, new_oid: &str) -> bool {
    let Ok(new) = git2::Oid::from_str(new_oid) else {
        return false;
    };
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(upstream) = repo.branch_upstream_name(&format!("refs/heads/{branch}")) {
        if let Ok(name) = upstream.as_str() {
            candidates.push(name.to_string());
        }
    }
    candidates.push(format!("refs/remotes/origin/{branch}"));
    candidates.iter().any(|refname| {
        repo.find_reference(refname)
            .ok()
            .and_then(|r| r.target())
            .is_some_and(|remote_oid| new == remote_oid)
    })
}

// ---------------------------------------------------------------------------
// pre-push
// ---------------------------------------------------------------------------

/// One `<local ref> <local sha> <remote ref> <remote sha>` line from the
/// pre-push stdin contract.
#[derive(Debug, Clone)]
pub struct PushRef {
    pub local_ref: String,
    pub local_sha: String,
    pub remote_ref: String,
    pub remote_sha: String,
}

impl PushRef {
    /// `true` when this ref update deletes the remote branch.
    #[must_use]
    pub fn is_delete(&self) -> bool {
        self.local_ref == "(delete)" || is_zero_sha(&self.local_sha)
    }

    /// Remote branch name, when this updates a branch (not a tag).
    #[must_use]
    pub fn remote_branch(&self) -> Option<&str> {
        self.remote_ref.strip_prefix("refs/heads/")
    }
}

fn is_zero_sha(sha: &str) -> bool {
    !sha.is_empty() && sha.chars().all(|c| c == '0')
}

/// Parse the pre-push stdin lines.
#[must_use]
pub fn parse_push_refs(input: &str) -> Vec<PushRef> {
    input
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            Some(PushRef {
                local_ref: parts.next()?.to_string(),
                local_sha: parts.next()?.to_string(),
                remote_ref: parts.next()?.to_string(),
                remote_sha: parts.next()?.to_string(),
            })
        })
        .collect()
}

/// The pre-push stage: protected-branch push/delete/force checks, branch
/// naming against `branch_prefixes`. The push set runs separately.
///
/// # Errors
///
/// Returns [`HookError::Config`] when `root` is not inside a git repository.
pub fn pre_push(
    root: &Path,
    policy: &GitPolicy,
    refs: &[PushRef],
    integrate_token: bool,
) -> Result<StageReport, HookError> {
    let repo = Repository::discover(root)
        .map_err(|e| HookError::Config(format!("not a git repository: {e}")))?;
    let mut report = StageReport::default();

    for r in refs {
        let Some(branch) = r.remote_branch() else {
            continue; // tags and other refs are out of scope
        };
        if branch == crate::ids::REGISTRY_BRANCH {
            // The registry is a data branch with its own profile (SPC-013
            // R-6): no branch naming, no test gate, and the append-only rule.
            registry_push(root, &repo, r, &mut report);
            continue;
        }
        let protected = policy.branch_is_protected(branch);

        if r.is_delete() {
            if protected && policy.delete_protected.is_active() {
                report.violations.push(Violation::new(
                    "git.delete_protected",
                    policy.delete_protected,
                    format!("push would delete protected branch '{branch}'"),
                    "protected branches are never deleted remotely; adjust git.protected_branches first if truly intended".to_string(),
                ));
            }
            continue;
        }

        if protected && policy.push_to_protected.is_active() && !integrate_token {
            report.violations.push(Violation::new(
                "git.push_to_protected",
                policy.push_to_protected,
                format!("direct push to protected branch '{branch}'"),
                SANCTIONED.to_string(),
            ));
        }

        if is_force_update(&repo, r) {
            if protected {
                if policy.force_push_protected.is_active() {
                    report.violations.push(Violation::new(
                        "git.force_push_protected",
                        policy.force_push_protected,
                        format!("non-fast-forward (force) push to protected branch '{branch}'"),
                        SANCTIONED.to_string(),
                    ));
                }
            } else if policy.force_push_unprotected.is_active() {
                report.violations.push(Violation::new(
                    "git.force_push_unprotected",
                    policy.force_push_unprotected,
                    format!("non-fast-forward (force) push to branch '{branch}'"),
                    "policy git.force_push_unprotected restricts force-pushes in this repo"
                        .to_string(),
                ));
            }
        }

        if !protected && policy.branch_naming.is_active() && !policy.branch_name_ok(branch) {
            report.violations.push(Violation::new(
                "git.branch_naming",
                policy.branch_naming,
                format!("branch '{branch}' does not match `{{prefix}}/{{kebab-name}}`"),
                format!(
                    "rename with a sanctioned prefix: {}",
                    policy.branch_prefixes.join(" ")
                ),
            ));
        }
    }

    // The push set (`test_gate_on_push`) is run by the CLI hook, which binds
    // each check to the pushed commits: see `run_push_targets`.

    Ok(report)
}

/// The registry's push rule (SPC-013 R-8, R-108, R-109): never deleted or
/// force-pushed, and every commit in the pushed range adds new `ids/` files
/// only or is a typed restore. It blocks whatever the policy says, and a
/// range that cannot be read blocks too.
fn registry_push(root: &Path, repo: &Repository, r: &PushRef, report: &mut StageReport) {
    let block = |message: String| {
        Violation::new(
            "registry.append_only",
            PolicyLevel::Block,
            message,
            "issue ids with `codeflow task|epic|spec new`; repair damage with `codeflow ids restore <id>...`".to_string(),
        )
    };
    if r.is_delete() {
        report.violations.push(block(
            "push would delete `codeflow/registry`; the registry only grows (R-8)".to_string(),
        ));
        return;
    }
    if is_force_update(repo, r) {
        report.violations.push(block(
            "non-fast-forward (force) push to `codeflow/registry`; the registry only grows (R-8)"
                .to_string(),
        ));
        return;
    }
    let git = crate::ids::Git::new(root);
    let exclude =
        (!is_zero_sha(&r.remote_sha) && !r.remote_sha.is_empty()).then_some(r.remote_sha.as_str());
    let findings = crate::ids::Ledger::read(&git, &r.local_sha)
        .and_then(|ledger| ledger.range_violations(&git, exclude));
    match findings {
        Ok(findings) => {
            for finding in findings {
                report.violations.push(block(format!(
                    "registry commit {}: {}",
                    crate::ids::ledger::short(&finding.commit),
                    finding.message
                )));
            }
        }
        Err(error) => report.violations.push(block(format!(
            "cannot read the pushed registry range: {error}"
        ))),
    }
}

/// `true` when the remote ref exists and the local sha does not descend from
/// it (a history rewrite). Unknown objects (e.g. shallow clones) skip the
/// check rather than guessing.
fn is_force_update(repo: &Repository, r: &PushRef) -> bool {
    if is_zero_sha(&r.remote_sha) || r.remote_sha.is_empty() {
        return false; // new branch on the remote
    }
    let (Ok(local), Ok(remote)) = (
        git2::Oid::from_str(&r.local_sha),
        git2::Oid::from_str(&r.remote_sha),
    ) else {
        return false;
    };
    if local == remote {
        return false;
    }
    match repo.graph_descendant_of(local, remote) {
        Ok(descends) => !descends,
        Err(_) => false, // remote sha unknown locally — cannot judge
    }
}

/// Time budget for the whole pre-push set. A push set that takes longer still
/// runs to its verdict, but the hook names the slowest step (TSK-132).
pub const PUSH_SET_BUDGET: std::time::Duration = std::time::Duration::from_secs(60);

/// One timed step of the push set.
#[derive(Debug, Clone)]
pub struct PushStep {
    /// A test-config target name, or the built-in command.
    pub name: String,
    pub duration: std::time::Duration,
    /// `true` for a test-config target, which its `modes.quick` key moves
    /// out of the push set; `false` for a built-in check.
    pub configurable: bool,
}

/// Run the test-config targets that define a `quick` mode, and only those
/// (charter §6.1 `test_gate_on_push`). The test suite belongs to the full
/// gate, so there is no `quick` to `essential` alias here. A missing config or
/// an empty set is a loud note, never a violation. Returns each target's
/// timing for the push-set budget.
pub fn run_push_targets(
    root: &Path,
    policy: &GitPolicy,
    report: &mut StageReport,
) -> Vec<PushStep> {
    let cfg_path = root.join(".codeflow").join("test-config.json");
    if !cfg_path.exists() {
        report.notes.push(
            "quick targets skipped: no .codeflow/test-config.json (run /cf-stack to add a \
             stack, or create .codeflow/test-config.json)"
                .to_string(),
        );
        return Vec::new();
    }
    match run_gate_exact(root, "quick") {
        Ok(GateOutcome::NoTargets { reason }) => {
            report.notes.push(format!(
                "quick targets skipped: {reason} (the push set runs the targets with a \
                 `quick` mode in .codeflow/test-config.json)"
            ));
            Vec::new()
        }
        Ok(GateOutcome::Completed {
            results, passed, ..
        }) => {
            if passed {
                report.notes.push(format!(
                    "quick targets passed on the working checkout ({} target(s); untracked \
                     files there can influence them, CI checks the pushed commit)",
                    results.len()
                ));
            } else {
                let failed: Vec<String> = results
                    .iter()
                    .filter(|r| !r.passed())
                    .map(|r| r.name.clone())
                    .collect();
                report.violations.push(Violation::new(
                    "git.test_gate_on_push",
                    policy.test_gate_on_push,
                    format!("push set failed for: {}", failed.join(", ")),
                    "fix the failing target(s), or run `codeflow test --mode quick` to reproduce"
                        .to_string(),
                ));
            }
            results.iter().map(target_step).collect()
        }
        Err(e) => {
            report.violations.push(Violation::new(
                "git.test_gate_on_push",
                policy.test_gate_on_push,
                format!("push set could not load test-config.json: {e}"),
                "repair .codeflow/test-config.json, then run `codeflow test --mode quick`"
                    .to_string(),
            ));
            Vec::new()
        }
    }
}

fn target_step(result: &GateTargetResult) -> PushStep {
    PushStep {
        name: result.name.clone(),
        duration: std::time::Duration::from_millis(result.duration_ms),
        configurable: true,
    }
}

/// When the whole push set took longer than `budget`, the note naming its
/// slowest step and what moves it: the `modes.quick` key for a test-config
/// target, or a pointer to run a built-in check alone.
#[must_use]
pub fn over_budget_note(
    steps: &[PushStep],
    total: std::time::Duration,
    budget: std::time::Duration,
) -> Option<String> {
    if total <= budget {
        return None;
    }
    let head = format!(
        "push set took {:.1}s, over its {}s budget",
        total.as_secs_f64(),
        budget.as_secs()
    );
    Some(match steps.iter().max_by_key(|s| s.duration) {
        Some(slow) if slow.configurable => format!(
            "{head}; slowest step: target '{}' ({:.1}s): remove its `modes.quick` in \
             .codeflow/test-config.json to move it to the full gate",
            slow.name,
            slow.duration.as_secs_f64()
        ),
        Some(slow) => format!(
            "{head}; slowest step: built-in `{}` ({:.1}s), which has no push-set key: \
             run it alone to see what makes it slow",
            slow.name,
            slow.duration.as_secs_f64()
        ),
        None => head,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::super::policy::PolicyLevel;
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn init_repo(dir: &Path, branch: &str) {
        git(dir, &["init", "-b", branch]);
        git(dir, &["config", "user.email", "t@example.com"]);
        git(dir, &["config", "user.name", "t"]);
        std::fs::write(dir.join("base.txt"), "base\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-m", "chore: init"]);
    }

    fn stage(dir: &Path, file: &str, content: &str) {
        let path = dir.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, content).unwrap();
        git(dir, &["add", file]);
    }

    fn release_policy() -> GitPolicy {
        GitPolicy {
            protected_branches: vec!["main".into(), "master".into(), "release/*".into()],
            ..GitPolicy::default()
        }
    }

    // -- pre-commit --

    #[test]
    fn test_pre_commit_clean_on_feature_branch() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        stage(dir.path(), "src/lib.rs", "pub fn hello() {}\n");
        let report = pre_commit(dir.path(), &GitPolicy::default(), false).unwrap();
        assert!(report.violations.is_empty(), "{:?}", report.violations);
    }

    #[test]
    fn test_secret_scan_traversal_error_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        stage(dir.path(), "src/lib.rs", "pub fn changed() {}\n");
        let repo = Repository::open(dir.path()).unwrap();
        let policy = GitPolicy::default();
        let mut report = StageReport::default();

        scan_staged(&repo, &policy, &mut report, true);

        assert!(report.violations.iter().any(|violation| {
            violation.rule == "git.secret_scan"
                && violation.message.contains("scan incomplete")
                && violation.level == PolicyLevel::Block
        }));
    }

    #[test]
    fn test_pre_commit_blocks_on_protected_branch() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        stage(dir.path(), "a.txt", "hello\n");
        let report = pre_commit(dir.path(), &GitPolicy::default(), false).unwrap();
        assert_eq!(report.violations.len(), 1);
        assert_eq!(report.violations[0].rule, "git.commit_to_protected");
        assert_eq!(report.violations[0].level, PolicyLevel::Block);
    }

    #[test]
    fn test_pre_commit_integrate_token_passes_protected() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        stage(dir.path(), "a.txt", "hello\n");
        let report = pre_commit(dir.path(), &GitPolicy::default(), true).unwrap();
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_pre_commit_warn_level_on_protected() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let policy = GitPolicy {
            commit_to_protected: PolicyLevel::Warn,
            ..GitPolicy::default()
        };
        let report = pre_commit(dir.path(), &policy, false).unwrap();
        assert_eq!(report.violations[0].level, PolicyLevel::Warn);
    }

    #[test]
    fn test_pre_commit_release_glob_extension() {
        // AC #3: release/* added to policy is honored by this plane too.
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "release/2.0");
        let report = pre_commit(dir.path(), &release_policy(), false).unwrap();
        assert_eq!(report.violations[0].rule, "git.commit_to_protected");
    }

    #[test]
    fn test_pre_commit_blocks_staged_env_file() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        stage(dir.path(), ".env", "DB_PASSWORD=hunter2hunter2\n");
        let report = pre_commit(dir.path(), &GitPolicy::default(), false).unwrap();
        assert!(report
            .violations
            .iter()
            .any(|v| v.rule == "git.secret_scan" && v.message.contains(".env")));
    }

    #[test]
    fn test_pre_commit_allows_staged_env_file_deletion() {
        // A dotenv file already tracked (committed before codeflow adoption):
        // ADDING it blocks, but staging its DELETION is the very remediation
        // the policy prescribes and must pass.
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        stage(dir.path(), ".env", "DB_PASSWORD=hunter2hunter2\n");
        let report = pre_commit(dir.path(), &GitPolicy::default(), false).unwrap();
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.rule == "git.secret_scan"),
            "adding a dotenv file still blocks"
        );
        git(
            dir.path(),
            &["commit", "-m", "chore: pre-adoption env file"],
        );
        git(dir.path(), &["rm", ".env"]);
        let report = pre_commit(dir.path(), &GitPolicy::default(), false).unwrap();
        assert!(
            report.violations.is_empty(),
            "deleting a tracked dotenv file must pass: {:?}",
            report.violations
        );
    }

    #[test]
    fn test_pre_commit_allows_env_example() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        stage(dir.path(), ".env.example", "DB_PASSWORD=\n");
        let report = pre_commit(dir.path(), &GitPolicy::default(), false).unwrap();
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_pre_commit_blocks_staged_secret_content() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        stage(
            dir.path(),
            "src/config.rs",
            "let key = \"AKIAIOSFODNN7EXAMPLF\";\n",
        );
        let report = pre_commit(dir.path(), &GitPolicy::default(), false).unwrap();
        assert_eq!(report.violations.len(), 1);
        assert_eq!(report.violations[0].rule, "git.secret_scan");
        assert!(report.violations[0].message.contains("src/config.rs"));
        assert!(report.violations[0].message.contains("AWS access key id"));
    }

    #[test]
    fn test_pre_commit_secret_scan_off_skips() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        stage(dir.path(), ".env", "X=1\n");
        let policy = GitPolicy {
            secret_scan: PolicyLevel::Off,
            ..GitPolicy::default()
        };
        let report = pre_commit(dir.path(), &policy, false).unwrap();
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_pre_commit_outside_repo_is_config_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(pre_commit(dir.path(), &GitPolicy::default(), false).is_err());
    }

    // -- commit-msg --

    #[test]
    fn test_commit_msg_valid() {
        let report = commit_msg(&GitPolicy::default(), "feat(hooks): add the guard\n", false);
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_commit_msg_exact_breaking_footer_ok() {
        let report = commit_msg(
            &GitPolicy::default(),
            "feat: new api\n\nBREAKING CHANGE: removes the old one\n",
            false,
        );
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_commit_msg_miscased_breaking_footer_blocked() {
        // a lowercase breaking footer would ship a major change as a minor bump
        let report = commit_msg(
            &GitPolicy::default(),
            "feat: new api\n\nbreaking change: removes the old one\n",
            false,
        );
        assert!(report
            .violations
            .iter()
            .any(|v| v.rule == "git.commit_format"));
    }

    #[test]
    fn test_commit_msg_malformed_subject_blocked() {
        let report = commit_msg(&GitPolicy::default(), "Added stuff\n", false);
        assert_eq!(report.violations[0].rule, "git.commit_format");
        assert_eq!(report.violations[0].level, PolicyLevel::Block);
    }

    #[test]
    fn test_commit_msg_attribution_blocked() {
        // AC #13: attribution trailer blocked at commit-msg.
        let msg = "feat: x\n\nCo-Authored-By: Claude Fable 5 <noreply@anthropic.com>\n";
        let report = commit_msg(&GitPolicy::default(), msg, false);
        assert!(report
            .violations
            .iter()
            .any(|v| v.rule == "git.ai_attribution"));
    }

    #[test]
    fn test_commit_msg_emoji_blocked() {
        let report = commit_msg(&GitPolicy::default(), "feat: ship \u{1F680}\n", false);
        assert!(report
            .violations
            .iter()
            .any(|v| v.rule == "git.commit_emoji"));
    }

    #[test]
    fn test_commit_msg_policy_character_in_subject_warns_by_default() {
        let report = commit_msg(&GitPolicy::default(), "feat: a \u{2014} b\n", false);
        let v = report
            .violations
            .iter()
            .find(|v| v.rule == "git.policy_characters")
            .expect("a policy_characters violation");
        // A writing guideline by default: reported, never a refusal.
        assert_eq!(v.level, PolicyLevel::Warn);
        assert!(v.message.contains("commit subject"), "{}", v.message);
        assert!(v.message.contains("em dash (U+2014)"), "{}", v.message);
        assert!(v.remedy.contains("a comma, colon"), "{}", v.remedy);
    }

    #[test]
    fn test_commit_msg_policy_character_in_body_blocked() {
        let msg = "feat: add ranges\n\n- pages 1\u{2013}3\n";
        let policy = GitPolicy {
            policy_characters: PolicyLevel::Block,
            ..GitPolicy::default()
        };
        let report = commit_msg(&policy, msg, false);
        let v = report
            .violations
            .iter()
            .find(|v| v.rule == "git.policy_characters")
            .expect("a policy_characters violation");
        assert_eq!(v.level, PolicyLevel::Block);
        assert!(v.message.contains("line 3"), "{}", v.message);
        assert!(v.message.contains("en dash (U+2013)"), "{}", v.message);
    }

    #[test]
    fn test_commit_msg_hyphen_passes_policy_characters() {
        let msg = "feat: add re-run flag\n\n- pages 1 to 3, a - b\n";
        let report = commit_msg(&GitPolicy::default(), msg, false);
        assert!(
            !report
                .violations
                .iter()
                .any(|v| v.rule == "git.policy_characters"),
            "{:?}",
            report.violations
        );
    }

    fn has_policy_character(report: &StageReport) -> bool {
        report
            .violations
            .iter()
            .any(|v| v.rule == "git.policy_characters")
    }

    // Codex EPC-017 review, finding 3: a committed message keeps a `#` body
    // line that git's `-m` path retains, so history is scanned as stored.
    #[test]
    fn test_committed_hash_body_line_is_scanned_for_policy_characters() {
        let msg = "feat: add ranges\n\n# pages 1\u{2014}3\n";
        let committed =
            commit_msg_from(&GitPolicy::default(), msg, false, &MessageSource::Committed);
        let v = committed
            .violations
            .iter()
            .find(|v| v.rule == "git.policy_characters")
            .expect("the retained `#` line is committed content");
        assert!(v.message.contains("line 3"), "{}", v.message);
    }

    fn pending(mode: Option<&str>, editor_used: bool) -> MessageSource {
        MessageSource::Pending(GitCleanup::resolve(mode, editor_used, None))
    }

    fn pending_flags(msg: &str, source: &MessageSource) -> bool {
        has_policy_character(&commit_msg_from(&GitPolicy::default(), msg, false, source))
    }

    fn retained(cleanup: &GitCleanup, msg: &str) -> String {
        cleanup.retained(msg)
    }

    /// Git's verbose or scissors cut signature under `prefix`.
    fn signature(prefix: &str) -> String {
        CUT_SIGNATURE.iter().fold(String::new(), |mut out, text| {
            out.push_str(prefix);
            out.push(' ');
            out.push_str(text);
            out.push('\n');
            out
        })
    }

    // Codex EPC-017 review round 2: Git hands commit-msg the message before
    // its cleanup, and `git commit -m` keeps `#` lines (whitespace mode), so
    // the live hook scans them; only a stripping cleanup drops them.
    #[test]
    fn test_pending_hash_line_follows_git_cleanup_mode() {
        let msg = "feat: add ranges\n\n# pages 1\u{2014}3\n";
        assert!(pending_flags(msg, &pending(None, false)), "-m");
        for mode in ["verbatim", "whitespace", "scissors", "unknown"] {
            assert!(pending_flags(msg, &pending(Some(mode), true)), "{mode}");
            assert!(pending_flags(msg, &pending(Some(mode), false)), "{mode}");
        }
        assert!(!pending_flags(msg, &pending(None, true)), "editor");
        assert!(!pending_flags(msg, &pending(Some("default"), true)));
        assert!(!pending_flags(msg, &pending(Some("strip"), false)));
        assert!(!pending_flags(msg, &pending(Some("strip"), true)));
    }

    // Codex EPC-017 review round 3, N2: editor use or commit.verbose does
    // not prove Git truncates. Only Git's own cut signature does.
    #[test]
    fn test_only_gits_cut_signature_ends_the_scanned_message() {
        let loose = "feat: add ranges\n\n# ---- >8 ----\n- pages 1\u{2013}3\n";
        let committed = commit_msg_from(
            &GitPolicy::default(),
            loose,
            false,
            &MessageSource::Committed,
        );
        assert!(
            has_policy_character(&committed),
            "{:?}",
            committed.violations
        );
        let bare = format!(
            "feat: add ranges\n\n# {}\n- pages 1\u{2013}3\n",
            CUT_SIGNATURE[0]
        );
        for msg in [loose, bare.as_str()] {
            // A bare cut line is only a comment to a stripping editor
            // session with no verbose flag; the bullet below it is kept.
            for editor_used in [false, true] {
                for mode in [None, Some("whitespace"), Some("verbatim"), Some("strip")] {
                    let source = pending(mode, editor_used);
                    assert!(pending_flags(msg, &source), "{mode:?} {editor_used}");
                }
            }
        }
        // Git's full signature marks the verbose preview, in any mode.
        let preview = format!(
            "feat: add ranges\n\n{}diff --git a/x b/x\n+pages 1\u{2013}3\n",
            signature("#")
        );
        for editor_used in [false, true] {
            for mode in [None, Some("whitespace"), Some("verbatim")] {
                let source = pending(mode, editor_used);
                assert!(!pending_flags(&preview, &source), "{mode:?} {editor_used}");
            }
        }
        // The signature must be whole and in the configured prefix.
        let partial = format!(
            "feat: add ranges\n\n# {}\n# {}\n- pages 1\u{2013}3\n",
            CUT_SIGNATURE[0], CUT_SIGNATURE[1]
        );
        assert!(pending_flags(&partial, &pending(Some("verbatim"), true)));
        let other = GitCleanup::resolve(Some("verbatim"), true, Some(";"));
        assert!(retained(&other, &preview).contains("+pages"));
    }

    // Codex EPC-017 review round 3, N3: the comment prefix is used byte for
    // byte, and `auto` leaves it unknown, so no line counts as a comment.
    #[test]
    fn test_pending_cleanup_uses_the_exact_comment_prefix() {
        let msg = "feat: add ranges\n\n# pages 1\u{2014}3\n; note \u{2014} here\n";
        let semicolon = GitCleanup::resolve(None, true, Some(";"));
        assert_eq!(
            "feat: add ranges\n\n# pages 1\u{2014}3\n",
            retained(&semicolon, msg)
        );

        let hash_space = GitCleanup::resolve(None, true, Some("# "));
        let spaced = "feat: x\n\n# dropped \u{2014} comment\n#kept \u{2014} line\n";
        assert_eq!(
            "feat: x\n\n#kept \u{2014} line\n",
            retained(&hash_space, spaced)
        );
        let cut = format!("feat: x\n\n{}diff\n", signature("# "));
        assert_eq!("feat: x\n", retained(&hash_space, &cut));

        for auto in ["auto", "AUTO", "Auto"] {
            let unknown = GitCleanup::resolve(None, true, Some(auto));
            assert_eq!(msg, retained(&unknown, msg), "{auto}");
            let preview = format!("feat: x\n\n{}+a\u{2014}b\n", signature("#"));
            assert_eq!(preview, retained(&unknown, &preview), "{auto}");
        }
        assert_eq!(
            GitCleanup::editor_default(),
            GitCleanup::resolve(Some("default"), true, Some("#"))
        );
    }

    #[test]
    fn test_merge_message_is_scanned_for_policy_characters() {
        let msg = "Merge branch 'feat/x' \u{2014} tidy\n";
        for source in [
            MessageSource::Pending(GitCleanup::editor_default()),
            MessageSource::Committed,
        ] {
            let report = commit_msg_from(&GitPolicy::default(), msg, true, &source);
            // The format exemption still holds; only the character rule fires.
            assert_eq!(1, report.violations.len(), "{:?}", report.violations);
            assert!(has_policy_character(&report), "{source:?}");
        }
        let clean = commit_msg(&GitPolicy::default(), "Merge branch 'feat/x'\n", true);
        assert!(clean.violations.is_empty(), "{:?}", clean.violations);
    }

    #[test]
    fn test_commit_msg_levels_from_policy() {
        // warn → still reported, at warn; off → silent (D7: nothing hardcoded).
        let warn_policy = GitPolicy {
            commit_format: PolicyLevel::Warn,
            ..GitPolicy::default()
        };
        let report = commit_msg(&warn_policy, "Bad subject\n", false);
        assert_eq!(report.violations[0].level, PolicyLevel::Warn);

        let off_policy = GitPolicy {
            commit_format: PolicyLevel::Off,
            commit_body: PolicyLevel::Off,
            ai_attribution: PolicyLevel::Off,
            commit_emoji: PolicyLevel::Off,
            policy_characters: PolicyLevel::Off,
            ..GitPolicy::default()
        };
        let msg = "Bad subject \u{1F680} \u{2014}\n\nGenerated with a robot\n";
        assert!(commit_msg(&off_policy, msg, false).violations.is_empty());
    }

    #[test]
    fn test_commit_msg_long_description_blocked() {
        // ADR-0020: a description over the 50-char budget blocks at commit_format.
        let subject = format!("feat: {}", "x".repeat(60));
        let report = commit_msg(&GitPolicy::default(), &format!("{subject}\n"), false);
        let v = report
            .violations
            .iter()
            .find(|v| v.rule == "git.commit_format")
            .expect("a commit_format violation");
        assert!(v.message.contains("description"), "{}", v.message);
    }

    #[test]
    fn test_commit_msg_story_body_blocked() {
        // ADR-0020: a prose body blocks at commit_body, naming the offending line.
        let msg = "feat: add a thing\n\nThis is a story about why the thing was added.\n";
        let report = commit_msg(&GitPolicy::default(), msg, false);
        let v = report
            .violations
            .iter()
            .find(|v| v.rule == "git.commit_body")
            .expect("a commit_body violation");
        assert_eq!(v.level, PolicyLevel::Block);
        assert!(v.message.contains("not a `- ` bullet"), "{}", v.message);
    }

    #[test]
    fn test_commit_msg_bullet_body_passes() {
        // A conforming bullet body with a BREAKING CHANGE footer is clean.
        let msg = "feat: add a thing\n\n- wire the new path\n- cover it with a test\n\nBREAKING CHANGE: the old path is gone\n";
        let report = commit_msg(&GitPolicy::default(), msg, false);
        assert!(report.violations.is_empty(), "{:?}", report.violations);
    }

    // -- footer trailers + ticket enforcement (ADR-0020 amendment) --

    #[test]
    fn test_commit_msg_trailers_block_by_default() {
        // Strict baseline: no trailer is allowed by default, so a `Refs:` or
        // `Signed-off-by:` footer blocks at commit_body until opted in.
        for msg in [
            "feat: add a thing\n\n- do it\n\nRefs: PROJ-142\n",
            "fix: y\n\nSigned-off-by: Ada Lovelace <ada@example.com>\n",
        ] {
            let report = commit_msg(&GitPolicy::default(), msg, false);
            assert!(
                report
                    .violations
                    .iter()
                    .any(|v| v.rule == "git.commit_body"),
                "{msg:?} should block by default"
            );
        }
    }

    #[test]
    fn test_commit_msg_opted_in_trailer_passes() {
        // A project opts `Refs` in → the trailer passes the body-shape check.
        let policy = GitPolicy {
            commit_footer_tokens: vec!["Refs".into()],
            ..GitPolicy::default()
        };
        let report = commit_msg(
            &policy,
            "feat: add a thing\n\n- do it\n\nRefs: PROJ-142\n",
            false,
        );
        assert!(report.violations.is_empty(), "{:?}", report.violations);
    }

    #[test]
    fn test_commit_msg_ai_coauthor_blocks_even_when_token_opted_in() {
        // With Co-authored-by opted in, the body shape accepts the token — but an
        // AI value STILL blocks via ai_attribution (orthogonal checks).
        let policy = GitPolicy {
            commit_footer_tokens: vec!["Co-authored-by".into()],
            ..GitPolicy::default()
        };
        let msg = "feat: x\n\nCo-authored-by: Claude Fable 5 <noreply@anthropic.com>\n";
        let report = commit_msg(&policy, msg, false);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.rule == "git.ai_attribution"),
            "AI co-author must block via attribution"
        );
        assert!(
            !report
                .violations
                .iter()
                .any(|v| v.rule == "git.commit_body"),
            "the opted-in trailer token must not trip the body-shape check"
        );
        // A human co-author with the token opted in clears both checks.
        let human = commit_msg(
            &policy,
            "feat: x\n\nCo-authored-by: Ada <ada@example.com>\n",
            false,
        );
        assert!(human.violations.is_empty(), "{:?}", human.violations);
    }

    #[test]
    fn test_commit_msg_human_coauthor_blocks_by_default() {
        // By default Co-authored-by is not opted in → the body shape blocks it
        // (human or AI alike) before attribution even matters.
        let msg = "feat: x\n\nCo-authored-by: Ada Lovelace <ada@example.com>\n";
        let report = commit_msg(&GitPolicy::default(), msg, false);
        assert!(report
            .violations
            .iter()
            .any(|v| v.rule == "git.commit_body"));
    }

    #[test]
    fn test_commit_msg_unknown_trailer_token_blocks_body() {
        let msg = "feat: x\n\nNote: a sneaky prose line masquerading as a trailer\n";
        let report = commit_msg(&GitPolicy::default(), msg, false);
        let v = report
            .violations
            .iter()
            .find(|v| v.rule == "git.commit_body")
            .expect("a commit_body violation");
        assert!(v.message.contains("Note"), "{}", v.message);
    }

    #[test]
    fn test_commit_msg_required_footer_missing_blocks() {
        // commit_required_footers=[Signed-off-by] (DCO) → a commit without it blocks.
        let policy = GitPolicy {
            commit_required_footers: vec!["Signed-off-by".into()],
            ..GitPolicy::default()
        };
        let missing = commit_msg(&policy, "feat: x\n\n- do it\n", false);
        let v = missing
            .violations
            .iter()
            .find(|v| v.rule == "git.commit_body" && v.message.contains("Signed-off-by"))
            .expect("a missing-required-footer violation");
        assert_eq!(v.level, PolicyLevel::Block);
        // Present (the required token is implicitly allowed) → clean.
        let ok = commit_msg(
            &policy,
            "feat: x\n\n- do it\n\nSigned-off-by: Ada <ada@example.com>\n",
            false,
        );
        assert!(ok.violations.is_empty(), "{:?}", ok.violations);
    }

    #[test]
    fn test_commit_msg_ticket_default_off_no_requirement() {
        // Default: no ticket keys configured → no commit_ticket finding at all,
        // and a stray `Refs:` blocks on shape (not opted in).
        let plain = commit_msg(&GitPolicy::default(), "feat: x\n\n- just a bullet\n", false);
        assert!(!plain
            .violations
            .iter()
            .any(|v| v.rule == "git.commit_ticket"));
    }

    #[test]
    fn test_commit_msg_ticket_allowed_optional() {
        // keys set, required Off: a matching ticket passes; absence is also fine.
        let policy = GitPolicy {
            commit_ticket_keys: vec!["Refs".into()],
            ..GitPolicy::default()
        };
        let present = commit_msg(&policy, "feat: x\n\n- do it\n\nRefs: PROJ-1\n", false);
        assert!(present.violations.is_empty(), "{:?}", present.violations);
        let absent = commit_msg(&policy, "feat: x\n\n- do it\n", false);
        assert!(!absent
            .violations
            .iter()
            .any(|v| v.rule == "git.commit_ticket"));
    }

    #[test]
    fn test_commit_msg_ticket_required_blocks_when_missing() {
        let policy = GitPolicy {
            commit_ticket_keys: vec!["Refs".into()],
            commit_ticket_required: PolicyLevel::Block,
            commit_ticket_pattern: r"^PROJ-\d+$".to_string(),
            ..GitPolicy::default()
        };
        // No ticket trailer → blocked at commit_ticket.
        let missing = commit_msg(&policy, "feat: x\n\n- no ticket here\n", false);
        let v = missing
            .violations
            .iter()
            .find(|v| v.rule == "git.commit_ticket")
            .expect("a commit_ticket violation");
        assert_eq!(v.level, PolicyLevel::Block);
        // A conforming `Refs: PROJ-1` trailer → clean.
        let ok = commit_msg(&policy, "feat: x\n\n- do it\n\nRefs: PROJ-1\n", false);
        assert!(ok.violations.is_empty(), "{:?}", ok.violations);
    }

    // -- contract-surface tripwire (ADR-0020) --

    fn watch_policy() -> GitPolicy {
        GitPolicy {
            breaking_watch_paths: vec!["src/api/**".into(), "config/schema.json".into()],
            ..GitPolicy::default()
        }
    }

    #[test]
    fn test_watch_paths_unmarked_touch_warns() {
        let files = vec!["src/api/routes.rs".to_string()];
        let report = commit_msg_with_files(
            &watch_policy(),
            "feat: tweak a route\n",
            &files,
            false,
            &MessageSource::Pending(GitCleanup::editor_default()),
        );
        let v = report
            .violations
            .iter()
            .find(|v| v.rule == "git.breaking_watch_paths")
            .expect("a tripwire warn");
        assert_eq!(v.level, PolicyLevel::Warn);
        assert!(v.message.contains("src/api/routes.rs"), "{}", v.message);
    }

    #[test]
    fn test_watch_paths_marked_does_not_warn() {
        let files = vec!["src/api/routes.rs".to_string()];
        // A `!` subject marker suppresses the nudge...
        let bang = commit_msg_with_files(
            &watch_policy(),
            "feat!: drop a route\n",
            &files,
            false,
            &MessageSource::Pending(GitCleanup::editor_default()),
        );
        assert!(!bang
            .violations
            .iter()
            .any(|v| v.rule == "git.breaking_watch_paths"));
        // ...and so does a BREAKING CHANGE footer.
        let footer = commit_msg_with_files(
            &watch_policy(),
            "feat: drop a route\n\nBREAKING CHANGE: the /old route is gone\n",
            &files,
            false,
            &MessageSource::Pending(GitCleanup::editor_default()),
        );
        assert!(!footer
            .violations
            .iter()
            .any(|v| v.rule == "git.breaking_watch_paths"));
    }

    #[test]
    fn test_watch_paths_untouched_does_not_warn() {
        let files = vec!["README.md".to_string(), "src/util/log.rs".to_string()];
        let report = commit_msg_with_files(
            &watch_policy(),
            "docs: tidy the readme\n",
            &files,
            false,
            &MessageSource::Pending(GitCleanup::editor_default()),
        );
        assert!(!report
            .violations
            .iter()
            .any(|v| v.rule == "git.breaking_watch_paths"));
    }

    #[test]
    fn test_watch_paths_empty_default_is_noop() {
        // The shipped default (no watched globs) never warns, even on any file.
        let files = vec!["src/api/routes.rs".to_string()];
        let report = commit_msg_with_files(
            &GitPolicy::default(),
            "feat: tweak a route\n",
            &files,
            false,
            &MessageSource::Pending(GitCleanup::editor_default()),
        );
        assert!(!report
            .violations
            .iter()
            .any(|v| v.rule == "git.breaking_watch_paths"));
    }

    #[test]
    fn test_commit_msg_ignores_comment_lines() {
        let msg = "feat: x\n# Co-Authored-By: Claude <noreply@anthropic.com>\n";
        assert!(commit_msg(&GitPolicy::default(), msg, false)
            .violations
            .is_empty());
    }

    #[test]
    fn test_commit_msg_ignores_scissors_section() {
        let msg = "feat: x\n# ------------------------ >8 ------------------------\ndiff: Generated with Claude\n";
        assert!(commit_msg(&GitPolicy::default(), msg, false)
            .violations
            .is_empty());
    }

    #[test]
    fn test_commit_msg_merge_subject_exempt_from_format() {
        let report = commit_msg(
            &GitPolicy::default(),
            "Merge branch 'main' into feat/x\n",
            true,
        );
        assert!(report.violations.is_empty());
    }

    // -- pre-merge-commit --

    #[test]
    fn test_pre_merge_commit_blocks_on_protected() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let report = pre_merge_commit(dir.path(), &GitPolicy::default(), false, false).unwrap();
        assert_eq!(report.violations.len(), 1);
        assert_eq!(report.violations[0].rule, "git.merge_to_protected");
        assert_eq!(report.violations[0].level, PolicyLevel::Block);
    }

    #[test]
    fn test_pre_merge_commit_feature_branch_clean() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        let report = pre_merge_commit(dir.path(), &GitPolicy::default(), false, false).unwrap();
        assert!(report.violations.is_empty(), "{:?}", report.violations);
    }

    #[test]
    fn test_pre_merge_commit_integrate_token_passes() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let report = pre_merge_commit(dir.path(), &GitPolicy::default(), true, false).unwrap();
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_pre_merge_commit_human_override_passes() {
        // ADR-0007: a human's CODEFLOW_HUMAN_OVERRIDE lets the git layer pass.
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let report = pre_merge_commit(dir.path(), &GitPolicy::default(), false, true).unwrap();
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_pre_merge_commit_warn_level() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let policy = GitPolicy {
            merge_to_protected: PolicyLevel::Warn,
            ..GitPolicy::default()
        };
        let report = pre_merge_commit(dir.path(), &policy, false, false).unwrap();
        assert_eq!(report.violations[0].level, PolicyLevel::Warn);
    }

    #[test]
    fn test_pre_merge_commit_off_silent() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let policy = GitPolicy {
            merge_to_protected: PolicyLevel::Off,
            ..GitPolicy::default()
        };
        let report = pre_merge_commit(dir.path(), &policy, false, false).unwrap();
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_pre_merge_commit_release_glob_extension() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "release/2.0");
        let report = pre_merge_commit(dir.path(), &release_policy(), false, false).unwrap();
        assert_eq!(report.violations[0].rule, "git.merge_to_protected");
    }

    // -- reference-transaction --

    #[test]
    fn test_ref_line_touches_local_branch() {
        assert!(ref_line_touches_local_branch("aaa bbb refs/heads/main"));
        assert!(ref_line_touches_local_branch(
            "aaa bbb refs/heads/release/2.0"
        ));
        assert!(!ref_line_touches_local_branch(
            "aaa bbb refs/remotes/origin/main"
        ));
        assert!(!ref_line_touches_local_branch("aaa bbb refs/tags/v1"));
        assert!(!ref_line_touches_local_branch("aaa bbb HEAD"));
        assert!(!ref_line_touches_local_branch("garbage line"));
    }

    /// Point `refs/remotes/origin/<branch>` at `oid` (a simulated fetched head).
    fn set_origin_ref(dir: &Path, branch: &str, oid: &str) {
        git(
            dir,
            &["update-ref", &format!("refs/remotes/origin/{branch}"), oid],
        );
    }

    const ZERO40: &str = "0000000000000000000000000000000000000000";
    const FAKE40: &str = "1111111111111111111111111111111111111111";

    #[test]
    fn test_reference_transaction_blocks_local_move_ahead_of_origin() {
        // A ff-merge / local commit moves main ahead of origin — blocked.
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let base = rev_parse(dir.path(), "HEAD");
        set_origin_ref(dir.path(), "main", &base);
        std::fs::write(dir.path().join("f.txt"), "x\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "feat: local"]);
        let ahead = rev_parse(dir.path(), "HEAD");
        let stdin = format!("{base} {ahead} refs/heads/main\n");
        let report =
            reference_transaction(dir.path(), &GitPolicy::default(), &stdin, false, false).unwrap();
        assert_eq!(report.violations.len(), 1);
        assert_eq!(report.violations[0].rule, "git.local_ref_protection");
        assert_eq!(report.violations[0].level, PolicyLevel::Block);
    }

    #[test]
    fn test_reference_transaction_allows_sync_to_origin_head() {
        // main moved to exactly origin/main (a `git pull` fast-forward) — allowed.
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        std::fs::write(dir.path().join("f.txt"), "x\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "feat: x"]);
        let new = rev_parse(dir.path(), "HEAD");
        set_origin_ref(dir.path(), "main", &new);
        let stdin = format!("{ZERO40} {new} refs/heads/main\n");
        let report =
            reference_transaction(dir.path(), &GitPolicy::default(), &stdin, false, false).unwrap();
        assert!(report.violations.is_empty(), "{:?}", report.violations);
    }

    #[test]
    fn test_reference_transaction_allows_sync_to_configured_upstream() {
        // B10: the ff-sync oracle follows the branch's CONFIGURED upstream, not
        // a hardcoded origin. A branch tracking `upstream` fast-forwards to
        // refs/remotes/upstream/main without a false-positive block (origin is
        // absent here).
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        std::fs::write(dir.path().join("f.txt"), "x\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "feat: x"]);
        let new = rev_parse(dir.path(), "HEAD");
        git(dir.path(), &["remote", "add", "upstream", "."]);
        git(
            dir.path(),
            &["update-ref", "refs/remotes/upstream/main", &new],
        );
        git(dir.path(), &["config", "branch.main.remote", "upstream"]);
        git(
            dir.path(),
            &["config", "branch.main.merge", "refs/heads/main"],
        );
        let stdin = format!("{ZERO40} {new} refs/heads/main\n");
        let report =
            reference_transaction(dir.path(), &GitPolicy::default(), &stdin, false, false).unwrap();
        assert!(report.violations.is_empty(), "{:?}", report.violations);
    }

    #[test]
    fn test_reference_transaction_blocks_move_behind_origin() {
        // ADR-0009: a move merely *behind* origin/main (e.g. `reset --hard` to
        // an ancestor) is no longer a sanctioned sync — the ancestor allowance
        // is dropped, so only an exact match to the remote head passes. Reset
        // of a protected branch to an ancestor is a history loss and blocks.
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let base = rev_parse(dir.path(), "HEAD");
        std::fs::write(dir.path().join("f.txt"), "x\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "feat: ahead"]);
        let ahead = rev_parse(dir.path(), "HEAD");
        set_origin_ref(dir.path(), "main", &ahead);
        let stdin = format!("{ahead} {base} refs/heads/main\n");
        let report =
            reference_transaction(dir.path(), &GitPolicy::default(), &stdin, false, false).unwrap();
        assert_eq!(report.violations.len(), 1);
        assert_eq!(report.violations[0].rule, "git.local_ref_protection");
    }

    #[test]
    fn test_reference_transaction_poisoned_remote_ref_still_blocks_non_matching_move() {
        // The oracle is narrowed to EXACT equality: even if the agent-writable
        // refs/remotes/origin/main is poisoned to some tip, a local move to a
        // DIFFERENT tip is not a sync and blocks. (The exact-match case that a
        // real `git pull` produces stays allowed — see the sync test.)
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let base = rev_parse(dir.path(), "HEAD");
        set_origin_ref(dir.path(), "main", &base); // "poisoned"/stale at base
        std::fs::write(dir.path().join("f.txt"), "x\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "feat: local"]);
        let ahead = rev_parse(dir.path(), "HEAD");
        let stdin = format!("{base} {ahead} refs/heads/main\n");
        let report =
            reference_transaction(dir.path(), &GitPolicy::default(), &stdin, false, false).unwrap();
        assert_eq!(report.violations[0].rule, "git.local_ref_protection");
    }

    #[test]
    fn test_reference_transaction_feature_branch_untouched() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        let stdin = "aaa bbb refs/heads/feat/x\n";
        let report =
            reference_transaction(dir.path(), &GitPolicy::default(), stdin, false, false).unwrap();
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_reference_transaction_delete_protected_blocked() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let head = rev_parse(dir.path(), "HEAD");
        let stdin = format!("{head} {ZERO40} refs/heads/main\n");
        let report =
            reference_transaction(dir.path(), &GitPolicy::default(), &stdin, false, false).unwrap();
        assert_eq!(report.violations[0].rule, "git.delete_protected");
    }

    #[test]
    fn test_reference_transaction_token_and_override_pass() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let head = rev_parse(dir.path(), "HEAD");
        let stdin = format!("{head} {FAKE40} refs/heads/main\n");
        // integrate token sanctions the git layer...
        assert!(
            reference_transaction(dir.path(), &GitPolicy::default(), &stdin, true, false)
                .unwrap()
                .violations
                .is_empty()
        );
        // ...and so does a human's override.
        assert!(
            reference_transaction(dir.path(), &GitPolicy::default(), &stdin, false, true)
                .unwrap()
                .violations
                .is_empty()
        );
    }

    #[test]
    fn test_reference_transaction_warn_level() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let head = rev_parse(dir.path(), "HEAD");
        let policy = GitPolicy {
            local_ref_protection: PolicyLevel::Warn,
            ..GitPolicy::default()
        };
        let stdin = format!("{head} {FAKE40} refs/heads/main\n");
        let report = reference_transaction(dir.path(), &policy, &stdin, false, false).unwrap();
        assert_eq!(report.violations[0].level, PolicyLevel::Warn);
    }

    #[test]
    fn test_reference_transaction_off_still_blocks_delete() {
        // local_ref_protection off → local moves allowed, but delete_protected
        // still blocks a protected-branch deletion (its own key).
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let head = rev_parse(dir.path(), "HEAD");
        let policy = GitPolicy {
            local_ref_protection: PolicyLevel::Off,
            ..GitPolicy::default()
        };
        let move_ahead = format!("{head} {FAKE40} refs/heads/main\n");
        assert!(
            reference_transaction(dir.path(), &policy, &move_ahead, false, false)
                .unwrap()
                .violations
                .is_empty()
        );
        let delete = format!("{head} {ZERO40} refs/heads/main\n");
        let report = reference_transaction(dir.path(), &policy, &delete, false, false).unwrap();
        assert_eq!(report.violations[0].rule, "git.delete_protected");
    }

    #[test]
    fn reference_transaction_closes_the_ff_merge_gap() {
        // ADR-0007 boundary, now CLOSED. A fast-forward merge creates no commit
        // (pre-merge-commit never fires) but it DOES move refs/heads/main, which
        // fires reference-transaction. Proven at the git level: pre-merge-commit
        // misses the ff-merge; reference-transaction catches the same move.
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let base = rev_parse(dir.path(), "HEAD");

        let set_exec = |p: &Path| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            let _ = p;
        };
        let git_try = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_INDEX_FILE")
                .output()
                .expect("git runs")
        };

        // A branch one commit ahead → merging it back fast-forwards.
        git(dir.path(), &["checkout", "-b", "feat/ff"]);
        std::fs::write(dir.path().join("ff.txt"), "ff\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "feat: ff"]);
        git(dir.path(), &["checkout", "main"]);

        // (1) Only pre-merge-commit wired (always-block): the ff-merge slips past.
        let pmc = dir.path().join(".git/hooks/pre-merge-commit");
        std::fs::write(&pmc, "#!/bin/sh\nexit 1\n").unwrap();
        set_exec(&pmc);
        let ff = git_try(&["merge", "--ff-only", "feat/ff"]);
        assert!(
            ff.status.success(),
            "pre-merge-commit does not fire on a fast-forward merge"
        );

        // Reset main back (no reference-transaction hook wired yet), then wire
        // an always-blocking reference-transaction and retry the same ff-merge.
        git(dir.path(), &["reset", "--hard", &base]);
        let rt = dir.path().join(".git/hooks/reference-transaction");
        std::fs::write(&rt, "#!/bin/sh\n[ \"$1\" = prepared ] && exit 1\nexit 0\n").unwrap();
        set_exec(&rt);
        let ff2 = git_try(&["merge", "--ff-only", "feat/ff"]);
        assert!(
            !ff2.status.success(),
            "reference-transaction fires on the ff-merge's ref update and closes the gap"
        );
    }

    // -- pre-push --

    fn pref(local_ref: &str, local: &str, remote_ref: &str, remote: &str) -> PushRef {
        PushRef {
            local_ref: local_ref.into(),
            local_sha: local.into(),
            remote_ref: remote_ref.into(),
            remote_sha: remote.into(),
        }
    }

    const ZERO: &str = "0000000000000000000000000000000000000000";

    #[test]
    fn test_parse_push_refs() {
        let input = format!("refs/heads/feat/x abc123 refs/heads/feat/x {ZERO}\n");
        let refs = parse_push_refs(&input);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].remote_branch(), Some("feat/x"));
        assert!(!refs[0].is_delete());
    }

    #[test]
    fn test_pre_push_feature_branch_allowed() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        let refs = [pref("refs/heads/feat/x", "abc1", "refs/heads/feat/x", ZERO)];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false).unwrap();
        assert!(report.violations.is_empty(), "{:?}", report.violations);
    }

    #[test]
    fn test_pre_push_to_protected_blocked() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let refs = [pref("refs/heads/main", "abc1", "refs/heads/main", ZERO)];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false).unwrap();
        assert_eq!(report.violations[0].rule, "git.push_to_protected");
    }

    #[test]
    fn test_pre_push_integrate_token_passes() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let refs = [pref("refs/heads/main", "abc1", "refs/heads/main", ZERO)];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, true).unwrap();
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_pre_push_delete_protected_blocked() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        let refs = [pref("(delete)", ZERO, "refs/heads/main", "abc1")];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false).unwrap();
        assert_eq!(report.violations[0].rule, "git.delete_protected");
    }

    #[test]
    fn test_pre_push_delete_feature_allowed() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        let refs = [pref("(delete)", ZERO, "refs/heads/feat/old", "abc1")];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false).unwrap();
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_pre_push_branch_naming_blocked() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "bad-name");
        let refs = [pref(
            "refs/heads/bad-name",
            "abc1",
            "refs/heads/bad-name",
            ZERO,
        )];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false).unwrap();
        assert_eq!(report.violations[0].rule, "git.branch_naming");
        assert!(report.violations[0].remedy.contains("feat/"));
    }

    #[test]
    fn test_pre_push_branch_naming_off_allows() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "bad-name");
        let policy = GitPolicy {
            branch_naming: PolicyLevel::Off,
            ..GitPolicy::default()
        };
        let refs = [pref(
            "refs/heads/bad-name",
            "abc1",
            "refs/heads/bad-name",
            ZERO,
        )];
        let report = pre_push(dir.path(), &policy, &refs, false).unwrap();
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_pre_push_tags_ignored() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        let refs = [pref("refs/tags/v1.0", "abc1", "refs/tags/v1.0", ZERO)];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false).unwrap();
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_pre_push_release_glob_extension() {
        // AC #3: the policy glob extension is honored at pre-push too.
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        let refs = [pref(
            "refs/heads/release/2.0",
            "abc1",
            "refs/heads/release/2.0",
            ZERO,
        )];
        let report = pre_push(dir.path(), &release_policy(), &refs, false).unwrap();
        assert_eq!(report.violations[0].rule, "git.push_to_protected");
    }

    #[test]
    fn test_pre_push_force_update_detection() {
        // Build real history: main at C1; branch rewrites to C2' not
        // descending from C2.
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        let c1 = rev_parse(dir.path(), "HEAD");
        std::fs::write(dir.path().join("f.txt"), "v1\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "feat: v1"]);
        let c2 = rev_parse(dir.path(), "HEAD");
        // Rewrite: drop C2, add a different commit.
        git(dir.path(), &["reset", "--hard", &c1]);
        std::fs::write(dir.path().join("g.txt"), "v2\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "feat: v2"]);
        let c2_prime = rev_parse(dir.path(), "HEAD");

        // Force-push to a feature branch: allowed by default policy (D8).
        let refs = [pref(
            "refs/heads/feat/x",
            &c2_prime,
            "refs/heads/feat/x",
            &c2,
        )];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false).unwrap();
        assert!(report.violations.is_empty(), "{:?}", report.violations);

        // Same rewrite against a protected branch: blocked.
        let refs = [pref("refs/heads/main", &c2_prime, "refs/heads/main", &c2)];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false).unwrap();
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.rule == "git.force_push_protected"),
            "{:?}",
            report.violations
        );

        // Fast-forward (c1 → c2_prime ancestor path): no force violation.
        let refs = [pref(
            "refs/heads/feat/x",
            &c2_prime,
            "refs/heads/feat/x",
            &c1,
        )];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false).unwrap();
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_pre_push_force_push_unprotected_blocked() {
        // force_push_unprotected defaults to Allow (D8), so the unprotected
        // force-push block path never runs under the default policy. Opt in with
        // Block: the same c2/c2' rewrite pushed to a FEATURE branch is a
        // non-fast-forward push and raises exactly one git.force_push_unprotected.
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        let c1 = rev_parse(dir.path(), "HEAD");
        std::fs::write(dir.path().join("f.txt"), "v1\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "feat: v1"]);
        let c2 = rev_parse(dir.path(), "HEAD");
        // Rewrite: drop C2, add a different commit not descending from it.
        git(dir.path(), &["reset", "--hard", &c1]);
        std::fs::write(dir.path().join("g.txt"), "v2\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "feat: v2"]);
        let c2_prime = rev_parse(dir.path(), "HEAD");

        let policy = GitPolicy {
            force_push_unprotected: PolicyLevel::Block,
            ..GitPolicy::default()
        };
        let refs = [pref(
            "refs/heads/feat/x",
            &c2_prime,
            "refs/heads/feat/x",
            &c2,
        )];
        let report = pre_push(dir.path(), &policy, &refs, false).unwrap();
        let forced: Vec<_> = report
            .violations
            .iter()
            .filter(|v| v.rule == "git.force_push_unprotected")
            .collect();
        assert_eq!(forced.len(), 1, "{:?}", report.violations);
        assert_eq!(report.violations.len(), 1, "{:?}", report.violations);
        assert_eq!(forced[0].level, PolicyLevel::Block);
    }

    fn rev_parse(dir: &Path, what: &str) -> String {
        let out = Command::new("git")
            .args(["rev-parse", what])
            .current_dir(dir)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    // -- push set: quick targets (the CLI hook binds them to the pushed tree) --

    fn run_targets(dir: &Path, policy: &GitPolicy) -> (StageReport, Vec<PushStep>) {
        let mut report = StageReport::default();
        let steps = run_push_targets(dir, policy, &mut report);
        (report, steps)
    }

    #[test]
    fn pre_push_ref_checks_never_run_targets() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        write_test_config(dir.path(), "false");
        let refs = [pref("refs/heads/feat/x", "abc1", "refs/heads/feat/x", ZERO)];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false).unwrap();
        assert!(report.violations.is_empty(), "{:?}", report.violations);
    }

    #[test]
    fn test_gate_skips_without_config() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        let (report, steps) = run_targets(dir.path(), &GitPolicy::default());
        assert!(report.violations.is_empty());
        assert!(steps.is_empty());
        assert!(
            report
                .notes
                .iter()
                .any(|n| n.contains("quick targets skipped")),
            "absence must be loud: {:?}",
            report.notes
        );
    }

    fn write_test_config(dir: &Path, command: &str) {
        let cf = dir.join(".codeflow");
        std::fs::create_dir_all(&cf).unwrap();
        let config = format!(
            r#"{{
  "schema_version": "1.0",
  "targets": [
    {{
      "name": "demo",
      "runner": "custom",
      "enabled": true,
      "modes": {{ "quick": {{ "command": "{command}" }} }}
    }}
  ]
}}"#
        );
        std::fs::write(cf.join("test-config.json"), config).unwrap();
    }

    /// Write raw bytes to `.codeflow/test-config.json`.
    fn write_raw_test_config(dir: &Path, body: &str) {
        let cf = dir.join(".codeflow");
        std::fs::create_dir_all(&cf).unwrap();
        std::fs::write(cf.join("test-config.json"), body).unwrap();
    }

    #[test]
    fn push_set_blocks_by_default() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        write_test_config(dir.path(), "false");
        let (report, steps) = run_targets(dir.path(), &GitPolicy::default());
        let gate: Vec<_> = report
            .violations
            .iter()
            .filter(|v| v.rule == "git.test_gate_on_push")
            .collect();
        assert_eq!(gate.len(), 1, "{:?}", report.violations);
        assert_eq!(gate[0].level, PolicyLevel::Block);
        assert!(gate[0].message.contains("push set failed for: demo"));
        assert_eq!(steps.len(), 1);
        assert!(steps[0].configurable);
    }

    #[test]
    fn test_gate_pass_is_quiet() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        write_test_config(dir.path(), "true");
        let (report, _) = run_targets(dir.path(), &GitPolicy::default());
        assert!(report.violations.is_empty(), "{:?}", report.violations);
    }

    #[test]
    fn test_gate_no_targets_is_a_skip_note() {
        // The only target is disabled, so NoTargets: a loud note, never a
        // violation (charter principle 8: degradation stays legible).
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        write_raw_test_config(
            dir.path(),
            r#"{"schema_version": "1.0", "targets": [
  {"name": "demo", "runner": "custom", "enabled": false,
   "modes": { "quick": { "command": "true" } } }]}"#,
        );
        let (report, _) = run_targets(dir.path(), &GitPolicy::default());
        assert!(report.violations.is_empty(), "{:?}", report.violations);
        assert!(
            report
                .notes
                .iter()
                .any(|n| n.contains("quick targets skipped") && n.contains("no enabled target")),
            "{:?}",
            report.notes
        );
    }

    #[test]
    fn push_set_runs_only_quick_targets_and_never_aliases_essential() {
        // TSK-132: a target without a `quick` mode is out of the push set even
        // when its `essential` mode would fail.
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        write_raw_test_config(
            dir.path(),
            r#"{"schema_version": "1.0", "targets": [
    { "name": "lint", "runner": "custom",
      "modes": { "quick": { "command": "true" }, "essential": { "command": "true" } } },
    { "name": "suite", "runner": "custom",
      "modes": { "essential": { "command": "false" }, "full": { "command": "false" } } }
]}"#,
        );
        let (report, steps) = run_targets(dir.path(), &GitPolicy::default());
        assert!(report.violations.is_empty(), "{:?}", report.violations);
        let names: Vec<&str> = steps.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["lint"]);
    }

    #[test]
    fn push_set_is_empty_for_an_essential_only_config() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        write_raw_test_config(
            dir.path(),
            r#"{"schema_version": "1.0", "targets": [
    { "name": "suite", "runner": "custom",
      "modes": { "essential": { "command": "false" }, "full": { "command": "false" } } }
]}"#,
        );
        let (report, steps) = run_targets(dir.path(), &GitPolicy::default());
        assert!(report.violations.is_empty(), "{:?}", report.violations);
        assert!(steps.is_empty());
        assert!(
            report
                .notes
                .iter()
                .any(|n| n.contains("quick targets skipped") && n.contains("`quick` mode")),
            "{:?}",
            report.notes
        );
    }

    #[test]
    fn test_gate_unreadable_config_is_a_violation() {
        // A populated but malformed config is a broken gate, not absence.
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        write_raw_test_config(dir.path(), "{ not json");
        let (report, _) = run_targets(dir.path(), &GitPolicy::default());
        let gate: Vec<_> = report
            .violations
            .iter()
            .filter(|v| v.rule == "git.test_gate_on_push")
            .collect();
        assert_eq!(gate.len(), 1, "{:?}", report.violations);
        assert!(gate[0].message.contains("could not load"));
        assert!(gate[0].message.contains("invalid test config"));
        assert!(!report.notes.iter().any(|n| n.contains("skipped")));
    }

    fn step(name: &str, secs: u64, configurable: bool) -> PushStep {
        PushStep {
            name: name.into(),
            duration: std::time::Duration::from_secs(secs),
            configurable,
        }
    }

    #[test]
    fn over_budget_names_a_slow_target_and_its_key() {
        let steps = [
            step("rust-format", 1, true),
            step("rust-clippy", 50, true),
            step("codeflow validate --docs", 2, false),
        ];
        let total = std::time::Duration::from_secs(61);
        let note = over_budget_note(&steps, total, PUSH_SET_BUDGET).unwrap();
        assert!(note.contains("61.0s, over its 60s budget"), "{note}");
        assert!(note.contains("target 'rust-clippy'"), "{note}");
        assert!(note.contains("`modes.quick`"), "{note}");
        assert!(over_budget_note(&steps, PUSH_SET_BUDGET, PUSH_SET_BUDGET).is_none());
    }

    #[test]
    fn over_budget_names_a_slow_built_in_check_without_a_quick_hint() {
        // Built-in work takes the total over budget while every configured
        // target is fast (T132-5).
        let steps = [
            step("lint", 2, true),
            step("codeflow ci --head abc --branch feat/x", 70, false),
        ];
        let note =
            over_budget_note(&steps, std::time::Duration::from_secs(72), PUSH_SET_BUDGET).unwrap();
        assert!(note.contains("built-in `codeflow ci"), "{note}");
        assert!(!note.contains("modes.quick"), "{note}");
        // No configured targets at all still gets a diagnosis.
        let only_builtin = [step("codeflow validate --docs", 65, false)];
        let note = over_budget_note(
            &only_builtin,
            std::time::Duration::from_secs(65),
            PUSH_SET_BUDGET,
        )
        .unwrap();
        assert!(note.contains("validate --docs"), "{note}");
    }

    // Regression: a one-parent commit named `Merge ...` is checked; a real
    // merge (is_merge = true) stays exempt (codex pre-flip review).
    #[test]
    fn test_fake_merge_subject_is_checked() {
        let fake = commit_msg(
            &GitPolicy::default(),
            "Merge bypass\n\nprose body a real check rejects\n",
            false,
        );
        assert!(!fake.violations.is_empty(), "fake merge must be checked");
        let real = commit_msg(&GitPolicy::default(), "Merge branch 'main'\n", true);
        assert!(real.violations.is_empty(), "real merge stays exempt");
    }
}
