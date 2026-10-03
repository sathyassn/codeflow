//! Adopter fit for the PR checks (SPC-013 R-82, R-84, R-115): the effective
//! level and origin of each rule `codeflow ci` runs, the PR-section level
//! while a kept brownfield template is diagnosed, the headings an accepted
//! mapping checks, and trusted automation profiles.
//!
//! The effective PR-section level is `warn` only while the mapping is
//! `diagnosed` and the policy file carries no `pr_sections` key: `init`
//! writes a fresh policy file without that key exactly when it diagnoses a
//! kept template, and `update` adds the key at its shipped default before it
//! diagnoses an older file that lacks it. A key present in the file is
//! explicit whatever its value, including one equal to the default.

use std::fmt;
use std::path::Path;

use serde_json::Value;

use super::policy::{AutomationProfile, GitPolicy, MappingState};
use super::PolicyLevel;

/// Where an effective level comes from (R-115).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelOrigin {
    /// The key is present in `.codeflow/policy.json`.
    Configured,
    /// The key is absent; the built-in default applies.
    ShippedDefault,
    /// A kept PR template awaits a decision and no level was configured.
    Diagnosed,
}

impl fmt::Display for LevelOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Configured => "configured",
            Self::ShippedDefault => "shipped default",
            Self::Diagnosed => "diagnosed",
        })
    }
}

/// A rule's effective level and its origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectiveLevel {
    pub level: PolicyLevel,
    pub origin: LevelOrigin,
}

/// The policy file as raw JSON, for key-presence questions the typed loader
/// cannot answer. `Ok(None)` when the file is absent; `Err` when it exists
/// but cannot be read or parsed (provenance unreadable).
///
/// # Errors
/// The read or parse failure, as text.
pub fn raw_policy(root: &Path) -> Result<Option<Value>, String> {
    let path = root.join(".codeflow").join("policy.json");
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// `true` when the raw policy carries `git.<key>`.
#[must_use]
pub fn git_key_present(raw: &Value, key: &str) -> bool {
    raw.get("git").and_then(|g| g.get(key)).is_some()
}

/// The effective level and origin of `git.<key>` with value `level`.
/// Unreadable provenance (`raw` is `Err`) keeps the configured level.
#[must_use]
pub fn effective(
    raw: &Result<Option<Value>, String>,
    key: &str,
    level: PolicyLevel,
) -> EffectiveLevel {
    let origin = match raw {
        Ok(Some(value)) if git_key_present(value, key) => LevelOrigin::Configured,
        Ok(_) => LevelOrigin::ShippedDefault,
        Err(_) => LevelOrigin::Configured,
    };
    EffectiveLevel { level, origin }
}

/// The effective `git.pr_sections` level (R-115): `warn` with origin
/// `diagnosed` while the mapping is diagnosed and the file sets no level;
/// otherwise the configured or default level.
#[must_use]
pub fn pr_sections_effective(
    raw: &Result<Option<Value>, String>,
    git: &GitPolicy,
) -> EffectiveLevel {
    let diagnosed = git
        .pr_section_mapping
        .as_ref()
        .is_some_and(|m| m.state == MappingState::Diagnosed);
    match raw {
        Ok(Some(value)) if diagnosed && !git_key_present(value, "pr_sections") => EffectiveLevel {
            level: PolicyLevel::Warn,
            origin: LevelOrigin::Diagnosed,
        },
        _ => effective(raw, "pr_sections", git.pr_sections),
    }
}

/// The effective `git.pr_summary` level: at most `warn`, with origin
/// `diagnosed`, while `pr_section_mapping` is diagnosed, since the Summary
/// it judges sits in a template the adopter has not yet decided on. The cap
/// follows the mapping's state whatever `pr_sections` says, and a level
/// already below warn stays as set; otherwise the configured or default
/// level.
#[must_use]
pub fn pr_summary_effective(
    raw: &Result<Option<Value>, String>,
    git: &GitPolicy,
) -> EffectiveLevel {
    let diagnosed = git
        .pr_section_mapping
        .as_ref()
        .is_some_and(|m| m.state == MappingState::Diagnosed);
    if diagnosed && git.pr_summary == PolicyLevel::Block {
        return EffectiveLevel {
            level: PolicyLevel::Warn,
            origin: LevelOrigin::Diagnosed,
        };
    }
    effective(raw, "pr_summary", git.pr_summary)
}

/// The pending decision `doctor` names, if any.
#[must_use]
pub fn pending_decision(git: &GitPolicy) -> Option<String> {
    let mapping = git.pr_section_mapping.as_ref()?;
    if mapping.state != MappingState::Diagnosed {
        return None;
    }
    let proposed = if mapping.headings.is_empty() {
        "no heading counterpart found".to_string()
    } else {
        mapping
            .headings
            .iter()
            .map(|(required, actual)| format!("{required} -> {actual}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    Some(format!(
        "kept PR template awaits a decision (accepted, refused or custom); proposed mapping: {proposed}"
    ))
}

/// Apply an accepted mapping to a heading list: each required heading the
/// mapping names is checked under the template's heading instead. Any other
/// state leaves the list unchanged.
#[must_use]
pub fn mapped_sections(git: &GitPolicy, sections: &[String]) -> Vec<String> {
    let Some(mapping) = git
        .pr_section_mapping
        .as_ref()
        .filter(|m| m.state == MappingState::Accepted)
    else {
        return sections.to_vec();
    };
    sections
        .iter()
        .map(|s| {
            mapping
                .headings
                .iter()
                .find(|(required, _)| required.eq_ignore_ascii_case(s))
                .map_or_else(|| s.clone(), |(_, actual)| actual.clone())
        })
        .collect()
}

/// The profile that applies to `actor` on `branch`, from the target-side
/// profile list. The actor `unknown` never matches.
#[must_use]
pub fn matching_profile<'p>(
    profiles: &'p [AutomationProfile],
    actor: &str,
    branch: &str,
) -> Option<&'p AutomationProfile> {
    profiles.iter().find(|p| p.matches(actor, branch))
}

/// The profile whose branch pattern matches, whatever the actor: reported
/// for an unknown or untrusted actor, never applied.
#[must_use]
pub fn pattern_only_profile<'p>(
    profiles: &'p [AutomationProfile],
    branch: &str,
) -> Option<&'p AutomationProfile> {
    profiles.iter().find(|p| p.branch_matches(branch))
}

/// A copy of `git` with the rules a matched profile skips turned off:
/// branch naming, the commit message shape rules (format, body, ticket and
/// required footers) and the PR Summary shape, since a bot writes its body
/// to its own format. Every other rule and every level is unchanged.
#[must_use]
pub fn apply_profile_exemptions(git: &GitPolicy) -> GitPolicy {
    let mut exempt = git.clone();
    exempt.branch_naming = PolicyLevel::Off;
    exempt.commit_format = PolicyLevel::Off;
    exempt.commit_body = PolicyLevel::Off;
    exempt.commit_ticket_required = PolicyLevel::Off;
    exempt.commit_ticket_keys = Vec::new();
    exempt.commit_required_footers = Vec::new();
    exempt.pr_summary = PolicyLevel::Off;
    exempt
}

/// `release.backend` in `.codeflow/project.toml` (SPC-013 R-97): who owns
/// the version of each release unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseBackend {
    /// The default at every tier: `CodeFlow` checks only the PR's Release
    /// impact declaration and calculates no version.
    None,
    /// Another tool owns the version; `CodeFlow` checks the declaration and the
    /// contract watches only.
    External,
    /// The project adopted `CodeFlow`'s release calculator explicitly.
    Codeflow,
}

impl fmt::Display for ReleaseBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::None => "none",
            Self::External => "external",
            Self::Codeflow => "codeflow",
        })
    }
}

/// Read `release.backend` from `.codeflow/project.toml`. An absent file,
/// table or key is `none`.
///
/// # Errors
/// The file cannot be parsed, or the value is not `none`, `external` or
/// `codeflow`.
pub fn release_backend(root: &Path) -> Result<ReleaseBackend, String> {
    let path = root.join(".codeflow").join("project.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(ReleaseBackend::None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    release_backend_str(&text, &path.display().to_string())
}

/// [`release_backend`] of project-state text, `name` naming its source in
/// errors (a path, or `<commit>:.codeflow/project.toml`).
///
/// # Errors
/// As [`release_backend`].
pub fn release_backend_str(text: &str, name: &str) -> Result<ReleaseBackend, String> {
    // The parse error is not echoed: it quotes file content.
    let table: toml::Table = toml::from_str(text)
        .map_err(|_| format!("{name} does not parse; release.backend unreadable"))?;
    let Some(release) = table.get("release") else {
        return Ok(ReleaseBackend::None);
    };
    let Some(release) = release.as_table() else {
        return Err("release: expected a table".to_string());
    };
    match release.get("backend").map(toml::Value::as_str) {
        None | Some(Some("none")) => Ok(ReleaseBackend::None),
        Some(Some("external")) => Ok(ReleaseBackend::External),
        Some(Some("codeflow")) => Ok(ReleaseBackend::Codeflow),
        Some(other) => Err(format!(
            "invalid release.backend {}; expected one of: none, external, codeflow",
            other.map_or_else(|| "(not a string)".to_string(), |v| format!("'{v}'"))
        )),
    }
}

/// Release tools that own a version, found by their configuration files.
const RELEASE_TOOLS: &[(&str, &str)] = &[
    ("release-please", "release-please-config.json"),
    ("release-please", ".release-please-manifest.json"),
    ("changesets", ".changeset/config.json"),
    ("semantic-release", ".releaserc"),
    ("semantic-release", ".releaserc.json"),
    ("semantic-release", ".releaserc.yml"),
    ("semantic-release", ".releaserc.yaml"),
    ("semantic-release", "release.config.js"),
    ("semantic-release", "release.config.cjs"),
    ("semantic-release", "release.config.mjs"),
    ("cargo-release", "release.toml"),
    ("goreleaser", ".goreleaser.yml"),
    ("goreleaser", ".goreleaser.yaml"),
];

/// Every release tool configured in `root`, as `(tool, file)`.
#[must_use]
pub fn detect_release_tools(root: &Path) -> Vec<(&'static str, &'static str)> {
    RELEASE_TOOLS
        .iter()
        .copied()
        .filter(|(_, file)| root.join(file).is_file())
        .collect()
}

/// A one-line finding about version authority, or `None` when the backend
/// and the detected tools agree: `none` beside a release tool should say
/// `external`; `codeflow` beside one has two version authorities.
#[must_use]
pub fn release_backend_finding(backend: ReleaseBackend, tools: &[(&str, &str)]) -> Option<String> {
    let (tool, file) = tools.first()?;
    match backend {
        ReleaseBackend::None => Some(format!(
            "{tool} owns versions here ({file}); set `[release] backend = \"external\"` in .codeflow/project.toml so CodeFlow checks only the Release impact declaration and contract watches"
        )),
        ReleaseBackend::Codeflow => Some(format!(
            "two version authorities: release.backend is codeflow and {tool} is configured ({file}); keep one per release unit"
        )),
        ReleaseBackend::External => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::policy::{Policy, PrSectionMapping};

    fn raw(json: &str) -> Result<Option<Value>, String> {
        serde_json::from_str(json)
            .map(Some)
            .map_err(|e| e.to_string())
    }

    fn git(json: &str) -> GitPolicy {
        serde_json::from_str::<Policy>(json).unwrap().git
    }

    const DIAGNOSED: &str = r#""pr_section_mapping":{"state":"diagnosed","headings":{"Summary":"What"},"decided":"none"}"#;

    #[test]
    fn diagnosed_without_a_configured_level_runs_at_warn() {
        let json = format!(r#"{{"git":{{{DIAGNOSED}}}}}"#);
        let level = pr_sections_effective(&raw(&json), &git(&json));
        assert_eq!(level.level, PolicyLevel::Warn);
        assert_eq!(level.origin, LevelOrigin::Diagnosed);
        assert!(pending_decision(&git(&json))
            .unwrap()
            .contains("Summary -> What"));
    }

    #[test]
    fn a_present_key_is_explicit_even_when_equal_to_the_default() {
        let json = format!(r#"{{"git":{{"pr_sections":"block",{DIAGNOSED}}}}}"#);
        let level = pr_sections_effective(&raw(&json), &git(&json));
        assert_eq!(level.level, PolicyLevel::Block);
        assert_eq!(level.origin, LevelOrigin::Configured);
    }

    #[test]
    fn unreadable_provenance_keeps_the_configured_level() {
        let g = GitPolicy {
            pr_section_mapping: Some(PrSectionMapping {
                state: MappingState::Diagnosed,
                headings: std::collections::BTreeMap::new(),
                decided: "none".into(),
            }),
            ..GitPolicy::default()
        };
        let level = pr_sections_effective(&Err("unreadable".into()), &g);
        assert_eq!(level.level, PolicyLevel::Block);
        assert_eq!(level.origin, LevelOrigin::Configured);
    }

    #[test]
    fn decided_states_leave_the_diagnosed_window() {
        for state in ["accepted", "refused", "custom"] {
            let json = format!(
                r#"{{"git":{{"pr_section_mapping":{{"state":"{state}","decided":"2026-09-26"}}}}}}"#
            );
            let level = pr_sections_effective(&raw(&json), &git(&json));
            assert_eq!(level.level, PolicyLevel::Block, "{state}");
            assert_eq!(level.origin, LevelOrigin::ShippedDefault, "{state}");
            assert!(pending_decision(&git(&json)).is_none());
        }
    }

    #[test]
    fn accepted_mapping_renames_only_mapped_headings() {
        let json = r#"{"git":{"pr_section_mapping":{"state":"accepted","headings":{"Summary":"What","Testing":"How tested"},"decided":"2026-09-26"}}}"#;
        let g = git(json);
        let mapped = mapped_sections(&g, &["summary".into(), "Changes".into(), "Testing".into()]);
        assert_eq!(mapped, ["What", "Changes", "How tested"]);
        let diagnosed = git(&format!(r#"{{"git":{{{DIAGNOSED}}}}}"#));
        assert_eq!(
            mapped_sections(&diagnosed, &["Summary".into()]),
            ["Summary"]
        );
    }

    #[test]
    fn profile_needs_a_trusted_actor_and_a_matching_branch() {
        let g = git(
            r#"{"git":{"automation_profiles":[{"name":"dependabot","actors":["dependabot[bot]"],"branch_pattern":"dependabot/**"}]}}"#,
        );
        let p = &g.automation_profiles;
        assert!(matching_profile(p, "dependabot[bot]", "dependabot/cargo/serde-1.0.1").is_some());
        assert!(matching_profile(p, "mallory", "dependabot/cargo/serde-1.0.1").is_none());
        assert!(matching_profile(p, "unknown", "dependabot/cargo/serde-1.0.1").is_none());
        assert!(matching_profile(p, "", "dependabot/cargo/serde-1.0.1").is_none());
        assert!(matching_profile(p, "dependabot[bot]", "feat/x").is_none());
        assert!(pattern_only_profile(p, "dependabot/cargo/x").is_some());
    }

    #[test]
    fn release_backend_defaults_to_none_and_accepts_the_three_values() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(release_backend(dir.path()), Ok(ReleaseBackend::None));
        std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
        let toml = dir.path().join(".codeflow/project.toml");
        std::fs::write(&toml, "tier = \"minimal\"\n").unwrap();
        assert_eq!(release_backend(dir.path()), Ok(ReleaseBackend::None));
        for (value, want) in [
            ("none", ReleaseBackend::None),
            ("external", ReleaseBackend::External),
            ("codeflow", ReleaseBackend::Codeflow),
        ] {
            std::fs::write(&toml, format!("[release]\nbackend = \"{value}\"\n")).unwrap();
            assert_eq!(release_backend(dir.path()), Ok(want));
        }
        std::fs::write(&toml, "[release]\nbackend = \"calver\"\n").unwrap();
        assert!(release_backend(dir.path()).unwrap_err().contains("calver"));
    }

    #[test]
    fn release_tools_are_recognised_and_one_authority_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        assert!(detect_release_tools(dir.path()).is_empty());
        std::fs::create_dir_all(dir.path().join(".changeset")).unwrap();
        std::fs::write(dir.path().join(".changeset/config.json"), "{}").unwrap();
        let tools = detect_release_tools(dir.path());
        assert_eq!(tools, [("changesets", ".changeset/config.json")]);
        assert!(release_backend_finding(ReleaseBackend::None, &tools)
            .unwrap()
            .contains("external"));
        assert!(release_backend_finding(ReleaseBackend::Codeflow, &tools)
            .unwrap()
            .contains("two version authorities"));
        assert!(release_backend_finding(ReleaseBackend::External, &tools).is_none());
        assert!(release_backend_finding(ReleaseBackend::Codeflow, &[]).is_none());
    }

    /// The cap follows the mapping's state, not the section check's
    /// origin: an explicit `pr_sections` key keeps that check at its level,
    /// but the Summary in a template still awaiting a decision only warns,
    /// and a level already below warn stays where the project set it.
    #[test]
    fn summary_cap_follows_the_mapping_state_whatever_pr_sections_says() {
        let mapping = serde_json::json!({"state": "diagnosed", "headings": {}, "decided": "none"});
        for (policy, expected) in [
            (
                serde_json::json!({"git": {"pr_sections": "block", "pr_section_mapping": mapping}}),
                (PolicyLevel::Warn, LevelOrigin::Diagnosed),
            ),
            (
                serde_json::json!({"git": {"pr_section_mapping": mapping}}),
                (PolicyLevel::Warn, LevelOrigin::Diagnosed),
            ),
            (
                serde_json::json!({"git": {"pr_sections": "block", "pr_summary": "block", "pr_section_mapping": mapping}}),
                (PolicyLevel::Warn, LevelOrigin::Diagnosed),
            ),
            (
                serde_json::json!({"git": {"pr_sections": "block", "pr_summary": "off", "pr_section_mapping": mapping}}),
                (PolicyLevel::Off, LevelOrigin::Configured),
            ),
            (
                serde_json::json!({"git": {"pr_sections": "block", "pr_summary": "allow", "pr_section_mapping": mapping}}),
                (PolicyLevel::Allow, LevelOrigin::Configured),
            ),
            (
                serde_json::json!({"git": {"pr_sections": "warn", "pr_summary": "warn", "pr_section_mapping": mapping}}),
                (PolicyLevel::Warn, LevelOrigin::Configured),
            ),
            (
                serde_json::json!({"git": {"pr_sections": "block", "pr_section_mapping": {"state": "accepted", "headings": {}, "decided": "2026-10-03"}}}),
                (PolicyLevel::Block, LevelOrigin::ShippedDefault),
            ),
            (
                serde_json::json!({"git": {}}),
                (PolicyLevel::Block, LevelOrigin::ShippedDefault),
            ),
        ] {
            let git = serde_json::from_value::<Policy>(policy.clone())
                .unwrap()
                .git;
            let raw = Ok(Some(policy.clone()));
            let got = pr_summary_effective(&raw, &git);
            assert_eq!((got.level, got.origin), expected, "{policy}");
        }
    }

    #[test]
    fn exemptions_touch_only_naming_and_message_shape() {
        let base = GitPolicy::default();
        let exempt = apply_profile_exemptions(&base);
        assert_eq!(exempt.branch_naming, PolicyLevel::Off);
        assert_eq!(exempt.commit_format, PolicyLevel::Off);
        assert_eq!(exempt.commit_body, PolicyLevel::Off);
        assert_eq!(exempt.ai_attribution, base.ai_attribution);
        assert_eq!(exempt.commit_emoji, base.commit_emoji);
        assert_eq!(exempt.policy_characters, base.policy_characters);
        assert_eq!(exempt.pr_sections, base.pr_sections);
        assert_eq!(exempt.pr_release_impact, base.pr_release_impact);
        assert_eq!(exempt.pr_summary, PolicyLevel::Off);
        assert_eq!(exempt.secret_scan, base.secret_scan);
    }
}
