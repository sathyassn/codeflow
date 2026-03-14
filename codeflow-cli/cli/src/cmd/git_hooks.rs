//! Git hooks command: manage git hooks (commit-msg, pre-commit, etc.).
//!
//! Subcommands mirror the Go CLI (`codeflow git-hooks <subcommand>`):
//! - `commit-msg <file>` — validate commit message format
//! - `post-commit` — log commit metadata
//! - `pre-push <remote> <url>` — validate push against protected branches
//! - `prepare-commit-msg <file> [source]` — generate commit message template
//! - `pre-commit-validate` — run pure-logic pre-commit checks

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Result, bail};
use clap::Subcommand;
use regex::Regex;

use super::config::{EnforcementPolicy, default_policy, load_enforcement_policy};
use crate::helpers;

/// Git hooks subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum GitHooksCommand {
    /// Validate commit message format
    #[command(name = "commit-msg")]
    CommitMsg {
        /// Path to the commit message file
        file: PathBuf,
    },
    /// Log commit metadata and show confirmation
    #[command(name = "post-commit")]
    PostCommit,
    /// Validate push against protected branches
    #[command(name = "pre-push")]
    PrePush {
        /// Remote name
        remote: String,
        /// Remote URL
        url: String,
    },
    /// Generate commit message template from branch name
    #[command(name = "prepare-commit-msg")]
    PrepareCommitMsg {
        /// Path to the commit message file
        file: PathBuf,
        /// Commit source (message, merge, commit, squash, or empty)
        source: Option<String>,
    },
    /// Run pure-logic pre-commit checks
    #[command(name = "pre-commit-validate")]
    PreCommitValidate,
}

pub fn run(cmd: Option<GitHooksCommand>) -> Result<()> {
    let Some(subcmd) = cmd else {
        println!("Git hook implementations");
        println!();
        println!("Usage:");
        println!("  codeflow git-hooks <command>");
        println!();
        println!("Available Commands:");
        println!("  commit-msg             Validate commit message format");
        println!("  post-commit            Log commit metadata and show confirmation");
        println!("  pre-push               Validate push against protected branches");
        println!("  prepare-commit-msg     Generate commit message template from branch name");
        println!("  pre-commit-validate    Run pure-logic pre-commit checks");
        return Ok(());
    };

    match subcmd {
        GitHooksCommand::CommitMsg { file } => run_commit_msg(&file),
        GitHooksCommand::PostCommit
        | GitHooksCommand::PrePush { .. }
        | GitHooksCommand::PreCommitValidate => Ok(()),
        GitHooksCommand::PrepareCommitMsg { file, source } => {
            run_prepare_commit_msg(&file, source.as_deref())
        }
    }
}

// ─── commit-msg ──────────────────────────────────────────────────────────────

fn run_commit_msg(msg_file: &Path) -> Result<()> {
    let raw = std::fs::read_to_string(msg_file)?;
    let policy = load_policy();
    validate_commit_msg(&raw, &policy)
}

fn load_policy() -> EnforcementPolicy {
    helpers::detect_project_dir()
        .ok()
        .and_then(|dir| load_enforcement_policy(&dir).ok())
        .unwrap_or_else(default_policy)
}

/// Validate a commit message against the enforcement policy.
/// Returns `Ok(())` for valid messages, or `Err` with collected errors.
fn validate_commit_msg(raw_msg: &str, policy: &EnforcementPolicy) -> Result<()> {
    // Strip git comment lines.
    let cleaned: Vec<&str> = raw_msg
        .lines()
        .filter(|line| !line.starts_with('#'))
        .collect();
    let commit_msg = cleaned.join("\n");

    if commit_msg.trim().is_empty() {
        bail!("commit message is empty");
    }

    let lines: Vec<&str> = commit_msg.split('\n').collect();
    let subject = lines[0];

    // Skip conditions: merge, revert, fixup/squash.
    if is_merge_commit(subject) || is_revert_commit(subject) || is_fixup_squash(subject) {
        return Ok(());
    }

    let mut errors: Vec<String> = Vec::new();

    check_ai_attribution(&commit_msg, policy, &mut errors);
    check_subject_length(subject, policy, &mut errors);
    check_subject_format(subject, policy, &mut errors);
    check_trailing_period(subject, policy, &mut errors);
    check_lowercase_type(subject, policy, &mut errors);

    if lines.len() > 1 {
        check_body(&lines, policy, &mut errors);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        bail!(
            "commit message validation failed with {} error(s):\n{}",
            errors.len(),
            errors.join("\n")
        );
    }
}

fn is_merge_commit(subject: &str) -> bool {
    subject.starts_with("Merge pull request #") || subject.starts_with("Merge branch ")
}

fn is_revert_commit(subject: &str) -> bool {
    subject.starts_with("Revert ")
}

fn is_fixup_squash(subject: &str) -> bool {
    subject.starts_with("fixup!") || subject.starts_with("squash!")
}

fn check_ai_attribution(msg: &str, policy: &EnforcementPolicy, errors: &mut Vec<String>) {
    let lower = msg.to_lowercase();
    for pattern in &policy.git_format.ai_attribution_patterns {
        if lower.contains(&pattern.to_lowercase()) {
            errors.push(format!("AI attribution detected: pattern '{pattern}'"));
            return;
        }
    }
}

fn check_subject_length(subject: &str, policy: &EnforcementPolicy, errors: &mut Vec<String>) {
    let max_len = if policy.git_format.subject.max_length == 0 {
        50
    } else {
        policy.git_format.subject.max_length
    };
    if subject.len() > max_len {
        errors.push(format!(
            "subject line too long: {} chars (max: {max_len})",
            subject.len()
        ));
    }
}

fn check_subject_format(subject: &str, policy: &EnforcementPolicy, errors: &mut Vec<String>) {
    let types = &policy.git_format.commit_types;
    if types.is_empty() {
        return;
    }
    // Build regex: ^(type1|type2|...): .+$
    // Scoped types like feat(api) are intentionally rejected.
    let escaped: Vec<String> = types.iter().map(|t| regex::escape(t)).collect();
    let pattern = format!("^({}): .+$", escaped.join("|"));
    let Ok(re) = Regex::new(&pattern) else {
        return;
    };
    if !re.is_match(subject) {
        errors.push(format!(
            "invalid commit format: '{}'. Required: type: description. Valid types: {}",
            subject,
            types.join(", ")
        ));
    }
}

fn check_trailing_period(subject: &str, policy: &EnforcementPolicy, errors: &mut Vec<String>) {
    if policy.git_format.subject.forbid_trailing_period && subject.ends_with('.') {
        errors.push("subject line should not end with a period".into());
    }
}

fn check_lowercase_type(subject: &str, policy: &EnforcementPolicy, errors: &mut Vec<String>) {
    if !policy.git_format.subject.require_lowercase_type {
        return;
    }
    if let Some(ch) = subject.chars().next() {
        if ch.is_uppercase() {
            errors.push("type should be lowercase".into());
        }
    }
}

fn check_body(lines: &[&str], policy: &EnforcementPolicy, errors: &mut Vec<String>) {
    let body_lines = &lines[1..];

    // Check blank line after subject.
    if !body_lines.is_empty() && !body_lines[0].trim().is_empty() {
        errors.push("missing blank line after subject".into());
    }

    // Collect non-empty body content.
    let content_lines: Vec<&str> = body_lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .copied()
        .collect();

    if content_lines.is_empty() {
        return;
    }

    // Detect GitHub squash-merge asterisk bullets.
    for line in &content_lines {
        if line.starts_with("* ") {
            errors.push(
                "GitHub squash-merge format detected ('* ' prefix). Use '- ' prefix bullets".into(),
            );
            break;
        }
    }

    // Check bullets-only format.
    let non_bullet: Vec<&&str> = content_lines
        .iter()
        .filter(|l| !l.starts_with("- "))
        .collect();
    if !non_bullet.is_empty() {
        let joined: Vec<&str> = non_bullet.iter().map(|l| **l).collect();
        errors.push(format!(
            "body must contain only bullet points (lines starting with '- '). Found non-bullet lines: {}",
            joined.join("; ")
        ));
    }

    // Count bullets.
    let max_bullets = if policy.git_format.body.max_bullets == 0 {
        3
    } else {
        policy.git_format.body.max_bullets
    };
    let bullet_count = content_lines.iter().filter(|l| l.starts_with("- ")).count();
    if bullet_count > max_bullets {
        errors.push(format!(
            "too many bullet points: {bullet_count} (max: {max_bullets})"
        ));
    }

    // Check blank lines between bullets.
    check_blank_between_bullets(body_lines, errors);

    // Check body line length.
    let max_line_len = if policy.git_format.body.line_max_length == 0 {
        72
    } else {
        policy.git_format.body.line_max_length
    };
    for line in &content_lines {
        if line.len() > max_line_len {
            errors.push(format!(
                "body line too long: {} chars (max: {max_line_len}): {line}",
                line.len()
            ));
        }
    }
}

fn check_blank_between_bullets(body_lines: &[&str], errors: &mut Vec<String>) {
    let mut in_bullets = false;
    let mut found_blank = false;
    for line in body_lines {
        if line.starts_with("- ") {
            if found_blank && in_bullets {
                errors.push("blank lines between bullets: bullets must be contiguous".into());
                return;
            }
            in_bullets = true;
            found_blank = false;
        } else if line.trim().is_empty() && in_bullets {
            found_blank = true;
        }
    }
}

// ─── prepare-commit-msg ──────────────────────────────────────────────────────

fn run_prepare_commit_msg(msg_file: &Path, source: Option<&str>) -> Result<()> {
    // Skip conditions.
    if let Some("message" | "merge" | "commit" | "squash") = source {
        return Ok(());
    }

    let policy = load_policy();
    let branch = get_current_branch().unwrap_or_default();
    let (commit_type, commit_scope) = extract_type_and_scope(&branch, &policy);

    // Read current file content.
    let current_msg = std::fs::read_to_string(msg_file).unwrap_or_default();

    // Only generate template if file is empty or contains only git comments.
    if !is_empty_or_comments(&current_msg) {
        return Ok(());
    }

    write_template(msg_file, &branch, &commit_type, &commit_scope, &policy)
}

fn get_current_branch() -> Option<String> {
    let output = Command::new("git")
        .args(["symbolic-ref", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let refname = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Some(refname.strip_prefix("refs/heads/")?.to_string())
}

fn extract_type_and_scope(branch: &str, policy: &EnforcementPolicy) -> (String, String) {
    // Match branch names like feat/something, fix/something, etc.
    let Ok(re) = Regex::new(r"^([a-z]+)/(.+)$") else {
        return (String::new(), String::new());
    };
    let Some(caps) = re.captures(branch) else {
        return (String::new(), String::new());
    };

    let branch_type = caps.get(1).map_or("", |m| m.as_str());
    let slug = caps.get(2).map_or("", |m| m.as_str());

    // Validate that the extracted type is a known commit type.
    let valid = policy
        .git_format
        .commit_types
        .iter()
        .any(|t| t == branch_type);
    if !valid {
        return (String::new(), String::new());
    }

    // Extract scope from slug (first component before '-').
    let Ok(scope_re) = Regex::new(r"^([a-z0-9]+)-") else {
        return (branch_type.to_string(), String::new());
    };
    let scope = scope_re
        .captures(slug)
        .and_then(|c| c.get(1))
        .map_or(String::new(), |m| m.as_str().to_string());

    (branch_type.to_string(), scope)
}

fn is_empty_or_comments(msg: &str) -> bool {
    for line in msg.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            return false;
        }
    }
    true
}

fn write_template(
    msg_file: &Path,
    branch: &str,
    commit_type: &str,
    commit_scope: &str,
    policy: &EnforcementPolicy,
) -> Result<()> {
    let mut buf = String::new();

    if commit_type.is_empty() {
        buf.push_str("type: description\n");
    } else if commit_scope.is_empty() {
        let _ = writeln!(buf, "{commit_type}: ");
    } else {
        let _ = writeln!(buf, "{commit_type}({commit_scope}): ");
    }

    buf.push('\n');
    buf.push_str("# Conventional commit format:\n");
    buf.push_str("#   type(scope): description\n");
    buf.push_str("#\n");

    let types = &policy.git_format.commit_types;
    if !types.is_empty() {
        let _ = writeln!(buf, "# Types: {}", types.join(", "));
    }

    buf.push_str("#\n");
    buf.push_str("# Examples:\n");
    buf.push_str("#   feat: add user authentication\n");
    buf.push_str("#   fix(api): resolve null pointer exception\n");
    buf.push_str("#   docs: update README\n");
    buf.push_str("#\n");

    if !branch.is_empty() {
        let _ = writeln!(buf, "# Branch: {branch}");
    }
    buf.push_str("#\n");

    std::fs::write(msg_file, buf)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_no_subcommand_shows_help() {
        let result = run(None);
        assert!(result.is_ok());
    }

    // ─── commit-msg validation tests ─────────────────────────────────────

    fn test_policy() -> EnforcementPolicy {
        default_policy()
    }

    #[test]
    fn test_valid_commit_msg() {
        let policy = test_policy();
        assert!(validate_commit_msg("feat: add new feature\n", &policy).is_ok());
        assert!(validate_commit_msg("fix: resolve bug\n", &policy).is_ok());
        assert!(validate_commit_msg("docs: update README\n", &policy).is_ok());
    }

    #[test]
    fn test_invalid_empty_message() {
        let policy = test_policy();
        assert!(validate_commit_msg("", &policy).is_err());
        assert!(validate_commit_msg("   \n", &policy).is_err());
    }

    #[test]
    fn test_invalid_missing_type() {
        let policy = test_policy();
        let result = validate_commit_msg("add new feature\n", &policy);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_uppercase_type() {
        let policy = test_policy();
        let result = validate_commit_msg("Feat: add new feature\n", &policy);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_scoped_type_rejected() {
        let policy = test_policy();
        let result = validate_commit_msg("feat(api): add endpoint\n", &policy);
        assert!(result.is_err());
    }

    #[test]
    fn test_subject_too_long() {
        let policy = test_policy();
        // 100 chars after "feat: " = 106 total, max is 50.
        let msg = format!("feat: {}\n", "a".repeat(100));
        let result = validate_commit_msg(&msg, &policy);
        assert!(result.is_err());
    }

    #[test]
    fn test_merge_commit_skipped() {
        let policy = test_policy();
        assert!(validate_commit_msg("Merge pull request #123 from branch\n", &policy).is_ok());
        assert!(validate_commit_msg("Merge branch 'main'\n", &policy).is_ok());
    }

    #[test]
    fn test_revert_commit_skipped() {
        let policy = test_policy();
        assert!(validate_commit_msg("Revert \"feat: something\"\n", &policy).is_ok());
    }

    #[test]
    fn test_fixup_squash_skipped() {
        let policy = test_policy();
        assert!(validate_commit_msg("fixup! feat: something\n", &policy).is_ok());
        assert!(validate_commit_msg("squash! fix: something\n", &policy).is_ok());
    }

    #[test]
    fn test_comment_lines_stripped() {
        let policy = test_policy();
        let msg = "feat: add feature\n# this is a comment\n";
        assert!(validate_commit_msg(msg, &policy).is_ok());
    }

    #[test]
    fn test_trailing_period_rejected() {
        let policy = test_policy();
        let result = validate_commit_msg("feat: add feature.\n", &policy);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_description_rejected() {
        let policy = test_policy();
        let result = validate_commit_msg("feat: \n", &policy);
        assert!(result.is_err());
    }

    // ─── body validation tests ──────────────────────────────────────────

    #[test]
    fn test_body_valid_bullets() {
        let policy = test_policy();
        let msg = "feat: add feature\n\n- bullet one\n- bullet two\n";
        assert!(validate_commit_msg(msg, &policy).is_ok());
    }

    #[test]
    fn test_body_missing_blank_line() {
        let policy = test_policy();
        let msg = "feat: add feature\n- no blank line\n";
        let result = validate_commit_msg(msg, &policy);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("missing blank line after subject"));
    }

    #[test]
    fn test_body_squash_merge_bullets() {
        let policy = test_policy();
        let msg = "feat: add feature\n\n* squash bullet\n";
        let result = validate_commit_msg(msg, &policy);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("squash-merge format"));
    }

    #[test]
    fn test_body_non_bullet_lines() {
        let policy = test_policy();
        let msg = "feat: add feature\n\nsome plain text\n";
        let result = validate_commit_msg(msg, &policy);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("bullet points"));
    }

    #[test]
    fn test_body_too_many_bullets() {
        let policy = test_policy();
        // Default max_bullets is 3; write 5.
        let msg = "feat: add feature\n\n- one\n- two\n- three\n- four\n- five\n";
        let result = validate_commit_msg(msg, &policy);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("too many bullet points"));
    }

    #[test]
    fn test_body_blank_between_bullets() {
        let policy = test_policy();
        let msg = "feat: add feature\n\n- one\n\n- two\n";
        let result = validate_commit_msg(msg, &policy);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("blank lines between bullets"));
    }

    #[test]
    fn test_body_line_too_long() {
        let policy = test_policy();
        // Default line_max_length is 72. Create a bullet that's 80 chars.
        let long_text = "a".repeat(78);
        let msg = format!("feat: add feature\n\n- {long_text}\n");
        let result = validate_commit_msg(&msg, &policy);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("body line too long"));
    }

    #[test]
    fn test_body_only_blank_lines() {
        let policy = test_policy();
        // Subject + blank line + more blank lines = body_lines are all empty.
        let msg = "feat: add feature\n\n\n\n";
        assert!(validate_commit_msg(msg, &policy).is_ok());
    }

    // ─── dispatch tests ─────────────────────────────────────────────────

    #[test]
    fn test_run_commit_msg_dispatch() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("msg");
        std::fs::write(&file, "feat: valid message\n").unwrap();
        let result = run(Some(GitHooksCommand::CommitMsg { file }));
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_commit_msg_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("msg");
        std::fs::write(&file, "bad message\n").unwrap();
        let result = run(Some(GitHooksCommand::CommitMsg { file }));
        assert!(result.is_err());
    }

    #[test]
    fn test_run_commit_msg_nonexistent_file() {
        let result = run(Some(GitHooksCommand::CommitMsg {
            file: PathBuf::from("/tmp/claude/nonexistent-commit-msg-file-xyz"),
        }));
        assert!(result.is_err());
    }

    #[test]
    fn test_run_prepare_commit_msg_dispatch() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("msg");
        std::fs::write(&file, "").unwrap();
        let result = run(Some(GitHooksCommand::PrepareCommitMsg {
            file,
            source: Some("message".to_string()),
        }));
        assert!(result.is_ok());
    }

    // ─── template tests ─────────────────────────────────────────────────

    #[test]
    fn test_write_template_no_type() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("msg");
        let policy = test_policy();
        write_template(&file, "main", "", "", &policy).unwrap();
        let content = std::fs::read_to_string(&file).unwrap();
        assert!(content.starts_with("type: description"));
    }

    #[test]
    fn test_write_template_type_no_scope() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("msg");
        let policy = test_policy();
        write_template(&file, "feat/something", "feat", "", &policy).unwrap();
        let content = std::fs::read_to_string(&file).unwrap();
        assert!(content.starts_with("feat: "));
        assert!(content.contains("# Branch: feat/something"));
    }

    #[test]
    fn test_write_template_type_and_scope() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("msg");
        let policy = test_policy();
        write_template(&file, "feat/api-add", "feat", "api", &policy).unwrap();
        let content = std::fs::read_to_string(&file).unwrap();
        assert!(content.starts_with("feat(api): "));
    }

    #[test]
    fn test_write_template_empty_branch() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("msg");
        let policy = test_policy();
        write_template(&file, "", "feat", "", &policy).unwrap();
        let content = std::fs::read_to_string(&file).unwrap();
        // Empty branch should not produce a "# Branch:" line.
        assert!(!content.contains("# Branch:"));
    }

    // ─── edge case tests ────────────────────────────────────────────────

    #[test]
    fn test_extract_type_and_scope_no_dash_in_slug() {
        let policy = default_policy();
        let (t, s) = extract_type_and_scope("feat/singlesegment", &policy);
        assert_eq!(t, "feat");
        // No dash => scope regex doesn't match => empty scope.
        assert!(s.is_empty());
    }

    #[test]
    fn test_prepare_skip_squash_source() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("msg");
        std::fs::write(&file, "").unwrap();
        let result = run_prepare_commit_msg(&file, Some("squash"));
        assert!(result.is_ok());
        let content = std::fs::read_to_string(&file).unwrap();
        assert!(content.is_empty());
    }

    // ─── prepare-commit-msg tests ────────────────────────────────────────

    #[test]
    fn test_prepare_skip_message_source() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("msg");
        std::fs::write(&file, "").unwrap();
        let result = run_prepare_commit_msg(&file, Some("message"));
        assert!(result.is_ok());
        let content = std::fs::read_to_string(&file).unwrap();
        assert!(content.is_empty());
    }

    #[test]
    fn test_prepare_skip_merge_source() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("msg");
        std::fs::write(&file, "").unwrap();
        let result = run_prepare_commit_msg(&file, Some("merge"));
        assert!(result.is_ok());
        let content = std::fs::read_to_string(&file).unwrap();
        assert!(content.is_empty());
    }

    #[test]
    fn test_prepare_skip_commit_source() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("msg");
        std::fs::write(&file, "").unwrap();
        let result = run_prepare_commit_msg(&file, Some("commit"));
        assert!(result.is_ok());
        let content = std::fs::read_to_string(&file).unwrap();
        assert!(content.is_empty());
    }

    #[test]
    fn test_prepare_generates_template_for_empty_source() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("msg");
        std::fs::write(&file, "").unwrap();
        let result = run_prepare_commit_msg(&file, Some(""));
        assert!(result.is_ok());
        let content = std::fs::read_to_string(&file).unwrap();
        assert!(content.len() > 5);
    }

    #[test]
    fn test_prepare_generates_template_for_none_source() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("msg");
        std::fs::write(&file, "").unwrap();
        let result = run_prepare_commit_msg(&file, None);
        assert!(result.is_ok());
        let content = std::fs::read_to_string(&file).unwrap();
        assert!(content.len() > 5);
    }

    #[test]
    fn test_prepare_does_not_overwrite_existing_content() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("msg");
        std::fs::write(&file, "existing message\n").unwrap();
        let result = run_prepare_commit_msg(&file, None);
        assert!(result.is_ok());
        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "existing message\n");
    }

    #[test]
    fn test_is_empty_or_comments() {
        assert!(is_empty_or_comments(""));
        assert!(is_empty_or_comments("# comment\n"));
        assert!(is_empty_or_comments("  \n# comment\n  \n"));
        assert!(!is_empty_or_comments("some text\n"));
    }

    #[test]
    fn test_extract_type_and_scope_feat_branch() {
        let policy = default_policy();
        let (t, s) = extract_type_and_scope("feat/abc-something", &policy);
        assert_eq!(t, "feat");
        assert_eq!(s, "abc");
    }

    #[test]
    fn test_extract_type_and_scope_fix_branch() {
        let policy = default_policy();
        let (t, s) = extract_type_and_scope("fix/conformance-test-isolation", &policy);
        assert_eq!(t, "fix");
        assert_eq!(s, "conformance");
    }

    #[test]
    fn test_extract_type_and_scope_invalid_type() {
        let policy = default_policy();
        let (t, _) = extract_type_and_scope("invalid/something", &policy);
        assert!(t.is_empty());
    }

    #[test]
    fn test_extract_type_and_scope_no_slash() {
        let policy = default_policy();
        let (t, _) = extract_type_and_scope("main", &policy);
        assert!(t.is_empty());
    }

    // ─── stub subcommand tests ───────────────────────────────────────────

    #[test]
    fn test_post_commit_stub() {
        let result = run(Some(GitHooksCommand::PostCommit));
        assert!(result.is_ok());
    }

    #[test]
    fn test_pre_push_stub() {
        let result = run(Some(GitHooksCommand::PrePush {
            remote: "origin".to_string(),
            url: "https://github.com/test/repo.git".to_string(),
        }));
        assert!(result.is_ok());
    }

    #[test]
    fn test_pre_commit_validate_stub() {
        let result = run(Some(GitHooksCommand::PreCommitValidate));
        assert!(result.is_ok());
    }
}
