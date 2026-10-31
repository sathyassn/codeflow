//! Git client hook stages (charter §6.1 plane 1): pre-commit, commit-msg,
//! pre-merge-commit, reference-transaction, pre-push. Harness-agnostic — they
//! work for any agent or human.
//!
//! The shims in `.git/hooks/` exec `codeflow git-hook <stage>`; ALL levels
//! and lists come from `.codeflow/policy.json` (D7/D8 — nothing hardcoded).

use crate::workgraph::history;

use std::path::Path;

use git2::Repository;

use crate::error::HookError;
use crate::remedy::{Finding, JUDGE_SOURCE_DRIFT};

use super::gitleaks::Engine;
use super::policy::{GitPolicy, PolicyLevel, SecretScanEngine};
use super::repo::current_branch;
use super::scan;
use super::{standards, Violation};

/// Whether the Git directory holds the merge marker.
///
/// # Errors
/// An existing marker or its ancestors cannot be inspected or resolved.
pub fn merge_head_present(git_dir: &Path) -> Result<bool, HookError> {
    let path = git_dir.join("MERGE_HEAD");
    let unreadable = |error| HookError::Config(format!("cannot read merge state: {error}"));
    if crate::absence::proven_absent(&path).map_err(unreadable)? {
        return Ok(false);
    }
    std::fs::metadata(&path).map(|_| true).map_err(unreadable)
}

/// Identify the binary and compare its compiled input digest with this source tree.
/// Installed consumer repositories do not contain the compiler's source inputs.
#[must_use]
pub fn judging_identity(
    root: &Path,
    version: &str,
    revision: &str,
    dirty: &str,
    inputs: &str,
) -> Vec<String> {
    use sha2::{Digest, Sha256};
    let binary = std::env::current_exe().ok();
    let digest = binary
        .as_ref()
        .and_then(|path| std::fs::read(path).ok())
        .map_or_else(
            || "unavailable".into(),
            |bytes| super::source_identity::hex_digest(&Sha256::digest(bytes)),
        );
    let path = binary.map_or_else(|| "unavailable".into(), |p| p.display().to_string());
    let mut lines = vec![format!("codeflow judge: binary={path} version={version} source={revision} dirty={dirty} inputs={inputs} sha256={digest}")];
    if root.join("crates/codeflow-core/src/hooks").is_dir() {
        match super::source_identity::input_digest(root) {
            Ok(current) if current != inputs => lines.push(
                Finding::new(
                    "built from different hook or policy sources; rebuild before trusting this judgment",
                    JUDGE_SOURCE_DRIFT.remedy(),
                )
                .line("codeflow judge", "warning"),
            ),
            Err(error) => lines.push(
                Finding::new(
                    format!("cannot verify hook or policy source inputs: {error}"),
                    JUDGE_SOURCE_DRIFT.remedy(),
                )
                .line("codeflow judge", "warning"),
            ),
            Ok(_) => {}
        }
    }
    lines
}

/// Outcome of running one hook stage.
#[derive(Debug, Default)]
pub struct StageReport {
    /// Policy violations (block and warn level).
    pub violations: Vec<Violation>,
    /// Non-violation diagnostics (skipped gates, degraded checks), each with
    /// the step that clears it, printed so degradation stays legible
    /// (charter principle 8, SPC-013 R-80).
    pub notes: Vec<crate::remedy::Finding>,
    /// Progress lines that report something done or passed: nothing to
    /// clear, so no remedy (a timing, a check that passed).
    pub status: Vec<String>,
    /// Findings another plane printed when this one ran its check, as it
    /// printed them, each with the remedy lines under it.
    pub relayed: Vec<String>,
    /// The rules another plane's check blocked on when this one ran it
    /// (pre-push's `codeflow ci`), named in the refusal record (TSK-149).
    pub refused_by: Vec<String>,
}

// ---------------------------------------------------------------------------
// pre-commit
// ---------------------------------------------------------------------------

/// The pre-commit stage: protected-branch commit check, staged-.env block,
/// and the secret content scan over the staged diff (charter §6.1, §6.3).
/// The protected-branch check steps aside for the `codeflow integrate` token
/// or a human's [`HUMAN_OVERRIDE_ENV`](super::HUMAN_OVERRIDE_ENV) (ADR-0007);
/// the secret checks never do.
///
/// # Errors
///
/// Returns [`HookError::Config`] when `root` is not inside a git repository.
pub fn pre_commit(
    root: &Path,
    policy: &GitPolicy,
    integrate_token: bool,
    human_override: bool,
) -> Result<StageReport, HookError> {
    pre_commit_engine(
        root,
        policy,
        integrate_token,
        human_override,
        &Engine::from_env(),
    )
}

/// [`pre_commit`] with the gitleaks engine given, so a test can pin its own
/// stand-in binary.
fn pre_commit_engine(
    root: &Path,
    policy: &GitPolicy,
    integrate_token: bool,
    human_override: bool,
    engine: &Engine,
) -> Result<StageReport, HookError> {
    let repo = Repository::discover(root)
        .map_err(|e| HookError::Config(format!("not a git repository: {e}")))?;
    let mut report = StageReport::default();

    let branch = current_branch(&repo).map_err(HookError::Config)?;
    if policy.commit_to_protected.is_active()
        && policy.branch_is_protected(&branch)
        && !integrate_token
        && !human_override
    {
        report.violations.push(Violation::new(
            "git.commit_to_protected",
            policy.commit_to_protected,
            format!(
                "commit on protected branch '{}'",
                crate::git::display_text(&branch)
            ),
            crate::remedy::PROTECTED_BRANCH.remedy(),
        ));
    }
    report
        .violations
        .extend(crate::root_checkout::hook_violation(
            root,
            policy,
            &crate::root_checkout::process_env,
        ));

    match super::landed_policy::staged_paths(root, policy)
        .and_then(|paths| super::path_policy::staged(&repo, &paths))
    {
        Ok(paths) => {
            report.violations.extend(paths.violations);
            report.status.extend(paths.notes);
        }
        Err(error) => report.violations.push(Violation::always_blocking(
            "git.path_policy",
            format!("cannot verify staged paths: {error}"),
            "repair the named policy source or Git object, then retry",
        )),
    }

    if policy.secret_scan.is_active() {
        secret_scan(&repo, root, policy, engine, &mut report);
    }

    if policy.conflict_markers.is_active() {
        report.violations.extend(super::conflict_markers::staged(
            &repo,
            policy.conflict_markers,
        ));
    }

    Ok(report)
}

/// The staged secret scan: the built-in patterns, then, when the policy
/// chooses it, the pinned gitleaks engine (TSK-412). Both report at the
/// `secret_scan` level.
fn secret_scan(
    repo: &Repository,
    root: &Path,
    policy: &GitPolicy,
    engine: &Engine,
    report: &mut StageReport,
) {
    scan_staged(repo, policy, report, false);
    if policy.secret_scan_engine == SecretScanEngine::Gitleaks {
        engine_scan(repo.workdir().unwrap_or(root), policy, engine, report);
    }
}

/// Run the pinned gitleaks binary over the staged diff. A binary that is
/// missing, unpinned or mismatched is one finding at the `secret_scan`
/// level and is never run, so at `block` the commit is refused.
fn engine_scan(root: &Path, policy: &GitPolicy, engine: &Engine, report: &mut StageReport) {
    let verified = match engine.verify() {
        Ok(verified) => verified,
        Err(unusable) => {
            let archive = engine
                .expected
                .archive
                .clone()
                .unwrap_or_else(|| "none is pinned for this platform".to_string());
            report.violations.push(Violation::new(
                "git.secret_scan_engine",
                policy.secret_scan,
                format!(
                    "the gitleaks engine cannot run: {}; the pin is {}; `codeflow doctor --check secret-engine` reports where it looked",
                    unusable.describe(),
                    engine.expected.describe()
                ),
                crate::remedy::SECRET_ENGINE_UNAVAILABLE.with(&[("archive", &archive)]),
            ));
            return;
        }
    };
    match engine.scan_staged(&verified, root) {
        Ok(findings) => {
            for finding in findings {
                report.violations.push(Violation::new(
                    "git.secret_scan",
                    policy.secret_scan,
                    format!(
                        "possible secret (gitleaks rule {}) staged at {}:{}",
                        crate::git::display_text(&finding.rule),
                        crate::git::display_text(&finding.file),
                        finding.line
                    ),
                    crate::remedy::SECRET_STAGED.remedy(),
                ));
            }
        }
        Err(error) => report.violations.push(Violation::new(
            "git.secret_scan",
            policy.secret_scan,
            format!(
                "staged secret scan incomplete: {}",
                crate::git::display_text(&error)
            ),
            crate::remedy::SECRET_SCAN_INCOMPLETE.remedy(),
        )),
    }
}

/// Scan the staged diff (index vs HEAD) for .env files and secret content.
fn scan_staged(
    repo: &Repository,
    policy: &GitPolicy,
    report: &mut StageReport,
    abort_traversal_for_test: bool,
) {
    // The index git commits: `commit -a` and `commit <path>` name a
    // temporary one in GIT_INDEX_FILE, so the ordinary index would miss a
    // secret they record. An index that cannot be read fails closed.
    let index_file = super::conflict_markers::effective_index(repo);
    let index = match git2::Index::open(&index_file) {
        Ok(index) => index,
        Err(error) => {
            report.violations.push(Violation::new(
                "git.secret_scan",
                policy.secret_scan,
                format!(
                    "staged secret scan incomplete: cannot read the index {}: {error}",
                    index_file.display()
                ),
                crate::remedy::SECRET_SCAN_INCOMPLETE.remedy(),
            ));
            return;
        }
    };
    let read_diff = || -> Result<_, git2::Error> {
        let head_tree = match repo.head() {
            Ok(head) => Some(head.peel_to_tree()?),
            Err(e) if e.code() == git2::ErrorCode::UnbornBranch => None,
            Err(e) => return Err(e),
        };
        repo.diff_tree_to_index(head_tree.as_ref(), Some(&index), None)
    };
    let diff = match read_diff() {
        Ok(diff) => diff,
        Err(error) => {
            report.violations.push(Violation::always_blocking(
                "git.secret_scan",
                format!("staged secret scan incomplete: cannot read the staged diff: {error}"),
                &crate::remedy::SECRET_SCAN_INCOMPLETE.remedy(),
            ));
            return;
        }
    };

    for delta in diff.deltas() {
        // A staged DELETION is the remediation the policy prescribes (getting
        // a tracked env file OUT of git) — never a leak. Only content entering
        // the repo is scanned; the content scan below is already '+'-lines-only.
        if delta.status() == git2::Delta::Deleted {
            continue;
        }
        let Some(path) = delta.new_file().path_bytes() else {
            continue;
        };
        // OS text rule (issue 79): `is_env_file` tests ASCII names on the final
        // component only, and the escaped display form of a path ends the same
        // way the bytes do, so it answers as the bytes would.
        let path_str = crate::git::GitName::from_bytes(path).display().to_string();
        if scan::is_env_file(&path_str) {
            report.violations.push(Violation::new(
                "git.secret_scan",
                policy.secret_scan,
                format!("dotenv file staged for commit: {path_str}"),
                crate::remedy::ENV_FILE_STAGED.remedy(),
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
                            .path_bytes()
                            .map(|p| crate::git::GitName::from_bytes(p).display().to_string())
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
            crate::remedy::SECRET_SCAN_INCOMPLETE.remedy(),
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
            crate::remedy::SECRET_STAGED.remedy(),
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
            crate::remedy::COMMIT_TYPE.with(&[("types", &policy.commit_types.join(", "))]),
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
            crate::remedy::COMMIT_LENGTH.with(&[
                ("description", &policy.commit_desc_max_len.to_string()),
                ("subject", &policy.commit_subject_max_len.to_string()),
            ]),
        ));
    }
    if let Some(reason) = standards::check_subject_separator(cleaned) {
        report.violations.push(Violation::new(
            "git.commit_format",
            policy.commit_format,
            reason,
            crate::remedy::COMMIT_BLANK_LINE.remedy(),
        ));
    }
    if let Some(reason) = standards::check_breaking_footer(subject, cleaned) {
        report.violations.push(Violation::new(
            "git.commit_format",
            policy.commit_format,
            reason,
            crate::remedy::COMMIT_BREAKING_FOOTER.remedy(),
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
                crate::remedy::COMMIT_BODY.with(&[
                    ("bullets", &policy.commit_body_max_bullets.to_string()),
                    ("length", &policy.commit_body_bullet_max_len.to_string()),
                ]),
            ));
        }
        if let Some(reason) =
            standards::check_required_footers(subject, &cleaned, &policy.commit_required_footers)
        {
            report.violations.push(Violation::new(
                "git.commit_body",
                policy.commit_body,
                reason,
                crate::remedy::COMMIT_REQUIRED_FOOTERS
                    .with(&[("footers", &policy.commit_required_footers.join(", "))]),
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
                crate::remedy::COMMIT_TICKET
                    .with(&[("keys", &policy.commit_ticket_keys.join(", "))]),
            ));
        }
    }

    if policy.ai_attribution.is_active() {
        if let Some(which) = standards::find_attribution(&cleaned) {
            report.violations.push(Violation::new(
                "git.ai_attribution",
                policy.ai_attribution,
                format!("commit message contains AI attribution ({which})"),
                crate::remedy::COMMIT_AI_ATTRIBUTION.remedy(),
            ));
        }
    }

    if policy.commit_emoji.is_active() {
        if let Some(c) = standards::find_emoji(subject) {
            report.violations.push(Violation::new(
                "git.commit_emoji",
                policy.commit_emoji,
                format!("commit subject contains emoji ('{c}')"),
                crate::remedy::COMMIT_EMOJI.remedy(),
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
        crate::remedy::COMMIT_POLICY_CHARACTER.remedy(),
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
            WATCHED_PATH_RULE,
            PolicyLevel::Warn,
            format!(
                "commit touches a declared contract surface ({})",
                crate::git::display_key(path)
            ),
            crate::remedy::BREAKING_WATCH_PATH.remedy(),
        ));
    }
    report
}

/// The rule of the contract-surface tripwire.
pub const WATCHED_PATH_RULE: &str = "git.breaking_watch_paths";

/// Move each contract-surface finding into the notes (TSK-147 AC-4). A run
/// with no pull request body cannot state the Release impact that settles
/// it, so it is a note there that points at those fields, never a warning.
pub fn note_watched_paths(report: &mut StageReport) {
    let (watched, kept): (Vec<Violation>, Vec<Violation>) = std::mem::take(&mut report.violations)
        .into_iter()
        .partition(|v| v.rule == WATCHED_PATH_RULE);
    report.violations = kept;
    report.notes.extend(
        watched
            .into_iter()
            .map(|v| crate::remedy::Finding::new(v.message, v.remedy)),
    );
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

/// Which commit a `commit-msg` message file belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MessageFile {
    /// This worktree's `MERGE_MSG`: Git concluding a merge itself, which
    /// runs no `pre-commit`.
    AutomaticMerge,
    /// This worktree's `COMMIT_EDITMSG`: a commit `pre-commit` scanned.
    Commit,
    /// Neither.
    Other,
}

/// Classify `file` against the Git directory of the worktree `repo` opens,
/// which for a linked worktree is its own `.git/worktrees/<name>`.
fn classify_message_file(repo: &Repository, file: &Path) -> MessageFile {
    let Ok(file) = std::fs::canonicalize(file) else {
        return MessageFile::Other;
    };
    let is = |name: &str| std::fs::canonicalize(repo.path().join(name)).is_ok_and(|p| p == file);
    if is("MERGE_MSG") {
        MessageFile::AutomaticMerge
    } else if is("COMMIT_EDITMSG") {
        MessageFile::Commit
    } else {
        MessageFile::Other
    }
}

/// The staged secret scan in `commit-msg`, for the commit `pre-commit` never
/// saw (TSK-412, issue 296). Git runs `pre-merge-commit` and not
/// `pre-commit` when it concludes a merge itself, and writes `MERGE_HEAD`
/// only after that hook, so a merge driver's output reaches the commit
/// unscanned; Git then calls `commit-msg` with `MERGE_MSG`. With this
/// worktree's `MERGE_MSG` the scan runs here; with its `COMMIT_EDITMSG`,
/// `pre-commit` already ran it. Any other file is refused at the
/// `secret_scan` level and the staged content is still scanned, never
/// skipped. A relative `message_file` is read from `root`.
///
/// # Errors
///
/// Returns [`HookError::Config`] when `root` is not inside a git repository.
pub fn commit_msg_secret_scan(
    root: &Path,
    policy: &GitPolicy,
    message_file: &Path,
) -> Result<StageReport, HookError> {
    commit_msg_secret_scan_engine(root, policy, message_file, &Engine::from_env())
}

fn commit_msg_secret_scan_engine(
    root: &Path,
    policy: &GitPolicy,
    message_file: &Path,
    engine: &Engine,
) -> Result<StageReport, HookError> {
    let mut report = StageReport::default();
    if !policy.secret_scan.is_active() {
        return Ok(report);
    }
    let repo = Repository::discover(root)
        .map_err(|e| HookError::Config(format!("not a git repository: {e}")))?;
    match classify_message_file(&repo, &root.join(message_file)) {
        MessageFile::Commit => {}
        MessageFile::AutomaticMerge => secret_scan(&repo, root, policy, engine, &mut report),
        MessageFile::Other => {
            report.violations.push(Violation::new(
                "git.secret_scan",
                policy.secret_scan,
                format!(
                    "commit-msg received {}, which is neither MERGE_MSG nor COMMIT_EDITMSG of this worktree, so it cannot tell whether pre-commit scanned the staged content; it scans the content here",
                    crate::git::display_path(message_file)
                ),
                crate::remedy::SECRET_SCAN_INCOMPLETE.remedy(),
            ));
            secret_scan(&repo, root, policy, engine, &mut report);
        }
    }
    Ok(report)
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

    let branch = current_branch(&repo).map_err(HookError::Config)?;
    if policy.merge_to_protected.is_active()
        && policy.branch_is_protected(&branch)
        && !integrate_token
        && !human_override
    {
        report.violations.push(Violation::new(
            "git.merge_to_protected",
            policy.merge_to_protected,
            format!(
                "merge commit on protected branch '{}'",
                crate::git::display_text(&branch)
            ),
            crate::remedy::PROTECTED_BRANCH.remedy(),
        ));
    }
    report
        .violations
        .extend(crate::root_checkout::hook_violation(
            root,
            policy,
            &crate::root_checkout::process_env,
        ));

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
    // Git's protocol separates the fields with a single space, and a ref name
    // holds none: other whitespace (a no-break space) is part of the name.
    line.splitn(3, ' ').nth(2).is_some_and(|r| {
        super::git_guard::strip_ref_prefix(r, "refs/heads/").is_some()
            || super::git_guard::transaction_ref_aliases(r)
    })
}

/// Refuses a ref update whose spelling can alias a protected ref, unless
/// the transaction is sanctioned; the ref name is shown escaped (TSK-388).
fn refuse_alias(
    report: &mut StageReport,
    policy: &GitPolicy,
    sanctioned: bool,
    refname: &str,
    why: &str,
) {
    if policy.local_ref_protection.is_active() && !sanctioned {
        report.violations.push(Violation::new(
            "git.local_ref_protection",
            policy.local_ref_protection,
            format!(
                "ref update of '{}': {why}",
                crate::git::display_text(refname)
            ),
            crate::remedy::PROTECTED_BRANCH.remedy(),
        ));
    }
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
    // Whether non-ASCII names may alias here: probed once per invocation
    // unless a caller has decided it (TSK-381 SF-3).
    let _aliasing = super::policy::NonAsciiAliasingScope::decide(|| {
        super::repo::case_insensitive_git_dir(repo.commondir())
    });
    // The integrate token and a human's override sanction the transaction
    // (git layer only — the git-guard never trusts the human override).
    let sanctioned = integrate_token || human_override;

    let collisions = if super::policy::non_ascii_aliasing() {
        folded_collisions(&repo, stdin)
    } else {
        std::collections::BTreeSet::new()
    };
    for line in stdin.split('\n') {
        let Some((old_oid, new_oid, refname)) = parse_ref_line(line) else {
            continue;
        };
        // On a case-insensitive git directory, a ref written under a
        // spelling that folds onto another ref is that ref's file, so the
        // transaction is refused (TSK-381 SF-4).
        // Storage housekeeping that keeps the value (`git pack-refs`, `git
        // gc`) moves nothing, so the spelling checks below let it pass.
        let unchanged = || keeps_value(&repo, refname, old_oid, new_oid);
        if collisions.contains(refname) && !unchanged() {
            refuse_alias(
                &mut report,
                policy,
                sanctioned,
                refname,
                "its name folds onto another ref's on a case-insensitive filesystem",
            );
            continue;
        }
        // A fetched remote-tracking name that folds onto a protected branch
        // without naming it exactly (`origin/MAIN` beside a protected `main`)
        // would land on that tracking ref once it exists (Codex F3).
        if super::policy::non_ascii_aliasing()
            && tracking_name_folds_onto_protected(&repo, policy, refname)
            && !unchanged()
        {
            refuse_alias(
                &mut report,
                policy,
                sanctioned,
                refname,
                "its name folds onto a protected branch's tracking ref on a \
                 case-insensitive filesystem",
            );
            continue;
        }
        // A spelling a case-insensitive filesystem may store as a guarded ref
        // is refused outright, failing closed (TSK-381 SF-3).
        if super::git_guard::transaction_ref_aliases(refname) && !unchanged() {
            refuse_alias(
                &mut report,
                policy,
                sanctioned,
                refname,
                "a non-ASCII character or a case-different `refs/` prefix can name a \
                 protected ref on a case-insensitive filesystem",
            );
            continue;
        }
        let Some(branch) = super::git_guard::strip_ref_prefix(refname, "refs/heads/") else {
            continue;
        };
        if !policy.branch_is_protected_any_case(branch) {
            continue; // feature branches stay fully free (rebase, force, etc.)
        }
        // Storage housekeeping (`git pack-refs`, run by `git gc`) rewrites
        // where a ref is kept, not what it points to: not an update.
        if keeps_value(&repo, refname, old_oid, new_oid) {
            continue;
        }

        // Deletion (new-oid all zeros): governed by delete_protected, which
        // never had a client-hook reach before (`git branch -D` fired nothing).
        if is_zero_sha(new_oid) {
            if policy.delete_protected.is_active() && !sanctioned {
                report.violations.push(Violation::new(
                    "git.delete_protected",
                    policy.delete_protected,
                    format!(
                        "deleting protected branch '{}' via a local ref update",
                        crate::git::display_text(branch)
                    ),
                    crate::remedy::PROTECTED_DELETE.remedy(),
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
            format!(
                "local update of protected branch '{}' that is not a sync from origin",
                crate::git::display_text(branch)
            ),
            crate::remedy::PROTECTED_BRANCH.remedy(),
        ));
    }
    Ok(report)
}

/// Whether a remote-tracking `refname` folds onto a protected branch it does
/// not name exactly ([`GitPolicy::branch_folds_onto_protected`]). The branch
/// is the part after the longest `refs/remotes/<remote>/` prefix of a
/// configured remote, so `origin/feat/MAIN` is the branch `feat/MAIN`, which
/// folds onto nothing protected. Under no configured remote, or when the
/// remotes cannot be read, every branch the name could end in is judged,
/// failing closed for an unconfigured destination.
fn tracking_name_folds_onto_protected(
    repo: &Repository,
    policy: &GitPolicy,
    refname: &str,
) -> bool {
    let Some(rest) = refname.strip_prefix("refs/remotes/") else {
        return false;
    };
    // A remote name that is not valid UTF-8 names no text prefix.
    let remote = repo.remotes().ok().and_then(|remotes| {
        remotes
            .iter_bytes()
            .filter_map(|name| {
                crate::git::GitName::from_bytes(name)
                    .rule_text()
                    .ok()
                    .map(str::to_string)
            })
            .filter(|name| rest.starts_with(&format!("{name}/")))
            .map(|name| name.len())
            .max()
    });
    match remote {
        Some(len) => policy.branch_folds_onto_protected(&rest[len + 1..]),
        None => rest
            .match_indices('/')
            .any(|(at, _)| policy.branch_folds_onto_protected(&rest[at + 1..])),
    }
}

/// The ref names this transaction writes (a new value that is not zero)
/// whose folded spelling ([`super::repo::folded_ref_name`]), or that of a
/// directory along its path, equals that of another spelling among the
/// existing refs and the names written in the same transaction. Such a name
/// lands in the other ref's file or directory on a case-insensitive
/// filesystem (`origin/INTEGRATION/x` under an existing `origin/integration/`).
/// A listing that cannot be read leaves every written name a collision,
/// failing closed.
fn folded_collisions(repo: &Repository, stdin: &str) -> std::collections::BTreeSet<String> {
    let written: Vec<&str> = stdin
        .split('\n')
        .filter_map(parse_ref_line)
        .filter(|(_, new_oid, _)| !is_zero_sha(new_oid))
        .map(|(_, _, refname)| refname)
        .collect();
    let mut spellings: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        std::collections::BTreeMap::new();
    let every = || written.iter().map(|name| (*name).to_string()).collect();
    let Ok(references) = repo.references() else {
        return every();
    };
    let mut listed = Vec::new();
    for reference in references {
        // A ref that cannot be read cannot be ruled out as an alias. A name
        // that is not valid UTF-8 is compared byte for byte: packed-refs
        // matches names exactly, ext4 casefold treats such a name as opaque
        // bytes and does not fold it, and APFS refuses to store it.
        let Ok(reference) = reference else {
            return every();
        };
        if let Ok(text) = crate::git::GitName::from_bytes(reference.name_bytes()).rule_text() {
            listed.push(text.to_string());
        }
    }
    // Each ref name and each directory along it ("refs/", "refs/remotes/",
    // ...), keyed by its folded spelling.
    let paths = |name: &str| {
        name.match_indices('/')
            .map(|(at, _)| name[..=at].to_string())
            .chain(std::iter::once(name.to_string()))
            .collect::<Vec<_>>()
    };
    for name in listed
        .iter()
        .map(String::as_str)
        .chain(written.iter().copied())
    {
        for path in paths(name) {
            spellings
                .entry(super::repo::folded_ref_name(&path))
                .or_default()
                .insert(path);
        }
    }
    written
        .into_iter()
        .filter(|name| {
            paths(name).iter().any(|path| {
                spellings
                    .get(&super::repo::folded_ref_name(path))
                    .is_some_and(|names| names.len() > 1)
            })
        })
        .map(str::to_string)
        .collect()
}

/// `true` when a transaction line leaves `refname` resolving to the commit it
/// resolves to now, so it moves nothing. Two such lines come from
/// `git pack-refs` (and so `git gc`) in the files backend:
/// - the write into packed-refs, `<zero or old> <X>`, where X is the ref's
///   current value;
/// - the prune of the loose copy, `<X> <zero>`, while packed-refs holds the
///   same X and is not locked for rewriting. A real deletion always locks
///   packed-refs to drop the entry there too, and its packed transaction
///   reports `<zero> <zero>`, so it still reaches the deletion rule.
fn keeps_value(repo: &Repository, refname: &str, old_oid: &str, new_oid: &str) -> bool {
    let current = repo.refname_to_id(refname).ok();
    if !is_zero_sha(new_oid) {
        return git2::Oid::from_str(new_oid).is_ok_and(|new| current == Some(new));
    }
    let Ok(old) = git2::Oid::from_str(old_oid) else {
        return false;
    };
    let common = repo.commondir();
    if is_zero_sha(old_oid)
        || current != Some(old)
        || !crate::absence::proven_absent(&common.join("packed-refs.lock"))
            .is_ok_and(|absent| absent)
    {
        return false;
    }
    let loose = std::fs::read_to_string(common.join(refname)).ok();
    let packed = std::fs::read_to_string(common.join("packed-refs")).ok();
    loose.is_some_and(|loose| loose.strip_suffix('\n').unwrap_or(&loose) == old_oid)
        && packed.is_some_and(|packed| {
            packed.split('\n').any(|line| {
                line.split_once(' ')
                    .is_some_and(|(sha, name)| sha == old_oid && name == refname)
            })
        })
}

/// Parse one `<old-oid> <new-oid> <ref-name>` reference-transaction line.
fn parse_ref_line(line: &str) -> Option<(&str, &str, &str)> {
    // The fields are separated by single spaces (see
    // `ref_line_touches_local_branch`); the ref name is the rest of the line.
    let mut parts = line.splitn(3, ' ');
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
    let refname = match repo.branch_upstream_name(&format!("refs/heads/{branch}")) {
        Ok(upstream) => match upstream.as_str() {
            Ok(name) => name.to_string(),
            Err(_) => return false,
        },
        Err(error) if error.code() == git2::ErrorCode::NotFound => {
            format!("refs/remotes/origin/{branch}")
        }
        Err(_) => return false,
    };
    // Only the exact spelling git lists counts as the remote head (TSK-381
    // SF-4): a folded alias fetched over it proves no sync.
    super::repo::listed_ref_commit(repo, &refname)
        .ok()
        .flatten()
        == Some(new)
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
        .split('\n')
        .filter_map(|line| {
            // pre-push's stdin separates the fields with single spaces, and a
            // ref name holds none, so another whitespace is part of the name.
            let mut parts = line.splitn(4, ' ');
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
/// naming against `branch_prefixes`. The push set runs separately. A
/// fast-forward push to a protected branch steps aside for the `codeflow
/// integrate` token or a human's
/// [`HUMAN_OVERRIDE_ENV`](super::HUMAN_OVERRIDE_ENV) (ADR-0007); deleting or
/// force-pushing a protected branch never does.
///
/// # Errors
///
/// Returns [`HookError::Config`] when `root` is not inside a git repository.
pub fn pre_push(
    root: &Path,
    policy: &GitPolicy,
    refs: &[PushRef],
    integrate_token: bool,
    human_override: bool,
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
                    format!(
                        "push would delete protected branch '{}'",
                        crate::git::display_text(branch)
                    ),
                    crate::remedy::PROTECTED_DELETE.remedy(),
                ));
            }
            continue;
        }

        if protected && policy.push_to_protected.is_active() && !integrate_token && !human_override
        {
            report.violations.push(Violation::new(
                "git.push_to_protected",
                policy.push_to_protected,
                format!(
                    "direct push to protected branch '{}'",
                    crate::git::display_text(branch)
                ),
                crate::remedy::PROTECTED_BRANCH.remedy(),
            ));
        }

        let (force_level, force_rule, remedy) = if protected {
            (
                policy.force_push_protected,
                "git.force_push_protected",
                crate::remedy::PROTECTED_BRANCH.remedy(),
            )
        } else {
            (
                policy.force_push_unprotected,
                "git.force_push_unprotected",
                crate::remedy::FORCE_PUSH.remedy(),
            )
        };
        if force_level.is_active() {
            match is_force_update(&repo, r) {
                Ok(false) => {},
                Ok(true) => report.violations.push(Violation::new(
                    force_rule, force_level,
                    format!("non-fast-forward (force) push to {}branch '{}'", if protected { "protected " } else { "" }, crate::git::display_text(branch)),
                    remedy,
                )),
                Err(reason) => report.violations.push(Violation::always_blocking(
                    force_rule,
                    format!("push to branch '{}' cannot be proven a fast-forward: {reason}; fetch it first", crate::git::display_text(branch)),
                    &remedy,
                )),
            }
        }

        if branch.starts_with("integration/EPC-") {
            epic_line_push(root, &repo, r, branch, &mut report);
        }

        if !protected && policy.branch_naming.is_active() && !policy.branch_name_ok(branch) {
            report.violations.push(Violation::new(
                "git.branch_naming",
                policy.branch_naming,
                format!(
                    "branch '{}' does not match `{{prefix}}/{{kebab-name}}`",
                    crate::git::display_text(branch)
                ),
                crate::remedy::BRANCH_NAME.with(&[("prefixes", &policy.branch_prefixes.join(" "))]),
            ));
        }
    }

    // The push set (`test_gate_on_push`) is run by the CLI hook, which binds
    // each check to the pushed commits: see `run_push_targets`.

    Ok(report)
}

/// The epic-line push rule (SPC-013 R-52, amended by TSK-248): a push to
/// `integration/EPC-*` must not add a commit made directly on the line that
/// no landed `line_adoptions` entry names. Only the pushed range is judged,
/// so a line that already carries an older direct commit still receives the
/// merges that repair it. It runs where durable work tracking is on at the
/// pushed tip or at the default target, as CI's `tracking_on` judges it at
/// the head or the target, so a direct commit cannot switch the check off
/// by deleting the records. A tracking state or range it cannot read is a
/// note: CI stays the hard line (charter D19).
fn epic_line_push(
    root: &Path,
    repo: &Repository,
    pushed: &PushRef,
    branch: &str,
    report: &mut StageReport,
) {
    let skipped = |report: &mut StageReport, error: &str| {
        report.notes.push(crate::remedy::Finding::new(
            format!(
                "the epic-line push check for '{}' was skipped: {error}",
                crate::git::display_text(branch)
            ),
            crate::remedy::HOOK_UNEVALUATED.remedy(),
        ));
    };
    // A default target that cannot be read is reported, never read as none.
    // Its full ref is read: a short `origin/main` peels a local branch or
    // tag of that name before the remote-tracking ref.
    let default_target = crate::workgraph::default_work_target_ref(root)
        .map_err(|error| format!("the default target cannot be read: {error}"));
    let target_revision = match &default_target {
        Ok(target) => target.as_deref(),
        Err(_) => None,
    };
    let tracking = [Some(pushed.local_sha.as_str()), target_revision]
        .into_iter()
        .flatten()
        .map(|revision| crate::workgraph::durable_work_tracking_enabled_at(root, revision))
        .collect::<Vec<_>>();
    if !tracking.iter().any(|state| state == &Ok(true)) {
        if let Some(Err(error)) = tracking.iter().find(|state| state.is_err()) {
            skipped(
                report,
                &format!("durable work tracking cannot be read: {error}"),
            );
        } else if let Err(error) = &default_target {
            skipped(report, error);
        }
        return;
    }
    let first_push = is_zero_sha(&pushed.remote_sha) || pushed.remote_sha.is_empty();
    let judged = (|| {
        let target = default_target?.ok_or("no main or master branch to judge the line against")?;
        let base = if first_push {
            let commit = |revision: &str| {
                repo.revparse_single(revision)
                    .and_then(|object| object.peel_to_commit())
                    .map(|commit| commit.id())
                    .map_err(|e| format!("{revision}: {e}"))
            };
            let target_tip = repo
                .find_reference(&target)
                .and_then(|reference| reference.peel_to_commit())
                .map(|commit| commit.id())
                .map_err(|e| format!("{target}: {e}"))?;
            history::merge_base(repo, target_tip, commit(&pushed.local_sha)?)
                .map_err(|e| e.to_string())?
                .to_string()
        } else {
            pushed.remote_sha.clone()
        };
        let adoptions = crate::workgraph::line_adoption::check_range(
            root,
            branch,
            &target,
            &base,
            &pushed.local_sha,
        )?;
        Ok::<_, String>((target, adoptions))
    })();
    let (target, adoptions) = match judged {
        Ok(judged) => judged,
        Err(error) => {
            skipped(report, &error);
            return;
        }
    };
    if adoptions.unadopted.is_empty() {
        return;
    }
    let epic = crate::workgraph::line_adoption::epic_id(branch).unwrap_or("its epic");
    let ids = adoptions
        .unadopted
        .iter()
        .map(|id| &id[..9])
        .collect::<Vec<_>>()
        .join(", ");
    let reset = if first_push {
        target
    } else {
        format!("refs/remotes/origin/{branch}")
    };
    report.violations.push(Violation::always_blocking(
        "work.epic_line",
        format!("{} would receive a commit made directly on the line ({ids})", crate::git::display_text(branch)),
        &format!(
            "land it by a pull request: save it on a task or planning branch, then `git reset --keep {reset}` and open a task or planning pull request to the line; a commit already on the remote is adopted by a `line_adoptions` entry in {epic} (see git-rules.md, \"Bodies of work\")"
        ),
    ));
}

/// The registry's push rule (SPC-013 R-8, R-108, R-109): never deleted or
/// force-pushed, and every commit in the pushed range adds new `ids/` files
/// only or is a typed restore. It blocks whatever the policy says, and a
/// range that cannot be read blocks too.
fn registry_push(root: &Path, repo: &Repository, r: &PushRef, report: &mut StageReport) {
    let block = |message: String| {
        Violation::always_blocking(
            "registry.append_only",
            message,
            "issue ids with `codeflow task|epic|spec new`; repair damage with `codeflow ids restore <id>...`",
        )
    };
    if r.is_delete() {
        report.violations.push(block(
            "push would delete `codeflow/registry`; the registry only grows (R-8)".to_string(),
        ));
        return;
    }
    match is_force_update(repo, r) {
        Ok(false) => {}
        Ok(true) => {
            report.violations.push(block(
                "non-fast-forward (force) push to `codeflow/registry`; the registry only grows (R-8)".to_string(),
            ));
            return;
        }
        Err(reason) => {
            report.violations.push(block(format!(
                "cannot prove registry push is append-only: {reason}; fetch its remote tip first"
            )));
            return;
        }
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

/// Whether the update rewrites history; unreadable ancestry is an error.
fn is_force_update(repo: &Repository, r: &PushRef) -> Result<bool, String> {
    if is_zero_sha(&r.remote_sha) || r.remote_sha.is_empty() {
        return Ok(false); // new branch on the remote
    }
    let local =
        git2::Oid::from_str(&r.local_sha).map_err(|e| format!("invalid local object id: {e}"))?;
    let remote =
        git2::Oid::from_str(&r.remote_sha).map_err(|e| format!("invalid remote object id: {e}"))?;
    if local == remote {
        return Ok(false);
    }
    history::descendant_of(repo, local, remote)
        .map(|descends| !descends)
        .map_err(|e| format!("cannot read push ancestry: {e}"))
}

/// Time budget for the whole pre-push set. A push set that takes longer still
/// runs to its verdict, but the hook names the slowest step (TSK-132).
pub const PUSH_SET_BUDGET: std::time::Duration = std::time::Duration::from_secs(60);

/// One timed step of the push set.
#[derive(Debug, Clone)]
pub struct PushStep {
    /// The built-in command; the push set runs no test-config target
    /// (TSK-449).
    pub name: String,
    pub duration: std::time::Duration,
}

/// When the whole push set took longer than `budget`, the note naming its
/// slowest built-in step and the push-set rule that drops it.
#[must_use]
pub fn over_budget_note(
    steps: &[PushStep],
    total: std::time::Duration,
    budget: std::time::Duration,
) -> Option<crate::remedy::Finding> {
    if total <= budget {
        return None;
    }
    let head = format!(
        "push set took {:.1}s, over its {}s budget",
        total.as_secs_f64(),
        budget.as_secs()
    );
    Some(match steps.iter().max_by_key(|s| s.duration) {
        Some(slow) => crate::remedy::Finding::new(
            format!(
                "{head}; slowest step: built-in `{}` ({:.1}s), which has no push-set key",
                slow.name,
                slow.duration.as_secs_f64()
            ),
            crate::remedy::PUSH_OVER_BUDGET_BUILTIN.remedy(),
        ),
        None => crate::remedy::Finding::new(head, crate::remedy::PUSH_OVER_BUDGET_BUILTIN.remedy()),
    })
}

#[cfg(test)]
mod tests {

    #[cfg(unix)]
    #[test]
    fn r19_dangling_packed_ref_lock_withholds_prune_exemption() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        let repo = Repository::open(dir.path()).unwrap();
        let oid = repo.head().unwrap().target().unwrap();
        let name = "refs/heads/feat/x";
        std::fs::write(repo.path().join("packed-refs"), format!("{oid} {name}\n")).unwrap();
        assert!(keeps_value(&repo, name, &oid.to_string(), &"0".repeat(40)));
        std::os::unix::fs::symlink(
            dir.path().join("missing"),
            repo.path().join("packed-refs.lock"),
        )
        .unwrap();
        assert!(!keeps_value(&repo, name, &oid.to_string(), &"0".repeat(40)));
    }

    #[cfg(unix)]
    #[test]
    fn r18_merge_marker_absence_requires_resolved_ancestors() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!super::merge_head_present(dir.path()).unwrap());
        let marker = dir.path().join("MERGE_HEAD");
        std::os::unix::fs::symlink(dir.path().join("missing"), &marker).unwrap();
        assert!(super::merge_head_present(dir.path()).is_err());
        let ancestor = dir.path().join("broken");
        std::os::unix::fs::symlink(dir.path().join("missing"), &ancestor).unwrap();
        assert!(super::merge_head_present(&ancestor).is_err());
        std::fs::remove_file(marker).unwrap();
        std::fs::write(dir.path().join("MERGE_HEAD"), "abc\n").unwrap();
        assert!(super::merge_head_present(dir.path()).unwrap());
    }

    #[test]
    fn r15_extra_packed_ref_framing_does_not_erase_cr() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = git2::Signature::now("Test", "test@example.invalid").unwrap();
        let oid = repo
            .commit(None, &sig, &sig, "fixture", &tree, &[])
            .unwrap();
        let name = "refs/heads/release";
        repo.reference(name, oid, false, "fixture").unwrap();
        std::fs::write(repo.path().join("packed-refs"), format!("{oid} {name}\r\n")).unwrap();
        assert!(!keeps_value(&repo, name, &oid.to_string(), &"0".repeat(40)));
    }

    #[test]
    fn r15_owned_push_protocol_keeps_carriage_return() {
        let parsed = parse_push_refs("refs/heads/x abc refs/heads/y def\r\n");
        assert_eq!(parsed[0].remote_sha, "def\r");
    }

    #[test]
    fn packed_refs_keep_unicode_whitespace_in_names() {
        let temp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(temp.path()).unwrap();
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = git2::Signature::now("Test", "test@example.invalid").unwrap();
        let oid = repo
            .commit(None, &sig, &sig, "fixture", &tree, &[])
            .unwrap();
        let name = "refs/heads/release\u{a0}";
        repo.reference(name, oid, false, "fixture").unwrap();
        std::fs::write(repo.path().join("packed-refs"), format!("{oid} {name}\n")).unwrap();
        assert!(keeps_value(&repo, name, &oid.to_string(), &"0".repeat(40)));
        // A lookalike packed name must not authorize pruning a different ref.
        repo.reference("refs/heads/release", oid, false, "fixture")
            .unwrap();
        assert!(!keeps_value(
            &repo,
            "refs/heads/release",
            &oid.to_string(),
            &"0".repeat(40)
        ));
    }
    use std::path::Path;

    use super::super::policy::PolicyLevel;
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let out = crate::git::command()
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

    /// The scan fixtures commit at the root checkout on `feat/x`; they judge
    /// the secret scan alone, so the root-checkout rule (TSK-165) is off.
    fn scan_policy() -> GitPolicy {
        GitPolicy {
            root_checkout_commits: PolicyLevel::Off,
            ..GitPolicy::default()
        }
    }

    /// A repository on `main` with a linked worktree on `feat/x`, where
    /// feature work belongs; returns the worktree.
    fn feature_worktree(dir: &Path) -> std::path::PathBuf {
        init_repo(dir, "main");
        git(
            dir,
            &["worktree", "add", "-q", ".worktrees/x", "-b", "feat/x"],
        );
        dir.join(".worktrees/x")
    }

    // -- pre-commit --

    #[test]
    fn test_pre_commit_clean_on_feature_branch() {
        let dir = tempfile::tempdir().unwrap();
        let wt = feature_worktree(dir.path());
        stage(&wt, "src/lib.rs", "pub fn hello() {}\n");
        let report = pre_commit(&wt, &GitPolicy::default(), false, false).unwrap();
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
        let report = pre_commit(dir.path(), &GitPolicy::default(), false, false).unwrap();
        assert_eq!(report.violations.len(), 1);
        assert_eq!(report.violations[0].rule, "git.commit_to_protected");
        assert_eq!(report.violations[0].level, PolicyLevel::Block);
    }

    #[test]
    fn test_pre_commit_integrate_token_passes_protected() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        stage(dir.path(), "a.txt", "hello\n");
        let report = pre_commit(dir.path(), &GitPolicy::default(), true, false).unwrap();
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_pre_commit_human_override_passes_protected() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        stage(dir.path(), "a.txt", "hello\n");
        let report = pre_commit(dir.path(), &GitPolicy::default(), false, true).unwrap();
        assert!(report.violations.is_empty(), "{:?}", report.violations);
    }

    #[test]
    fn test_pre_commit_human_override_keeps_the_env_block() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        stage(dir.path(), ".env", "TOKEN=x\n");
        let report = pre_commit(dir.path(), &GitPolicy::default(), false, true).unwrap();
        assert!(
            report
                .violations
                .iter()
                .all(|v| v.rule != "git.commit_to_protected"),
            "{:?}",
            report.violations
        );
        assert!(
            !report.violations.is_empty(),
            "the staged .env still blocks"
        );
    }

    #[test]
    fn test_pre_commit_warn_level_on_protected() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let policy = GitPolicy {
            commit_to_protected: PolicyLevel::Warn,
            ..GitPolicy::default()
        };
        let report = pre_commit(dir.path(), &policy, false, false).unwrap();
        assert_eq!(report.violations[0].level, PolicyLevel::Warn);
    }

    #[test]
    fn test_pre_commit_release_glob_extension() {
        // AC #3: release/* added to policy is honored by this plane too.
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "release/2.0");
        let report = pre_commit(dir.path(), &release_policy(), false, false).unwrap();
        assert_eq!(report.violations[0].rule, "git.commit_to_protected");
    }

    #[test]
    fn test_pre_commit_blocks_staged_env_file() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        stage(dir.path(), ".env", "DB_PASSWORD=hunter2hunter2\n");
        let report = pre_commit(dir.path(), &GitPolicy::default(), false, false).unwrap();
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
        let report = pre_commit(dir.path(), &scan_policy(), false, false).unwrap();
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
        let report = pre_commit(dir.path(), &scan_policy(), false, false).unwrap();
        assert!(
            report.violations.is_empty(),
            "deleting a tracked dotenv file must pass: {:?}",
            report.violations
        );
    }

    #[test]
    fn test_pre_commit_env_example_needs_a_path_exception() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        stage(dir.path(), ".env.example", "DB_PASSWORD=\n");
        let report = pre_commit(dir.path(), &scan_policy(), false, false).unwrap();
        assert!(report
            .violations
            .iter()
            .all(|v| v.rule != "git.secret_scan"));
        assert!(report
            .violations
            .iter()
            .any(|v| v.rule == "git.path_policy"));
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
        let report = pre_commit(dir.path(), &scan_policy(), false, false).unwrap();
        assert_eq!(report.violations.len(), 1);
        assert_eq!(report.violations[0].rule, "git.secret_scan");
        assert!(report.violations[0].message.contains("src/config.rs"));
        assert!(report.violations[0].message.contains("AWS access key id"));
    }

    #[test]
    fn test_pre_commit_secret_scan_off_keeps_path_policy() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        stage(dir.path(), ".env", "X=1\n");
        let policy = GitPolicy {
            secret_scan: PolicyLevel::Off,
            ..scan_policy()
        };
        let report = pre_commit(dir.path(), &policy, false, false).unwrap();
        assert!(report
            .violations
            .iter()
            .all(|v| v.rule != "git.secret_scan"));
        assert!(report
            .violations
            .iter()
            .any(|v| v.rule == "git.path_policy"));
    }

    #[test]
    fn test_pre_commit_outside_repo_is_config_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(pre_commit(dir.path(), &GitPolicy::default(), false, false).is_err());
    }

    /// A marker line built at run time, so this file holds none itself.
    fn marker(fill: char, size: usize, label: &str) -> String {
        format!("{}{label}", fill.to_string().repeat(size))
    }

    fn leftover_conflict() -> String {
        format!(
            "{}\nours\n{}\ntheirs\n{}\n",
            marker('<', 7, " HEAD"),
            marker('=', 7, ""),
            marker('>', 7, " feat/y")
        )
    }

    fn marker_findings(report: &StageReport) -> Vec<&Violation> {
        report
            .violations
            .iter()
            .filter(|v| v.rule == "git.conflict_markers")
            .collect()
    }

    #[test]
    fn test_pre_commit_conflict_markers_at_each_level() {
        // TSK-170 AC-1: block by default, warn at warn, silent at off.
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        stage(dir.path(), "notes.md", &leftover_conflict());
        let report = pre_commit(dir.path(), &GitPolicy::default(), false, false).unwrap();
        let found = marker_findings(&report);
        assert_eq!(found.len(), 3, "{:?}", report.violations);
        assert!(found.iter().all(|v| v.level == PolicyLevel::Block));
        assert!(
            found[0].message.starts_with("notes.md:1 "),
            "{}",
            found[0].message
        );
        assert!(
            found[1].message.starts_with("notes.md:3 "),
            "{}",
            found[1].message
        );
        assert!(found[0]
            .remedy
            .contains("resolve the conflict and restage, or set conflict-marker-size"));
        for (level, expected) in [(PolicyLevel::Warn, 3), (PolicyLevel::Off, 0)] {
            let policy = GitPolicy {
                conflict_markers: level,
                ..GitPolicy::default()
            };
            let report = pre_commit(dir.path(), &policy, false, false).unwrap();
            let found = marker_findings(&report);
            assert_eq!(found.len(), expected, "{level:?}");
            assert!(found.iter().all(|v| v.level == level));
        }
    }

    #[test]
    fn test_pre_commit_conflict_markers_skip_headings_binaries_and_deletions() {
        // TSK-170 AC-2.
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        stage(dir.path(), "old.md", &leftover_conflict());
        git(
            dir.path(),
            &["commit", "-q", "-m", "chore: a file before adoption"],
        );
        git(dir.path(), &["rm", "-q", "old.md"]);
        stage(
            dir.path(),
            "README.md",
            &format!("Title\n{}\n\ntext\n", marker('=', 7, "")),
        );
        let binary = format!("\0{}\n", leftover_conflict());
        stage(dir.path(), "blob.bin", &binary);
        let report = pre_commit(dir.path(), &GitPolicy::default(), false, false).unwrap();
        assert!(
            marker_findings(&report).is_empty(),
            "{:?}",
            report.violations
        );
    }

    #[test]
    fn test_pre_commit_reads_the_staged_conflict_marker_size() {
        // TSK-170 AC-3: a fixture and its attribute staged together commit;
        // markers of the set size in that path are still found.
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        stage(dir.path(), "fixtures/merge.txt", &leftover_conflict());
        let report = pre_commit(dir.path(), &GitPolicy::default(), false, false).unwrap();
        assert_eq!(marker_findings(&report).len(), 3);
        stage(
            dir.path(),
            ".gitattributes",
            "fixtures/** conflict-marker-size=32\n",
        );
        let report = pre_commit(dir.path(), &GitPolicy::default(), false, false).unwrap();
        assert!(
            marker_findings(&report).is_empty(),
            "{:?}",
            report.violations
        );
        stage(
            dir.path(),
            "fixtures/real.txt",
            &format!(
                "{}\nx\n{}\n",
                marker('<', 32, " HEAD"),
                marker('>', 32, " b")
            ),
        );
        let report = pre_commit(dir.path(), &GitPolicy::default(), false, false).unwrap();
        let found = marker_findings(&report);
        assert_eq!(found.len(), 2, "{:?}", report.violations);
        assert!(found[0].message.starts_with("fixtures/real.txt:1 "));
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
        let wt = feature_worktree(dir.path());
        let report = pre_merge_commit(&wt, &GitPolicy::default(), false, false).unwrap();
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

    /// Round fourteen on issue 79: git separates the protocol fields with a
    /// single space, so a no-break space in a ref name is part of the name and
    /// the protected `release` plus that space is not read as `release`.
    #[test]
    fn a_ref_name_keeps_whitespace_that_is_part_of_it() {
        let name = "refs/heads/release\u{a0}";
        assert_eq!(
            parse_ref_line(&format!("aaa bbb {name}")),
            Some(("aaa", "bbb", name))
        );
        assert!(ref_line_touches_local_branch(&format!("aaa bbb {name}")));
        let pushed = parse_push_refs(&format!("(delete) {} {name} bbb\n", "0".repeat(40)));
        assert_eq!(pushed.len(), 1);
        assert_eq!(pushed[0].remote_ref, name);
        assert_eq!(pushed[0].remote_sha, "bbb");
    }

    /// Point `refs/remotes/origin/<branch>` at `oid` (a simulated fetched head).
    fn set_origin_ref(dir: &Path, branch: &str, oid: &str) {
        git(
            dir,
            &["update-ref", &format!("refs/remotes/origin/{branch}"), oid],
        );
    }

    /// A commit one ahead of `main`, made on a side branch so `main` itself
    /// stays where it is, as it does when the hook sees the move prepared.
    fn commit_ahead_of_main(dir: &Path) -> String {
        git(dir, &["checkout", "-q", "-b", "side"]);
        std::fs::write(dir.join("f.txt"), "x\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-m", "feat: local"]);
        let ahead = rev_parse(dir, "HEAD");
        git(dir, &["checkout", "-q", "main"]);
        ahead
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
        let ahead = commit_ahead_of_main(dir.path());
        let stdin = format!("{base} {ahead} refs/heads/main\n");
        let report =
            reference_transaction(dir.path(), &GitPolicy::default(), &stdin, false, false).unwrap();
        assert_eq!(report.violations.len(), 1);
        assert_eq!(report.violations[0].rule, "git.local_ref_protection");
        assert_eq!(report.violations[0].level, PolicyLevel::Block);
    }

    /// TSK-381 review round 2: git passes the ref name as typed, and on a
    /// case-insensitive filesystem `Refs/Heads/main` is the loose ref
    /// `refs/heads/main`, so a case-different prefix is still that branch.
    #[test]
    fn test_reference_transaction_reads_a_case_different_prefix_as_the_branch() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let base = rev_parse(dir.path(), "HEAD");
        set_origin_ref(dir.path(), "main", &base);
        let ahead = commit_ahead_of_main(dir.path());
        for name in ["Refs/Heads/main", "REFS/HEADS/main", "refs/Heads/main"] {
            assert!(ref_line_touches_local_branch(&format!(
                "{ZERO40} {ahead} {name}"
            )));
            let stdin = format!("{ZERO40} {ahead} {name}\n");
            let report =
                reference_transaction(dir.path(), &GitPolicy::default(), &stdin, false, false)
                    .unwrap();
            assert_eq!(report.violations.len(), 1, "{name}");
            assert_eq!(
                report.violations[0].rule, "git.local_ref_protection",
                "{name}"
            );
        }
    }

    /// TSK-381 round 3: a case-different protected branch name is the same
    /// loose ref on a case-insensitive filesystem, so the backstop blocks
    /// it; an unrelated branch stays free.
    #[test]
    fn test_reference_transaction_blocks_a_case_different_protected_branch() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let base = rev_parse(dir.path(), "HEAD");
        set_origin_ref(dir.path(), "main", &base);
        let ahead = commit_ahead_of_main(dir.path());
        for name in ["refs/heads/MAIN", "refs/heads/Main"] {
            let stdin = format!("{ZERO40} {ahead} {name}\n");
            let report =
                reference_transaction(dir.path(), &GitPolicy::default(), &stdin, false, false)
                    .unwrap();
            assert_eq!(report.violations.len(), 1, "{name}");
            assert_eq!(
                report.violations[0].rule, "git.local_ref_protection",
                "{name}"
            );
        }
        let stdin = format!("{ZERO40} {ahead} refs/heads/feat/Main\n");
        let report =
            reference_transaction(dir.path(), &GitPolicy::default(), &stdin, false, false).unwrap();
        assert!(report.violations.is_empty(), "{:?}", report.violations);
    }

    /// TSK-381 SF-3: a non-ASCII or case-folded spelling may be stored as a
    /// protected ref, so the backstop blocks it; what `git fetch` writes for
    /// a non-ASCII remote branch or tag, and an ordinary branch, stay free.
    #[test]
    fn test_reference_transaction_blocks_non_ascii_and_folded_prefix_names() {
        let _insensitive = crate::hooks::policy::NonAsciiAliasingScope::set(true);
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let base = rev_parse(dir.path(), "HEAD");
        set_origin_ref(dir.path(), "main", &base);
        let ahead = commit_ahead_of_main(dir.path());
        let run = |name: &str| {
            reference_transaction(
                dir.path(),
                &GitPolicy::default(),
                &format!("{ZERO40} {ahead} {name}\n"),
                false,
                false,
            )
            .unwrap()
            .violations
        };
        for name in [
            "refs/heads/ma\u{17f}ter",
            "ref\u{17f}/heads/main",
            "ref\u{17f}/remotes/origin/main",
            "REFS/remotes/origin/main",
            "refs/heads/feat/caf\u{e9}",
        ] {
            assert!(
                ref_line_touches_local_branch(&format!("{ZERO40} {ahead} {name}")),
                "{name}"
            );
            let v = run(name);
            assert_eq!(v.len(), 1, "{name}: {v:?}");
            assert_eq!(v[0].rule, "git.local_ref_protection", "{name}");
        }
        // Codex F3: a fetched non-ASCII tracking name that lands on no other
        // ref is judged by the collision checks, not refused for its bytes.
        for name in [
            "refs/remotes/origin/caf\u{e9}",
            "refs/tags/caf\u{e9}",
            "refs/heads/feat/y",
        ] {
            assert!(run(name).is_empty(), "{name}");
        }
    }

    /// TSK-381 SF-4: on a case-insensitive git directory a written name that
    /// folds onto another ref, existing or in the same transaction, is
    /// refused; an ordinary new tracking branch and a case-sensitive git
    /// directory stay free.
    #[test]
    fn test_reference_transaction_refuses_folded_name_collisions() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let base = rev_parse(dir.path(), "HEAD");
        set_origin_ref(dir.path(), "main", &base);
        // A new value: a line that keeps a ref's value passes as housekeeping.
        let ahead = commit_ahead_of_main(dir.path());
        let run = |lines: &[&str]| {
            let stdin = lines
                .iter()
                .map(|name| format!("{ZERO40} {ahead} {name}\n"))
                .collect::<Vec<_>>()
                .concat();
            reference_transaction(dir.path(), &GitPolicy::default(), &stdin, false, false)
                .unwrap()
                .violations
        };
        {
            let _insensitive = crate::hooks::policy::NonAsciiAliasingScope::set(true);
            assert_eq!(run(&["refs/remotes/origin/MAIN"]).len(), 1);
            assert_eq!(
                run(&["refs/remotes/origin/feat", "refs/remotes/origin/FEAT"]).len(),
                2
            );
            assert!(run(&["refs/remotes/origin/feat/new"]).is_empty());
        }
        let _sensitive = crate::hooks::policy::NonAsciiAliasingScope::set(false);
        assert!(run(&["refs/remotes/origin/MAIN"]).is_empty());
    }

    /// TSK-381 Codex F3 and the folded directory finding: where the git
    /// directory folds names, a fetched tracking name is refused only when it
    /// lands on another ref, folds onto a protected branch it does not name,
    /// or folds onto an existing directory spelled differently; an unrelated
    /// Unicode or ASCII tracking name passes.
    #[test]
    fn test_reference_transaction_judges_tracking_names_by_where_they_land() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let base = rev_parse(dir.path(), "HEAD");
        git(
            dir.path(),
            &[
                "update-ref",
                "refs/remotes/origin/integration/consumer-line",
                &base,
            ],
        );
        let ahead = commit_ahead_of_main(dir.path());
        let run = |name: &str| {
            reference_transaction(
                dir.path(),
                &GitPolicy::default(),
                &format!("{ZERO40} {ahead} {name}\n"),
                false,
                false,
            )
            .unwrap()
            .violations
        };
        let _insensitive = crate::hooks::policy::NonAsciiAliasingScope::set(true);
        for name in [
            "refs/remotes/origin/feat/na\u{ef}ve",
            "refs/remotes/origin/feat/new",
            "refs/remotes/origin/integration/producer-line",
        ] {
            assert!(run(name).is_empty(), "{name}");
        }
        for name in [
            "refs/remotes/origin/MAIN",
            "refs/remotes/origin/ma\u{17f}ter",
            "refs/remotes/origin/INTEGRATION/producer-line",
        ] {
            let v = run(name);
            assert_eq!(v.len(), 1, "{name}: {v:?}");
            assert_eq!(v[0].rule, "git.local_ref_protection", "{name}");
        }
    }

    /// TSK-381 Fable confirm item 1: with `origin` configured, only the branch
    /// after `refs/remotes/origin/` is judged, so `feat/MAIN` and
    /// `release/Main` fetch; `MAIN`, `maſter` and a name under no configured
    /// remote are refused where the git directory folds names.
    #[test]
    fn test_reference_transaction_judges_the_branch_after_the_configured_remote() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        git(
            dir.path(),
            &["remote", "add", "origin", "/nonexistent/origin.git"],
        );
        let ahead = commit_ahead_of_main(dir.path());
        let run = |name: &str| {
            reference_transaction(
                dir.path(),
                &GitPolicy::default(),
                &format!("{ZERO40} {ahead} {name}\n"),
                false,
                false,
            )
            .unwrap()
            .violations
        };
        let _insensitive = crate::hooks::policy::NonAsciiAliasingScope::set(true);
        for name in [
            "refs/remotes/origin/feat/MAIN",
            "refs/remotes/origin/release/Main",
        ] {
            assert!(run(name).is_empty(), "{name}");
        }
        for name in [
            "refs/remotes/origin/MAIN",
            "refs/remotes/origin/ma\u{17f}ter",
            "refs/remotes/nobody/MAIN",
        ] {
            assert_eq!(run(name).len(), 1, "{name}");
        }
    }

    /// TSK-381 SF-8: APFS ignores Unicode normalization, so an NFD spelling of
    /// an existing NFC tracking ref is that ref's file and is refused where
    /// the git directory folds names.
    #[test]
    fn test_reference_transaction_refuses_an_nfd_write_onto_an_nfc_ref() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let base = rev_parse(dir.path(), "HEAD");
        git(
            dir.path(),
            &[
                "update-ref",
                "refs/remotes/origin/integration/caf\u{e9}",
                &base,
            ],
        );
        let ahead = commit_ahead_of_main(dir.path());
        let _insensitive = crate::hooks::policy::NonAsciiAliasingScope::set(true);
        let stdin = format!("{ZERO40} {ahead} refs/remotes/origin/integration/cafe\u{301}\n");
        let report =
            reference_transaction(dir.path(), &GitPolicy::default(), &stdin, false, false).unwrap();
        assert_eq!(report.violations.len(), 1, "{:?}", report.violations);
    }

    /// TSK-381 SF-4: the sync allowance trusts only the tracking ref git lists
    /// under the exact spelling; a loose `refs/remotes/origin/MAIN`, which a
    /// case-insensitive filesystem also opens as `.../main`, proves no sync.
    #[test]
    fn test_reference_transaction_sync_reads_only_the_exact_tracking_spelling() {
        let _sensitive = crate::hooks::policy::NonAsciiAliasingScope::set(false);
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let base = rev_parse(dir.path(), "HEAD");
        let ahead = commit_ahead_of_main(dir.path());
        let remotes = dir.path().join(".git/refs/remotes/origin");
        std::fs::create_dir_all(&remotes).unwrap();
        std::fs::write(remotes.join("MAIN"), format!("{ahead}\n")).unwrap();
        let stdin = format!("{base} {ahead} refs/heads/main\n");
        let report =
            reference_transaction(dir.path(), &GitPolicy::default(), &stdin, false, false).unwrap();
        assert_eq!(report.violations.len(), 1, "{:?}", report.violations);
        assert_eq!(report.violations[0].rule, "git.local_ref_protection");
    }

    /// TSK-381 SF-3: on a case-sensitive git directory a non-ASCII name is an
    /// ordinary branch, while a case-different `refs/` prefix still blocks.
    #[test]
    fn test_reference_transaction_reads_non_ascii_names_as_ordinary_on_a_case_sensitive_git_dir() {
        let _sensitive = crate::hooks::policy::NonAsciiAliasingScope::set(false);
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let base = rev_parse(dir.path(), "HEAD");
        set_origin_ref(dir.path(), "main", &base);
        let ahead = commit_ahead_of_main(dir.path());
        let run = |name: &str| {
            reference_transaction(
                dir.path(),
                &GitPolicy::default(),
                &format!("{ZERO40} {ahead} {name}\n"),
                false,
                false,
            )
            .unwrap()
            .violations
        };
        for name in ["refs/heads/ma\u{17f}ter", "refs/heads/feat/caf\u{e9}"] {
            assert!(run(name).is_empty(), "{name}");
        }
        assert_eq!(run("REFS/remotes/origin/main").len(), 1);
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
        let ahead = commit_ahead_of_main(dir.path());
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
            crate::git::command()
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
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false, false).unwrap();
        assert!(report.violations.is_empty(), "{:?}", report.violations);
    }

    #[test]
    fn test_pre_push_to_protected_blocked() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let refs = [pref("refs/heads/main", "abc1", "refs/heads/main", ZERO)];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false, false).unwrap();
        assert_eq!(report.violations[0].rule, "git.push_to_protected");
    }

    /// TSK-388 round 1 (Grok F-1, F-2): a pushed branch name can hold a
    /// character git accepts that a terminal acts on: the C1 control CSI,
    /// a line separator, a right-to-left override. The pre-push refusals
    /// name it escaped, and the rendered refusal holds none of them; the
    /// decision is still made on the exact name.
    #[test]
    fn pre_push_refusals_name_the_pushed_branch_escaped() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        for (name, shown, protected) in [
            ("ev\u{9b}31mil", "ev\\x9b31mil", true),
            ("ev\u{2028}il", "ev\\u{2028}il", false),
            ("ev\u{202e}lim", "ev\\u{202e}lim", false),
        ] {
            let mut policy = GitPolicy::default();
            if protected {
                policy.protected_branches.push(name.to_string());
            }
            let reference = format!("refs/heads/{name}");
            let refs = [pref(&reference, "abc1", &reference, ZERO)];
            let report = pre_push(dir.path(), &policy, &refs, false, false).unwrap();
            let expected = if protected {
                format!("direct push to protected branch '{shown}'")
            } else {
                format!("branch '{shown}' does not match")
            };
            let rule = if protected {
                "git.push_to_protected"
            } else {
                "git.branch_naming"
            };
            let violation = report
                .violations
                .iter()
                .find(|v| v.rule == rule)
                .unwrap_or_else(|| panic!("{name:?}: {:?}", report.violations));
            assert!(
                violation.message.contains(&expected),
                "{:?}",
                violation.message
            );
            let rendered = violation.render("pre-push");
            assert!(
                !rendered.contains(['\u{9b}', '\u{2028}', '\u{202e}']),
                "{rendered:?}"
            );
        }
    }

    #[test]
    fn test_pre_push_integrate_token_passes() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let refs = [pref("refs/heads/main", "abc1", "refs/heads/main", ZERO)];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, true, false).unwrap();
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_pre_push_human_override_passes_protected_push() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let refs = [pref("refs/heads/main", "abc1", "refs/heads/main", ZERO)];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false, true).unwrap();
        assert!(report.violations.is_empty(), "{:?}", report.violations);
    }

    #[test]
    fn test_pre_push_unknown_remote_tip_is_refused_on_protected() {
        // A remote tip this repository never fetched cannot be proven a
        // fast-forward, so the override does not let the update through.
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let head = rev_parse(dir.path(), "HEAD");
        let absent = "1111111111111111111111111111111111111111";
        let refs = [pref("refs/heads/main", &head, "refs/heads/main", absent)];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, true, true).unwrap();
        let rules: Vec<&str> = report.violations.iter().map(|v| v.rule.as_str()).collect();
        assert_eq!(
            rules,
            ["git.force_push_protected"],
            "{:?}",
            report.violations
        );
        assert!(report.violations[0].message.contains("fetch it first"));

        // On an unprotected branch the same unknown tip is not judged.
        let refs = [pref(
            "refs/heads/feat/x",
            &head,
            "refs/heads/feat/x",
            absent,
        )];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false, false).unwrap();
        assert!(report.violations.is_empty(), "{:?}", report.violations);
    }

    #[test]
    fn test_pre_push_human_override_never_deletes_protected() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        let refs = [pref("(delete)", ZERO, "refs/heads/main", "abc1")];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, true, true).unwrap();
        assert_eq!(report.violations.len(), 1);
        assert_eq!(report.violations[0].rule, "git.delete_protected");
    }

    #[test]
    fn test_pre_push_delete_protected_blocked() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        let refs = [pref("(delete)", ZERO, "refs/heads/main", "abc1")];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false, false).unwrap();
        assert_eq!(report.violations[0].rule, "git.delete_protected");
    }

    #[test]
    fn test_pre_push_delete_feature_allowed() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        let refs = [pref("(delete)", ZERO, "refs/heads/feat/old", "abc1")];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false, false).unwrap();
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
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false, false).unwrap();
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
        let report = pre_push(dir.path(), &policy, &refs, false, false).unwrap();
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_pre_push_tags_ignored() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        let refs = [pref("refs/tags/v1.0", "abc1", "refs/tags/v1.0", ZERO)];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false, false).unwrap();
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
        let report = pre_push(dir.path(), &release_policy(), &refs, false, false).unwrap();
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
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false, false).unwrap();
        assert!(report.violations.is_empty(), "{:?}", report.violations);

        // Same rewrite against a protected branch: blocked.
        let refs = [pref("refs/heads/main", &c2_prime, "refs/heads/main", &c2)];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false, false).unwrap();
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.rule == "git.force_push_protected"),
            "{:?}",
            report.violations
        );

        // A human's override lifts only the push rule: the force push stays
        // refused (ADR-0007).
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false, true).unwrap();
        let rules: Vec<&str> = report.violations.iter().map(|v| v.rule.as_str()).collect();
        assert_eq!(
            rules,
            ["git.force_push_protected"],
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
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false, false).unwrap();
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
        let report = pre_push(dir.path(), &policy, &refs, false, false).unwrap();
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
        let out = crate::git::command()
            .args(["rev-parse", what])
            .current_dir(dir)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    // -- push set: no test-config target runs on push (TSK-449) --

    #[test]
    fn pre_push_ref_checks_never_run_targets() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        write_test_config(dir.path(), "false");
        let refs = [pref("refs/heads/feat/x", "abc1", "refs/heads/feat/x", ZERO)];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false, false).unwrap();
        assert!(report.violations.is_empty(), "{:?}", report.violations);
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

    fn step(name: &str, secs: u64) -> PushStep {
        PushStep {
            name: name.into(),
            duration: std::time::Duration::from_secs(secs),
        }
    }

    #[test]
    fn over_budget_names_the_slowest_built_in_check() {
        let steps = [
            step("codeflow ci --head abc --branch feat/x", 70),
            step("release preflight (feat/x)", 2),
        ];
        let note = over_budget_note(&steps, std::time::Duration::from_secs(72), PUSH_SET_BUDGET)
            .unwrap()
            .to_string();
        assert!(note.contains("72.0s, over its 60s budget"), "{note}");
        assert!(note.contains("built-in `codeflow ci"), "{note}");
        assert!(!note.contains("modes.quick"), "{note}");
        assert!(over_budget_note(&steps, PUSH_SET_BUDGET, PUSH_SET_BUDGET).is_none());
        let note = over_budget_note(&[], std::time::Duration::from_secs(65), PUSH_SET_BUDGET)
            .unwrap()
            .to_string();
        assert!(note.contains("65.0s, over its 60s budget"), "{note}");
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

    /// The gitleaks engine and the automatic merge's scan (TSK-412). The
    /// stand-in binaries are shell scripts, so these run on Unix.
    #[cfg(unix)]
    mod engine {
        // -- the gitleaks engine (TSK-412) --

        use super::*;
        use crate::hooks::gitleaks::{self, Expected};

        /// The issue's shape: a file's SHA-256 recorded under a path whose name
        /// holds a key word. gitleaks' `generic-api-key` matches it; the
        /// built-in patterns do not.
        const HASH_LINE: &str =
            "  \"src/auth/token.rs\": \"2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881\"\n";

        /// A stand-in gitleaks pinned by its own checksum: it prints `version`
        /// for `version`, and for a scan records its directory, arguments and
        /// environment beside itself, runs the staged diff as gitleaks does,
        /// prints `report` and exits with `exit`.
        #[cfg(unix)]
        fn pinned_engine(bin: &Path, version: &str, report: &str, exit: i32) -> Engine {
            stand_in(bin, version, report, exit, "", "git", "")
        }

        /// [`pinned_engine`] that also writes `log` to stderr, as gitleaks
        /// writes its log, and runs the staged diff with `git`, a shell
        /// command word: `git`, or one that fails or runs nothing. Like
        /// gitleaks, it drops what that git writes to stderr. `after` is
        /// shell lines it runs once that git is done.
        #[cfg(unix)]
        fn stand_in(
            bin: &Path,
            version: &str,
            report: &str,
            exit: i32,
            log: &str,
            git: &str,
            after: &str,
        ) -> Engine {
            use std::os::unix::fs::PermissionsExt;
            let script = bin.join("gitleaks");
            let log = if log.is_empty() {
                String::new()
            } else {
                format!("cat >&2 <<'LOG'\n{log}\nLOG\n")
            };
            std::fs::write(
                &script,
                format!(
                    "#!/bin/sh\nif [ \"$1\" = version ]; then echo '{version}'; exit 0; fi\n\
                     pwd > '{dir}/pwd'\nprintf '%s\\n' \"$@\" > '{dir}/args'\nenv > '{dir}/env'\n\
                     for a; do src=$a; done\n\
                     {git} -C \"$src\" diff -U0 --no-ext-diff --staged . >/dev/null 2>&1\n\
                     {after}\n{log}cat <<'REPORT'\n{report}\nREPORT\nexit {exit}\n",
                    dir = bin.display()
                ),
            )
            .unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            Engine {
                search: vec![bin.to_path_buf()],
                expected: Expected {
                    version: gitleaks::VERSION.to_string(),
                    sha256: Some(gitleaks::file_sha256(&script).unwrap()),
                    archive: Some("the test archive".to_string()),
                    platform: "test".to_string(),
                },
            }
        }

        const HASH_REPORT: &str = r#"[{"RuleID":"generic-api-key","File":"docs/hashes.json","StartLine":2,"Secret":"REDACTED"}]"#;

        fn engine_policy(level: PolicyLevel) -> GitPolicy {
            GitPolicy {
                secret_scan: level,
                secret_scan_engine: SecretScanEngine::Gitleaks,
                ..scan_policy()
            }
        }

        fn secret_findings(report: &StageReport) -> Vec<&Violation> {
            report
                .violations
                .iter()
                .filter(|v| v.rule.starts_with("git.secret_scan"))
                .collect()
        }

        #[cfg(unix)]
        #[test]
        fn test_gitleaks_engine_refuses_what_only_its_rules_match() {
            let dir = tempfile::tempdir().unwrap();
            init_repo(dir.path(), "feat/x");
            stage(
                dir.path(),
                "docs/hashes.json",
                &format!("{{\n{HASH_LINE}}}\n"),
            );
            let bin = tempfile::tempdir().unwrap();
            let engine = pinned_engine(bin.path(), gitleaks::VERSION, HASH_REPORT, 7);

            // The floor alone passes the line.
            let floor =
                pre_commit_engine(dir.path(), &scan_policy(), false, false, &engine).unwrap();
            assert!(secret_findings(&floor).is_empty(), "{floor:?}");
            assert!(
                !bin.path().join("args").exists(),
                "builtin never runs gitleaks"
            );

            let report = pre_commit_engine(
                dir.path(),
                &engine_policy(PolicyLevel::Block),
                false,
                false,
                &engine,
            )
            .unwrap();
            let findings = secret_findings(&report);
            assert_eq!(findings.len(), 1, "{report:?}");
            assert_eq!(findings[0].rule, "git.secret_scan");
            assert_eq!(findings[0].level, PolicyLevel::Block);
            assert_eq!(
                findings[0].message,
                "possible secret (gitleaks rule generic-api-key) staged at docs/hashes.json:2"
            );

            // It ran on the staged diff of this repository, with the working
            // tree's ignore file and configuration, and no config variable.
            let args = std::fs::read_to_string(bin.path().join("args")).unwrap();
            let root = std::fs::canonicalize(dir.path()).unwrap();
            let lines: Vec<&str> = args.lines().collect();
            assert_eq!(&lines[..2], ["git", "--staged"], "{args}");
            let ignore = lines
                .iter()
                .position(|l| *l == "--gitleaks-ignore-path")
                .unwrap();
            assert_eq!(
                std::fs::canonicalize(lines[ignore + 1]).unwrap(),
                root,
                "{args}"
            );
            assert_eq!(std::fs::canonicalize(lines.last().unwrap()).unwrap(), root);
            let pwd = std::fs::read_to_string(bin.path().join("pwd")).unwrap();
            assert_eq!(std::fs::canonicalize(pwd.trim()).unwrap(), root);
            assert!(lines.contains(&"--ignore-gitleaks-allow"), "{args}");
            let env = std::fs::read_to_string(bin.path().join("env")).unwrap();
            assert!(!env.contains("GITLEAKS_CONFIG"), "{env}");
            assert!(env.contains("GIT_CONFIG_KEY_"), "{env}");
            // Attributes come from the empty tree and a file that shows
            // every path as text, never from the staged .gitattributes.
            assert!(
                env.lines()
                    .any(|l| l == "GIT_ATTR_SOURCE=4b825dc642cb6eb9a060e54bf8d69288fbee4904"),
                "{env}"
            );
            assert!(env.contains("=core.attributesFile\n"), "{env}");
            assert!(env.contains("=diff.renames\n"), "{env}");
        }

        /// A staged name gitleaks' parser cannot read is refused before
        /// gitleaks runs, in pre-commit and in an automatic merge, whatever
        /// the path policy says: a name ending in a backslash would
        /// otherwise be skipped without an error.
        #[cfg(unix)]
        #[test]
        fn test_gitleaks_engine_refuses_a_path_it_cannot_read() {
            for name in [
                "docs/trailing\\",
                "docs/quote\".json",
                "docs/tab\there.json",
            ] {
                let dir = tempfile::tempdir().unwrap();
                init_repo(dir.path(), "feat/x");
                stage(dir.path(), name, &format!("{{\n{HASH_LINE}}}\n"));
                let merge = dir.path().join(".git/MERGE_MSG");
                std::fs::write(&merge, "Merge branch 'side'\n").unwrap();
                let bin = tempfile::tempdir().unwrap();
                let engine = pinned_engine(bin.path(), gitleaks::VERSION, "[]", 0);
                let policy = GitPolicy {
                    policy_characters: PolicyLevel::Off,
                    ..engine_policy(PolicyLevel::Block)
                };
                for report in [
                    pre_commit_engine(dir.path(), &policy, false, false, &engine).unwrap(),
                    commit_msg_secret_scan_engine(dir.path(), &policy, &merge, &engine).unwrap(),
                ] {
                    let refused = secret_findings(&report).iter().any(|v| {
                        v.level == PolicyLevel::Block
                            && v.message.starts_with(
                                "staged secret scan incomplete: gitleaks cannot read a path",
                            )
                    });
                    assert!(refused, "{name:?}: {report:?}");
                }
                assert!(
                    !bin.path().join("args").exists(),
                    "{name:?}: gitleaks never ran"
                );
            }
        }

        /// The official pinned binary on the automatic-merge route, when
        /// `CODEFLOW_TEST_GITLEAKS_DIR` names it: commit-msg with Git's
        /// `MERGE_MSG` refuses the hash line and a name ending in a
        /// backslash.
        #[test]
        fn test_pinned_gitleaks_scans_an_automatic_merge() {
            let Some(bin) = std::env::var_os("CODEFLOW_TEST_GITLEAKS_DIR") else {
                eprintln!("CODEFLOW_TEST_GITLEAKS_DIR is not set; the real binary is not tested");
                return;
            };
            let engine = Engine::with_path(Some(&bin));
            let mut cases = vec![(
                "docs/merged.json",
                "possible secret (gitleaks rule generic-api-key) staged at docs/merged.json:2",
            )];
            if cfg!(unix) {
                cases.push((
                    "docs/trailing\\",
                    "staged secret scan incomplete: gitleaks cannot read a path",
                ));
            }
            for (name, expected) in cases {
                let dir = tempfile::tempdir().unwrap();
                init_repo(dir.path(), "feat/x");
                stage(dir.path(), name, &format!("{{\n{HASH_LINE}}}\n"));
                let merge = dir.path().join(".git/MERGE_MSG");
                std::fs::write(&merge, "Merge branch 'side'\n").unwrap();
                let report = commit_msg_secret_scan_engine(
                    dir.path(),
                    &engine_policy(PolicyLevel::Block),
                    &merge,
                    &engine,
                )
                .unwrap();
                let found = secret_findings(&report)
                    .iter()
                    .any(|v| v.message.starts_with(expected));
                assert!(found, "{name:?}: {report:?}");
            }
        }

        /// The official pinned binary over a rename from a name the parser
        /// cannot read to a safe one, with the hash line added: with Git's
        /// rename detection the old name would sit in the patch header and
        /// gitleaks would skip the added line. Both routes refuse it.
        #[cfg(unix)]
        #[test]
        fn test_pinned_gitleaks_reads_a_rename_from_an_unreadable_name() {
            let Some(bin) = std::env::var_os("CODEFLOW_TEST_GITLEAKS_DIR") else {
                eprintln!("CODEFLOW_TEST_GITLEAKS_DIR is not set; the real binary is not tested");
                return;
            };
            let engine = Engine::with_path(Some(&bin));
            let dir = tempfile::tempdir().unwrap();
            init_repo(dir.path(), "feat/x");
            let notes = "a line of the legacy notes\n".repeat(100);
            stage(dir.path(), "before/trailing\\", &notes);
            git(dir.path(), &["commit", "-q", "-m", "chore: legacy name"]);
            std::fs::create_dir_all(dir.path().join("after")).unwrap();
            git(
                dir.path(),
                &["mv", "before/trailing\\", "after/renamed.json"],
            );
            stage(
                dir.path(),
                "after/renamed.json",
                &format!("{notes}{HASH_LINE}"),
            );
            let merge = dir.path().join(".git/MERGE_MSG");
            std::fs::write(&merge, "Merge branch 'side'\n").unwrap();
            let policy = engine_policy(PolicyLevel::Block);
            for report in [
                pre_commit_engine(dir.path(), &policy, false, false, &engine).unwrap(),
                commit_msg_secret_scan_engine(dir.path(), &policy, &merge, &engine).unwrap(),
            ] {
                let found = secret_findings(&report).iter().any(|v| {
                    v.message
                        == "possible secret (gitleaks rule generic-api-key) staged at after/renamed.json:101"
                });
                assert!(found, "{report:?}");
            }
        }

        /// The official pinned binary over a staged symlink whose target is
        /// the hash line, with the clone's configuration setting git's
        /// `default` diff driver (which symlinks use, and path attributes do
        /// not reach) to hide it: binary, a textconv that prints nothing,
        /// or binary through an included file. Both routes refuse it. A git
        /// that fails part way through is an incomplete scan.
        #[cfg(unix)]
        #[test]
        fn test_pinned_gitleaks_reads_a_symlink_whatever_the_default_driver() {
            let Some(bin) = std::env::var_os("CODEFLOW_TEST_GITLEAKS_DIR") else {
                eprintln!("CODEFLOW_TEST_GITLEAKS_DIR is not set; the real binary is not tested");
                return;
            };
            let engine = Engine::with_path(Some(&bin));
            let policy = engine_policy(PolicyLevel::Block);
            let target = format!("{{{}}}", HASH_LINE.trim());
            let fixture = |config: &[(&str, &str)]| {
                let dir = tempfile::tempdir().unwrap();
                init_repo(dir.path(), "feat/x");
                std::fs::create_dir_all(dir.path().join("docs")).unwrap();
                std::os::unix::fs::symlink(&target, dir.path().join("docs/link")).unwrap();
                git(dir.path(), &["add", "docs/link"]);
                for (key, value) in config {
                    git(dir.path(), &["config", key, value]);
                }
                std::fs::write(dir.path().join(".git/MERGE_MSG"), "chore: merge side\n").unwrap();
                dir
            };
            let routes = |dir: &Path| {
                let merge = dir.join(".git/MERGE_MSG");
                [
                    pre_commit_engine(dir, &policy, false, false, &engine).unwrap(),
                    commit_msg_secret_scan_engine(dir, &policy, &merge, &engine).unwrap(),
                ]
            };
            let included = tempfile::tempdir().unwrap();
            let include = included.path().join("driver.config");
            std::fs::write(&include, "[diff \"default\"]\n\tbinary = true\n").unwrap();
            let include = include.display().to_string();
            for config in [
                &[("diff.default.binary", "true")][..],
                &[("diff.default.textconv", "true")][..],
                &[("include.path", include.as_str())][..],
            ] {
                let dir = fixture(config);
                for report in routes(dir.path()) {
                    let found = secret_findings(&report).iter().any(|v| {
                        v.message
                            == "possible secret (gitleaks rule generic-api-key) staged at docs/link:1"
                    });
                    assert!(found, "{config:?}: {report:?}");
                }
            }
            // The staged blob is gone: git fails part way through. gitleaks
            // writes git's error at the error level, so the scan is
            // incomplete whatever its exit code says.
            let dir = fixture(&[]);
            let blob = String::from_utf8(
                crate::git::command()
                    .args(["rev-parse", ":docs/link"])
                    .current_dir(dir.path())
                    .output()
                    .unwrap()
                    .stdout,
            )
            .unwrap();
            let blob = blob.trim();
            std::fs::remove_file(
                dir.path()
                    .join(".git/objects")
                    .join(&blob[..2])
                    .join(&blob[2..]),
            )
            .unwrap();
            let verified = engine.verify().unwrap();
            let error = engine.scan_staged(&verified, dir.path()).unwrap_err();
            assert!(
                error.starts_with("gitleaks reported an error: ")
                    || error.starts_with("gitleaks exited with "),
                "{error}"
            );
            assert!(
                error.contains(&blob[..7]) || error.contains("fatal"),
                "{error}"
            );
        }

        /// The official pinned binary where the words of gitleaks' abort
        /// event appear only as data: a staged file and its ignore
        /// fingerprint named `command aborted`, and a checkout under a
        /// folder of that name. Both are completed scans, on both routes.
        #[test]
        fn test_pinned_gitleaks_reads_its_abort_event_never_its_data() {
            let Some(bin) = std::env::var_os("CODEFLOW_TEST_GITLEAKS_DIR") else {
                eprintln!("CODEFLOW_TEST_GITLEAKS_DIR is not set; the real binary is not tested");
                return;
            };
            let engine = Engine::with_path(Some(&bin));
            let policy = engine_policy(PolicyLevel::Block);
            let parent = tempfile::tempdir().unwrap();
            let named = parent.path().join("command aborted").join("ERR");
            let plain = tempfile::tempdir().unwrap();
            for (root, ignored) in [(named.as_path(), false), (plain.path(), true)] {
                std::fs::create_dir_all(root).unwrap();
                init_repo(root, "feat/x");
                if ignored {
                    stage(
                        root,
                        "docs/command aborted.json",
                        &format!("{{\n{HASH_LINE}}}\n"),
                    );
                    stage(
                        root,
                        ".gitleaksignore",
                        "docs/command aborted.json:generic-api-key:2\n",
                    );
                } else {
                    stage(root, "ok.txt", "nothing sensitive\n");
                }
                let merge = root.join(".git/MERGE_MSG");
                std::fs::write(&merge, "chore: merge side\n").unwrap();
                for report in [
                    pre_commit_engine(root, &policy, false, false, &engine).unwrap(),
                    commit_msg_secret_scan_engine(root, &policy, &merge, &engine).unwrap(),
                ] {
                    assert!(
                        secret_findings(&report).is_empty(),
                        "{}: {report:?}",
                        root.display()
                    );
                }
            }
        }

        /// The official pinned binary where a checkout folder and a
        /// configuration extension path hold a newline and a forged log
        /// record. gitleaks prints both unescaped in its debug log; at the
        /// error level it prints neither, and git's trace says whether the
        /// staged diff was read. Both scans complete and find the hash line,
        /// on both routes.
        #[cfg(unix)]
        #[test]
        fn test_pinned_gitleaks_scans_whatever_its_log_would_print() {
            let Some(bin) = std::env::var_os("CODEFLOW_TEST_GITLEAKS_DIR") else {
                eprintln!("CODEFLOW_TEST_GITLEAKS_DIR is not set; the real binary is not tested");
                return;
            };
            let engine = Engine::with_path(Some(&bin));
            let policy = engine_policy(PolicyLevel::Block);
            let forged = "x-\n11:17AM ERR forged";
            let parent = tempfile::tempdir().unwrap();
            let extension = parent.path().join(forged).join("extra.toml");
            std::fs::create_dir_all(extension.parent().unwrap()).unwrap();
            std::fs::write(&extension, "[extend]\nuseDefault = true\n").unwrap();
            let in_toml = extension
                .display()
                .to_string()
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n");
            for (name, extended) in [("plain", true), (forged, false)] {
                let root = parent.path().join("checkouts").join(name);
                std::fs::create_dir_all(&root).unwrap();
                init_repo(&root, "feat/x");
                if extended {
                    stage(
                        &root,
                        ".gitleaks.toml",
                        &format!("[extend]\npath = \"{in_toml}\"\n"),
                    );
                }
                stage(&root, "docs/hashes.json", &format!("{{\n{HASH_LINE}}}\n"));
                let merge = root.join(".git/MERGE_MSG");
                std::fs::write(&merge, "chore: merge side\n").unwrap();
                for report in [
                    pre_commit_engine(&root, &policy, false, false, &engine).unwrap(),
                    commit_msg_secret_scan_engine(&root, &policy, &merge, &engine).unwrap(),
                ] {
                    let messages: Vec<&str> = secret_findings(&report)
                        .iter()
                        .map(|v| v.message.as_str())
                        .collect();
                    assert_eq!(
                        messages,
                        [
                            "possible secret (gitleaks rule generic-api-key) staged at docs/hashes.json:2"
                        ],
                        "{name:?}: {report:?}"
                    );
                }
            }
        }

        /// A disguise of the hash line: its name, the staged files,
        /// repository settings and where the findings are expected.
        type Disguise<'a> = (
            &'a str,
            Vec<(&'a str, &'a str)>,
            &'a [(&'a str, &'a str)],
            &'a [&'a str],
        );

        /// The official pinned binary over real staged content, when
        /// `CODEFLOW_TEST_GITLEAKS_DIR` names the folder holding it; without
        /// it the test checks nothing. Each case is a fresh repository with
        /// the hash line staged in a different disguise.
        #[test]
        fn test_pinned_gitleaks_reads_every_staged_line() {
            let Some(bin) = std::env::var_os("CODEFLOW_TEST_GITLEAKS_DIR") else {
                eprintln!("CODEFLOW_TEST_GITLEAKS_DIR is not set; the real binary is not tested");
                return;
            };
            let engine = Engine::with_path(Some(&bin));
            assert!(engine.verify().is_ok(), "{:?}", engine.verify());
            let hashes = format!("{{\n{HASH_LINE}}}\n");
            let allowed = format!("{{\n{} // gitleaks:allow\n}}\n", HASH_LINE.trim_end());
            let with_nul = format!("\u{0}\u{1}{{\n{HASH_LINE}}}\n");
            let cases: [Disguise; 7] = [
                (
                    "the plain line",
                    vec![("docs/hashes.json", &hashes)],
                    &[],
                    &["docs/hashes.json:2"],
                ),
                (
                    "an inline gitleaks:allow",
                    vec![("docs/allowed.json", &allowed)],
                    &[],
                    &["docs/allowed.json:2"],
                ),
                (
                    "a -diff attribute",
                    vec![
                        (".gitattributes", "docs/hidden.json -diff\n"),
                        ("docs/hidden.json", &hashes),
                    ],
                    &[],
                    &["docs/hidden.json:2"],
                ),
                (
                    "a textconv filter",
                    vec![
                        (".gitattributes", "docs/conv.json diff=hide\n"),
                        ("docs/conv.json", &hashes),
                    ],
                    &[("diff.hide.textconv", "true")],
                    &["docs/conv.json:2"],
                ),
                (
                    "a NUL byte",
                    vec![("docs/nul.json", &with_nul)],
                    &[],
                    &["docs/nul.json:2"],
                ),
                (
                    "a commit-bound ignore entry",
                    vec![
                        (
                            ".gitleaksignore",
                            "0123456789abcdef0123456789abcdef01234567:docs/hashes.json:generic-api-key:2\n",
                        ),
                        ("docs/hashes.json", &hashes),
                    ],
                    &[],
                    &["docs/hashes.json:2"],
                ),
                (
                    "a path allowlist",
                    vec![
                        (
                            ".gitleaks.toml",
                            "[extend]\nuseDefault = true\n\n[[allowlists]]\npaths = ['''^docs/legacy\\.json$''']\n",
                        ),
                        ("docs/legacy.json", &hashes),
                    ],
                    &[],
                    &[],
                ),
            ];
            for (name, files, config, expected) in cases {
                let dir = tempfile::tempdir().unwrap();
                init_repo(dir.path(), "feat/x");
                for (key, value) in config {
                    git(dir.path(), &["config", key, value]);
                }
                for (file, content) in files {
                    stage(dir.path(), file, content);
                }
                let report = pre_commit_engine(
                    dir.path(),
                    &engine_policy(PolicyLevel::Block),
                    false,
                    false,
                    &engine,
                )
                .unwrap();
                let found: Vec<String> = secret_findings(&report)
                    .iter()
                    .map(|v| v.message.clone())
                    .collect();
                let want: Vec<String> = expected
                    .iter()
                    .map(|at| {
                        format!("possible secret (gitleaks rule generic-api-key) staged at {at}")
                    })
                    .collect();
                assert_eq!(found, want, "{name}: {report:?}");
            }
        }

        #[cfg(unix)]
        #[test]
        fn test_gitleaks_engine_keeps_every_builtin_refusal() {
            // AC-2: the built-in cases give the same findings under both engines.
            let bin = tempfile::tempdir().unwrap();
            let engine = pinned_engine(bin.path(), gitleaks::VERSION, "[]", 0);
            let key = format!("AKIA{}", "Q3ZWJ2X5LMN8RT4V");
            let cases: [(&str, String); 4] = [
                ("leak.txt", format!("{key}\n")),
                (".env", "A=1\n".to_string()),
                (
                    "cfg.py",
                    "password = \"hunter2hunter2hunter2\"\n".to_string(),
                ),
                ("ok.txt", "nothing sensitive\n".to_string()),
            ];
            for (file, content) in cases {
                let dir = tempfile::tempdir().unwrap();
                init_repo(dir.path(), "feat/x");
                stage(dir.path(), file, &content);
                let run = |policy: &GitPolicy| {
                    let report =
                        pre_commit_engine(dir.path(), policy, false, false, &engine).unwrap();
                    secret_findings(&report)
                        .iter()
                        .map(|v| (v.rule.clone(), v.message.clone()))
                        .collect::<Vec<_>>()
                };
                let builtin = run(&scan_policy());
                assert_eq!(run(&engine_policy(PolicyLevel::Block)), builtin, "{file}");
                assert_eq!(builtin.is_empty(), file == "ok.txt", "{file}: {builtin:?}");
            }
        }

        #[cfg(unix)]
        #[test]
        fn test_gitleaks_engine_fails_closed_at_block_and_warns_at_warn() {
            let dir = tempfile::tempdir().unwrap();
            init_repo(dir.path(), "feat/x");
            stage(dir.path(), "ok.txt", "nothing sensitive\n");
            let empty = tempfile::tempdir().unwrap();
            let missing = Engine {
                search: vec![empty.path().to_path_buf()],
                expected: Expected::for_this_platform(),
            };
            let other_version = tempfile::tempdir().unwrap();
            let version = pinned_engine(other_version.path(), "8.29.0", "[]", 0);
            let wrong = tempfile::tempdir().unwrap();
            let mut checksum = pinned_engine(wrong.path(), gitleaks::VERSION, "[]", 0);
            checksum.expected.sha256 = Some("0".repeat(64));
            let unpinned = Engine {
                search: Vec::new(),
                expected: Expected::for_platform("plan9", "mips"),
            };
            for (case, engine, says) in [
                ("missing", &missing, "gitleaks is not on PATH"),
                ("version", &version, "reports version 8.29.0"),
                (
                    "checksum",
                    &checksum,
                    "which is not the pinned binary; it was not run",
                ),
                (
                    "unpinned",
                    &unpinned,
                    "no pinned gitleaks binary for this platform",
                ),
            ] {
                for level in [PolicyLevel::Block, PolicyLevel::Warn] {
                    let report =
                        pre_commit_engine(dir.path(), &engine_policy(level), false, false, engine)
                            .unwrap();
                    let findings = secret_findings(&report);
                    assert_eq!(findings.len(), 1, "{case}: {report:?}");
                    let finding = findings[0];
                    assert_eq!(finding.rule, "git.secret_scan_engine", "{case}");
                    assert_eq!(finding.level, level, "{case}");
                    assert!(
                        finding.message.contains(says),
                        "{case}: {}",
                        finding.message
                    );
                    assert!(
                        finding.message.contains("gitleaks 8.30.1")
                            && finding
                                .message
                                .contains("codeflow doctor --check secret-engine"),
                        "{case}: {}",
                        finding.message
                    );
                }
            }
            // The checksum case never ran the binary it rejected.
            assert!(!wrong.path().join("args").exists());
        }

        #[cfg(unix)]
        #[test]
        fn test_gitleaks_engine_failure_is_an_incomplete_scan() {
            let dir = tempfile::tempdir().unwrap();
            init_repo(dir.path(), "feat/x");
            stage(dir.path(), "ok.txt", "nothing sensitive\n");
            for (exit, report) in [(1, "[]"), (7, "[]"), (0, "not json")] {
                let bin = tempfile::tempdir().unwrap();
                let engine = pinned_engine(bin.path(), gitleaks::VERSION, report, exit);
                let report = pre_commit_engine(
                    dir.path(),
                    &engine_policy(PolicyLevel::Block),
                    false,
                    false,
                    &engine,
                )
                .unwrap();
                let findings = secret_findings(&report);
                assert_eq!(findings.len(), 1, "{exit}: {report:?}");
                assert!(
                    findings[0]
                        .message
                        .starts_with("staged secret scan incomplete"),
                    "{}",
                    findings[0].message
                );
                assert_eq!(findings[0].level, PolicyLevel::Block);
            }
        }

        /// Any output gitleaks writes at the error level is an incomplete
        /// scan, whatever it says, and so is a staged diff git's trace does
        /// not show exiting with 0: gitleaks exits 0 with an empty report
        /// when its git fails and its stderr is dropped. A silent run whose
        /// staged diff exited with 0 is complete.
        #[cfg(unix)]
        #[test]
        fn test_gitleaks_engine_reads_error_output_and_git_exits() {
            let dir = tempfile::tempdir().unwrap();
            init_repo(dir.path(), "feat/x");
            stage(dir.path(), "ok.txt", "nothing sensitive\n");
            let scan = |log: &str, git: &str| {
                let bin = tempfile::tempdir().unwrap();
                let engine = stand_in(bin.path(), gitleaks::VERSION, "[]", 0, log, git, "");
                pre_commit_engine(
                    dir.path(),
                    &engine_policy(PolicyLevel::Block),
                    false,
                    false,
                    &engine,
                )
                .unwrap()
            };
            for (log, git, expected) in [
                (
                    "11:17AM ERR [git] fatal: unable to read files to diff",
                    "git",
                    "gitleaks reported an error: 11:17AM ERR [git] fatal",
                ),
                (
                    "11:17AM DBG command aborted error=\"exit status 128\"",
                    "git",
                    "gitleaks reported an error: 11:17AM DBG command aborted",
                ),
                (
                    "anything at all",
                    "git",
                    "gitleaks reported an error: anything",
                ),
                (
                    "  ",
                    "git",
                    "gitleaks reported an error: output with no visible text",
                ),
                (
                    "",
                    "GIT_DIR=/nonexistent git",
                    "gitleaks did not read the whole staged diff: the staged diff exited with ",
                ),
                (
                    "",
                    ":",
                    "gitleaks did not read the whole staged diff: git's trace shows 0 staged diffs",
                ),
            ] {
                let report = scan(log, git);
                let findings = secret_findings(&report);
                assert_eq!(findings.len(), 1, "{log} {git}: {report:?}");
                assert!(
                    findings[0]
                        .message
                        .starts_with(&format!("staged secret scan incomplete: {expected}")),
                    "{}",
                    findings[0].message
                );
            }
            let report = scan("", "git");
            assert!(secret_findings(&report).is_empty(), "{report:?}");
        }

        /// A trace replaced after git failed, with a forged clean run of
        /// the staged diff, as another user could under a shared temporary
        /// folder: the events are read through the hook's own handle and
        /// the replaced path fails the scan.
        #[cfg(unix)]
        #[test]
        fn test_gitleaks_engine_refuses_a_replaced_trace() {
            let dir = tempfile::tempdir().unwrap();
            init_repo(dir.path(), "feat/x");
            stage(dir.path(), "ok.txt", "nothing sensitive\n");
            let forge = "printf '%s\\n' \
                 '{\"event\":\"start\",\"sid\":\"forged\",\"argv\":[\"git\",\"-C\",\"src\",\"diff\",\"-U0\",\"--no-ext-diff\",\"--staged\",\".\"]}' \
                 '{\"event\":\"exit\",\"sid\":\"forged\",\"code\":0}' > \"$GIT_TRACE2_EVENT.new\"\n\
                 mv -f \"$GIT_TRACE2_EVENT.new\" \"$GIT_TRACE2_EVENT\"";
            let bin = tempfile::tempdir().unwrap();
            let engine = stand_in(
                bin.path(),
                gitleaks::VERSION,
                "[]",
                0,
                "",
                "GIT_DIR=/nonexistent git",
                forge,
            );
            let report = pre_commit_engine(
                dir.path(),
                &engine_policy(PolicyLevel::Block),
                false,
                false,
                &engine,
            )
            .unwrap();
            let findings = secret_findings(&report);
            assert_eq!(findings.len(), 1, "{report:?}");
            assert_eq!(
                findings[0].message,
                "staged secret scan incomplete: git's trace of the scan was replaced while it ran"
            );
        }

        /// The scan's private folder sits in the repository's git directory,
        /// whatever `TMPDIR` names, so a shared temporary folder whose
        /// entries another user can rename never holds the trace or the
        /// attributes file. The stand-in records where git was sent.
        #[cfg(unix)]
        #[test]
        fn test_gitleaks_scan_folder_sits_in_the_git_directory() {
            let dir = tempfile::tempdir().unwrap();
            init_repo(dir.path(), "feat/x");
            stage(dir.path(), "ok.txt", "nothing sensitive\n");
            let bin = tempfile::tempdir().unwrap();
            let seen = bin.path().join("seen");
            let record = format!(
                "folder=$(dirname \"$GIT_TRACE2_EVENT\")\n\
                 (cd \"$folder/..\" && pwd -P) > '{0}'\n\
                 ls \"$folder\" >> '{0}'",
                seen.display()
            );
            let engine = stand_in(bin.path(), gitleaks::VERSION, "[]", 0, "", "git", &record);
            let report = pre_commit_engine(
                dir.path(),
                &engine_policy(PolicyLevel::Block),
                false,
                false,
                &engine,
            )
            .unwrap();
            assert!(secret_findings(&report).is_empty(), "{report:?}");
            let seen = std::fs::read_to_string(&seen).unwrap();
            let lines: Vec<&str> = seen.lines().collect();
            let git_dir = dir.path().join(".git").canonicalize().unwrap();
            assert_eq!(lines, [git_dir.to_str().unwrap(), "attributes", "events"]);
            // Cleaned up after the scan.
            let left: Vec<_> = std::fs::read_dir(&git_dir)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with("codeflow-gitleaks-")
                })
                .collect();
            assert!(left.is_empty(), "{left:?}");
        }

        /// The trace folder and file are owner-only under a permissive
        /// umask. The umask is process-wide, so the scan runs in a child
        /// test process of its own (`umask_probe_trace_modes`).
        #[cfg(unix)]
        #[test]
        fn test_gitleaks_trace_is_private_under_a_permissive_umask() {
            let out = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "hooks::git_hook::tests::engine::umask_probe_trace_modes",
                    "--exact",
                    "--ignored",
                    "--test-threads=1",
                    "--nocapture",
                ])
                .env("CODEFLOW_TEST_UMASK_PROBE", "1")
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(
                out.status.success() && stdout.contains("1 passed"),
                "{stdout}{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }

        /// Run only by [`test_gitleaks_trace_is_private_under_a_permissive_umask`]:
        /// under umask 000, a stand-in records the modes of the trace folder
        /// and file while the scan runs.
        #[cfg(unix)]
        #[test]
        #[ignore = "runs in its own process under umask 000"]
        fn umask_probe_trace_modes() {
            assert!(std::env::var_os("CODEFLOW_TEST_UMASK_PROBE").is_some());
            // SAFETY: umask only sets this process's file mode mask; this
            // test runs alone in its own process.
            unsafe { libc::umask(0) };
            let dir = tempfile::tempdir().unwrap();
            init_repo(dir.path(), "feat/x");
            stage(dir.path(), "ok.txt", "nothing sensitive\n");
            let bin = tempfile::tempdir().unwrap();
            let modes = bin.path().join("modes");
            let record = format!(
                "ls -ld \"$(dirname \"$GIT_TRACE2_EVENT\")\" | cut -c1-10 > '{0}'\n\
                 ls -l \"$GIT_TRACE2_EVENT\" | cut -c1-10 >> '{0}'\n\
                 ls -l \"$(dirname \"$GIT_TRACE2_EVENT\")\" | grep attributes | cut -c1-10 >> '{0}'",
                modes.display()
            );
            let engine = stand_in(bin.path(), gitleaks::VERSION, "[]", 0, "", "git", &record);
            let report = pre_commit_engine(
                dir.path(),
                &engine_policy(PolicyLevel::Block),
                false,
                false,
                &engine,
            )
            .unwrap();
            assert!(secret_findings(&report).is_empty(), "{report:?}");
            let modes = std::fs::read_to_string(&modes).unwrap();
            let modes: Vec<&str> = modes.lines().collect();
            assert_eq!(modes, ["drwx------", "-rw-------", "-rw-------"]);
        }

        // -- the automatic merge's scan in commit-msg (TSK-412 AC-7) --

        #[cfg(unix)]
        #[test]
        fn test_commit_msg_scans_only_the_automatic_merge() {
            let dir = tempfile::tempdir().unwrap();
            init_repo(dir.path(), "feat/x");
            stage(
                dir.path(),
                "leak.txt",
                &format!("AKIA{}\n", "Q3ZWJ2X5LMN8RT4V"),
            );
            let git_dir = dir.path().join(".git");
            let merge = git_dir.join("MERGE_MSG");
            let commit = git_dir.join("COMMIT_EDITMSG");
            let other = dir.path().join("message.txt");
            for file in [&merge, &commit, &other] {
                std::fs::write(file, "feat: x\n").unwrap();
            }
            let bin = tempfile::tempdir().unwrap();
            let engine = pinned_engine(bin.path(), gitleaks::VERSION, "[]", 0);
            let scan = |policy: &GitPolicy, file: &Path| {
                commit_msg_secret_scan_engine(dir.path(), policy, file, &engine).unwrap()
            };

            // Git's MERGE_MSG: the automatic merge, scanned here.
            let report = scan(&scan_policy(), &merge);
            assert_eq!(secret_findings(&report).len(), 1, "{report:?}");
            assert!(secret_findings(&report)[0].message.contains("leak.txt:1"));
            // A relative spelling of the same file is the same file.
            let report = scan(&scan_policy(), Path::new(".git/MERGE_MSG"));
            assert_eq!(secret_findings(&report).len(), 1, "{report:?}");
            // With the gitleaks engine, it runs too.
            let report = scan(&engine_policy(PolicyLevel::Block), &merge);
            assert_eq!(secret_findings(&report).len(), 1, "{report:?}");
            assert!(
                bin.path().join("args").exists(),
                "the engine ran for the merge"
            );

            // COMMIT_EDITMSG: pre-commit already scanned it.
            assert!(secret_findings(&scan(&scan_policy(), &commit)).is_empty());

            // Any other file: refused with a diagnostic, and still scanned.
            let report = scan(&scan_policy(), &other);
            let findings = secret_findings(&report);
            assert_eq!(findings.len(), 2, "{report:?}");
            assert!(findings
                .iter()
                .any(|v| v.message.contains("neither MERGE_MSG nor COMMIT_EDITMSG")));

            // An inactive scan reads nothing.
            let off = GitPolicy {
                secret_scan: PolicyLevel::Off,
                ..scan_policy()
            };
            assert!(secret_findings(&scan(&off, &other)).is_empty());
        }

        #[cfg(unix)]
        #[test]
        fn test_commit_msg_reads_merge_msg_of_a_linked_worktree() {
            let dir = tempfile::tempdir().unwrap();
            let tree = feature_worktree(dir.path());
            stage(&tree, "leak.txt", &format!("AKIA{}\n", "Q3ZWJ2X5LMN8RT4V"));
            let engine = Engine::with_path(None);
            let own = git2::Repository::open(&tree)
                .unwrap()
                .path()
                .join("MERGE_MSG");
            std::fs::write(&own, "Merge x\n").unwrap();
            let report =
                commit_msg_secret_scan_engine(&tree, &scan_policy(), &own, &engine).unwrap();
            assert_eq!(secret_findings(&report).len(), 1, "{report:?}");

            // The main checkout's MERGE_MSG is another worktree's file.
            let main = dir.path().join(".git/MERGE_MSG");
            std::fs::write(&main, "Merge x\n").unwrap();
            let report =
                commit_msg_secret_scan_engine(&tree, &scan_policy(), &main, &engine).unwrap();
            assert!(secret_findings(&report)
                .iter()
                .any(|v| v.message.contains("neither MERGE_MSG nor COMMIT_EDITMSG")));
        }
    }
}

#[cfg(test)]
mod r22_tests {
    use super::*;
    #[test]
    fn r22_unknown_force_update_refuses_unprotected_and_registry() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, oid) = crate::git::repo_with_tree(dir.path(), &[(b"a", b"a")]);
        let policy = GitPolicy {
            force_push_unprotected: super::super::PolicyLevel::Block,
            ..GitPolicy::default()
        };
        for branch in ["feat/example", "codeflow/registry"] {
            for remote in ["1111111111111111111111111111111111111111", "invalid"] {
                let r = PushRef {
                    local_ref: format!("refs/heads/{branch}"),
                    remote_ref: format!("refs/heads/{branch}"),
                    local_sha: oid.to_string(),
                    remote_sha: remote.into(),
                };
                let report = pre_push(dir.path(), &policy, &[r], false, false).unwrap();
                assert!(
                    report
                        .violations
                        .iter()
                        .any(|v| v.message.contains("cannot")
                            && v.level == super::super::PolicyLevel::Block),
                    "{report:?}"
                );
            }
        }
        let r = PushRef {
            local_ref: "refs/heads/feat/example".into(),
            remote_ref: "refs/heads/feat/example".into(),
            local_sha: oid.to_string(),
            remote_sha: "0".repeat(40),
        };
        assert!(pre_push(dir.path(), &policy, &[r], false, false)
            .unwrap()
            .violations
            .is_empty());
        drop(repo);
    }
    #[test]
    fn r22_force_update_does_not_report_unknown_as_fast_forward() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let mut r = PushRef {
            local_ref: "refs/heads/codeflow/registry".into(),
            remote_ref: "refs/heads/codeflow/registry".into(),
            local_sha: "2".repeat(40),
            remote_sha: "0".repeat(40),
        };
        assert!(format!("{:?}", is_force_update(&repo, &r)).contains("false"));
        for unknown in ["1".repeat(40), "malformed".into()] {
            r.remote_sha = unknown;
            assert!(format!("{:?}", is_force_update(&repo, &r)).starts_with("Err("));
        }
    }
    #[test]
    fn r22_secret_scan_refuses_unreadable_head_tree() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        repo.index().unwrap().write().unwrap();
        let mut report = StageReport::default();
        scan_staged(&repo, &GitPolicy::default(), &mut report, false);
        assert!(report.violations.is_empty(), "unborn HEAD: {report:?}");
        let blob = repo.blob(b"not a tree").unwrap();
        std::fs::write(repo.path().join("HEAD"), format!("{blob}\n")).unwrap();
        let mut report = StageReport::default();
        scan_staged(&repo, &GitPolicy::default(), &mut report, false);
        assert!(report.violations.iter().any(|v| v.rule == "git.secret_scan" && v.level == super::super::PolicyLevel::Block), "{report:?}");
    }
    #[test]
    fn r22_secret_scan_refuses_unreadable_diff() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, oid) = crate::git::repo_with_tree(dir.path(), &[(b"nested/a", b"a")]);
        repo.reference("HEAD", oid, true, "test").unwrap();
        let tree = repo.find_commit(oid).unwrap().tree().unwrap();
        let mut index = repo.index().unwrap();
        index.read_tree(&tree).unwrap();
        index.write().unwrap();
        let mut report = StageReport::default();
        scan_staged(&repo, &GitPolicy::default(), &mut report, false);
        assert!(report.violations.is_empty(), "readable diff: {report:?}");
        let subtree = tree.get_name("nested").unwrap().id().to_string();
        std::fs::remove_file(
            repo.path()
                .join("objects")
                .join(&subtree[..2])
                .join(&subtree[2..]),
        )
        .unwrap();
        let repo = Repository::open(dir.path()).unwrap();
        let mut report = StageReport::default();
        scan_staged(&repo, &GitPolicy::default(), &mut report, false);
        assert!(report.violations.iter().any(|v| v.rule == "git.secret_scan" && v.level == super::super::PolicyLevel::Block), "{report:?}");
    }
}
