//! Kept brownfield PR templates (SPC-013 R-84, R-115).
//!
//! A repository that already has a PR template keeps it: `init` installs no
//! second template and `update` never merges into it or writes a sidecar.
//! When the kept template's headings do not carry the required PR sections,
//! `init` or `update` records `git.pr_section_mapping` in the `diagnosed`
//! state with a proposed heading mapping, and edits nothing else in the
//! policy file byte for byte, apart from the one case R-115 names: a policy
//! file this very `init` run created loses its `pr_sections` key, so the
//! check runs at `warn` until the operator decides. An upgrade whose older
//! policy file lacks the key gets it at the shipped default, so its level
//! stays what it was.
//!
//! The decision (accepted, refused or custom) is recorded by
//! [`record_decision`]; a refused mapping appends the missing headings to the
//! template.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use super::json_edit;
use super::state::{
    guard_beneath_root, read_beneath_root, write_beneath_root, InstalledManifest, SyncBatch,
};
use super::ScaffoldError;
use crate::hooks::policy::{MappingState, Policy};

/// Where `CodeFlow` installs its PR template.
pub const MANAGED_TEMPLATE: &str = ".github/pull_request_template.md";

const POLICY: &str = ".codeflow/policy.json";

/// A PR template the project already had.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeptTemplate {
    /// Repository-relative path.
    pub path: String,
    /// Its `##` and `###` headings, in order.
    pub headings: Vec<String>,
}

/// Every PR template location GitHub reads: `.github/`, the root and
/// `docs/` (any case), and each Markdown file in
/// `.github/PULL_REQUEST_TEMPLATE/`.
fn candidates(root: &Path) -> Result<Vec<String>, ScaffoldError> {
    let mut found = Vec::new();
    let mut subtemplates = Vec::new();
    for dir in [".github", "", "docs"] {
        let directory = root.join(dir);
        if crate::absence::proven_absent(&directory)
            .map_err(|error| ScaffoldError::io(&directory, error))?
        {
            continue;
        }
        let mut names = Vec::new();
        for entry in
            std::fs::read_dir(&directory).map_err(|error| ScaffoldError::io(&directory, error))?
        {
            let entry = entry.map_err(|error| ScaffoldError::io(&directory, error))?;
            let name = entry.file_name();
            let kind = entry
                .file_type()
                .map_err(|error| ScaffoldError::io(entry.path(), error))?;
            if kind.is_file() && name.eq_ignore_ascii_case("pull_request_template.md") {
                let name = name
                    .into_string()
                    .map_err(|_| ScaffoldError::InvalidState {
                        what: "PR template".into(),
                        detail: "path is not valid UTF-8".into(),
                    })?;
                names.push(if dir.is_empty() {
                    name
                } else {
                    format!("{dir}/{name}")
                });
            } else if dir == ".github"
                && kind.is_dir()
                && name.eq_ignore_ascii_case("pull_request_template")
            {
                let text_name = |name: std::ffi::OsString| {
                    name.into_string().map_err(|_| ScaffoldError::InvalidState {
                        what: "PR template".into(),
                        detail: "path is not valid UTF-8".into(),
                    })
                };
                let dir_name = text_name(name)?;
                let mut children = Vec::new();
                for child in std::fs::read_dir(entry.path())
                    .map_err(|error| ScaffoldError::io(entry.path(), error))?
                {
                    let child = child.map_err(|error| ScaffoldError::io(entry.path(), error))?;
                    let child_name = text_name(child.file_name())?;
                    if child_name.to_ascii_lowercase().ends_with(".md") {
                        children.push(format!(".github/{dir_name}/{child_name}"));
                    }
                }
                children.sort();
                subtemplates.extend(children);
            }
        }
        names.sort();
        found.extend(names);
    }
    found.extend(subtemplates);
    Ok(found)
}
/// Read the first project-owned PR template; unreadable candidates refuse the decision.
///
/// # Errors
///
/// Returns an error when a template inventory or candidate cannot be read safely.
pub fn find_kept(
    root: &Path,
    shipped: &str,
    installed: &InstalledManifest,
) -> Result<Option<KeptTemplate>, ScaffoldError> {
    for path in candidates(root)? {
        let text = match read_beneath_root(root, &path) {
            Err(ScaffoldError::UnsafeSymlink { .. }) => continue,
            result => result?,
        }
        .ok_or_else(|| ScaffoldError::InvalidState {
            what: path.clone(),
            detail: "template disappeared during reading".into(),
        })?;
        let ours = text == shipped
            || (path == MANAGED_TEMPLATE && installed.files.contains_key(MANAGED_TEMPLATE));
        if !ours {
            return Ok(Some(KeptTemplate {
                headings: headings(&text),
                path,
            }));
        }
    }
    Ok(None)
}

/// The `##` and `###` headings of a Markdown template, outside HTML comments.
#[must_use]
pub fn headings(text: &str) -> Vec<String> {
    let mut visible = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("<!--") {
        visible.push_str(&rest[..start]);
        let Some(end) = rest[start..].find("-->") else {
            rest = "";
            break;
        };
        rest = &rest[start + end + 3..];
    }
    visible.push_str(rest);
    visible
        .lines()
        .filter_map(|line| {
            let t = line.trim_start_matches([' ', '\t']);
            let body = t.strip_prefix("### ").or_else(|| t.strip_prefix("## "))?;
            let name = body
                .trim_matches([' ', '\t'])
                .trim_end_matches('#')
                .trim_matches([' ', '\t']);
            (!name.is_empty()).then(|| name.to_string())
        })
        .collect()
}

/// Common template headings that carry each required section's content.
fn synonyms(required: &str) -> &'static [&'static str] {
    match required.to_ascii_lowercase().as_str() {
        "summary" => &[
            "description",
            "what",
            "what does this pr do",
            "what does this pr do?",
            "overview",
            "purpose",
            "about",
            "motivation",
            "why",
        ],
        "changes" => &[
            "what changed",
            "changes made",
            "change log",
            "changelog",
            "details",
            "implementation",
            "how",
            "list of changes",
        ],
        "testing" => &[
            "how tested",
            "how was this tested",
            "how was this tested?",
            "how has this been tested",
            "how has this been tested?",
            "test plan",
            "tests",
            "verification",
        ],
        "reviews" => &["review", "reviewers", "checklist"],
        "release impact" => &["release notes", "breaking changes", "impact"],
        _ => &[],
    }
}

/// A proposed mapping of required headings to the template's headings, or
/// `None` when the template already carries every required heading. A
/// required heading with no counterpart is left out of the mapping, so it
/// stays required under its own name.
#[must_use]
pub fn propose(required: &[String], template: &[String]) -> Option<BTreeMap<String, String>> {
    let has = |name: &str| template.iter().any(|h| h.eq_ignore_ascii_case(name));
    if required.iter().all(|r| has(r)) {
        return None;
    }
    let mut used: Vec<String> = required
        .iter()
        .filter(|r| has(r))
        .map(|r| r.to_ascii_lowercase())
        .collect();
    let mut mapping = BTreeMap::new();
    for req in required.iter().filter(|r| !has(r)) {
        let pick = synonyms(req).iter().find_map(|syn| {
            template
                .iter()
                .find(|h| h.eq_ignore_ascii_case(syn) && !used.contains(&h.to_ascii_lowercase()))
        });
        if let Some(actual) = pick {
            used.push(actual.to_ascii_lowercase());
            mapping.insert(req.clone(), actual.clone());
        }
    }
    Some(mapping)
}

fn mapping_json(state: MappingState, headings: &BTreeMap<String, String>, decided: &str) -> Value {
    serde_json::json!({
        "state": state.to_string(),
        "headings": headings,
        "decided": decided,
    })
}

/// Today as `YYYY-MM-DD` (UTC), for a decision date.
#[must_use]
pub fn today() -> String {
    super::init::today_utc()
}

/// Report lines for adopter automation `init` finds (TSK-107, from the
/// TSK-089 adoption plan): dependency bots whose pull requests fail branch
/// naming and commit format until a trusted automation profile names them,
/// and release tools that own versions, which want `release.backend =
/// "external"`. Read-only; nothing is written.
#[must_use]
pub fn automation_notes(root: &Path) -> Vec<String> {
    let mut notes = Vec::new();
    let bots: &[(&str, &[&str], &str, &str)] = &[
        (
            "Dependabot",
            &[".github/dependabot.yml", ".github/dependabot.yaml"],
            "dependabot[bot]",
            "dependabot/**",
        ),
        (
            "Renovate",
            &[
                "renovate.json",
                "renovate.json5",
                ".github/renovate.json",
                ".renovaterc",
                ".renovaterc.json",
            ],
            "renovate[bot]",
            "renovate/**",
        ),
    ];
    for (bot, files, actor, pattern) in bots {
        if let Some(file) = files.iter().find(|f| root.join(f).is_file()) {
            notes.push(format!(
                "{bot} is configured ({file}): its pull requests fail branch naming and commit format until you add a trusted profile to git.automation_profiles, e.g. {{\"name\": \"{}\", \"actors\": [\"{actor}\"], \"branch_pattern\": \"{pattern}\", \"sections\": {{...}}}} (see `codeflow policy explain`)",
                bot.to_ascii_lowercase()
            ));
        }
    }
    let backend = crate::hooks::adoption::release_backend(root)
        .unwrap_or(crate::hooks::adoption::ReleaseBackend::None);
    if let Some(finding) = crate::hooks::adoption::release_backend_finding(
        backend,
        &crate::hooks::adoption::detect_release_tools(root),
    ) {
        notes.push(finding);
    }
    notes
}

/// What [`diagnose`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diagnosis {
    /// The template carries every required heading; nothing recorded.
    Satisfied,
    /// A mapping is already recorded in this state; nothing rewritten.
    Recorded(MappingState),
    /// `diagnosed` was recorded now with this proposed mapping.
    Diagnosed {
        mapping: BTreeMap<String, String>,
        /// Required headings with no counterpart in the template.
        unmatched: Vec<String>,
    },
}

/// Diagnose a kept template against the policy's required sections and
/// record `diagnosed` when they differ. `policy_created` is `true` when this
/// run created the policy file (a fresh install with no prior policy).
///
/// # Errors
/// The policy file cannot be read, parsed or written, or the edit could not
/// be applied without changing another byte.
pub fn diagnose(
    root: &Path,
    kept: &KeptTemplate,
    policy_created: bool,
) -> Result<Diagnosis, ScaffoldError> {
    let (text, original, policy) = read_policy(root)?;
    if let Some(mapping) = &policy.git.pr_section_mapping {
        return Ok(Diagnosis::Recorded(mapping.state));
    }
    let mut required = policy.git.pr_required_sections.clone();
    for code in &policy.git.pr_code_sections {
        if !required.iter().any(|r| r.eq_ignore_ascii_case(code)) {
            required.push(code.clone());
        }
    }
    let Some(mapping) = propose(&required, &kept.headings) else {
        return Ok(Diagnosis::Satisfied);
    };
    let unmatched: Vec<String> = required
        .iter()
        .filter(|r| !kept.headings.iter().any(|h| h.eq_ignore_ascii_case(r)))
        .filter(|r| !mapping.contains_key(*r))
        .cloned()
        .collect();

    let has_level = original
        .get("git")
        .and_then(|g| g.get("pr_sections"))
        .is_some();
    let mut edited = text.clone();
    let mut expected = original.clone();
    if original.get("git").is_none() {
        // A sparse policy (`{}`, or sections other than `git`) is valid and
        // takes every default; the mapping needs a `git` object to live in
        // (TSK-107 review F5). Nothing else is rewritten.
        edited = json_edit::insert_member(&edited, &[], "git", "{}")
            .ok_or_else(|| edit_error("pr_section_mapping"))?;
        expected["git"] = serde_json::json!({});
    }
    if policy_created && has_level {
        edited = json_edit::remove_member_line(&edited, &["git", "pr_sections"])
            .ok_or_else(|| edit_error("pr_sections"))?;
        expected["git"]
            .as_object_mut()
            .map(|g| g.remove("pr_sections"));
    } else if !policy_created && !has_level {
        let level = Policy::default().git.pr_sections.to_string();
        edited =
            json_edit::insert_member(&edited, &["git"], "pr_sections", &format!("\"{level}\""))
                .ok_or_else(|| edit_error("pr_sections"))?;
        expected["git"]["pr_sections"] = Value::String(level);
    }
    let value = mapping_json(MappingState::Diagnosed, &mapping, "none");
    edited = json_edit::insert_member(
        &edited,
        &["git"],
        "pr_section_mapping",
        &serde_json::to_string(&value).unwrap_or_default(),
    )
    .ok_or_else(|| edit_error("pr_section_mapping"))?;
    expected["git"]["pr_section_mapping"] = value;
    let edited =
        json_edit::verified(edited, &expected).ok_or_else(|| edit_error("pr_section_mapping"))?;
    write_beneath_root(root, POLICY, edited.as_bytes())?;
    Ok(Diagnosis::Diagnosed { mapping, unmatched })
}

/// The policy file's text, parsed value and typed view, read without
/// following a symlink.
fn read_policy(root: &Path) -> Result<(String, Value, Policy), ScaffoldError> {
    let text = read_beneath_root(root, POLICY)?.ok_or_else(|| ScaffoldError::InvalidState {
        what: POLICY.to_string(),
        detail: "no policy file".to_string(),
    })?;
    let original: Value = serde_json::from_str(&text).map_err(|e| ScaffoldError::InvalidState {
        what: POLICY.to_string(),
        detail: e.to_string(),
    })?;
    let policy: Policy =
        serde_json::from_str(&text).map_err(|error| ScaffoldError::InvalidState {
            what: POLICY.into(),
            detail: error.to_string(),
        })?;
    Ok((text, original, policy))
}

fn edit_error(key: &str) -> ScaffoldError {
    ScaffoldError::InvalidState {
        what: POLICY.to_string(),
        detail: format!(
            "cannot record git.{key} without rewriting other bytes; add it by hand (see `codeflow policy explain`)"
        ),
    }
}

/// A one-paragraph report line for a diagnosis.
#[must_use]
pub fn describe(
    kept: &KeptTemplate,
    diagnosis: &Diagnosis,
    policy_created: bool,
) -> Option<String> {
    match diagnosis {
        Diagnosis::Satisfied => Some(format!(
            "kept PR template {}: it carries every required section; no second template installed",
            kept.path
        )),
        Diagnosis::Recorded(state) => Some(format!(
            "kept PR template {}: git.pr_section_mapping is {state}; template left as the project owns it",
            kept.path
        )),
        Diagnosis::Diagnosed { mapping, unmatched } => {
            let proposed = if mapping.is_empty() {
                "none".to_string()
            } else {
                mapping
                    .iter()
                    .map(|(r, a)| format!("{r} -> {a}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let unmatched = if unmatched.is_empty() {
                String::new()
            } else {
                format!("; no counterpart for: {}", unmatched.join(", "))
            };
            let level = if policy_created {
                "the PR-section check runs at warn (diagnosed) until you decide"
            } else {
                "the PR-section check keeps its configured level"
            };
            Some(format!(
                "kept PR template {} differs from the required PR sections: recorded git.pr_section_mapping = diagnosed with proposed mapping {proposed}{unmatched}; {level}. Decide: accepted (check the mapped headings), refused (append the missing headings to the template) or custom (set git.pr_required_sections in a reviewed change); `codeflow doctor` repeats this until then",
                kept.path
            ))
        }
    }
}

/// Record the operator's decision on a diagnosed mapping, dated `today`.
/// `refused` appends the missing required headings to the kept template.
/// Returns a report line.
///
/// # Errors
/// No diagnosed mapping is recorded, a decision of `diagnosed` was given,
/// or the files cannot be read or written.
pub fn record_decision(
    root: &Path,
    kept: &KeptTemplate,
    decision: MappingState,
    today: &str,
) -> Result<String, ScaffoldError> {
    let (text, original, policy) = read_policy(root)?;
    let mapping = policy
        .git
        .pr_section_mapping
        .filter(|m| m.state == MappingState::Diagnosed && decision != MappingState::Diagnosed)
        .ok_or_else(|| ScaffoldError::InvalidState {
            what: POLICY.to_string(),
            detail: "no diagnosed git.pr_section_mapping to decide".to_string(),
        })?;
    let value = mapping_json(decision, &mapping.headings, today);
    let edited = json_edit::replace_value(
        &text,
        &["git", "pr_section_mapping"],
        &serde_json::to_string(&value).unwrap_or_default(),
    )
    .ok_or_else(|| edit_error("pr_section_mapping"))?;
    let mut expected = original;
    expected["git"]["pr_section_mapping"] = value;
    let edited =
        json_edit::verified(edited, &expected).ok_or_else(|| edit_error("pr_section_mapping"))?;

    // A refusal appends to the template: read it beneath the root first, so
    // a template reached through a symlink refuses before anything is
    // recorded (TSK-107 review F1).
    let mut appended = None;
    if decision == MappingState::Refused {
        let mut required = policy.git.pr_required_sections.clone();
        for code in &policy.git.pr_code_sections {
            if !required.iter().any(|r| r.eq_ignore_ascii_case(code)) {
                required.push(code.clone());
            }
        }
        let missing: Vec<&String> = required
            .iter()
            .filter(|r| !kept.headings.iter().any(|h| h.eq_ignore_ascii_case(r)))
            .collect();
        if !missing.is_empty() {
            guard_beneath_root(root, Path::new(&kept.path))?;
            let mut template = read_beneath_root(root, &kept.path)?.ok_or_else(|| {
                ScaffoldError::InvalidState {
                    what: kept.path.clone(),
                    detail: "the kept PR template is gone".to_string(),
                }
            })?;
            if !template.ends_with('\n') {
                template.push('\n');
            }
            for heading in &missing {
                template.push_str("\n## ");
                template.push_str(heading);
                template.push_str("\n\n<!-- Required by git.pr_required_sections. -->\n");
            }
            let added = missing
                .iter()
                .map(|h| format!("## {h}"))
                .collect::<Vec<_>>()
                .join(", ");
            appended = Some((template, added));
        }
    }
    // Both writes share one run's flushes (TSK-153).
    let batch = SyncBatch::begin();
    write_beneath_root(root, POLICY, edited.as_bytes())?;
    let mut line =
        format!("PR template decision recorded: git.pr_section_mapping = {decision} ({today})");
    if let Some((template, added)) = appended {
        write_beneath_root(root, &kept.path, template.as_bytes())?;
        line = format!("{line}; appended {added} to {}", kept.path);
    } else if decision == MappingState::Custom {
        line.push_str(
            "; set git.pr_required_sections and git.pr_code_sections to your template's headings in a reviewed change",
        );
    }
    batch.finish()?;
    Ok(line)
}

#[cfg(test)]
mod tests {

    #[cfg(unix)]
    #[test]
    fn r18_template_backslash_name_keeps_its_identity() {
        let dir = tempfile::tempdir().unwrap();
        let templates = dir.path().join(".github/PULL_REQUEST_TEMPLATE");
        std::fs::create_dir_all(templates.join("custom")).unwrap();
        std::fs::write(templates.join(r"custom\name.md"), "## Literal backslash\n").unwrap();
        std::fs::write(templates.join("custom/name.md"), "## Different file\n").unwrap();
        let kept = find_kept(dir.path(), "shipped", &InstalledManifest::new("0"))
            .unwrap()
            .unwrap();
        assert_eq!(kept.path, r".github/PULL_REQUEST_TEMPLATE/custom\name.md");
        assert_eq!(kept.headings, ["Literal backslash"]);
    }

    use super::*;

    const SHIPPED: &str = include_str!("../../../../assets/base/policy.json");

    fn write(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    const PROJECT_TEMPLATE: &str =
        "## What\n\n<!-- ## Hidden -->\n\n## Why\n\n### How tested\n\n- [ ] Tests pass\n";

    #[test]
    fn automation_notes_name_bots_and_release_tools() {
        let dir = tempfile::tempdir().unwrap();
        assert!(automation_notes(dir.path()).is_empty());
        write(dir.path(), ".github/dependabot.yml", "version: 2\n");
        write(dir.path(), "release-please-config.json", "{}");
        let notes = automation_notes(dir.path());
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!(notes[0].contains("dependabot[bot]") && notes[0].contains("dependabot/**"));
        assert!(notes[1].contains("release-please") && notes[1].contains("external"));
    }

    #[test]
    fn headings_skip_comments_and_keep_order() {
        assert_eq!(headings(PROJECT_TEMPLATE), ["What", "Why", "How tested"]);
    }

    #[test]
    fn propose_maps_synonyms_and_leaves_the_rest() {
        let required: Vec<String> = ["Summary", "Changes", "Reviews", "Release impact", "Testing"]
            .map(String::from)
            .to_vec();
        let mapping = propose(&required, &headings(PROJECT_TEMPLATE)).unwrap();
        assert_eq!(mapping.get("Summary").map(String::as_str), Some("What"));
        assert_eq!(
            mapping.get("Testing").map(String::as_str),
            Some("How tested")
        );
        assert!(!mapping.contains_key("Changes"));
        assert!(propose(&["Summary".into()], &["summary".into()]).is_none());
    }

    #[test]
    fn finds_templates_in_every_github_location() {
        let dir = tempfile::tempdir().unwrap();
        let installed = InstalledManifest::new("0");
        assert!(find_kept(dir.path(), "shipped", &installed)
            .unwrap()
            .is_none());
        write(
            dir.path(),
            ".github/PULL_REQUEST_TEMPLATE/feature.md",
            "## What\n",
        );
        assert_eq!(
            find_kept(dir.path(), "shipped", &installed)
                .unwrap()
                .unwrap()
                .path,
            ".github/PULL_REQUEST_TEMPLATE/feature.md"
        );
        write(dir.path(), "docs/PULL_REQUEST_TEMPLATE.md", "## What\n");
        assert_eq!(
            find_kept(dir.path(), "shipped", &installed)
                .unwrap()
                .unwrap()
                .path,
            "docs/PULL_REQUEST_TEMPLATE.md"
        );
        write(dir.path(), ".github/pull_request_template.md", "shipped");
        assert_eq!(
            find_kept(dir.path(), "shipped", &installed)
                .unwrap()
                .unwrap()
                .path,
            "docs/PULL_REQUEST_TEMPLATE.md",
            "the shipped template is never a kept one"
        );
    }

    fn diagnosed_repo(policy: &str, created: bool) -> (tempfile::TempDir, KeptTemplate, Diagnosis) {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), POLICY, policy);
        write(dir.path(), MANAGED_TEMPLATE, PROJECT_TEMPLATE);
        let kept = KeptTemplate {
            path: MANAGED_TEMPLATE.into(),
            headings: headings(PROJECT_TEMPLATE),
        };
        let d = diagnose(dir.path(), &kept, created).unwrap();
        (dir, kept, d)
    }

    fn read(dir: &Path, rel: &str) -> String {
        std::fs::read_to_string(dir.join(rel)).unwrap()
    }

    #[test]
    fn fresh_policy_loses_only_its_level_and_gains_the_mapping() {
        let (dir, _, d) = diagnosed_repo(SHIPPED, true);
        assert!(matches!(d, Diagnosis::Diagnosed { .. }));
        let after = read(dir.path(), POLICY);
        let v: Value = serde_json::from_str(&after).unwrap();
        assert!(v["git"].get("pr_sections").is_none());
        assert_eq!(v["git"]["pr_section_mapping"]["state"], "diagnosed");
        // Every other line of the shipped file is kept byte for byte.
        let kept: Vec<&str> = SHIPPED
            .lines()
            .filter(|l| !l.contains("\"pr_sections\""))
            .collect();
        let now: Vec<&str> = after
            .lines()
            .filter(|l| !l.contains("\"pr_section_mapping\""))
            .collect();
        assert_eq!(kept, now);
    }

    #[test]
    fn upgrade_keeps_every_policy_byte_and_an_equal_to_default_block() {
        let (dir, _, _) = diagnosed_repo(SHIPPED, false);
        let after = read(dir.path(), POLICY);
        let without: String = after
            .lines()
            .filter(|l| !l.contains("\"pr_section_mapping\""))
            .flat_map(|l| [l, "\n"])
            .collect();
        assert_eq!(without, SHIPPED);
        let v: Value = serde_json::from_str(&after).unwrap();
        assert_eq!(v["git"]["pr_sections"], "block");
    }

    #[test]
    fn upgrade_of_a_file_without_a_level_pins_the_default_level() {
        let policy = r#"{"git":{"pr_required_sections":["Summary"]}}"#;
        let (dir, _, _) = diagnosed_repo(policy, false);
        let v: Value = serde_json::from_str(&read(dir.path(), POLICY)).unwrap();
        assert_eq!(v["git"]["pr_sections"], "block");
        assert_eq!(v["git"]["pr_required_sections"][0], "Summary");
    }

    #[test]
    fn a_recorded_mapping_is_never_rewritten() {
        let (dir, kept, _) = diagnosed_repo(SHIPPED, false);
        let before = read(dir.path(), POLICY);
        let again = diagnose(dir.path(), &kept, false).unwrap();
        assert_eq!(again, Diagnosis::Recorded(MappingState::Diagnosed));
        assert_eq!(read(dir.path(), POLICY), before);
    }

    /// Codex review F1: a kept template reached through a symlinked
    /// directory is never found, and a refusal naming one records nothing
    /// and writes nothing outside the repository.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_template_is_neither_found_nor_written() {
        let (dir, _, _) = diagnosed_repo(SHIPPED, false);
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(
            outside.path().join("PULL_REQUEST_TEMPLATE.md"),
            PROJECT_TEMPLATE,
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("docs")).unwrap();
        std::fs::remove_file(dir.path().join(MANAGED_TEMPLATE)).unwrap();
        assert!(
            find_kept(dir.path(), "shipped", &InstalledManifest::new("0"))
                .unwrap()
                .is_none()
        );

        let linked = KeptTemplate {
            path: "docs/PULL_REQUEST_TEMPLATE.md".into(),
            headings: headings(PROJECT_TEMPLATE),
        };
        let before = read(dir.path(), POLICY);
        assert!(record_decision(dir.path(), &linked, MappingState::Refused, "2026-09-27").is_err());
        assert_eq!(read(dir.path(), POLICY), before, "nothing recorded");
        assert_eq!(
            std::fs::read_to_string(outside.path().join("PULL_REQUEST_TEMPLATE.md")).unwrap(),
            PROJECT_TEMPLATE
        );
    }

    /// Codex review F5: a sparse policy gains a `git` object in place.
    #[test]
    fn a_policy_without_a_git_object_gains_one_in_place() {
        for prior in ["{}\n", "{\n  \"recall\": {\n    \"share\": false\n  }\n}\n"] {
            let (dir, _, d) = diagnosed_repo(prior, false);
            assert!(matches!(d, Diagnosis::Diagnosed { .. }), "{prior}");
            let v: Value = serde_json::from_str(&read(dir.path(), POLICY)).unwrap();
            assert_eq!(v["git"]["pr_sections"], "block", "{prior}");
            assert_eq!(
                v["git"]["pr_section_mapping"]["state"], "diagnosed",
                "{prior}"
            );
            let before: Value = serde_json::from_str(prior).unwrap();
            for (key, value) in before.as_object().unwrap() {
                assert_eq!(&v[key], value, "{prior}");
            }
        }
    }

    #[test]
    fn decisions_are_recorded_and_refused_appends_headings() {
        for decision in [
            MappingState::Accepted,
            MappingState::Refused,
            MappingState::Custom,
        ] {
            let (dir, kept, _) = diagnosed_repo(SHIPPED, false);
            let line = record_decision(dir.path(), &kept, decision, "2026-09-26").unwrap();
            assert!(line.contains(&decision.to_string()), "{line}");
            let v: Value = serde_json::from_str(&read(dir.path(), POLICY)).unwrap();
            assert_eq!(
                v["git"]["pr_section_mapping"]["state"],
                decision.to_string()
            );
            assert_eq!(v["git"]["pr_section_mapping"]["decided"], "2026-09-26");
            let template = read(dir.path(), MANAGED_TEMPLATE);
            if decision == MappingState::Refused {
                assert!(template.contains("\n## Summary\n"), "{template}");
                assert!(template.contains("\n## Reviews\n"), "{template}");
                // Release impact left the shipped required list (TSK-184).
                assert!(!template.contains("Release impact"), "{template}");
            } else {
                assert_eq!(template, PROJECT_TEMPLATE);
            }
            assert!(record_decision(dir.path(), &kept, decision, "2026-09-26").is_err());
        }
    }
}

#[cfg(test)]
mod r16_core_regressions {
    #[test]
    fn r16_invalid_typed_policy_is_not_default() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(".codeflow")).unwrap();
        std::fs::write(
            root.path().join(".codeflow/policy.json"),
            r#"{"git":{"push_to_protected":"not-a-level"}}"#,
        )
        .unwrap();
        assert!(super::read_policy(root.path()).is_err());
    }
}

#[cfg(test)]
mod r16_obtaining_regressions {

    #[test]
    fn r16_template_inventory_error_refuses() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".github"), "not a directory").unwrap();
        assert!(super::candidates(dir.path()).is_err());
    }
}
