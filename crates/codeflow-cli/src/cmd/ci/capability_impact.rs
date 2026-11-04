//! Catalog agreement at immutable revisions, without checkout input.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use codeflow_core::capability::{
    load_capability_catalog_from_tree, CapabilityCatalog, CapabilityRecord,
};
use codeflow_core::hooks::{GitPolicy, Violation};
use codeflow_core::remedy::PR_CAPABILITY_IMPACT;
use codeflow_core::workgraph::classify::path_sets;

const RULE: &str = "git.pr_capability_impact";

#[derive(Default)]
pub(super) struct Report {
    pub findings: Vec<Violation>,
    pub notes: Vec<String>,
    pub ran: bool,
}

#[allow(clippy::too_many_arguments)] // CI has already resolved this context.
pub(super) fn evaluate(
    root: &Path,
    policy: &GitPolicy,
    raw: &Result<Option<serde_json::Value>, String>,
    authority: Option<&str>,
    base: Option<&str>,
    head: &str,
    body: Option<&str>,
    inventory: Option<&[super::change_class::RangeEntry]>,
    local_push: bool,
    hosted: bool,
) -> Report {
    let mut report = Report::default();
    if local_push && !hosted {
        report.notes.push(
            "capability impact: local push is not judged; deferred to hosted actual-target CI"
                .into(),
        );
        return report;
    }
    if let Err(error) = raw {
        report.ran = true;
        report.findings.push(Violation::always_blocking(
            RULE,
            format!("cannot read capability policy provenance: {error}"),
            PR_CAPABILITY_IMPACT.text,
        ));
        return report;
    }
    let configured = raw
        .as_ref()
        .ok()
        .and_then(Option::as_ref)
        .and_then(|raw| raw.pointer("/git/pr_capability_impact"));
    if configured.is_none() {
        report.notes.push(format!(
            "{RULE} is absent at the policy authority; not checked"
        ));
        return report;
    }
    let level = policy.pr_capability_impact;
    if !level.is_active() {
        report.notes.push(format!("{RULE} is {level}; not checked"));
        return report;
    }
    report.ran = true;
    let started = std::time::Instant::now();
    let result = if local_push && hosted {
        Err("a hosted pull request cannot use a local push advertisement".into())
    } else {
        check(root, policy, authority, base, head, body, inventory)
    };
    match result {
        Ok(note) => report.notes.push(note),
        Err(message) => report.findings.push(Violation::new(
            RULE,
            level,
            message,
            PR_CAPABILITY_IMPACT.remedy(),
        )),
    }
    report.notes.push(format!(
        "{RULE}: {} at {level} in {:.3}s",
        if report.findings.is_empty() {
            "verified"
        } else {
            "not verified"
        },
        started.elapsed().as_secs_f64()
    ));
    report
}

#[allow(clippy::too_many_arguments)]
fn check(
    root: &Path,
    policy: &GitPolicy,
    authority: Option<&str>,
    base: Option<&str>,
    head: &str,
    body: Option<&str>,
    inventory: Option<&[super::change_class::RangeEntry]>,
) -> Result<String, String> {
    if body.is_some_and(|body| body.len() > super::pr_body::CAPABILITY_MAX_BODY_BYTES) {
        return Err(format!(
            "capability declaration body exceeds {} bytes",
            super::pr_body::CAPABILITY_MAX_BODY_BYTES
        ));
    }
    let authority =
        authority.ok_or("cannot verify capability impact without a pinned policy authority")?;
    let base = base.ok_or("cannot verify capability impact without a pinned target")?;
    let context = codeflow_core::capability::impact::context(root, authority, base, head, policy)?;
    let baseline = context.merge_base;
    // Reuse the body classifier's exact range. Without a body it has not run;
    // only then read the inventory to decide the light-documentation exemption.
    let owned_inventory;
    let inventory = if let Some(inventory) = inventory {
        inventory
    } else {
        owned_inventory = super::change_class::range_inventory(root, base, head)?;
        &owned_inventory
    };
    let paths = context.paths;
    let catalog_path = inventory.iter().any(|entry| {
        entry.path.rule_text().is_ok_and(|path| {
            path == "docs/capabilities.md" || path.starts_with("docs/capabilities/")
        })
    });
    let behaviour = inventory.iter().any(|entry| {
        entry
            .path
            .rule_text()
            .map_or(true, |path| path_sets().is_behaviour(path))
    });
    let light = super::change_class::classify(Some(inventory), &paths).light;
    if light
        && !catalog_path
        && !behaviour
        && body.is_none_or(|body| {
            matches!(
                super::find_section(body, "Capability impact"),
                super::SectionState::Missing
            )
        })
    {
        return Ok("capability impact: light documentation range; assessment not required".into());
    }
    if body.is_none() {
        return Err(
            "PR body unavailable; capability impact cannot be verified; supply CODEFLOW_PR_BODY"
                .into(),
        );
    }
    let load = |sha: &str| {
        load_capability_catalog_from_tree(root, sha).map_err(|e| format!("catalog at {sha}: {e}"))
    };
    let mut before = load(&baseline)?;
    let after = if head == baseline {
        before.clone()
    } else {
        load(head)?
    };
    let registry = join_legacy_identities(root, &mut before, &after)?;
    if after.records.is_empty() {
        // A real removal retains its identity-specific refusal. Otherwise
        // authority presence is needed only to distinguish bootstrap from
        // a branch that forked before the authority gained its catalog.
        delta(&before, &after)?;
        let authority_empty = if authority == baseline {
            before.records.is_empty()
        } else if authority == head {
            true
        } else {
            load(authority)?.records.is_empty()
        };
        return Err(if authority_empty {
            "catalog is empty at the authority and head; bootstrap the catalog with valid records and matching Added declarations"
        } else {
            "the merge base predates the authority's catalog; merge the target into this branch before declaring capability impact"
        }.into());
    }
    agreement(&before, &after, body)?;
    Ok(format!(
        "capability impact: authority {authority}, merge-base {baseline}, head {head}; registry {}; declaration agrees with catalog delta",
        registry.as_deref().unwrap_or("not needed")
    ))
}

/// Only the registry can connect a UID-less legacy entry to a typed record.
fn join_legacy_identities(
    root: &Path,
    before: &mut CapabilityCatalog,
    after: &CapabilityCatalog,
) -> Result<Option<String>, String> {
    let mut pending = Vec::new();
    for (index, previous) in before.records.iter().enumerate() {
        if previous.uid.is_some() {
            continue;
        }
        // Unconverted legacy entries keep their existing identity.
        if after
            .records
            .iter()
            .any(|record| record.uid.is_none() && record.entry.id == previous.entry.id)
        {
            continue;
        }
        let candidates: Vec<_> = after
            .records
            .iter()
            .filter(|record| {
                record.uid.is_some()
                    && (record.entry.id == previous.entry.id
                        || record.former_ids.contains(&previous.entry.id))
            })
            .collect();
        if candidates.is_empty() {
            return Err(format!(
                "{} identity removed or replaced; deprecate it instead of deleting it",
                previous.entry.id
            ));
        }
        if candidates.len() > 1 {
            return Err(format!(
                "{} migration is ambiguous: several records claim its legacy identity",
                previous.entry.id
            ));
        }
        // Retarget preserves UID but binds the new number; former_ids supplies
        // the old-to-current ID edge, never a substitute for the UID binding.
        let number = codeflow_core::ids::RegId::parse(&candidates[0].entry.id)
            .and_then(|id| id.seq())
            .ok_or_else(|| format!("invalid capability id {}", candidates[0].entry.id))?;
        pending.push((index, number, candidates[0]));
    }
    if pending.is_empty() {
        return Ok(None);
    }
    let numbers: Vec<_> = pending.iter().map(|(_, number, _)| *number).collect();
    let (sha, bindings) = codeflow_core::capability::impact::registry_bindings(root, &numbers)?;
    let mut identities: BTreeSet<_> = before
        .records
        .iter()
        .filter_map(|record| record.uid.clone())
        .collect();
    let mut joins = Vec::new();
    for ((index, _, candidate), uid) in pending.into_iter().zip(bindings) {
        if candidate.uid.as_ref() != Some(&uid) {
            return Err(format!(
                "{} migration UID disagrees with registry at {sha}",
                before.records[index].entry.id
            ));
        }
        if !identities.insert(uid.clone()) {
            return Err(format!("duplicate migration UID {uid} at registry {sha}"));
        }
        joins.push(format!(
            "join {} -> {} uid {uid} registry {sha}",
            before.records[index].entry.id, candidate.entry.id
        ));
        before.records[index].uid = Some(uid);
    }
    Ok(Some(format!("{sha}; {}", joins.join("; "))))
}

fn normalize(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn strings(values: &mut [String]) {
    for value in values {
        *value = normalize(value);
    }
}
fn optional(value: &mut Option<String>) {
    if let Some(value) = value {
        *value = normalize(value);
    }
}

/// Parsed fields erase key order. Keep list order; erase source location.
fn frontmatter(record: &CapabilityRecord) -> CapabilityRecord {
    let mut record = record.clone();
    record.file.clear();
    record.entry.line = 0;
    record.prose.clear();
    for value in [
        &mut record.entry.id,
        &mut record.entry.name,
        &mut record.entry.area,
        &mut record.entry.status,
    ] {
        *value = normalize(value);
    }
    for values in [
        &mut record.entry.verified_by,
        &mut record.entry.epics,
        &mut record.entry.epic_id,
        &mut record.entry.specs,
        &mut record.entry.depends_on,
        &mut record.entry.capabilities,
        &mut record.entry.adrs,
        &mut record.entry.related,
        &mut record.entry.superseded_by,
        &mut record.audience,
        &mut record.tags,
        &mut record.former_ids,
    ] {
        strings(values);
    }
    for value in [
        &mut record.uid,
        &mut record.summary,
        &mut record.stage,
        &mut record.kind,
        &mut record.quality,
        &mut record.measure,
        &mut record.group,
        &mut record.since,
    ] {
        optional(value);
    }
    record
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Change {
    Added,
    Changed,
    Prose,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verb {
    Added,
    Changed,
    Improved,
}

fn identity(record: &CapabilityRecord) -> String {
    record.uid.as_ref().map_or_else(
        || format!("legacy:{}", record.entry.id),
        |uid| format!("uid:{uid}"),
    )
}

fn delta(
    before: &CapabilityCatalog,
    after: &CapabilityCatalog,
) -> Result<BTreeMap<String, Change>, String> {
    let old: BTreeMap<_, _> = before.records.iter().map(|r| (identity(r), r)).collect();
    let new: BTreeMap<_, _> = after.records.iter().map(|r| (identity(r), r)).collect();
    let mut changes = BTreeMap::new();
    for (identity, record) in &old {
        if !new.contains_key(identity) {
            return Err(format!(
                "{} identity removed or replaced; deprecate it instead of deleting it",
                record.entry.id
            ));
        }
    }
    for (identity, record) in new {
        let change = match old.get(&identity) {
            None => Some(Change::Added),
            Some(previous) if frontmatter(previous) != frontmatter(record) => Some(Change::Changed),
            Some(previous) if normalize(&previous.prose) != normalize(&record.prose) => {
                Some(Change::Prose)
            }
            Some(_) => None,
        };
        if let Some(change) = change {
            changes.insert(record.entry.id.clone(), change);
        }
    }
    Ok(changes)
}

fn agreement(
    before: &CapabilityCatalog,
    after: &CapabilityCatalog,
    body: Option<&str>,
) -> Result<(), String> {
    let changes = delta(before, after)?;
    let body = body.ok_or("PR body unavailable; capability impact cannot be verified")?;
    let lines = super::pr_body::capability_lines(body)?;
    if lines.is_empty() {
        return Err("Capability impact section is empty".into());
    }
    let records: BTreeMap<_, _> = after
        .records
        .iter()
        .map(|r| (r.entry.id.as_str(), r))
        .collect();
    let mut declared = BTreeSet::new();
    let mut none = false;
    for line in &lines {
        let (label, context) = line.split_once(':').ok_or_else(|| {
            format!("invalid capability line '{line}': expected verb, id, name and context")
        })?;
        if label.eq_ignore_ascii_case("none") {
            if none || super::pr_body::placeholder(context) {
                return Err(format!(
                    "invalid capability line '{line}': None needs one substantive reason"
                ));
            }
            none = true;
            continue;
        }
        let (verb, rest) = label
            .split_once(' ')
            .ok_or_else(|| format!("invalid capability line '{line}': missing id and name"))?;
        let verb = match verb.to_ascii_lowercase().as_str() {
            "added" => Verb::Added,
            "changed" => Verb::Changed,
            "improved" => Verb::Improved,
            _ => {
                return Err(format!(
                    "invalid capability line '{line}': expected Added, Changed, Improved or None"
                ))
            }
        };
        let (id, name) = rest
            .trim()
            .split_once(' ')
            .ok_or_else(|| format!("invalid capability line '{line}': missing capability name"))?;
        let record = records
            .get(id)
            .ok_or_else(|| format!("invalid capability line '{line}': unknown id {id}"))?;
        if normalize(name) != normalize(&record.entry.name) || super::pr_body::placeholder(context)
        {
            return Err(format!("invalid capability line '{line}': supply the catalog name and substantive user context"));
        }
        if !declared.insert(id.to_string()) {
            return Err(format!(
                "duplicate capability declaration for {id}: '{line}'"
            ));
        }
        let actual = changes.get(id);
        let matches = match verb {
            Verb::Added => actual == Some(&Change::Added),
            Verb::Changed => matches!(actual, Some(Change::Changed | Change::Prose)),
            Verb::Improved => {
                matches!(actual, None | Some(Change::Prose))
                    && context.to_ascii_lowercase().contains("entry unchanged")
                    && !super::pr_body::placeholder(
                        &context.to_ascii_lowercase().replace("entry unchanged", ""),
                    )
            }
        };
        if !matches {
            return Err(format!("capability line '{line}' disagrees with the net catalog delta; Improved requires unchanged frontmatter and an 'entry unchanged' clause"));
        }
    }
    if none && (!changes.is_empty() || !declared.is_empty()) {
        return Err("None cannot accompany capability changes or other declarations".into());
    }
    for id in changes.keys() {
        if !declared.contains(id) {
            return Err(format!("catalog delta for {id} is undeclared"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeflow_core::hooks::PolicyLevel;

    #[test]
    fn capability_push_deferral_cannot_replace_hosted_admission() {
        let policy = GitPolicy {
            pr_capability_impact: PolicyLevel::Block,
            ..GitPolicy::default()
        };
        let raw = Ok(Some(
            serde_json::json!({"git":{"pr_capability_impact":"block"}}),
        ));
        let local = evaluate(
            Path::new("missing"),
            &policy,
            &raw,
            None,
            None,
            "",
            None,
            None,
            true,
            false,
        );
        assert!(!local.ran && local.findings.is_empty());
        let hosted = evaluate(
            Path::new("missing"),
            &policy,
            &raw,
            None,
            None,
            "",
            None,
            None,
            true,
            true,
        );
        assert!(hosted.ran && !hosted.findings.is_empty());
    }

    #[test]
    fn capability_delta_ignores_source_filename() {
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        for (path, content) in [
            (".codeflow/project.toml", "areas = ['engine']\n"),
            ("docs/capabilities/vocabulary.yaml", "inventory: incomplete\ngroups: [{name: records, about: Records}]\ntags: [{name: rust, about: Rust}]\n"),
            ("docs/capabilities/CAP-101-catalog.md", include_str!("../../../../codeflow-core/tests/fixtures/capability-catalog/valid.md")),
        ] {
            let path = dir.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        let before = codeflow_core::capability::load_capability_catalog(dir.path()).unwrap();
        let mut after = before.clone();
        // Admission separately requires filename/id/name agreement. This
        // tests the delta's independence from source location alone.
        after.records[0].file = "docs/capabilities/CAP-101-other.md".into();
        assert!(delta(&before, &after).unwrap().is_empty());
    }

    /// Read-only measurement, never part of the routine CI suite. The policy
    /// is an in-memory test input; no repository policy or ref is changed.
    #[test]
    #[ignore = "timing probe requires the pinned EPC-026 repository history"]
    fn capability_repository_catalog_timing() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let base = "6d9473e4b3ec2619b5b9dfb3b8fdb57220729542";
        let head = "99644cf16e157eb33a887f18be3943b383657eb1";
        let policy = GitPolicy {
            pr_capability_impact: PolicyLevel::Block,
            ..GitPolicy::default()
        };
        let raw = Ok(Some(
            serde_json::json!({"git":{"pr_capability_impact":"block"}}),
        ));
        for _ in 0..3 {
            let report = evaluate(
                &root, &policy, &raw, Some(base), Some(base), head,
                Some("## Capability impact\n\n- None: The schema reference is outside the legacy capability entries.\n"),
                None, false, false,
            );
            println!("{}", report.notes.join("\n"));
            assert!(
                report.ran && report.findings.is_empty(),
                "{:?}",
                report.findings
            );
        }
    }
}
