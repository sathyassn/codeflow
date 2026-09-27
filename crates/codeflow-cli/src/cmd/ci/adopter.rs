//! Adopter fit in `codeflow ci` (TSK-107; SPC-013 R-82, R-84, R-113, R-115):
//! trusted automation profiles read from the target side, the effective
//! PR-section level while a kept template is diagnosed, the headings an
//! accepted mapping checks, the effective level and origin of every check,
//! and the two-step upgrade message for policy keys this binary cannot read.

use std::path::Path;
use std::process::Command;

use codeflow_core::hooks::adoption::{self, EffectiveLevel, LevelOrigin};
use codeflow_core::hooks::policy::{AutomationProfile, Policy, UNKNOWN_ACTOR};
use codeflow_core::hooks::policy_schema;
use codeflow_core::hooks::{GitPolicy, PolicyLevel, Violation};

use super::pr_body::find_section;
use super::SectionState;

/// The policy `codeflow ci` evaluates with, after adopter fit is applied.
pub(super) struct Adoption {
    /// The effective policy: the PR-section level and headings resolved, and
    /// a matched profile's exemptions applied.
    pub git: GitPolicy,
    /// The trusted profile that applies to this run, if any.
    pub profile: Option<AutomationProfile>,
    /// Findings about the adopter configuration itself.
    pub violations: Vec<Violation>,
}

/// Resolve adopter fit for this run and print what applies: the actor, any
/// profile, and the effective level and origin of every check.
pub(super) fn resolve(
    root: &Path,
    git: &GitPolicy,
    base_sha: Option<&str>,
    actor: &str,
    branch: &str,
    has_body: bool,
) -> Adoption {
    let raw = adoption::raw_policy(root);
    let mut effective = git.clone();
    let pr_sections = adoption::pr_sections_effective(&raw, git);
    effective.pr_sections = pr_sections.level;
    effective.pr_required_sections = adoption::mapped_sections(git, &git.pr_required_sections);
    effective.pr_code_sections = adoption::mapped_sections(git, &git.pr_code_sections);

    let actor = if actor.trim().is_empty() {
        UNKNOWN_ACTOR
    } else {
        actor.trim()
    };
    println!("codeflow ci: actor '{actor}'");
    let profiles = match base_sha {
        Some(base) => target_profiles(root, base),
        None => Vec::new(),
    };
    let profile = adoption::matching_profile(&profiles, actor, branch).cloned();
    if let Some(p) = &profile {
        println!(
            "codeflow ci: automation profile '{}' applies (actor '{actor}', branch '{branch}', read from the target side): branch naming and the commit message shape rules are skipped; classification: automation profile",
            p.name
        );
        effective = adoption::apply_profile_exemptions(&effective);
    } else if let Some(p) = adoption::pattern_only_profile(&profiles, branch) {
        println!(
            "codeflow ci: branch '{branch}' matches automation profile '{}' but actor '{actor}' is not one of its trusted actors: no exemption or supplied content applies",
            p.name
        );
    }

    let tracked = codeflow_core::workgraph::durable_work_tracking_enabled(root).unwrap_or(false);
    print_levels(
        &raw,
        git,
        &effective,
        pr_sections,
        profile.as_ref(),
        Ran { has_body, tracked },
    );
    if let Some(pending) = adoption::pending_decision(git) {
        println!("codeflow ci: {pending}");
    }
    let mut violations = Vec::new();
    match adoption::release_backend(root) {
        Ok(backend) => {
            println!(
                "codeflow ci: release backend '{backend}' (.codeflow/project.toml release.backend)"
            );
            if let Some(finding) =
                adoption::release_backend_finding(backend, &adoption::detect_release_tools(root))
            {
                println!("codeflow ci: {finding}");
            }
        }
        Err(error) => violations.push(Violation::new(
            "release.backend",
            PolicyLevel::Block,
            error,
            "set `[release] backend` in .codeflow/project.toml to none, external or codeflow"
                .to_string(),
        )),
    }
    Adoption {
        git: effective,
        profile,
        violations,
    }
}

/// Add the content a matched profile supplies for each PR section the bot
/// body omits. A section the body carries, even empty, is left to the check.
pub(super) fn supply_sections(profile: Option<&AutomationProfile>, body: &str) -> String {
    let Some(profile) = profile else {
        return body.to_string();
    };
    let mut out = body.to_string();
    for (heading, content) in &profile.sections {
        if find_section(body, heading) == SectionState::Missing {
            if !out.ends_with('\n') && !out.is_empty() {
                out.push('\n');
            }
            out.push_str("\n## ");
            out.push_str(heading);
            out.push_str("\n\n");
            out.push_str(content);
            out.push('\n');
            println!(
                "codeflow ci: section '## {heading}' supplied by automation profile '{}'",
                profile.name
            );
        }
    }
    out
}

/// The automation profiles in the policy of `base`, the target side of the
/// range. A head cannot add or widen a profile for itself. A missing or
/// invalid target policy yields no profile.
fn target_profiles(root: &Path, base: &str) -> Vec<AutomationProfile> {
    let Ok(out) = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["show", &format!("{base}:.codeflow/policy.json")])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&out.stdout);
    if policy_schema::validate_policy_str(&text).is_err() {
        println!(
            "codeflow ci: the target-side policy is not valid for this binary; no automation profile applies"
        );
        return Vec::new();
    }
    serde_json::from_str::<Policy>(&text)
        .map(|p| p.git.automation_profiles)
        .unwrap_or_default()
}

/// Which optional checks this run includes.
#[derive(Clone, Copy)]
struct Ran {
    /// A PR body was provided, so the PR-body checks run.
    has_body: bool,
    /// Durable work tracking is on, so the work-record checks run.
    tracked: bool,
}

/// Print the effective level and origin of every check `codeflow ci` runs.
fn print_levels(
    raw: &Result<Option<serde_json::Value>, String>,
    configured: &GitPolicy,
    effective: &GitPolicy,
    pr_sections: EffectiveLevel,
    profile: Option<&AutomationProfile>,
    ran: Ran,
) {
    let rows: [(&str, PolicyLevel, PolicyLevel); 9] = [
        (
            "commit_format",
            configured.commit_format,
            effective.commit_format,
        ),
        ("commit_body", configured.commit_body, effective.commit_body),
        (
            "ai_attribution",
            configured.ai_attribution,
            effective.ai_attribution,
        ),
        (
            "commit_emoji",
            configured.commit_emoji,
            effective.commit_emoji,
        ),
        (
            "policy_characters",
            configured.policy_characters,
            effective.policy_characters,
        ),
        (
            "branch_naming",
            configured.branch_naming,
            effective.branch_naming,
        ),
        ("pr_sections", pr_sections.level, effective.pr_sections),
        (
            "pr_release_impact",
            configured.pr_release_impact,
            effective.pr_release_impact,
        ),
        (
            "work_records",
            configured.work_records_level(),
            configured.work_records_level(),
        ),
    ];
    for (key, level, applied) in rows {
        let runs = match key {
            "pr_sections" | "pr_release_impact" => ran.has_body,
            "work_records" => ran.tracked,
            _ => true,
        };
        if !runs {
            continue;
        }
        let origin = if key == "pr_sections" {
            pr_sections.origin
        } else {
            adoption::effective(raw, key, level).origin
        };
        let skipped = match profile {
            Some(p) if applied != level => format!("; skipped by automation profile '{}'", p.name),
            _ => String::new(),
        };
        println!("codeflow ci: level {key} = {level} ({origin}){skipped}");
    }
    if ran.has_body && pr_sections.origin == LevelOrigin::Diagnosed {
        println!(
            "codeflow ci: the PR-section check runs at warn while the kept PR template is diagnosed; decide in pr_section_mapping (accepted, refused or custom)"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn profile() -> AutomationProfile {
        AutomationProfile {
            name: "changesets".into(),
            actors: vec!["github-actions[bot]".into()],
            branch_pattern: "changeset-release/*".into(),
            sections: BTreeMap::from([
                (
                    "Testing".to_string(),
                    "Release PR; CI runs the suite.".to_string(),
                ),
                ("Summary".to_string(), "never used".to_string()),
            ]),
        }
    }

    #[test]
    fn supplies_only_omitted_sections() {
        let body = "## Summary\n\nVersion packages.\n";
        let out = supply_sections(Some(&profile()), body);
        assert!(out.contains("## Testing\n\nRelease PR; CI runs the suite."));
        assert!(!out.contains("never used"));
        assert_eq!(supply_sections(None, body), body);
    }
}
