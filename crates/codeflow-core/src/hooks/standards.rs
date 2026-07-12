//! Branch and commit standards (charter §6.4): conventional commit format,
//! the no-AI-attribution rule, and the no-emoji rule.
//!
//! Shared by the commit-msg git hook and the git-guard PR-body scan so both
//! planes flag identical content (AC #13). What counts as a violation lives
//! here; whether it blocks, warns, or passes lives in `policy.json`.

use std::sync::OnceLock;

use regex::Regex;

/// Conventional commit subject: `type(scope)!: description`.
fn conventional_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(?P<type>[a-z][a-z0-9]*)(\((?P<scope>[^)]+)\))?(?P<bang>!)?: (?P<desc>\S.*)$")
            .expect("valid regex")
    })
}

/// AI attribution trailers and lines (charter §6.4: `Co-Authored-By` AI
/// trailers, "Generated with …" lines, robot emoji).
fn attribution_res() -> &'static [(Regex, &'static str)] {
    static RES: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    RES.get_or_init(|| {
        vec![
            (
                Regex::new(
                    r"(?i)co-authored-by:.*(claude|anthropic|openai|chatgpt|gpt|gemini|copilot|codex|cursor|devin|aider|fable|\[bot\]|noreply@anthropic)",
                )
                .expect("valid regex"),
                "Co-Authored-By AI trailer",
            ),
            (
                Regex::new(r"(?i)generated (with|by)").expect("valid regex"),
                "\"Generated with/by\" line",
            ),
            (
                Regex::new("\u{1F916}").expect("valid regex"),
                "robot emoji",
            ),
        ]
    })
}

/// Auto-generated subjects exempt from the conventional-format check.
const FORMAT_EXEMPT_PREFIXES: &[&str] = &["Merge ", "Revert ", "Reapply ", "fixup!", "squash!"];

/// Check a commit subject against the conventional format
/// `type(scope): description` with `type` from the policy whitelist,
/// imperative description, no trailing period.
///
/// Returns `None` when the subject conforms (or is an exempt auto-generated
/// subject), otherwise the reason it fails.
#[must_use]
pub fn check_commit_format(subject: &str, allowed_types: &[String]) -> Option<String> {
    if FORMAT_EXEMPT_PREFIXES
        .iter()
        .any(|p| subject.starts_with(p))
    {
        return None;
    }
    let Some(caps) = conventional_re().captures(subject) else {
        return Some(format!(
            "subject does not match `type(scope): description` — got {subject:?}"
        ));
    };
    let ctype = &caps["type"];
    if !allowed_types.iter().any(|t| t == ctype) {
        return Some(format!(
            "commit type {ctype:?} is not in the policy whitelist ({})",
            allowed_types.join(", ")
        ));
    }
    if caps["desc"].trim_end().ends_with('.') {
        return Some("subject must not end with a trailing period".to_string());
    }
    None
}

/// A Conventional-Commits breaking-change footer must be exactly
/// `BREAKING CHANGE:` or `BREAKING-CHANGE:` (uppercase) to be recognized by
/// versioning tooling (release-plz, git-cliff, ...). A mis-cased footer
/// (`breaking change:`) is silently treated as non-breaking — so a MAJOR change
/// would ship as a MINOR bump. Flag it, so the only ways to signal a breaking
/// change are both unambiguous: the subject `!` marker, or the exact footer.
///
/// Skipped when the subject already carries `!` (breaking is already signaled;
/// a "breaking change" line in the body is then just prose). Returns `None` when
/// clean, otherwise the reason.
#[must_use]
pub fn check_breaking_footer(subject: &str, message: &str) -> Option<String> {
    if conventional_re()
        .captures(subject)
        .is_some_and(|c| c.name("bang").is_some())
    {
        return None;
    }
    for raw in message.lines() {
        let line = raw.trim_start();
        let lower = line.to_ascii_lowercase();
        let token_len = if lower.starts_with("breaking change") {
            "breaking change".len()
        } else if lower.starts_with("breaking-change") {
            "breaking-change".len()
        } else {
            continue;
        };
        // A footer is `token:` — require the colon (after optional spaces).
        if !line[token_len..].trim_start().starts_with(':') {
            continue;
        }
        let token = &line[..token_len];
        if token != "BREAKING CHANGE" && token != "BREAKING-CHANGE" {
            return Some(format!(
                "breaking-change footer {token:?} must be exactly `BREAKING CHANGE:` or \
                 `BREAKING-CHANGE:` (uppercase) to register as a breaking change — \
                 otherwise it is silently treated as non-breaking"
            ));
        }
    }
    None
}

/// Scan text (commit message or PR body) for AI attribution.
///
/// Returns the human-readable name of the matched pattern, or `None` when
/// the text is clean.
#[must_use]
pub fn find_attribution(text: &str) -> Option<&'static str> {
    attribution_res()
        .iter()
        .find(|(re, _)| re.is_match(text))
        .map(|(_, name)| *name)
}

/// Find the first emoji character in `text` (commit subject or PR body).
///
/// Covers the emoji presentation blocks plus misc-symbols/dingbats — the v1
/// standard is strict, and `block` is the shipped default.
#[must_use]
pub fn find_emoji(text: &str) -> Option<char> {
    text.chars().find(|&c| is_emoji_char(c))
}

fn is_emoji_char(c: char) -> bool {
    matches!(u32::from(c),
        0x1F000..=0x1FAFF   // emoji, symbols, pictographs, flags
        | 0x2600..=0x27BF   // misc symbols + dingbats
        | 0x2B00..=0x2BFF   // misc symbols and arrows (stars, heavy arrows)
        | 0xFE0F            // emoji variation selector
        | 0x1FB00..=0x1FBFF // symbols for legacy computing
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn types() -> Vec<String> {
        super::super::policy::GitPolicy::default().commit_types
    }

    // -- commit format --

    #[test]
    fn test_format_valid_subjects() {
        for s in [
            "feat(hooks): add git-guard evaluation",
            "fix: handle unborn head",
            "refactor(core)!: split policy loading",
            "chore: bump deps",
        ] {
            assert_eq!(check_commit_format(s, &types()), None, "{s} should pass");
        }
    }

    #[test]
    fn test_format_malformed_subject() {
        assert!(check_commit_format("Added some stuff", &types()).is_some());
        assert!(check_commit_format("feat add thing", &types()).is_some());
        assert!(check_commit_format("feat:missing space", &types()).is_some());
        assert!(check_commit_format("FEAT: shouting type", &types()).is_some());
    }

    #[test]
    fn test_format_unknown_type() {
        let reason = check_commit_format("feet: walk faster", &types()).unwrap();
        assert!(reason.contains("feet"));
        assert!(reason.contains("whitelist"));
    }

    #[test]
    fn test_format_trailing_period() {
        let reason = check_commit_format("feat: add thing.", &types()).unwrap();
        assert!(reason.contains("trailing period"));
    }

    #[test]
    fn test_format_exempt_auto_generated() {
        for s in [
            "Merge branch 'main' into feat/x",
            "Revert \"feat: add thing\"",
            "fixup! feat: add thing",
            "squash! feat: add thing",
        ] {
            assert_eq!(check_commit_format(s, &types()), None, "{s} exempt");
        }
    }

    #[test]
    fn test_format_respects_policy_whitelist() {
        // Levels and lists come from policy, nothing hardcoded (D7).
        let narrow = vec!["feat".to_string()];
        assert!(check_commit_format("fix: now rejected", &narrow).is_some());
        assert_eq!(check_commit_format("feat: still fine", &narrow), None);
    }

    // -- breaking-change footer --

    #[test]
    fn test_breaking_footer_exact_forms_pass() {
        for msg in [
            "feat: add x\n\nBREAKING CHANGE: removes y",
            "feat: add x\n\nBREAKING-CHANGE: removes y",
        ] {
            assert_eq!(check_breaking_footer("feat: add x", msg), None, "{msg}");
        }
    }

    #[test]
    fn test_breaking_footer_miscased_flagged() {
        for msg in [
            "feat: add x\n\nbreaking change: removes y",
            "feat: add x\n\nBreaking Change: removes y",
            "feat: add x\n\nbreaking-change: removes y",
        ] {
            assert!(check_breaking_footer("feat: add x", msg).is_some(), "{msg}");
        }
    }

    #[test]
    fn test_breaking_footer_bang_subject_exempt() {
        // subject already signals breaking with `!`; a body mention is just prose
        let msg = "feat!: add x\n\nbreaking change: mentioned in prose";
        assert_eq!(check_breaking_footer("feat!: add x", msg), None);
    }

    #[test]
    fn test_breaking_footer_prose_not_flagged() {
        // "breaking change" not at footer position (line start + colon) is prose
        let msg = "fix: thing\n\nThis is not a breaking change: really";
        assert_eq!(check_breaking_footer("fix: thing", msg), None);
    }

    // -- attribution --

    #[test]
    fn test_attribution_co_authored_by_ai() {
        let msg = "feat: x\n\nCo-Authored-By: Claude Fable 5 <noreply@anthropic.com>";
        assert_eq!(find_attribution(msg), Some("Co-Authored-By AI trailer"));
    }

    #[test]
    fn test_attribution_generated_with() {
        let body = "Summary\n\nGenerated with [Claude Code](https://claude.com)";
        assert_eq!(find_attribution(body), Some("\"Generated with/by\" line"));
    }

    #[test]
    fn test_attribution_robot_emoji() {
        assert_eq!(find_attribution("nice \u{1F916} work"), Some("robot emoji"));
    }

    #[test]
    fn test_attribution_human_coauthor_allowed() {
        let msg = "feat: x\n\nCo-Authored-By: Ada Lovelace <ada@example.com>";
        assert_eq!(find_attribution(msg), None);
    }

    #[test]
    fn test_attribution_clean_text() {
        assert_eq!(find_attribution("fix(core): handle empty refs"), None);
    }

    // -- emoji --

    #[test]
    fn test_emoji_detected_in_subject() {
        assert!(find_emoji("feat: ship it \u{1F680}").is_some());
        assert!(find_emoji("fix: sparkle \u{2728}").is_some());
    }

    #[test]
    fn test_plain_text_and_arrows_allowed() {
        assert_eq!(find_emoji("feat: rename a -> b"), None);
        assert_eq!(find_emoji("docs: explain → flow"), None); // U+2192 arrow
        assert_eq!(find_emoji("fix: ümlaut handling"), None);
    }
}
