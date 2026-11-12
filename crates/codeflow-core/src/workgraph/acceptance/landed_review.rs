//! The reviewer a completion names when its task lands on its epic line
//! with no pull request (ADR-0102, TSK-1433). No pull request body carries a
//! Reviews row for such a landing, so the task's record alone shows the
//! review. The completion binding and the records rule already hold its
//! acceptance block to the commit the landing carries and to an approving
//! verdict; this check adds the reviewer: named first in the block's
//! `review` field, and of another lineage than the one the record's
//! `- Builder:` line names.
//!
//! [`problem`] reads only the record, so the line's push and the epic's pull
//! request into the default branch reach the same verdict for the same
//! completion. Like the rest of the acceptance check it proves what the
//! record states, not that the review took place (see [`super::SCOPE_NOTE`]).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use super::RecordView;
use crate::workgraph::record_text::AcceptanceBlock;

/// The phrase every finding of this check carries, so `codeflow ci` can give
/// it its own remedy ([`super::is_landed_review`]).
pub(super) const LANDED_REVIEW: &str = "lands on its epic line with no pull request";

/// Why the reviewer `block` names does not admit `task`'s landing on its
/// epic line, or `None` when it does: the reviewer is of a lineage the
/// record names for its builder, or names no known lineage while the record
/// names the builder's.
///
/// # Errors
/// The shipped model catalog, which names the lineages, does not parse.
pub(super) fn problem(
    task: &RecordView,
    block: &AcceptanceBlock,
) -> Result<Option<String>, String> {
    let words = lineage_words()?;
    let built_by = builder(&task.body)
        .map(|line| lineages(words, line))
        .unwrap_or_default();
    if built_by.is_empty() {
        return Ok(None);
    }
    let seat = reviewer(&block.review);
    let reviewed_by = lineages(words, seat);
    let named = |set: &BTreeSet<&str>| set.iter().copied().collect::<Vec<_>>().join(", ");
    if reviewed_by.is_empty() {
        let known: BTreeSet<&str> = words.values().map(String::as_str).collect();
        return Ok(Some(format!(
            "{}: {LANDED_REVIEW}, and its acceptance block's review names no reviewer of a known lineage ({}) before its first comma or semicolon, so it does not show a review by another lineage than the builder's ({})",
            task.id,
            named(&known),
            named(&built_by)
        )));
    }
    let shared: BTreeSet<&str> = reviewed_by.intersection(&built_by).copied().collect();
    if shared.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!(
        "{}: {LANDED_REVIEW}, and its reviewer `{}` is of the lineage the record names for its builder ({}), so the review is not by another lineage",
        task.id,
        crate::git::display_text(seat),
        named(&shared)
    )))
}

/// The reviewer a `review` field names: its text before the first comma or
/// semicolon, as the derived Reviews row reads it.
fn reviewer(review: &str) -> &str {
    review
        .split([',', ';'])
        .next()
        .unwrap_or_default()
        .trim_matches([' ', '\t', '"', '\''])
}

/// The value of the first `- Builder:` item in the record's
/// `## Execution contract` section.
fn builder(body: &str) -> Option<&str> {
    let mut in_contract = false;
    for line in body.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            in_contract = heading.trim_matches([' ', '\t']) == "Execution contract";
            continue;
        }
        if in_contract {
            if let Some(value) = line.strip_prefix("- Builder:") {
                return Some(value.trim_matches([' ', '\t']));
            }
        }
    }
    None
}

/// Each word that names exactly one lineage in the model catalog shipped
/// with this binary: family, provider and lineage names, product line ids
/// and the words of each model id and alias. The installed copy is never
/// read, so a change in the judged tree cannot rename a lineage.
fn lineage_words() -> Result<&'static BTreeMap<String, String>, String> {
    static WORDS: OnceLock<Result<BTreeMap<String, String>, String>> = OnceLock::new();
    WORDS
        .get_or_init(|| {
            let catalog = crate::model_catalog::shipped_catalog()?;
            let mut seen: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
            let mut add = |text: &str, lineage: &str| {
                for word in words(text) {
                    seen.entry(word).or_default().insert(lineage.to_string());
                }
            };
            for family in &catalog.families {
                for name in [&family.id, &family.provider, &family.lineage] {
                    add(name, &family.lineage);
                }
            }
            for line in &catalog.lines {
                let Some(family) = catalog
                    .families
                    .iter()
                    .find(|family| family.id == line.family)
                else {
                    continue;
                };
                add(&line.id, &family.lineage);
                for version in &line.versions {
                    for name in [&version.id, &version.alias, &version.pinned_id] {
                        add(name, &family.lineage);
                    }
                }
            }
            Ok(seen
                .into_iter()
                .filter_map(|(word, lineages)| {
                    let mut lineages = lineages.into_iter();
                    let lineage = lineages.next()?;
                    lineages.next().is_none().then_some((word, lineage))
                })
                .collect())
        })
        .as_ref()
        .map_err(|error| format!("the shipped model catalog does not parse: {error}"))
}

/// The lineages `text` names by any of `words`.
fn lineages<'w>(words: &'w BTreeMap<String, String>, text: &str) -> BTreeSet<&'w str> {
    self::words(text)
        .filter_map(|word| words.get(&word).map(String::as_str))
        .collect()
}

/// The lower-case letter-only words of `text` of three letters or more:
/// `GPT-6.1 Sol` gives `gpt` and `sol`.
fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_ascii_alphabetic())
        .filter(|word| word.len() >= 3)
        .map(str::to_ascii_lowercase)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workgraph::work_start::RecordKind;

    fn record(builder: Option<&str>) -> RecordView {
        let contract = builder.map_or_else(String::new, |builder| {
            format!("## Execution contract\n\n- Builder: {builder}\n\n")
        });
        let text = format!(
            "---\nid: TSK-001\nepic_id: EPC-001\nstatus: complete\n---\n\n# TSK-001: work\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall work.\n\n{contract}## Closeout\n\nDone.\n"
        );
        RecordView::parse(
            RecordKind::Task,
            "project-management/tasks/TSK-001.md",
            &text,
        )
        .unwrap()
    }

    fn block(review: &str) -> AcceptanceBlock {
        AcceptanceBlock {
            superseded: false,
            reason: None,
            reviewed: "0".repeat(40),
            review: review.into(),
            criteria: Vec::new(),
            journey: "verified | ran".into(),
            not_verified: "none".into(),
            follow_ups: "none: nothing".into(),
            verdict: "approved".into(),
        }
    }

    fn judge(builder: Option<&str>, review: &str) -> Option<String> {
        problem(&record(builder), &block(review)).unwrap()
    }

    #[test]
    fn a_reviewer_of_another_lineage_admits_the_landing() {
        for (builder, review) in [
            (
                "Claude Opus 5.5 at medium",
                "Codex GPT-6.1 Sol at medium, whole unit",
            ),
            ("Codex GPT-6 Astra at high", "Grok 4.7; whole unit"),
            ("Grok 4.7", "Claude Fable 5.1 as cf-reviewer, whole unit"),
            (
                "Codex GPT-6.1 Sol, else a Claude Opus 5.5 subagent",
                "Grok 4.7 at high, whole unit",
            ),
        ] {
            assert_eq!(judge(Some(builder), review), None, "{builder} / {review}");
        }
    }

    #[test]
    fn a_reviewer_of_the_builders_lineage_is_refused() {
        for (builder, review) in [
            ("Claude Opus 5.5 at medium", "Claude Fable 5.1, whole unit"),
            (
                "Codex GPT-6.1 Sol at medium",
                "GPT-6 Astra (gpt-6-astra, high), whole unit",
            ),
            (
                "Codex GPT-6.1 Sol, else a Claude Opus 5.5 subagent",
                "Sonnet 5.5; whole unit",
            ),
        ] {
            let found = judge(Some(builder), review).expect(review);
            assert!(
                found.contains("is of the lineage the record names for its builder"),
                "{found}"
            );
            assert!(found.contains(LANDED_REVIEW), "{found}");
        }
    }

    #[test]
    fn a_review_that_names_no_known_reviewer_is_refused() {
        let found =
            judge(Some("Claude Opus 5.5"), "https://example.test/pr/1#review").expect("refused");
        assert!(
            found.contains("names no reviewer of a known lineage (claude, codex, grok)"),
            "{found}"
        );
        // A lineage named after the first comma does not name the reviewer.
        assert!(judge(Some("Claude Opus 5.5"), "Reduced assurance, Codex was out").is_some());
    }

    #[test]
    fn the_rule_holds_a_record_to_the_builder_lineage_it_names() {
        assert_eq!(judge(Some("anyone."), "Claude Fable 5.1"), None);
        assert_eq!(judge(None, "Claude Fable 5.1"), None);
    }

    #[test]
    fn the_builder_is_read_only_from_the_execution_contract() {
        assert_eq!(builder("## Description\n\n- Builder: Claude\n"), None);
        assert_eq!(
            builder(
                "## Execution contract\n\n- Source: x\n- Builder: Codex GPT-6.1 Sol.\n\n## Closeout\n- Builder: Claude\n"
            ),
            Some("Codex GPT-6.1 Sol.")
        );
    }

    #[test]
    fn the_shipped_catalog_names_each_lineage() {
        let words = lineage_words().unwrap();
        for (word, lineage) in [
            ("claude", "claude"),
            ("anthropic", "claude"),
            ("opus", "claude"),
            ("fable", "claude"),
            ("sonnet", "claude"),
            ("codex", "codex"),
            ("openai", "codex"),
            ("gpt", "codex"),
            ("astra", "codex"),
            ("sol", "codex"),
            ("grok", "grok"),
            ("xai", "grok"),
        ] {
            assert_eq!(words.get(word).map(String::as_str), Some(lineage), "{word}");
        }
    }
}
