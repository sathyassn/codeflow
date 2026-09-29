//! The v2 hook plane (charter §3.3; harness parity ADR-0008): the custom hooks.
//!
//! | Hook | Event | Module |
//! |---|---|---|
//! | `git-guard` | `PreToolUse` (Bash/PowerShell) | [`git_guard`] |
//! | `exec-guard` | `PreToolUse` (Bash/PowerShell) | [`exec_guard`] |
//! | `session-orient` | `SessionStart` | [`orient`], [`guidance`] after compaction or resume |
//! | `prompt-reminder` | `UserPromptSubmit` | [`guidance`] |
//! | `session-summary` | `SessionEnd` | [`session_summary`] |
//! | `delegate-turn` | `Stop` / `StopFailure` | [`delegate_turn`] |
//! | git-hook shims | pre-commit / commit-msg / pre-merge-commit / reference-transaction / pre-push | [`git_hook`] |
//!
//! The two `PreToolUse` guards share a lenient payload contract (`tool_name`,
//! `tool_input.command`, `cwd`) that is byte-compatible with the Codex hooks
//! engine, so the same `codeflow hook <guard>` binaries can bind an interactive
//! Codex session via `.codex/hooks.json` and a Grok Build session via
//! `.grok/hooks/codeflow.json` as well as Claude. (Headless `codex exec` /
//! `grok -p` do not invoke project `PreToolUse` hooks, so those invocations
//! are not work-session lanes — ADR-0008.)
//!
//! `session-orient` is likewise wired for Codex `SessionStart` (ADR-0013): the
//! same plain-text handler, so an interactive Codex session opens with the
//! orientation digest and re-orients after a compaction (`source=compact`).
//!
//! All enforcement levels are read from `.codeflow/policy.json` (charter D7:
//! policy in config, not code) via [`policy::Policy`]; nothing is hardcoded.
//! Every blocking message names the violated policy rule and the sanctioned
//! path (charter §6.2).

pub mod adoption;
pub mod delegate_turn;
pub mod exec_guard;
pub mod git_guard;
pub mod git_hook;
mod git_target;
pub mod guidance;
pub mod orient;
pub mod policy;
pub mod policy_schema;
pub mod repo;
pub mod scan;
pub mod session_summary;
pub mod standards;

pub use policy::{GitPolicy, GuidanceSection, Policy, PolicyLevel, SecuritySection};
pub use repo::RepoInfo;

use crate::remedy::Remedy;

/// Environment variable carrying the `codeflow integrate` gate-context token
/// (charter §6.2, D9). When set, the sanctioned local merge path is active
/// and protected-branch commit/push checks step aside. It is a discipline
/// aid, not a security boundary (charter §15) — CI + remote protection are
/// the hard line.
pub const INTEGRATE_TOKEN_ENV: &str = "CODEFLOW_INTEGRATE_TOKEN";

/// Environment variable a **human** exports to override the git-layer merge
/// guard — `CODEFLOW_HUMAN_OVERRIDE=1 git merge …` lets a person land a merge
/// on a protected branch from their own terminal (ADR-0007).
///
/// HUMAN-ONLY by contract: it is honored **only** by the git-client hook
/// plane (which cannot tell who invoked git), never by the Claude `git-guard`
/// layer — an agent does not get to claim humanity. The git-guard actively
/// blocks any in-session attempt to set this (or [`INTEGRATE_TOKEN_ENV`]):
/// setting an override token from inside a session is bypass, not override.
pub const HUMAN_OVERRIDE_ENV: &str = "CODEFLOW_HUMAN_OVERRIDE";

/// A single policy violation found by a hook plane.
#[derive(Debug, Clone)]
pub struct Violation {
    /// Stable rule identifier. Policy-backed rules are dotted paths into
    /// `policy.json` (for example `git.push_to_protected`); lifecycle
    /// invariants use their own namespace.
    pub rule: String,
    /// Enforcement level the policy assigns this rule (block or warn).
    pub level: PolicyLevel,
    /// Human-readable explanation of what was attempted.
    pub message: String,
    /// The sanctioned path: what to do instead. A violation that can print
    /// at warn names the step that clears it (R-80).
    pub remedy: Remedy,
    /// An always-blocking rule: it has no level to lower (R-80).
    pub level_fixed: bool,
}

impl Violation {
    /// Build a violation for `rule` at `level`, with a catalogued remedy.
    #[must_use]
    pub fn new(rule: &str, level: PolicyLevel, message: String, remedy: Remedy) -> Self {
        Self {
            rule: rule.to_string(),
            level,
            message,
            remedy,
            level_fixed: false,
        }
    }

    /// Build a violation of an always-blocking rule (R-80). It never prints
    /// as a warning, so its sanctioned path may be any text.
    #[must_use]
    pub fn always_blocking(rule: &str, message: String, sanctioned: &str) -> Self {
        Self {
            level_fixed: true,
            ..Self::new(
                rule,
                PolicyLevel::Block,
                message,
                Remedy::sanctioned(sanctioned),
            )
        }
    }

    /// The level this finding prints at when a plane whose own rule is
    /// `running` runs its check (R-80): the lower of the two where the
    /// finding's rule permits a downgrade. An always-blocking rule, a rule
    /// an adopter cannot adjust, and a `block` the project configured in
    /// `raw` (the policy file as JSON; unreadable counts as configured) keep
    /// their level.
    #[must_use]
    pub fn level_under(
        &self,
        running: PolicyLevel,
        raw: &Result<Option<serde_json::Value>, String>,
    ) -> PolicyLevel {
        if self.level_fixed {
            return self.level;
        }
        let Some(key) = adjustable_key(&self.rule) else {
            return self.level;
        };
        let configured = match raw {
            Ok(Some(value)) => adoption::git_key_present(value, key),
            Ok(None) => false,
            Err(_) => true,
        };
        if configured && self.level == PolicyLevel::Block {
            self.level
        } else {
            self.level.lower(running)
        }
    }

    /// Render the violation for stderr, prefixed with the emitting plane
    /// (e.g. `git-guard`, `pre-commit`).
    #[must_use]
    pub fn render(&self, plane: &str) -> String {
        let verdict = match self.level {
            PolicyLevel::Block => "BLOCKED",
            PolicyLevel::Warn => "warning",
            PolicyLevel::Allow | PolicyLevel::Off => "note",
        };
        format!(
            "codeflow {plane}: {verdict} — policy rule {rule} ({level})\n  {msg}\n  sanctioned: {remedy}\n  policy file: .codeflow/policy.json",
            rule = self.rule,
            level = self.level,
            msg = self.message,
            remedy = self.remedy,
        )
    }

    /// Render a non-configurable lifecycle invariant without falsely
    /// attributing it to `policy.json`.
    #[must_use]
    pub fn render_invariant(&self, plane: &str, authority: &str) -> String {
        let verdict = match self.level {
            PolicyLevel::Block => "BLOCKED",
            PolicyLevel::Warn => "warning",
            PolicyLevel::Allow | PolicyLevel::Off => "note",
        };
        format!(
            "codeflow {plane}: {verdict} — lifecycle invariant {rule} ({level})\n  {msg}\n  sanctioned: {remedy}\n  authority: {authority}",
            rule = self.rule,
            level = self.level,
            msg = self.message,
            remedy = self.remedy,
        )
    }
}

/// The always-blocking `git` rules (R-80): secret scan, protected branches
/// and hook integrity. They have no level to lower.
const ALWAYS_BLOCKING_GIT: &[&str] = &[
    "secret_scan",
    "commit_to_protected",
    "push_to_protected",
    "merge_to_protected",
    "force_push_protected",
    "delete_protected",
    "local_ref_protection",
    "hook_integrity",
];

/// The level keys of the `git` policy: every field of
/// [`policy::GitPolicy`] that holds a [`PolicyLevel`].
pub const LEVEL_KEYS: &[&str] = &[
    "commit_to_protected",
    "push_to_protected",
    "force_push_protected",
    "force_push_unprotected",
    "delete_protected",
    "hard_reset_protected",
    "merge_to_protected",
    "pr_merge_to_protected",
    "local_ref_protection",
    "hook_integrity",
    "root_checkout_commits",
    "commit_format",
    "commit_body",
    "commit_ticket_required",
    "ai_attribution",
    "commit_emoji",
    "policy_characters",
    "pr_sections",
    "pr_release_impact",
    "work_records",
    "work_planning",
    "branch_naming",
    "secret_scan",
    "test_gate_on_push",
    "security_review",
    "dep_audit",
];

/// The rules whose level is set by a key of another name.
const RULE_KEYS: &[(&str, &str)] = &[
    ("git.commit_ticket", "commit_ticket_required"),
    ("work.acceptance_binding", "work_records"),
    ("work.journey_criterion", "work_records"),
    ("work.valid_graph", "work_planning"),
    ("work.stable_planning_anchor", "work_planning"),
    ("work.task_record", "work_planning"),
];

/// The `git` policy key that sets `rule`'s level, when an adopter may adjust
/// it (the Adjustable column of R-80); `None` for a rule with a fixed level,
/// and for any rule without a level key, which then keeps its level.
#[must_use]
pub fn adjustable_key(rule: &str) -> Option<&'static str> {
    if let Some((_, key)) = RULE_KEYS.iter().find(|(named, _)| *named == rule) {
        return Some(key);
    }
    let key = rule.strip_prefix("git.")?;
    if ALWAYS_BLOCKING_GIT.contains(&key) {
        return None;
    }
    LEVEL_KEYS
        .iter()
        .copied()
        .find(|level_key| *level_key == key)
}

/// `true` when any violation in the slice is at block level.
#[must_use]
pub fn any_blocking(violations: &[Violation]) -> bool {
    violations.iter().any(|v| v.level == PolicyLevel::Block)
}

/// Current UTC time as an RFC 3339 string (second precision).
///
/// Hand-rolled from `SystemTime` so the hook plane needs no clock dependency;
/// uses the standard civil-from-days algorithm.
#[must_use]
pub fn rfc3339_utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    rfc3339_from_unix(secs)
}

/// Format a unix timestamp (seconds) as RFC 3339 UTC.
#[must_use]
#[allow(
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::many_single_char_names
)]
pub fn rfc3339_from_unix(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);

    // Civil-from-days (Howard Hinnant's algorithm), epoch 1970-01-01.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mth = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mth <= 2 { y + 1 } else { y };

    format!("{y:04}-{mth:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(rule: &str, level: PolicyLevel) -> Violation {
        Violation::new(
            rule,
            level,
            "m".into(),
            crate::remedy::COMMIT_BLANK_LINE.remedy(),
        )
    }

    #[test]
    fn a_running_plane_lowers_only_an_adjustable_unconfigured_block() {
        // SPC-013 R-80, TSK-147 AC-2.
        let none: Result<Option<serde_json::Value>, String> = Ok(None);
        let emoji_block = Ok(Some(serde_json::json!({"git": {"commit_emoji": "block"}})));
        let format = finding("git.commit_format", PolicyLevel::Block);
        assert_eq!(
            format.level_under(PolicyLevel::Warn, &none),
            PolicyLevel::Warn
        );
        assert_eq!(
            format.level_under(PolicyLevel::Block, &none),
            PolicyLevel::Block
        );
        // A warning never rises to the running plane's block.
        let warned = finding("git.commit_format", PolicyLevel::Warn);
        assert_eq!(
            warned.level_under(PolicyLevel::Block, &none),
            PolicyLevel::Warn
        );
        // A block the project configured keeps its level.
        let emoji = finding("git.commit_emoji", PolicyLevel::Block);
        assert_eq!(
            emoji.level_under(PolicyLevel::Warn, &emoji_block),
            PolicyLevel::Block
        );
        assert_eq!(
            emoji.level_under(PolicyLevel::Warn, &none),
            PolicyLevel::Warn
        );
        // Unreadable provenance counts as configured.
        let unreadable = Err("bad json".to_string());
        assert_eq!(
            emoji.level_under(PolicyLevel::Warn, &unreadable),
            PolicyLevel::Block
        );
        // Always-blocking and non-adjustable rules keep their level.
        for rule in [
            "git.secret_scan",
            "git.push_to_protected",
            "work.id_registry",
        ] {
            let kept = finding(rule, PolicyLevel::Block);
            assert_eq!(
                kept.level_under(PolicyLevel::Warn, &none),
                PolicyLevel::Block,
                "{rule}"
            );
        }
        let fixed = Violation::always_blocking("work.records", "m".into(), "s");
        assert_eq!(
            fixed.level_under(PolicyLevel::Warn, &none),
            PolicyLevel::Block
        );
        // The work levels are adjustable through their own keys.
        assert_eq!(
            adjustable_key("work.journey_criterion"),
            Some("work_records")
        );
        assert_eq!(adjustable_key("work.valid_graph"), Some("work_planning"));
        assert_eq!(adjustable_key("work.classification"), None);
        // A rule named apart from its key reads that key (TSK-147 F1), and
        // a rule with no level key is not adjustable.
        assert_eq!(
            adjustable_key("git.commit_ticket"),
            Some("commit_ticket_required")
        );
        assert_eq!(adjustable_key("git.breaking_watch_paths"), None);
        let raw = Ok(Some(
            serde_json::json!({"git": {"commit_ticket_required": "block"}}),
        ));
        assert_eq!(
            finding("git.commit_ticket", PolicyLevel::Block).level_under(PolicyLevel::Warn, &raw),
            PolicyLevel::Block
        );
    }

    #[test]
    fn the_level_keys_are_every_level_field_of_the_git_policy() {
        let defaults = serde_json::to_value(policy::GitPolicy::default()).unwrap();
        let fields: std::collections::BTreeSet<&str> = defaults
            .as_object()
            .unwrap()
            .iter()
            .filter(|(key, value)| {
                // A level field holds a level and refuses a string that is
                // not one; a plain string field (such as `direct_changes`)
                // takes it.
                if serde_json::from_value::<PolicyLevel>((*value).clone()).is_err() {
                    return false;
                }
                let mut probe = defaults.clone();
                probe[key.as_str()] = "not-a-level".into();
                serde_json::from_value::<policy::GitPolicy>(probe).is_err()
            })
            .map(|(key, _)| key.as_str())
            .collect();
        let listed: std::collections::BTreeSet<&str> = LEVEL_KEYS.iter().copied().collect();
        assert_eq!(listed, fields);
        for (_, key) in RULE_KEYS {
            assert!(LEVEL_KEYS.contains(key), "{key}");
        }
    }

    #[test]
    fn test_rfc3339_epoch() {
        assert_eq!(rfc3339_from_unix(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn test_rfc3339_known_timestamp() {
        // 2026-06-12T00:00:00Z == 1781222400
        assert_eq!(rfc3339_from_unix(1_781_222_400), "2026-06-12T00:00:00Z");
    }

    #[test]
    fn test_rfc3339_leap_year_day() {
        // 2024-02-29T12:30:45Z == 1709209845
        assert_eq!(rfc3339_from_unix(1_709_209_845), "2024-02-29T12:30:45Z");
    }

    #[test]
    fn test_rfc3339_now_shape() {
        let now = rfc3339_utc_now();
        assert_eq!(now.len(), 20);
        assert!(now.ends_with('Z'));
        assert_eq!(&now[4..5], "-");
    }

    #[test]
    fn test_violation_render_names_rule_and_remedy() {
        let v = Violation::new(
            "git.push_to_protected",
            PolicyLevel::Block,
            "direct push to protected branch 'main'".into(),
            crate::remedy::PROTECTED_BRANCH.remedy(),
        );
        let out = v.render("git-guard");
        assert!(out.contains("BLOCKED"));
        assert!(out.contains("git.push_to_protected"));
        assert!(out.contains("codeflow integrate"));
        assert!(out.contains(".codeflow/policy.json"));
    }

    #[test]
    fn test_violation_render_warn() {
        let v = Violation::new(
            "git.commit_format",
            PolicyLevel::Warn,
            "bad subject".into(),
            crate::remedy::COMMIT_BLANK_LINE.remedy(),
        );
        assert!(v.render("commit-msg").contains("warning"));
    }

    #[test]
    fn test_any_blocking() {
        let warn = Violation::new(
            "r",
            PolicyLevel::Warn,
            String::new(),
            crate::remedy::COMMIT_BLANK_LINE.remedy(),
        );
        let block = Violation::always_blocking("r", String::new(), "");
        assert!(!any_blocking(std::slice::from_ref(&warn)));
        assert!(any_blocking(&[warn, block]));
        assert!(!any_blocking(&[]));
    }
}
