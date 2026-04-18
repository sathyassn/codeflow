//! `GhPrGuard` hook handler for PR body validation and protected branch enforcement.
//!
//! Extracted from `pre_tool_use` so it can evolve independently and be tested
//! in isolation.  Validation uses a simple line-by-line markdown heading parser
//! rather than substring matching, enabling reliable detection of the canonical
//! `## Test Results` section structure introduced by the generic testing subsystem.

use std::sync::OnceLock;

use regex::Regex;

use super::{BlockCategory, HookEvent, HookHandler, HookInput, HookOutput};
use crate::error::HookError;
use crate::hooks::pre_tool_use::PrBodyConfig;

// ---------------------------------------------------------------------------
// Regex helpers (compiled once)
// ---------------------------------------------------------------------------

pub(crate) fn gh_pr_merge_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:^|\s|&&|\|)gh\s+pr\s+merge(?:\s|$)").expect("valid regex"))
}

pub(crate) fn pr_number_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"gh\s+pr\s+merge\s+(\d+)").expect("valid regex"))
}

pub(crate) fn gh_pr_create_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Match both `gh pr create` and `gh pr edit <N>` — both set the PR body
        // and therefore need the same body-validation invariants.
        Regex::new(r"(?:^|\s|&&|\|)gh\s+pr\s+(?:create|edit)(?:\s|$)").expect("valid regex")
    })
}

// ---------------------------------------------------------------------------
// Branch pattern matching helper (mirrors pre_tool_use)
// ---------------------------------------------------------------------------

/// Match a branch name against a protection pattern that may contain `*`.
fn match_branch_pattern(branch: &str, pattern: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix("/*") {
        branch == prefix || branch.starts_with(&format!("{prefix}/"))
    } else {
        branch == pattern
    }
}

// ---------------------------------------------------------------------------
// Unicode helpers
// ---------------------------------------------------------------------------

/// Strips zero-width Unicode characters that could be used to bypass pattern matching.
#[must_use]
pub fn strip_zero_width(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(*c as u32, 0x200B | 0x200C | 0x200D | 0xFEFF | 0x2060))
        .collect()
}

/// Returns true if the character is in a common emoji Unicode range.
#[must_use]
pub fn is_emoji(c: char) -> bool {
    matches!(c as u32,
        0x00A9              // Copyright
        | 0x00AE            // Registered
        | 0x203C            // Double exclamation
        | 0x2049            // Exclamation question
        | 0x2122            // Trademark
        | 0x2194..=0x2199   // Arrows
        | 0x21A9..=0x21AA   // Curved arrows
        | 0x231A..=0x231B   // Watch/hourglass
        | 0x23E9..=0x23F3   // Media controls
        | 0x24C2            // Circled M
        | 0x25AA..=0x25FE   // Geometric shapes
        | 0x2600..=0x26FF   // Misc symbols
        | 0x2700..=0x27BF   // Dingbats
        | 0xFE00..=0xFE0F   // Variation selectors
        | 0x1F000..=0x1F02F // Mahjong
        | 0x1F0A0..=0x1F0FF // Playing cards
        | 0x1F1E0..=0x1F1FF // Flags
        | 0x1F300..=0x1F5FF // Misc symbols & pictographs
        | 0x1F600..=0x1F64F // Emoticons
        | 0x1F680..=0x1F6FF // Transport & map
        | 0x1F900..=0x1F9FF // Supplemental symbols
        | 0x1FA00..=0x1FA6F // Chess symbols
        | 0x1FA70..=0x1FAFF // Symbols extended
    )
}

// ---------------------------------------------------------------------------
// --body-file path extraction + body content resolution
// ---------------------------------------------------------------------------

/// Extract the value of `--body-file=<path>` or `--body-file <path>` from a
/// shell command string. Returns `None` when the flag is absent.
///
/// Understood forms:
/// - `--body-file=/path/to/file.md`
/// - `--body-file /path/to/file.md`
/// - `--body-file "/path with spaces.md"`
/// - `--body-file '/path with spaces.md'`
///
/// The function is deliberately conservative: it walks the command as tokens,
/// respecting matched single and double quotes so an inline `--body-file` that
/// appears INSIDE a quoted string (e.g. documentation in a heredoc) is not
/// misread as an actual flag.
#[must_use]
pub fn extract_body_file_path(command: &str) -> Option<String> {
    let tokens = tokenise_shell(command);
    let mut iter = tokens.into_iter();
    while let Some(tok) = iter.next() {
        if let Some(val) = tok.strip_prefix("--body-file=") {
            return Some(val.to_string());
        }
        if tok == "--body-file" {
            return iter.next();
        }
    }
    None
}

/// Tokenise a shell command respecting single- and double-quoted strings.
/// Unclosed quotes are tolerated: the partial token is returned as-is.
fn tokenise_shell(command: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut iter = command.chars().peekable();

    while let Some(c) = iter.next() {
        match c {
            '\\' if !in_single => {
                // Bash backslash escapes everywhere except inside single quotes.
                // Preserve the next char literally so `--body-file "\"foo\""` survives.
                if let Some(next) = iter.next() {
                    current.push(next);
                }
            }
            '\'' if !in_double => {
                in_single = !in_single;
            }
            '"' if !in_single => {
                in_double = !in_double;
            }
            c if c.is_whitespace() && !in_single && !in_double => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(c),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Failure modes for [`load_body_content`].
#[derive(Debug, PartialEq, Eq)]
pub enum BodyLoadError {
    /// Command provides neither `--body` nor `--body-file`.
    MissingBodyFlag,
    /// `--body-file=-` explicitly rejected (stdin not supported).
    StdinNotSupported,
    /// File path does not exist or cannot be read.
    FileUnreadable(String),
    /// File contents are not valid UTF-8.
    NotUtf8(String),
}

impl std::fmt::Display for BodyLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingBodyFlag => write!(f, "Missing --body or --body-file flag"),
            Self::StdinNotSupported => write!(
                f,
                "--body-file=- (stdin) is not supported by the hook validator; \
                 use --body-file with a real file path"
            ),
            Self::FileUnreadable(msg) => write!(f, "--body-file unreadable: {msg}"),
            Self::NotUtf8(path) => {
                write!(f, "--body-file contents are not valid UTF-8: {path}")
            }
        }
    }
}

/// Resolve the PR body content for validation.
///
/// Resolution order:
/// 1. `--body-file <path>` — read and return the file's text content.
/// 2. `--body "..."` / `--body=...` — return the original command (the inline
///    body is embedded there; existing substring matchers search it directly).
/// 3. Neither present → [`BodyLoadError::MissingBodyFlag`].
///
/// Returning the raw command in case (2) preserves backward compatibility with
/// every existing test and validator that scans `command.contains(...)`.
///
/// # Errors
///
/// Returns [`BodyLoadError::StdinNotSupported`] when the caller passes
/// `--body-file=-`, [`BodyLoadError::FileUnreadable`] when the file cannot be
/// opened or read, [`BodyLoadError::NotUtf8`] when the file's bytes are not
/// valid UTF-8, or [`BodyLoadError::MissingBodyFlag`] when neither `--body`
/// nor `--body-file` appears in the command.
pub fn load_body_content(command: &str) -> Result<String, BodyLoadError> {
    if let Some(path) = extract_body_file_path(command) {
        if path == "-" {
            return Err(BodyLoadError::StdinNotSupported);
        }
        match std::fs::read(&path) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(s) => Ok(s),
                Err(_) => Err(BodyLoadError::NotUtf8(path)),
            },
            Err(e) => Err(BodyLoadError::FileUnreadable(format!("{path}: {e}"))),
        }
    } else if has_inline_body_flag(command) {
        Ok(command.to_string())
    } else {
        Err(BodyLoadError::MissingBodyFlag)
    }
}

/// True when the command uses the inline `--body <text>` or `--body=<text>`
/// form (as distinct from `--body-file …`).
fn has_inline_body_flag(command: &str) -> bool {
    // A bare `--body` (with optional `=value`) but explicitly NOT `--body-file`.
    // We scan tokens to avoid false positives when `--body-file` is present.
    let tokens = tokenise_shell(command);
    let mut saw_inline = false;
    for tok in &tokens {
        if tok == "--body" || tok.starts_with("--body=") {
            saw_inline = true;
        }
    }
    saw_inline
}

// ---------------------------------------------------------------------------
// DI trait for PR resolution
// ---------------------------------------------------------------------------

/// Trait for resolving PR target branches (dependency injection for testability).
pub trait PRResolver: Send + Sync {
    /// Resolve the target (base) branch for a given PR number.
    /// Returns `None` if the PR cannot be resolved.
    fn resolve_target_branch(&self, pr_number: &str) -> Option<String>;
}

/// Production resolver that calls `gh pr view`.
pub struct OsPRResolver;

impl PRResolver for OsPRResolver {
    fn resolve_target_branch(&self, pr_number: &str) -> Option<String> {
        let output = std::process::Command::new("gh")
            .args([
                "pr",
                "view",
                pr_number,
                "--json",
                "baseRefName",
                "-q",
                ".baseRefName",
            ])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if branch.is_empty() {
            None
        } else {
            Some(branch)
        }
    }
}

// ---------------------------------------------------------------------------
// Markdown heading parser
// ---------------------------------------------------------------------------

/// Heading entry produced by the line-by-line parser.
#[derive(Debug, PartialEq)]
struct Heading {
    level: usize,
    text: String,
    line_index: usize,
}

/// Parse all ATX headings from a multi-line string.
fn parse_headings(text: &str) -> Vec<Heading> {
    text.lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                let level = trimmed.chars().take_while(|&c| c == '#').count();
                let rest = trimmed[level..].trim();
                if !rest.is_empty() {
                    return Some(Heading {
                        level,
                        text: rest.to_string(),
                        line_index: i,
                    });
                }
            }
            None
        })
        .collect()
}

/// Collect non-blank content lines under heading at `heading_line`, stopping
/// when a heading at level <= `stop_level` is encountered (or at end of input).
fn lines_under_heading<'a>(
    all_lines: &[&'a str],
    heading_line: usize,
    stop_level: usize,
) -> Vec<&'a str> {
    let mut result = Vec::new();
    for line in all_lines.iter().skip(heading_line + 1) {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            let level = trimmed.chars().take_while(|&c| c == '#').count();
            if level <= stop_level {
                break;
            }
        }
        if !trimmed.is_empty() {
            result.push(*line);
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Test Results section validation (markdown heading tree parser)
// ---------------------------------------------------------------------------

/// Validate the `## Test Results` section structure.
///
/// Canonical structure:
/// ```markdown
/// ## Test Results
/// ### 1. Overall Test Pass Status
/// - Result: ...
/// ### 2. Overall Coverage
/// - Workspace: N%
/// ### 3. Modified File Coverage
/// | File | Coverage | ... |
/// ```
///
/// Fresh-project path (no targets configured):
/// ```markdown
/// ## Test Results
/// No test targets configured.
/// ```
///
/// Legacy format (rejected):
/// ```markdown
/// ## Test Stats
/// ```
#[must_use]
pub fn validate_test_results_section(body: &str) -> Vec<String> {
    let mut errors = Vec::new();

    let headings = parse_headings(body);

    // Reject legacy "## Test Stats" — must regenerate with the new CLI.
    if headings
        .iter()
        .any(|h| h.level == 2 && h.text == "Test Stats")
    {
        errors.push(
            "legacy PR body format detected ('## Test Stats'); regenerate via \
             `codeflow test --mode full`"
                .to_string(),
        );
        return errors;
    }

    // Find "## Test Results".
    let tr_heading = match headings
        .iter()
        .find(|h| h.level == 2 && h.text == "Test Results")
    {
        Some(h) => h,
        None => return errors, // Missing heading — caught by required_sections check.
    };

    let all_lines: Vec<&str> = body.lines().collect();

    // Collect non-blank content lines directly under "## Test Results" (before any sub-heading).
    let direct_content = lines_under_heading(&all_lines, tr_heading.line_index, 2);
    let joined = direct_content.join(" ");
    let trimmed = joined.trim();

    // Fresh-project path: content is exactly "No test targets configured."
    if trimmed == "No test targets configured." {
        return errors;
    }

    // Validate the three mandatory sub-sections.
    let sub_required: &[(&str, &str, usize)] = &[
        ("1. Overall Test Pass Status", "Result:", 3),
        ("2. Overall Coverage", "%", 3),
        ("3. Modified File Coverage", "|", 3),
    ];

    for (text, signal, level) in sub_required {
        let heading_text = format!("{} {}", "#".repeat(*level), text);
        if let Some(h) = headings
            .iter()
            .find(|h| h.level == *level && h.text == *text)
        {
            let content = lines_under_heading(&all_lines, h.line_index, *level);
            let content_str = content.join("\n");
            if !content_str.contains(signal) {
                errors.push(format!(
                    "Section '{heading_text}' missing expected content (looked for '{signal}')"
                ));
            }
        } else {
            errors.push(format!("Missing required sub-section: '{heading_text}'"));
        }
    }

    errors
}

// ---------------------------------------------------------------------------
// GhPrGuard handler
// ---------------------------------------------------------------------------

/// GitHub PR guard: blocks `gh pr merge` on protected branches
/// and validates `gh pr create` body content using markdown heading parsing.
pub struct GhPrGuard<R: PRResolver = OsPRResolver> {
    protected_branches: Vec<String>,
    resolver: R,
    pr_config: PrBodyConfig,
    ai_patterns: Vec<String>,
}

impl GhPrGuard<OsPRResolver> {
    #[must_use]
    pub fn new(
        protected_branches: Vec<String>,
        pr_config: PrBodyConfig,
        ai_patterns: Vec<String>,
    ) -> Self {
        Self {
            protected_branches,
            resolver: OsPRResolver,
            pr_config,
            ai_patterns,
        }
    }
}

impl<R: PRResolver> GhPrGuard<R> {
    #[must_use]
    pub fn with_resolver(
        protected_branches: Vec<String>,
        resolver: R,
        pr_config: PrBodyConfig,
        ai_patterns: Vec<String>,
    ) -> Self {
        Self {
            protected_branches,
            resolver,
            pr_config,
            ai_patterns,
        }
    }
}

impl<R: PRResolver> HookHandler for GhPrGuard<R> {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        let tool_name = input.tool_name.as_deref().unwrap_or("");

        if tool_name != "Bash" {
            return Ok(HookOutput::Allow);
        }

        let command = input
            .tool_input
            .as_ref()
            .and_then(|v| v.get("command"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if command.is_empty() {
            return Ok(HookOutput::Allow);
        }

        // --- PR merge guard ---
        if gh_pr_merge_re().is_match(command) {
            if let Some(caps) = pr_number_re().captures(command) {
                let pr_number = &caps[1];
                if let Some(target_branch) = self.resolver.resolve_target_branch(pr_number) {
                    for protected in &self.protected_branches {
                        if match_branch_pattern(&target_branch, protected) {
                            return Ok(HookOutput::Block {
                                reason: format!(
                                    "BLOCKED: Cannot merge PR #{pr_number} into protected branch \
                                     '{target_branch}'.\nProtected branches require manual merge \
                                     via GitHub UI or admin override.\nMatched protection \
                                     pattern: {protected}\n\nMUST: Do not attempt to merge into \
                                     protected branches via CLI.\n"
                                ),
                                category: Some(BlockCategory::GhPrGuard),
                            });
                        }
                    }
                }
            }
        }

        // --- PR body validation (gh pr create / gh pr edit) ---
        if gh_pr_create_re().is_match(command) {
            let mut errors: Vec<String> = Vec::new();

            // Resolve body content — either from --body-file <path> or from the
            // command itself (inline --body). Any resolution failure becomes a
            // block reason so the PR can't be created/edited with an empty or
            // unreadable body.
            let body_content = match load_body_content(command) {
                Ok(body) => body,
                Err(BodyLoadError::MissingBodyFlag) => {
                    if self.pr_config.require_body_flag {
                        errors.push(
                            "Missing --body or --body-file flag: PR body is required".to_string(),
                        );
                    }
                    // Fall back to command-as-body for the remaining checks — this
                    // preserves historical behaviour for configurations that don't
                    // require the flag.
                    command.to_string()
                }
                Err(other) => {
                    return Ok(HookOutput::Block {
                        reason: format!(
                            "BLOCKED: PR body validation failed:\n  - {other}\n\n\
                             MUST: Provide --body \"…\" or --body-file <path> pointing \
                             at a readable UTF-8 file.\n"
                        ),
                        category: Some(BlockCategory::GhPrGuard),
                    });
                }
            };

            for section in &self.pr_config.required_sections {
                if !body_content.contains(section.as_str()) {
                    errors.push(format!("Missing required PR section: '{section}'"));
                }
            }

            let lower_body = strip_zero_width(&body_content.to_lowercase());
            for pattern in &self.ai_patterns {
                if lower_body.contains(&pattern.to_lowercase()) {
                    errors.push(format!(
                        "AI attribution detected in PR body: pattern '{pattern}'"
                    ));
                    break;
                }
            }

            if self.pr_config.forbid_emoji {
                for ch in body_content.chars() {
                    if is_emoji(ch) {
                        errors.push(format!("Emoji character detected in PR body: '{ch}'"));
                        break;
                    }
                }
            }

            if self
                .pr_config
                .required_sections
                .iter()
                .any(|s| s.contains("Test"))
            {
                errors.extend(validate_test_results_section(&body_content));
            }

            if !errors.is_empty() {
                return Ok(HookOutput::Block {
                    reason: format!(
                        "BLOCKED: PR body validation failed with {} error(s):\n{}\n\n\
                         Required sections: {}\nMUST: Fix the PR body and retry.\n",
                        errors.len(),
                        errors
                            .iter()
                            .map(|e| format!("  - {e}"))
                            .collect::<Vec<_>>()
                            .join("\n"),
                        self.pr_config.required_sections.join(", "),
                    ),
                    category: Some(BlockCategory::GhPrGuard),
                });
            }
        }

        Ok(HookOutput::Allow)
    }

    fn name(&self) -> &'static str {
        "gh-pr-guard"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::PreToolUse]
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::pre_tool_use::PrBodyConfig;

    fn make_guard_with_resolver<R: PRResolver>(resolver: R) -> GhPrGuard<R> {
        let pr_config = PrBodyConfig {
            required_sections: vec![
                "## Summary".to_string(),
                "## Testing".to_string(),
                "## Test Results".to_string(),
                "### 1. Overall Test Pass Status".to_string(),
                "### 2. Overall Coverage".to_string(),
                "### 3. Modified File Coverage".to_string(),
            ],
            require_body_flag: true,
            forbid_emoji: true,
        };
        GhPrGuard::with_resolver(
            vec!["main".to_string(), "master".to_string()],
            resolver,
            pr_config,
            vec!["Generated with".to_string(), "claude code".to_string()],
        )
    }

    struct NullResolver;
    impl PRResolver for NullResolver {
        fn resolve_target_branch(&self, _pr: &str) -> Option<String> {
            None
        }
    }

    fn make_input(command: &str) -> crate::hooks::HookInput {
        use serde_json::json;
        crate::hooks::HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: Some(json!({ "command": command })),
            ..Default::default()
        }
    }

    fn canonical_pr_command() -> String {
        r#"gh pr create --title "feat: add feature" --body "
## Summary
- Add new feature

## Testing
- Ran full test suite

## Test Results
### 1. Overall Test Pass Status
- Suite: `codeflow test --mode full`
- Result: 500 passed, 0 failed

### 2. Overall Coverage
- Workspace: 92%
- CLI crate: 90%

### 3. Modified File Coverage
| File | Coverage | Threshold | Status |
|------|----------|-----------|--------|
| src/lib.rs | 94% | 85% | PASS |
""#
        .to_string()
    }

    /// Canonical body content (what a valid --body-file would hold).
    fn canonical_body_content() -> &'static str {
        "\n## Summary\n- Add new feature\n\
         \n## Testing\n- Ran full test suite\n\
         \n## Test Results\n### 1. Overall Test Pass Status\n\
         - Suite: `codeflow test --mode full`\n\
         - Result: 500 passed, 0 failed\n\
         \n### 2. Overall Coverage\n- Workspace: 92%\n- CLI crate: 90%\n\
         \n### 3. Modified File Coverage\n| File | Coverage | Threshold | Status |\n\
         |------|----------|-----------|--------|\n\
         | src/lib.rs | 94% | 85% | PASS |\n"
    }

    // AC #20a: valid 3-target body passes
    #[test]
    fn valid_canonical_body_passes() {
        let guard = make_guard_with_resolver(NullResolver);
        let input = make_input(&canonical_pr_command());
        let result = guard.handle(input).unwrap();
        assert_eq!(result.exit_code(), 0, "canonical body should pass");
    }

    // AC #20b: fresh-project body — validate_test_results_section passes
    #[test]
    fn fresh_project_test_results_validation_passes() {
        let body = "## Test Results\nNo test targets configured.\n";
        let errors = validate_test_results_section(body);
        assert!(
            errors.is_empty(),
            "fresh-project body should pass; got: {errors:?}"
        );
    }

    // AC #20c: missing "## Test Results" in guard → blocked
    #[test]
    fn missing_test_results_section_blocked() {
        let cmd = r#"gh pr create --title "feat: x" --body "
## Summary
- x

## Testing
- y
""#;
        let guard = make_guard_with_resolver(NullResolver);
        let result = guard.handle(make_input(cmd)).unwrap();
        assert!(
            matches!(result, HookOutput::Block { .. }),
            "missing ## Test Results should block"
        );
    }

    // AC #20d: incomplete sub-sections → validation errors
    #[test]
    fn empty_sub_sections_produce_errors() {
        let body = "## Test Results\n### 1. Overall Test Pass Status\n\
                    ### 2. Overall Coverage\n\
                    ### 3. Modified File Coverage\n";
        let errors = validate_test_results_section(body);
        assert!(
            !errors.is_empty(),
            "empty sub-sections should produce errors"
        );
    }

    // AC #20e: AI attribution → blocked (regression)
    #[test]
    fn ai_attribution_blocked() {
        let cmd = format!("{} Generated with Claude Code", canonical_pr_command());
        let guard = make_guard_with_resolver(NullResolver);
        let result = guard.handle(make_input(&cmd)).unwrap();
        assert!(
            matches!(result, HookOutput::Block { .. }),
            "AI attribution should block"
        );
    }

    // AC #20f: emoji → blocked (regression)
    #[test]
    fn emoji_blocked() {
        let cmd = format!("{} \u{1F680}", canonical_pr_command());
        let guard = make_guard_with_resolver(NullResolver);
        let result = guard.handle(make_input(&cmd)).unwrap();
        assert!(
            matches!(result, HookOutput::Block { .. }),
            "emoji should block"
        );
    }

    // AC #34: legacy "## Test Stats" → fails with migration message
    #[test]
    fn legacy_test_stats_fails_with_migration_hint() {
        let body = "gh pr create --body \"\n## Summary\n- x\n## Testing\n- y\n\
                    ## Test Stats\n### 1. Overall Test Pass Status\n- Result: pass\n\
                    ### 2. Overall Coverage\n- 90%\n\
                    #### Exempted Files\n| F | C | T | R |\n|-|-|-|-|\n\
                    ### 3. Modified File Coverage\n| F | C | T | S |\n|-|-|-|-|\n\"";
        let errors = validate_test_results_section(body);
        assert!(!errors.is_empty(), "legacy ## Test Stats should fail");
        assert!(
            errors[0].contains("codeflow test --mode full"),
            "error must reference regeneration command; got: {:?}",
            errors[0]
        );
    }

    // --- --body-file support ---

    /// `gh pr edit` matches the regex so `--body-file` on edit also validates.
    #[test]
    fn gh_pr_edit_triggers_body_validation() {
        assert!(gh_pr_create_re().is_match("gh pr edit 295 --body ''"));
        assert!(gh_pr_create_re().is_match("gh pr edit 295 --body-file body.md"));
    }

    #[test]
    fn extract_body_file_path_space_form() {
        let cmd = "gh pr create --title t --body-file /tmp/body.md";
        assert_eq!(
            extract_body_file_path(cmd),
            Some("/tmp/body.md".to_string())
        );
    }

    #[test]
    fn extract_body_file_path_equals_form() {
        let cmd = "gh pr create --title t --body-file=/tmp/body.md";
        assert_eq!(
            extract_body_file_path(cmd),
            Some("/tmp/body.md".to_string())
        );
    }

    #[test]
    fn extract_body_file_path_with_spaces() {
        let cmd = "gh pr create --title t --body-file \"/tmp/body file.md\"";
        assert_eq!(
            extract_body_file_path(cmd),
            Some("/tmp/body file.md".to_string())
        );
    }

    #[test]
    fn extract_body_file_path_absent() {
        let cmd = "gh pr create --title t --body 'inline body'";
        assert_eq!(extract_body_file_path(cmd), None);
    }

    /// Valid body in a file → guard allows.
    #[test]
    fn body_file_with_canonical_content_passes() {
        let dir = tempfile::tempdir().unwrap();
        let body_path = dir.path().join("body.md");
        std::fs::write(&body_path, canonical_body_content()).unwrap();
        let cmd = format!(
            "gh pr create --title \"feat: x\" --body-file {}",
            body_path.display()
        );

        let guard = make_guard_with_resolver(NullResolver);
        let result = guard.handle(make_input(&cmd)).unwrap();
        assert_eq!(
            result.exit_code(),
            0,
            "--body-file with canonical content must pass: {result:?}"
        );
    }

    /// `--body-file=<path>` form is parsed identically.
    #[test]
    fn body_file_equals_form_passes() {
        let dir = tempfile::tempdir().unwrap();
        let body_path = dir.path().join("body.md");
        std::fs::write(&body_path, canonical_body_content()).unwrap();
        let cmd = format!(
            "gh pr create --title \"feat: x\" --body-file={}",
            body_path.display()
        );

        let guard = make_guard_with_resolver(NullResolver);
        let result = guard.handle(make_input(&cmd)).unwrap();
        assert_eq!(result.exit_code(), 0, "--body-file= form must pass");
    }

    /// File missing a required section → guard blocks with the section-level error.
    #[test]
    fn body_file_missing_required_section_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let body_path = dir.path().join("body.md");
        let partial = "## Summary\n- x\n## Testing\n- y\n";
        std::fs::write(&body_path, partial).unwrap();
        let cmd = format!(
            "gh pr create --title \"feat: x\" --body-file {}",
            body_path.display()
        );

        let guard = make_guard_with_resolver(NullResolver);
        let result = guard.handle(make_input(&cmd)).unwrap();
        assert!(
            matches!(result, HookOutput::Block { .. }),
            "missing required section in body-file must block"
        );
        if let HookOutput::Block { reason, .. } = result {
            assert!(
                reason.contains("## Test Results"),
                "block reason must name missing section; got: {reason}"
            );
        }
    }

    /// AI attribution inside the file → blocked (regression parity with inline).
    #[test]
    fn body_file_with_ai_attribution_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let body_path = dir.path().join("body.md");
        let with_ai = format!(
            "{}\n\nGenerated with Claude Code\n",
            canonical_body_content()
        );
        std::fs::write(&body_path, with_ai).unwrap();
        let cmd = format!(
            "gh pr create --title \"feat: x\" --body-file {}",
            body_path.display()
        );

        let guard = make_guard_with_resolver(NullResolver);
        let result = guard.handle(make_input(&cmd)).unwrap();
        assert!(
            matches!(result, HookOutput::Block { .. }),
            "AI attribution in body-file must block"
        );
    }

    /// Emoji inside the file → blocked (regression parity with inline).
    #[test]
    fn body_file_with_emoji_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let body_path = dir.path().join("body.md");
        let with_emoji = format!("{}\n\u{1F680}\n", canonical_body_content());
        std::fs::write(&body_path, with_emoji).unwrap();
        let cmd = format!(
            "gh pr create --title \"feat: x\" --body-file {}",
            body_path.display()
        );

        let guard = make_guard_with_resolver(NullResolver);
        let result = guard.handle(make_input(&cmd)).unwrap();
        assert!(
            matches!(result, HookOutput::Block { .. }),
            "emoji in body-file must block"
        );
    }

    /// Nonexistent path → blocked with an unreadable-file message.
    #[test]
    fn body_file_nonexistent_path_blocks() {
        let cmd = "gh pr create --title \"feat: x\" --body-file /nonexistent/does-not-exist.md";
        let guard = make_guard_with_resolver(NullResolver);
        let result = guard.handle(make_input(cmd)).unwrap();
        if let HookOutput::Block { reason, .. } = result {
            assert!(
                reason.contains("--body-file unreadable"),
                "block must name unreadable-file error; got: {reason}"
            );
        } else {
            panic!("nonexistent body-file path must block");
        }
    }

    /// `--body-file=-` explicitly rejected.
    #[test]
    fn body_file_stdin_rejected() {
        let cmd = "gh pr create --title \"feat: x\" --body-file=-";
        let guard = make_guard_with_resolver(NullResolver);
        let result = guard.handle(make_input(cmd)).unwrap();
        if let HookOutput::Block { reason, .. } = result {
            assert!(
                reason.contains("stdin"),
                "block must reject --body-file=-; got: {reason}"
            );
        } else {
            panic!("--body-file=- must block");
        }
    }

    /// Non-UTF8 file content → blocked.
    #[test]
    fn body_file_non_utf8_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let body_path = dir.path().join("body.md");
        // Invalid UTF-8 byte sequence.
        std::fs::write(&body_path, [0xFF, 0xFE, 0xFD]).unwrap();
        let cmd = format!(
            "gh pr create --title \"feat: x\" --body-file {}",
            body_path.display()
        );

        let guard = make_guard_with_resolver(NullResolver);
        let result = guard.handle(make_input(&cmd)).unwrap();
        if let HookOutput::Block { reason, .. } = result {
            assert!(
                reason.contains("not valid UTF-8"),
                "block must name non-UTF8 error; got: {reason}"
            );
        } else {
            panic!("non-UTF8 body-file must block");
        }
    }

    /// Inline `--body` path remains unchanged (regression guard).
    #[test]
    fn inline_body_still_validates() {
        let cmd = r#"gh pr create --title "feat" --body "
## Summary
- x

## Testing
- y
""#;
        // This command lacks the Test Results section → must block via inline path.
        let guard = make_guard_with_resolver(NullResolver);
        let result = guard.handle(make_input(cmd)).unwrap();
        assert!(
            matches!(result, HookOutput::Block { .. }),
            "inline --body path must still validate required sections"
        );
    }
}
