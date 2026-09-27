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

use super::state::{write_file, InstalledManifest};
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
fn candidates(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for dir in [".github", "", "docs"] {
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
            continue;
        };
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.eq_ignore_ascii_case("pull_request_template.md"))
            .collect();
        names.sort();
        for name in names {
            found.push(if dir.is_empty() {
                name
            } else {
                format!("{dir}/{name}")
            });
        }
    }
    if let Ok(entries) = std::fs::read_dir(root.join(".github")) {
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.eq_ignore_ascii_case("pull_request_template")
                || !entry.file_type().is_ok_and(|t| t.is_dir())
            {
                continue;
            }
            let mut files: Vec<String> = std::fs::read_dir(entry.path())
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.to_ascii_lowercase().ends_with(".md"))
                .collect();
            files.sort();
            found.extend(files.into_iter().map(|f| format!(".github/{name}/{f}")));
        }
    }
    found
}

/// The project's own PR template, if it has one: the first template found
/// that is not `CodeFlow`'s (its bytes differ from `shipped` and `CodeFlow` has
/// no install record for it).
#[must_use]
pub fn find_kept(
    root: &Path,
    shipped: &str,
    installed: &InstalledManifest,
) -> Option<KeptTemplate> {
    candidates(root).into_iter().find_map(|path| {
        let text = std::fs::read_to_string(root.join(&path)).ok()?;
        let ours = text == shipped
            || (path == MANAGED_TEMPLATE && installed.files.contains_key(MANAGED_TEMPLATE));
        (!ours).then(|| KeptTemplate {
            headings: headings(&text),
            path,
        })
    })
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
            let t = line.trim_start();
            let body = t.strip_prefix("### ").or_else(|| t.strip_prefix("## "))?;
            let name = body.trim().trim_end_matches('#').trim();
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

/// The byte span of the value of `git.<key>` in `text`, or `None`.
fn git_value_span(text: &str, key: &str) -> Option<(usize, usize)> {
    let git_open = git_object_open(text)?;
    let needle = format!("\"{key}\"");
    let mut from = git_open;
    loop {
        let at = text[from..].find(&needle)? + from;
        let after = &text[at + needle.len()..];
        let colon = after.len() - after.trim_start().len();
        if after.trim_start().starts_with(':') {
            let value_from = at + needle.len() + colon + 1;
            let ws = text[value_from..].len() - text[value_from..].trim_start().len();
            let start = value_from + ws;
            let mut stream =
                serde_json::Deserializer::from_str(&text[start..]).into_iter::<Value>();
            stream.next()?.ok()?;
            return Some((start, start + stream.byte_offset()));
        }
        from = at + needle.len();
    }
}

/// The byte offset just after the `{` that opens the top-level `git` object.
fn git_object_open(text: &str) -> Option<usize> {
    let re = regex::Regex::new(r#""git"\s*:\s*\{"#).ok()?;
    re.find(text).map(|m| m.end())
}

/// Insert `"key": value` as the first member of the `git` object, matching
/// the file's indentation. `None` when the text has no `git` object.
fn insert_git_key(text: &str, key: &str, value: &str) -> Option<String> {
    let open = git_object_open(text)?;
    let rest = &text[open..];
    let empty = rest.trim_start().starts_with('}');
    let indent = rest.strip_prefix('\n').map_or_else(
        || "    ".to_string(),
        |r| r.chars().take_while(|c| *c == ' ' || *c == '\t').collect(),
    );
    let member = if empty {
        format!("\n{indent}\"{key}\": {value}\n  ")
    } else if rest.starts_with('\n') {
        format!("\n{indent}\"{key}\": {value},")
    } else {
        format!("\"{key}\": {value}, ")
    };
    Some(format!("{}{member}{}", &text[..open], rest))
}

/// Remove the whole line that carries `git.<key>` (with its comma), when the
/// key sits on its own line followed by another member.
fn remove_git_key_line(text: &str, key: &str) -> Option<String> {
    let (start, end) = git_value_span(text, key)?;
    let line_start = text[..start].rfind('\n')? + 1;
    let after = &text[end..];
    let comma = after.trim_start_matches([' ', '\t']);
    let comma = comma.strip_prefix(',')?;
    let line_end = comma.find('\n')?;
    let consumed = end + (after.len() - comma.len()) + line_end + 1;
    if !text[consumed..].trim_start().starts_with('"') {
        return None;
    }
    Some(format!("{}{}", &text[..line_start], &text[consumed..]))
}

/// Parse `edited` and check it equals `original` with exactly the expected
/// change to `git`; refuse the edit otherwise.
fn verified(edited: String, original: &Value, change: impl Fn(&mut Value)) -> Option<String> {
    let parsed: Value = serde_json::from_str(&edited).ok()?;
    let mut expected = original.clone();
    change(&mut expected);
    (parsed == expected).then_some(edited)
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
    let path = root.join(POLICY);
    let text = std::fs::read_to_string(&path).map_err(|e| ScaffoldError::io(&path, e))?;
    let original: Value = serde_json::from_str(&text).map_err(|e| ScaffoldError::InvalidState {
        what: POLICY.to_string(),
        detail: e.to_string(),
    })?;
    let policy: Policy = serde_json::from_str(&text).unwrap_or_default();
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
    if policy_created && has_level {
        edited =
            remove_git_key_line(&edited, "pr_sections").ok_or_else(|| edit_error("pr_sections"))?;
        expected["git"]
            .as_object_mut()
            .map(|g| g.remove("pr_sections"));
    } else if !policy_created && !has_level {
        let level = Policy::default().git.pr_sections.to_string();
        edited = insert_git_key(&edited, "pr_sections", &format!("\"{level}\""))
            .ok_or_else(|| edit_error("pr_sections"))?;
        expected["git"]["pr_sections"] = Value::String(level);
    }
    let value = mapping_json(MappingState::Diagnosed, &mapping, "none");
    edited = insert_git_key(
        &edited,
        "pr_section_mapping",
        &serde_json::to_string(&value).unwrap_or_default(),
    )
    .ok_or_else(|| edit_error("pr_section_mapping"))?;
    let edited = verified(edited, &expected, |v| {
        v["git"]["pr_section_mapping"] = value.clone();
    })
    .ok_or_else(|| edit_error("pr_section_mapping"))?;
    write_file(&path, edited.as_bytes())?;
    Ok(Diagnosis::Diagnosed { mapping, unmatched })
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
    let path = root.join(POLICY);
    let text = std::fs::read_to_string(&path).map_err(|e| ScaffoldError::io(&path, e))?;
    let original: Value = serde_json::from_str(&text).map_err(|e| ScaffoldError::InvalidState {
        what: POLICY.to_string(),
        detail: e.to_string(),
    })?;
    let policy: Policy = serde_json::from_str(&text).unwrap_or_default();
    let mapping = policy
        .git
        .pr_section_mapping
        .filter(|m| m.state == MappingState::Diagnosed && decision != MappingState::Diagnosed)
        .ok_or_else(|| ScaffoldError::InvalidState {
            what: POLICY.to_string(),
            detail: "no diagnosed git.pr_section_mapping to decide".to_string(),
        })?;
    let value = mapping_json(decision, &mapping.headings, today);
    let (start, end) = git_value_span(&text, "pr_section_mapping")
        .ok_or_else(|| edit_error("pr_section_mapping"))?;
    let edited = format!(
        "{}{}{}",
        &text[..start],
        serde_json::to_string(&value).unwrap_or_default(),
        &text[end..]
    );
    let edited = verified(edited, &original, |v| {
        v["git"]["pr_section_mapping"] = value.clone();
    })
    .ok_or_else(|| edit_error("pr_section_mapping"))?;
    write_file(&path, edited.as_bytes())?;

    let mut line =
        format!("PR template decision recorded: git.pr_section_mapping = {decision} ({today})");
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
            let tpath = root.join(&kept.path);
            let mut template =
                std::fs::read_to_string(&tpath).map_err(|e| ScaffoldError::io(&tpath, e))?;
            if !template.ends_with('\n') {
                template.push('\n');
            }
            for heading in &missing {
                template.push_str("\n## ");
                template.push_str(heading);
                template.push_str("\n\n<!-- Required by git.pr_required_sections. -->\n");
            }
            write_file(&tpath, template.as_bytes())?;
            let added = missing
                .iter()
                .map(|h| format!("## {h}"))
                .collect::<Vec<_>>()
                .join(", ");
            line = format!("{line}; appended {added} to {}", kept.path);
        }
    } else if decision == MappingState::Custom {
        line.push_str(
            "; set git.pr_required_sections and git.pr_code_sections to your template's headings in a reviewed change",
        );
    }
    Ok(line)
}

#[cfg(test)]
mod tests {
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
        assert!(find_kept(dir.path(), "shipped", &installed).is_none());
        write(
            dir.path(),
            ".github/PULL_REQUEST_TEMPLATE/feature.md",
            "## What\n",
        );
        assert_eq!(
            find_kept(dir.path(), "shipped", &installed).unwrap().path,
            ".github/PULL_REQUEST_TEMPLATE/feature.md"
        );
        write(dir.path(), "docs/PULL_REQUEST_TEMPLATE.md", "## What\n");
        assert_eq!(
            find_kept(dir.path(), "shipped", &installed).unwrap().path,
            "docs/PULL_REQUEST_TEMPLATE.md"
        );
        write(dir.path(), ".github/pull_request_template.md", "shipped");
        assert_eq!(
            find_kept(dir.path(), "shipped", &installed).unwrap().path,
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
                assert!(template.contains("\n## Release impact\n"), "{template}");
            } else {
                assert_eq!(template, PROJECT_TEMPLATE);
            }
            assert!(record_decision(dir.path(), &kept, decision, "2026-09-26").is_err());
        }
    }
}
