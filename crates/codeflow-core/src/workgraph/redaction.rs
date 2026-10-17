//! Record-only redactions preserve acceptance results and review provenance.
use git2::{Oid, Repository};
use serde::Deserialize;

use super::lifecycle::RecordView;
use super::record_text::{acceptance_blocks, outcome_word, scan, section_span};
use super::work_start::RecordKind;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Redaction {
    pub source: String,
    pub sha256: String,
    pub review: String,
}

pub(crate) fn annotation(record: &RecordView) -> Result<Option<Redaction>, String> {
    let (mut fields, _) = crate::validate::parse_frontmatter(record.content.as_bytes())
        .map_err(|error| error.to_string())?;
    fields
        .remove("redaction")
        .map(serde_yaml::from_value)
        .transpose()
        .map_err(|error| format!("redaction: {error}"))
}

/// Compare the replaced record with its redacted copy. This proves structure,
/// never that the replacement words mean the same thing.
pub(crate) fn preserves(old: &RecordView, new: &RecordView) -> Result<(), String> {
    let note = annotation(new)?.ok_or("missing redaction source, sha256 and review")?;
    if note.review.trim().is_empty() || note.review.trim() == "none" {
        return Err("redaction review is empty".into());
    }
    if note.sha256 != crate::scaffold::sha256_hex(old.content.as_bytes()) {
        return Err("redaction sha256 does not match the original record".into());
    }
    if old.kind != RecordKind::Task || old.path != new.path || old.status != new.status {
        return Err("redaction changes the record path, kind or status".into());
    }
    let fields = |record: &RecordView| -> Result<_, String> {
        let (mut fields, _) = crate::validate::parse_frontmatter(record.content.as_bytes())
            .map_err(|error| error.to_string())?;
        fields.remove("redaction");
        Ok(fields)
    };
    if fields(old)? != fields(new)? {
        return Err("redaction changes frontmatter outside redaction".into());
    }
    if let Some(problem) =
        super::acceptance::correction_problem(&old.criteria.items, &new.criteria.items)
    {
        return Err(problem);
    }
    if old.criteria.errors != new.criteria.errors || !new.criteria.errors.is_empty() {
        return Err("redaction changes the criteria structure".into());
    }
    let before = acceptance_blocks(&old.body);
    let after = acceptance_blocks(&new.body);
    if before.is_empty() || before.len() != after.len() {
        return Err("redaction changes the acceptance block count".into());
    }
    for (old, new) in before.iter().zip(&after) {
        let old = old.parsed.as_ref().map_err(ToString::to_string)?;
        let new = new.parsed.as_ref().map_err(ToString::to_string)?;
        if old.superseded != new.superseded {
            return Err("redaction changes acceptance or acceptance_superseded status".into());
        }
        if old.reviewed != new.reviewed {
            return Err("redaction changes reviewed".into());
        }
        if old.review != new.review {
            return Err("redaction changes review provenance".into());
        }
        if old.verdict != new.verdict {
            return Err("redaction changes verdict".into());
        }
        if outcome_word(&old.journey) != outcome_word(&new.journey) {
            return Err("redaction changes the journey result".into());
        }
        if old.criteria.len() != new.criteria.len() {
            return Err("redaction changes criterion results".into());
        }
        for ((id, result), (next_id, next)) in old.criteria.iter().zip(&new.criteria) {
            if id != next_id || result.outcome != next.outcome {
                return Err(format!("redaction changes {id} result"));
            }
            if result.outcome == "waived" && result.evidence != next.evidence {
                return Err(format!("redaction changes {id} waiver amendment"));
            }
        }
    }
    // Restore the allowed text regions before comparing everything else.
    let mut restored = new.body.clone();
    let lines: Vec<_> = restored.lines().collect();
    let scanned = scan(&lines);
    let (start, end) = section_span(&scanned, "## Closeout").ok_or("missing Closeout")?;
    let mut closeout = lines[start..end].join("\n");
    for (old, new) in before.iter().zip(&after) {
        closeout = closeout.replacen(&new.inner, &old.inner, 1);
    }
    restored = lines[..start]
        .iter()
        .copied()
        .chain(std::iter::once(closeout.as_str()))
        .chain(lines[end..].iter().copied())
        .collect::<Vec<_>>()
        .join("\n");
    let without_criteria = |body: &str| {
        let lines: Vec<_> = body.lines().collect();
        let scanned = scan(&lines);
        let section = section_span(&scanned, "## Acceptance Criteria");
        lines
            .iter()
            .enumerate()
            .filter(|(i, _)| !section.is_some_and(|(a, b)| (a..b).contains(i)))
            .map(|(_, line)| *line)
            .collect::<Vec<_>>()
            .join("\n")
    };
    if without_criteria(&old.body) != without_criteria(&restored) {
        return Err("redaction changes text outside acceptance blocks and criteria".into());
    }
    Ok(())
}

/// A changed redaction is judged against the source record and the entire
/// proposed range. The source must already be on the target at the branch point.
pub(crate) fn check(
    repo: &Repository,
    anchor: Oid,
    head: Oid,
    old: Option<&RecordView>,
    new: &RecordView,
) -> Result<bool, String> {
    let Some(note) = annotation(new)? else {
        if old.map(annotation).transpose()?.flatten().is_some() {
            return Err("redaction removes the original digest and review".into());
        }
        return Ok(false);
    };
    let Some(old) = old else {
        return Err("redaction has no original record on the target".into());
    };
    if old.content == new.content {
        return Ok(false);
    }
    // An existing annotation does not turn later implementation into a redaction.
    if annotation(old)?.is_some_and(|prior| {
        prior.source == note.source && prior.sha256 == note.sha256 && prior.review == note.review
    }) {
        return Ok(false);
    }
    let source = super::acceptance::commit_of(repo, &note.source)
        .ok_or("redaction source is not a commit here")?;
    if !super::record_text::is_full_sha(&note.source)
        || !super::acceptance::is_ancestor_or_same(repo, source, anchor)?
    {
        return Err(
            "redaction source must be a full commit on the target before this range".into(),
        );
    }
    let original = super::acceptance::blob_at(repo, source, &new.path)?
        .ok_or("redaction source has no original record")?;
    if original != old.content {
        return Err("redaction source is not the target's current record".into());
    }
    let paths =
        super::lifecycle::changed_paths(repo, &anchor.to_string(), Some(&head.to_string()))?;
    if let Some(path) = paths.iter().find(|path| *path != &new.path) {
        return Err(format!("redaction changes {path} outside the record"));
    }
    if let Some(problem) = super::amendment::range_problem(repo, anchor, head, &paths) {
        return Err(problem);
    }
    preserves(old, new)?;
    Ok(true)
}

/// Keep the accepted record ids together with the findings that justify them.
pub(crate) fn judge_range(
    repo: &Repository,
    anchor: Oid,
    head: Oid,
    before: &super::lifecycle::Graph,
    after: &super::lifecycle::Graph,
    carried: bool,
) -> (
    std::collections::BTreeSet<String>,
    Vec<super::acceptance::Finding>,
) {
    use super::acceptance::{finding, Finding, BINDING_RULE};
    let mut accepted = std::collections::BTreeSet::new();
    let mut findings = Vec::new();
    for task in after
        .records
        .values()
        .filter(|r| r.kind == RecordKind::Task)
    {
        let result = match carried.then(|| check_landed(repo, head, task)) {
            Some(Ok(Some(accepted))) => Ok(accepted),
            Some(Err(problem)) => Err(problem),
            _ => check(repo, anchor, head, before.records.get(&task.id), task),
        };
        match result {
            Ok(true) => {
                accepted.insert(task.id.clone());
                findings.push(Finding { rule: BINDING_RULE, message: format!("{}: record-only redaction preserves results, reviewed and review provenance; review must confirm unchanged meaning", task.id), note: true, epic_record: None });
            }
            Err(problem) => findings.push(finding(BINDING_RULE, format!("{}: {problem}", task.id))),
            Ok(false) => {}
        }
    }
    (accepted, findings)
}

/// A verified line carries the original record-only redaction landing.
/// Later unrelated line work is outside that landing's range. `Some(false)`
/// is a later landing that kept the annotation but changed the record; it
/// receives ordinary completion checks, not the redaction exception.
pub(crate) fn check_landed(
    repo: &Repository,
    head: Oid,
    new: &RecordView,
) -> Result<Option<bool>, String> {
    if annotation(new)?.is_none() {
        return Ok(None);
    }
    let mut introduced = head;
    loop {
        let commit = repo.find_commit(introduced).map_err(|e| e.to_string())?;
        let mut preceding = None;
        for parent in super::history::parents(repo, &commit).map_err(|e| e.to_string())? {
            if super::acceptance::blob_at(repo, parent, &new.path)?.as_deref()
                == Some(new.content.as_str())
            {
                preceding = Some(parent);
                break;
            }
        }
        match preceding {
            Some(parent) => introduced = parent,
            None => break,
        }
    }
    let Some(base) = super::acceptance::source_landing_base(repo, head, introduced)? else {
        return Ok(None);
    };
    let old = super::acceptance::blob_at(repo, base, &new.path)?
        .map(|text| RecordView::parse(RecordKind::Task, &new.path, &text))
        .transpose()?;
    check(repo, base, introduced, old.as_ref(), new).map(Some)
}

/// Recover a prior completion through redactions already landed on the target.
/// A future fix keeps the redacted archive rather than reintroducing its text.
pub(crate) fn retained_completion(
    repo: &Repository,
    anchor: Oid,
    old: &RecordView,
) -> Result<Option<RecordView>, String> {
    use super::record_text::render_acceptance;
    let Some(active) = old.active_blocks().into_iter().next() else {
        return Ok(None);
    };
    let original_block = active.parsed.as_ref().map_err(ToString::to_string)?;
    let Some(text) = super::acceptance::blob_at(repo, anchor, &old.path)? else {
        return Ok(None);
    };
    let latest = RecordView::parse(RecordKind::Task, &old.path, &text)?;
    let mut current = latest.clone();
    let mut at = anchor;
    let mut validated = false;
    loop {
        for (index, archived) in current.superseded_blocks().iter().enumerate() {
            let mut block = archived.parsed.clone().map_err(|e| e.to_string())?;
            block.superseded = false;
            block.reason = None;
            if validated && &block == original_block {
                let mut redacted = latest.superseded_blocks()[index]
                    .parsed
                    .clone()
                    .map_err(|e| e.to_string())?;
                redacted.superseded = false;
                redacted.reason = None;
                let content = old.content.replacen(
                    &active.inner,
                    render_acceptance(&redacted).trim_end_matches('\n'),
                    1,
                );
                let mut retained = RecordView::parse(RecordKind::Task, &old.path, &content)?;
                retained.criteria = old.criteria.clone();
                return Ok(Some(retained));
            }
        }
        let Some(note) = annotation(&current)? else {
            return Ok(None);
        };
        if check_landed(repo, at, &current)? != Some(true) {
            return Ok(None);
        }
        let source = super::acceptance::commit_of(repo, &note.source)
            .ok_or("redaction source does not resolve")?;
        if source == at {
            return Err("redaction source cannot be its own revision".into());
        }
        let text = super::acceptance::blob_at(repo, source, &old.path)?
            .ok_or("redaction source record is missing")?;
        current = RecordView::parse(RecordKind::Task, &old.path, &text)?;
        at = source;
        validated = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(text: &str) -> RecordView {
        RecordView::parse(
            RecordKind::Task,
            "project-management/tasks/TSK-001.md",
            text,
        )
        .unwrap()
    }

    fn original() -> String {
        format!("---\nid: TSK-001\nstatus: complete\n---\n\n## Acceptance Criteria\n- AC-1 (journey) Serve customer-name.\n\n## Closeout\n\n```yaml\nacceptance:\n  reviewed: {}\n  review: https://example.test/review\n  criteria:\n    AC-1: verified | customer-name test\n  journey: verified | customer-name journey\n  not_verified: none\n  follow_ups: none: complete\n  verdict: approved\n```\n", "a".repeat(40))
    }

    fn replacement(old: &str) -> String {
        old.replace("customer-name", "customer").replacen("---\n", &format!("---\nredaction: {{source: {}, sha256: {}, review: 'https://example.test/redaction'}}\n", "b".repeat(40), crate::scaffold::sha256_hex(old.as_bytes())), 1)
    }

    #[test]
    fn redaction_preserves_every_structural_result() {
        let old = original();
        let new = replacement(&old);
        assert!(preserves(&record(&old), &record(&new)).is_ok());
        for (from, to, message) in [
            ("AC-1 (journey)", "AC-2 (journey)", "criteria set"),
            ("(journey)", "(after release)", "tags"),
            ("status: complete", "status: todo", "status"),
            ("AC-1: verified", "AC-1: failed", "AC-1 result"),
            (
                "review: https://example.test/review",
                "review: changed",
                "review provenance",
            ),
            ("not_verified: none", "not_verified: omitted text", ""),
        ] {
            let result = preserves(&record(&old), &record(&new.replace(from, to)));
            if message.is_empty() {
                assert!(result.is_ok());
            } else {
                assert!(result.unwrap_err().contains(message));
            }
        }
    }

    #[test]
    fn redaction_keeps_waiver_commits_and_unrelated_prose() {
        let old = original().replace(
            "verified | customer-name test",
            &format!("waived | {}", "c".repeat(40)),
        );
        let new = replacement(&old);
        assert!(preserves(&record(&old), &record(&new)).is_ok());
        let changed = new.replace(&"c".repeat(40), &"d".repeat(40));
        assert!(preserves(&record(&old), &record(&changed))
            .unwrap_err()
            .contains("waiver amendment"));
        let changed = format!("{new}\n## Unrelated\nextra\n");
        assert!(preserves(&record(&old), &record(&changed))
            .unwrap_err()
            .contains("outside acceptance"));
    }

    #[test]
    fn binding_remedy_points_to_the_waiver_format() {
        assert!(crate::remedy::ACCEPTANCE_BINDING
            .with(&[("note", super::super::acceptance::SCOPE_NOTE)])
            .to_string()
            .contains("project-organization.md#acceptance-waivers-and-redactions"));
    }
}
