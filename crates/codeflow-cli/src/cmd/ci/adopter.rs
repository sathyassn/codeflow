//! Adopter fit in `codeflow ci` (TSK-107; SPC-013 R-82, R-84, R-113, R-115):
//! trusted automation profiles read from the target side, the effective
//! PR-section level while a kept template is diagnosed, the headings an
//! accepted mapping checks, the effective level and origin of every check,
//! and the two-step upgrade message for policy keys this binary cannot read.

use std::path::Path;

use codeflow_core::hooks::adoption::{self, EffectiveLevel, LevelOrigin};
use codeflow_core::hooks::policy::{AutomationProfile, UNKNOWN_ACTOR};
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

/// The actor a profile may trust (SPC-013 R-82; TSK-107 review F2). Only a
/// GitHub Actions run for a pull request event from the same repository
/// carries an identity, and `--actor` must match the event's actor. A local
/// run, another CI, a fork or a mismatch is `unknown` whatever the flag says,
/// with the reason.
pub(super) fn trusted_actor(
    flag: &str,
    env: &dyn Fn(&str) -> Option<std::ffi::OsString>,
) -> Result<(String, Option<String>), String> {
    if flag.is_empty() || flag == UNKNOWN_ACTOR {
        return Ok((UNKNOWN_ACTOR.to_string(), None));
    }
    let untrusted = |why: String| {
        Ok((
            UNKNOWN_ACTOR.to_string(),
            Some(format!("--actor '{flag}' is not trusted: {why}")),
        ))
    };
    let text = |key: &str| {
        env(key)
            .map(std::ffi::OsString::into_string)
            .transpose()
            .map_err(|_| format!("cannot read {key} as UTF-8"))
    };
    if text("GITHUB_ACTIONS")?.as_deref() != Some("true") {
        return untrusted("not a GitHub Actions run, so there is no event identity".to_string());
    }
    let event = text("GITHUB_EVENT_NAME")?.unwrap_or_default();
    if event != "pull_request_target" && event != "pull_request" {
        return untrusted(format!("the '{event}' event is not a pull request event"));
    }
    let Some(path) = env("GITHUB_EVENT_PATH") else {
        return untrusted("GITHUB_EVENT_PATH is absent, so there is no event identity".into());
    };
    let bytes =
        std::fs::read(&path).map_err(|error| format!("cannot read GitHub event: {error}"))?;
    let payload: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("cannot read GitHub event JSON: {error}"))?;
    let repo = |side: &str| {
        payload
            .pointer(&format!("/pull_request/{side}/repo/full_name"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    match (repo("head"), repo("base")) {
        (Some(head), Some(base)) if head == base => {}
        _ => return untrusted("a fork pull request carries no trusted identity".to_string()),
    }
    if text("GITHUB_ACTOR")?.as_deref() != Some(flag) {
        return untrusted("it is not the event's actor".to_string());
    }
    Ok((flag.to_string(), None))
}

/// Resolve adopter fit for this run and print what applies: the actor, any
/// profile, and the effective level and origin of every check.
pub(super) fn resolve(
    root: &Path,
    git: &GitPolicy,
    raw: &Result<Option<serde_json::Value>, String>,
    profiles: &[AutomationProfile],
    actor: &str,
    branch: &str,
    has_body: bool,
) -> Result<Adoption, String> {
    let mut effective = git.clone();
    let pr_sections = adoption::pr_sections_effective(raw, git);
    effective.pr_sections = pr_sections.level;
    let pr_summary = adoption::pr_summary_effective(raw, git);
    effective.pr_summary = pr_summary.level;
    effective.pr_required_sections = adoption::mapped_sections(git, &git.pr_required_sections);
    effective.pr_code_sections = adoption::mapped_sections(git, &git.pr_code_sections);

    let (actor, why) = trusted_actor(actor, &|key| std::env::var_os(key))?;
    let actor = actor.as_str();
    println!("codeflow ci: actor '{actor}'");
    if let Some(why) = why {
        println!("codeflow ci: {why}");
    }
    let profile = adoption::matching_profile(profiles, actor, branch).cloned();
    if let Some(p) = &profile {
        println!(
            "codeflow ci: automation profile '{}' applies (actor '{actor}', branch '{branch}', read from the target side): branch naming, the commit message shape rules and the PR Summary shape are skipped; classification: automation profile",
            p.name
        );
        effective = adoption::apply_profile_exemptions(&effective);
    } else if let Some(p) = adoption::pattern_only_profile(profiles, branch) {
        println!(
            "codeflow ci: branch '{branch}' matches automation profile '{}' but actor '{actor}' is not one of its trusted actors: no exemption or supplied content applies",
            p.name
        );
    }

    let tracked = codeflow_core::workgraph::durable_work_tracking_enabled(root).unwrap_or(false);
    print_levels(
        raw,
        git,
        &effective,
        Levels {
            pr_sections,
            pr_summary,
        },
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
        Err(error) => violations.push(Violation::always_blocking(
            "release.backend",
            error,
            "set `[release] backend` in .codeflow/project.toml to none, external or codeflow",
        )),
    }
    Ok(Adoption {
        git: effective,
        profile,
        violations,
    })
}

/// Put the unit name a matched profile supplies on a `Task:` line when the
/// bot body has none: a bot names no task record, so the profile names the
/// unit its pull requests are (TSK-184). A body with its own line is left to
/// classification.
pub(super) fn supply_task(profile: Option<&AutomationProfile>, body: &str) -> String {
    let Some((profile, task)) = profile.and_then(|p| p.task.as_deref().map(|t| (p, t))) else {
        return body.to_string();
    };
    if body
        .lines()
        .any(|line| line.trim_start().starts_with("Task:"))
    {
        return body.to_string();
    }
    println!(
        "codeflow ci: `Task: {task}` supplied by automation profile '{}'",
        profile.name
    );
    format!("Task: {task}\n\n{body}")
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

/// Validate the head's own policy and project state as data (SPC-013 R-113;
/// TSK-107 review F3). The rules are judged by the target's policy, but a
/// head that raises the policy past what this binary reads fails here with
/// the two-step upgrade order, even when the checkout is the target and the
/// head's own workflow no longer validates it. `Some(2)` stops the run.
pub(super) fn check_head_config(root: &Path, head: &str) -> Option<i32> {
    let show = |path: &str| {
        let out = codeflow_core::git::command()
            .arg("-C")
            .arg(root)
            .args(["show", &format!("{head}:{path}")])
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).to_string())
    };
    let mut failed = false;
    if let Some(text) = show(".codeflow/policy.json") {
        if let Err(errors) = policy_schema::validate_policy_str(&text) {
            for e in &errors {
                eprintln!("codeflow ci: head policy error: {e}");
            }
            eprintln!(
                "codeflow ci: error: the head's .codeflow/policy.json is not valid for this codeflow; nothing was verified"
            );
            if let Some(hint) =
                policy_schema::upgrade_order_hint(&errors, env!("CARGO_PKG_VERSION"))
            {
                eprintln!("codeflow ci: {hint}");
            }
            failed = true;
        }
    }
    if let Some(text) = show(".codeflow/project.toml") {
        if let Err(e) = adoption::release_backend_str(&text, "the head's .codeflow/project.toml") {
            eprintln!("codeflow ci: error: {e}");
            failed = true;
        }
    }
    failed.then_some(2)
}

/// Which optional checks this run includes.
#[derive(Clone, Copy)]
struct Ran {
    /// A PR body was provided, so the PR-body checks run.
    has_body: bool,
    /// Durable work tracking is on, so the work-record checks run.
    tracked: bool,
}

/// The levels adopter fit resolves before the profile applies, with their
/// origins.
#[derive(Clone, Copy)]
struct Levels {
    pr_sections: EffectiveLevel,
    pr_summary: EffectiveLevel,
}

/// Print the effective level and origin of every check `codeflow ci` runs.
fn print_levels(
    raw: &Result<Option<serde_json::Value>, String>,
    configured: &GitPolicy,
    effective: &GitPolicy,
    levels: Levels,
    profile: Option<&AutomationProfile>,
    ran: Ran,
) {
    let Levels {
        pr_sections,
        pr_summary,
    } = levels;
    let rows: [(&str, PolicyLevel, PolicyLevel); 11] = [
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
        ("pr_summary", pr_summary.level, effective.pr_summary),
        (
            "work_records",
            configured.work_records_level(),
            configured.work_records_level(),
        ),
        (
            "work_planning",
            configured.work_planning_level(),
            configured.work_planning_level(),
        ),
    ];
    for (key, level, applied) in rows {
        let runs = match key {
            "pr_sections" | "pr_release_impact" | "pr_summary" => ran.has_body,
            "work_records" | "work_planning" => ran.tracked,
            _ => true,
        };
        if !runs {
            continue;
        }
        let origin = match key {
            "pr_sections" => pr_sections.origin,
            "pr_summary" => pr_summary.origin,
            _ => adoption::effective(raw, key, level).origin,
        };
        let skipped = match profile {
            Some(p) if applied != level => format!("; skipped by automation profile '{}'", p.name),
            _ => String::new(),
        };
        println!("codeflow ci: level {key} = {level} ({origin}){skipped}");
    }
    if ran.has_body && pr_sections.origin == LevelOrigin::Diagnosed {
        println!(
            "codeflow ci: the PR-section and Summary shape checks run at warn while the kept PR template is diagnosed; decide in pr_section_mapping (accepted, refused or custom)"
        );
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn r17_absent_event_path_keeps_actor_untrusted() {
        let env = |key: &str| match key {
            "GITHUB_ACTIONS" => Some("true".into()),
            "GITHUB_EVENT_NAME" => Some("pull_request".into()),
            "GITHUB_ACTOR" => Some("automation".into()),
            _ => None,
        };
        let (actor, why) = super::trusted_actor("automation", &env).unwrap();
        assert_eq!(actor, super::UNKNOWN_ACTOR);
        assert!(why.unwrap().contains("GITHUB_EVENT_PATH is absent"));
    }

    #[test]
    fn r16_unreadable_actor_event_refuses() {
        let dir = tempfile::tempdir().unwrap();
        let event = dir.path().join("event.json");
        std::fs::write(&event, b"\xff").unwrap();
        let env = |key: &str| match key {
            "GITHUB_ACTIONS" => Some("true".into()),
            "GITHUB_EVENT_NAME" => Some("pull_request".into()),
            "GITHUB_EVENT_PATH" => Some(event.as_os_str().to_owned()),
            "GITHUB_ACTOR" => Some("automation".into()),
            _ => None,
        };
        assert!(super::trusted_actor("automation", &env)
            .unwrap_err()
            .contains("GitHub event"));
    }

    #[test]
    fn r15_actor_requires_exact_identity() {
        let temp = tempfile::tempdir().unwrap();
        let event = temp.path().join("event.json");
        std::fs::write(&event, r#"{"pull_request":{"head":{"repo":{"full_name":"o/r"}},"base":{"repo":{"full_name":"o/r"}}}}"#).unwrap();
        let env = |key: &str| match key {
            "GITHUB_ACTIONS" => Some("true".into()),
            "GITHUB_EVENT_NAME" => Some("pull_request".into()),
            "GITHUB_EVENT_PATH" => Some(event.to_str().unwrap().into()),
            "GITHUB_ACTOR" => Some("automation".into()),
            _ => None,
        };
        let (actor, why) = super::trusted_actor("automation\u{a0}", &env).unwrap();
        assert_eq!(actor, "unknown");
        assert!(why.is_some());
    }

    #[test]
    fn only_a_same_repository_pull_request_event_carries_an_actor() {
        use super::trusted_actor;
        let dir = std::env::temp_dir().join(format!("codeflow-actor-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let same = dir.join("same.json");
        let fork = dir.join("fork.json");
        let event = |head: &str| {
            format!(
                r#"{{"pull_request":{{"head":{{"repo":{{"full_name":"{head}"}}}},"base":{{"repo":{{"full_name":"o/r"}}}}}}}}"#
            )
        };
        std::fs::write(&same, event("o/r")).unwrap();
        std::fs::write(&fork, event("mallory/r")).unwrap();
        let bot = "dependabot[bot]";
        let ci = |path: &std::path::Path, actor: &'static str| {
            let path = path.display().to_string();
            move |key: &str| match key {
                "GITHUB_ACTIONS" => Some("true".into()),
                "GITHUB_EVENT_NAME" => Some("pull_request_target".into()),
                "GITHUB_EVENT_PATH" => Some(path.clone().into()),
                "GITHUB_ACTOR" => Some(actor.into()),
                _ => None,
            }
        };
        // A local run: the flag alone is never an identity.
        let (actor, why) = trusted_actor(bot, &|_| None).unwrap();
        assert_eq!(actor, "unknown");
        assert!(why.unwrap().contains("not a GitHub Actions run"));
        // The trusted path: same repository, the event's own actor.
        assert_eq!(
            trusted_actor(bot, &ci(&same, bot)).unwrap(),
            (bot.to_string(), None)
        );
        // A fork, and a flag that is not the event's actor.
        let (actor, why) = trusted_actor(bot, &ci(&fork, bot)).unwrap();
        assert_eq!(actor, "unknown");
        assert!(why.unwrap().contains("fork"));
        let (actor, _) = trusted_actor(bot, &ci(&same, "mallory")).unwrap();
        assert_eq!(actor, "unknown");
        let _ = std::fs::remove_dir_all(&dir);
    }

    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn a_profile_supplies_the_task_line_only_when_the_body_has_none() {
        let supplied = supply_task(Some(&profile()), "## Summary\n\nBump.\n");
        assert!(supplied.starts_with("Task: release pull request\n\n## Summary"));
        let own = "Task: TSK-001\n\n## Summary\n";
        assert_eq!(supply_task(Some(&profile()), own), own);
        assert_eq!(supply_task(None, own), own);
        let mut silent = profile();
        silent.task = None;
        assert_eq!(supply_task(Some(&silent), "## Summary\n"), "## Summary\n");
    }

    fn profile() -> AutomationProfile {
        AutomationProfile {
            name: "changesets".into(),
            actors: vec!["github-actions[bot]".into()],
            branch_pattern: "changeset-release/*".into(),
            task: Some("release pull request".into()),
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
