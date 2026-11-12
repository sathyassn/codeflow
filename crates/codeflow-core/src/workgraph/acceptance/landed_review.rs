//! The lineages a completion records when its task lands on a marked epic's
//! line with no pull request (ADR-0102, TSK-1433).
//!
//! An epic whose record on the default branch sets `main_landing:
//! whole-epic` lands its tasks on its line with no pull request, so no pull
//! request body carries a Reviews row for them, and the task's record alone
//! shows the review. The completion binding and the records rule already
//! hold the acceptance block to the commit the landing carries and to an
//! approving verdict. On such a line this check adds the lineages:
//!
//! - the block names the one lineage that built the task in `built_by`;
//! - the reviewer named first in `review` is of another lineage;
//! - or, when it is of the same lineage, the block carries the recorded
//!   `reduced_assurance` statement in its fixed form: items separated by
//!   `;`, each exactly `<lineage id> probed unavailable` with a lineage id
//!   of `claude`, `codex` or `grok`, one item for each lineage other than
//!   the builder's and none for the builder's, for example
//!   `codex probed unavailable; grok probed unavailable`.
//!
//! What it is and is not: all three fields are written by the agents under
//! review, so the check catches a mistake or drift between what a
//! completion says about its builder and its reviewer, and the fixed form
//! makes a careless, mistaken or contradicted reduced-assurance statement
//! refuse, such as one that only names the other lineages, says one is
//! available, leaves out the probe or adds words such as `not probed`. It
//! requires the assertion and does not catch a forged one: it does not show
//! that any lineage was probed, and it is no proof that the review was
//! independent or took place (see
//! [`super::SCOPE_NOTE`]). It reads only the completion in the
//! tree: never a verdict file and never the path the `review` field cites.
//! Checking a completion against its verdict file is the adopter's own
//! landing process.
//!
//! The marker is read once per run, only from the epic record at the
//! default branch's commit the run is judged against (ADR-0091), never from
//! the line, a pull request head or the working tree. On every other line,
//! and on a line whose epic is unmarked, the check is inert. [`problem`]
//! reads only the record, so the line's push and the epic's pull request
//! into the default branch reach the same verdict for the same completion.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use git2::{Oid, Repository};

use super::{Graph, RecordKind, RecordView};
use crate::workgraph::record_text::AcceptanceBlock;

/// The phrase every lineage finding of this check carries, so `codeflow ci`
/// can give it its own remedy ([`super::is_landed_review`]).
pub(super) const LANDED_REVIEW: &str = "lands on its marked epic's line with no pull request";

/// The phrase a marker this run cannot read carries, so `codeflow ci` can
/// give it its own remedy ([`super::is_landing_marker`]).
pub(super) const LANDING_MARKER: &str = "cannot read its epic's `main_landing` marker";

/// The epic record field that marks an epic whose tasks land on its line
/// with no pull request (ADR-0102).
const MAIN_LANDING: &str = "main_landing";

/// The marker value whose epic lands its tasks with no pull request.
const WHOLE_EPIC: &str = "whole-epic";

/// The marker value of an epic that lands through checkpoint pull requests,
/// which an absent field also means.
const CHECKPOINT_PRS: &str = "checkpoint-prs";

/// Whether the epic whose line `line` is marks itself `whole-epic` in its
/// record at the default branch's commit: `at`, the commit the run names as
/// its policy authority (`--policy-from`, which the pre-push hook and the
/// hosted workflow set to the default branch's tip), else the tip of the
/// default branch this clone resolves. An epic record that commit does not
/// hold, or one with no `main_landing` field, is unmarked.
///
/// # Errors
/// The refusal, naming the field and the values it accepts, when the
/// default branch's commit or the epic record there cannot be read, or the
/// field holds a value other than `whole-epic` or `checkpoint-prs`.
pub(super) fn marked(repo: &Repository, line: &str, at: Option<Oid>) -> Result<bool, String> {
    let refuse = |reason: String| {
        format!(
            "{line} {LANDING_MARKER}: {reason}; the field is read only from the epic record at the default branch's commit, and accepts `{WHOLE_EPIC}` or `{CHECKPOINT_PRS}` (an absent field means `{CHECKPOINT_PRS}`)"
        )
    };
    let epic = crate::workgraph::line_adoption::epic_id(line).map_err(&refuse)?;
    let tip = match at {
        Some(tip) => tip,
        None => default_tip(repo).map_err(&refuse)?,
    };
    let graph = Graph::from_revision(repo, &tip.to_string())
        .map_err(|error| refuse(format!("the records at {tip} cannot be read: {error}")))?;
    let Some(record) = graph
        .records
        .get(epic)
        .filter(|record| record.kind == RecordKind::Epic)
    else {
        return Ok(false);
    };
    let (fields, _) = crate::validate::parse_frontmatter(record.content.as_bytes())
        .map_err(|error| refuse(format!("{} at {tip}: {error}", record.path)))?;
    match fields.get(MAIN_LANDING) {
        None => Ok(false),
        Some(serde_yaml::Value::String(value)) if value == WHOLE_EPIC => Ok(true),
        Some(serde_yaml::Value::String(value)) if value == CHECKPOINT_PRS => Ok(false),
        Some(value) => Err(refuse(format!(
            "{} at {tip} sets `{MAIN_LANDING}` to `{}`",
            record.path,
            crate::git::display_text(
                serde_yaml::to_string(value)
                    .unwrap_or_default()
                    .trim_end_matches('\n')
            )
        ))),
    }
}

/// The tip of the default branch this clone resolves: the first of the
/// references [`super::target_tips`] lists for the name
/// [`super::default_target_name`] finds.
fn default_tip(repo: &Repository) -> Result<Oid, String> {
    let name = super::default_target_name(repo)?;
    super::target_tips(repo, &name.name)?
        .first()
        .map(|(_, tip)| *tip)
        .ok_or_else(|| {
            format!(
                "the default branch `{}` does not resolve here, and the run was given no default branch's commit",
                crate::git::display_text(&name.name)
            )
        })
}

/// Why the lineages `block` records do not admit `task`'s landing on its
/// marked epic's line, or `None` when they do.
///
/// # Errors
/// The shipped model catalog, which names the lineages, does not parse.
pub(super) fn problem(
    task: &RecordView,
    block: &AcceptanceBlock,
) -> Result<Option<String>, String> {
    let words = lineage_words()?;
    let known: BTreeSet<&str> = words.values().map(String::as_str).collect();
    let named = |set: &BTreeSet<&str>| set.iter().copied().collect::<Vec<_>>().join(", ");
    let built = match block.built_by.as_deref() {
        None => {
            return Ok(Some(format!(
                "{}: {LANDED_REVIEW}, and its acceptance block has no `built_by`, the one lineage that built it ({})",
                task.id,
                named(&known)
            )))
        }
        Some(value) => match known.get(value) {
            Some(lineage) => *lineage,
            None => {
                return Ok(Some(format!(
                    "{}: {LANDED_REVIEW}, and its acceptance block's `built_by: {}` is not one lineage id ({})",
                    task.id,
                    crate::git::display_text(value),
                    named(&known)
                )))
            }
        },
    };
    let seat = reviewer(&block.review);
    let reviewed_by = lineages(words, seat);
    if reviewed_by.is_empty() {
        return Ok(Some(format!(
            "{}: {LANDED_REVIEW}, and its acceptance block's review names no reviewer of a known lineage ({}) before its first comma or semicolon, so it does not show who reviewed the work `built_by` says {built} built",
            task.id,
            named(&known)
        )));
    }
    if !reviewed_by.contains(built) {
        return Ok(None);
    }
    let others: BTreeSet<&str> = known.iter().copied().filter(|l| *l != built).collect();
    let Some(reason) =
        reduced_assurance_problem(block.reduced_assurance.as_deref(), built, &known, &others)
    else {
        return Ok(None);
    };
    let example = others
        .iter()
        .map(|lineage| format!("{lineage} {PROBED_UNAVAILABLE}"))
        .collect::<Vec<_>>()
        .join("; ");
    Ok(Some(format!(
        "{}: {LANDED_REVIEW}, and its reviewer `{}` is of the lineage `built_by` names ({built}), so its acceptance block needs `reduced_assurance` recording each other lineage as probed and unavailable: one item per other lineage ({}), separated by `;`, each exactly `<lineage id> {PROBED_UNAVAILABLE}`, for example `{example}`; {reason}",
        task.id,
        crate::git::display_text(seat),
        named(&others)
    )))
}

/// The words that follow the lineage id in each `reduced_assurance` item,
/// and nothing after them.
const PROBED_UNAVAILABLE: &str = "probed unavailable";

/// Why `statement`, a block's `reduced_assurance`, does not record each
/// lineage in `others` as probed and unavailable in the fixed form, or
/// `None` when it does. The form: items separated by `;`, each exactly
/// three words, a lineage id of `known` (exact, as `built_by` is), then
/// `probed`, then `unavailable`, with no other word or punctuation; exactly
/// one item for each lineage in `others` and none for `built`. For example
/// `codex probed unavailable; grok probed unavailable`. Only ASCII spaces
/// and tabs separate and trim words, so any other blank stays inside a word
/// and refuses. The form makes a careless, mistaken or contradicted
/// statement refuse; it requires the assertion and does not show that the
/// probe took place.
fn reduced_assurance_problem(
    statement: Option<&str>,
    built: &str,
    known: &BTreeSet<&str>,
    others: &BTreeSet<&str>,
) -> Option<String> {
    let Some(statement) = statement else {
        return Some("it has no `reduced_assurance`".into());
    };
    let mut recorded = BTreeSet::new();
    for item in statement
        .split(';')
        .map(|item| item.trim_matches([' ', '\t']))
    {
        let shown = crate::git::display_text(item);
        let mut words = item.split([' ', '\t']).filter(|word| !word.is_empty());
        let Some(lineage) = words.next().and_then(|word| known.get(word)).copied() else {
            return Some(format!(
                "the item `{shown}` does not start with a lineage id"
            ));
        };
        if !words.eq(PROBED_UNAVAILABLE.split(' ')) {
            return Some(format!(
                "the item `{shown}` is not exactly `{lineage} {PROBED_UNAVAILABLE}`"
            ));
        }
        if lineage == built {
            return Some(format!(
                "the item `{shown}` names {built}, the builder's own lineage"
            ));
        }
        if !recorded.insert(lineage) {
            return Some(format!("{lineage} has more than one item"));
        }
    }
    let missing: Vec<&str> = others.difference(&recorded).copied().collect();
    (!missing.is_empty()).then(|| {
        format!(
            "no item records {} as probed and unavailable",
            missing.join(", ")
        )
    })
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

    fn record() -> RecordView {
        let text = "---\nid: TSK-001\nepic_id: EPC-001\nstatus: complete\n---\n\n# TSK-001: work\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall work.\n\n## Execution contract\n\n- Builder: Codex GPT-6.1 Sol, else a Claude Opus 5.5 subagent.\n\n## Closeout\n\nDone.\n";
        RecordView::parse(
            RecordKind::Task,
            "project-management/tasks/TSK-001.md",
            text,
        )
        .unwrap()
    }

    fn block(built_by: Option<&str>, review: &str, reduced: Option<&str>) -> AcceptanceBlock {
        AcceptanceBlock {
            superseded: false,
            reason: None,
            reviewed: "0".repeat(40),
            review: review.into(),
            built_by: built_by.map(Into::into),
            reduced_assurance: reduced.map(Into::into),
            criteria: Vec::new(),
            journey: "verified | ran".into(),
            not_verified: "none".into(),
            follow_ups: "none: nothing".into(),
            verdict: "approved".into(),
        }
    }

    fn judge(built_by: Option<&str>, review: &str, reduced: Option<&str>) -> Option<String> {
        problem(&record(), &block(built_by, review, reduced)).unwrap()
    }

    #[test]
    fn a_reviewer_of_another_lineage_admits_the_landing() {
        for (built_by, review) in [
            ("claude", "Codex GPT-6.1 Sol at medium, whole unit"),
            ("codex", "Grok 4.7; whole unit"),
            ("grok", "Claude Fable 5.1 as cf-reviewer, whole unit"),
            // The plan's Builder line names Codex and a Claude fallback;
            // only `built_by` says who built.
            ("codex", "Claude Fable 5.1, whole unit"),
        ] {
            assert_eq!(
                judge(Some(built_by), review, None),
                None,
                "{built_by} / {review}"
            );
        }
    }

    #[test]
    fn a_reviewer_of_the_builders_lineage_is_refused() {
        for (built_by, review) in [
            ("claude", "Claude Fable 5.1, whole unit"),
            ("codex", "GPT-6 Astra (gpt-6-astra, high), whole unit"),
            ("claude", "Sonnet 5.5; whole unit"),
        ] {
            let found = judge(Some(built_by), review, None).expect(review);
            assert!(
                found.contains("is of the lineage `built_by` names"),
                "{found}"
            );
            assert!(found.contains(LANDED_REVIEW), "{found}");
        }
    }

    #[test]
    fn a_same_lineage_review_passes_only_with_the_reduced_assurance_statement() {
        let review = "Claude Opus 5.5 subagent at medium, whole unit";
        for stated in [
            "codex probed unavailable; grok probed unavailable",
            "grok probed unavailable;codex probed unavailable",
            "  codex\tprobed   unavailable  ;  grok probed unavailable  ",
        ] {
            assert_eq!(
                judge(Some("claude"), review, Some(stated)),
                None,
                "{stated}"
            );
        }
        let found = judge(Some("claude"), review, None).expect("no statement");
        assert!(found.contains("it has no `reduced_assurance`"), "{found}");
        assert!(
            found.contains("one item per other lineage (codex, grok), separated by `;`"),
            "{found}"
        );
        assert!(
            found.contains(
                "each exactly `<lineage id> probed unavailable`, for example `codex probed unavailable; grok probed unavailable`"
            ),
            "{found}"
        );
        let found = judge(Some("codex"), "GPT-6 Astra, whole unit", None).expect("codex");
        assert!(
            found.contains("one item per other lineage (claude, grok)"),
            "{found}"
        );
    }

    #[test]
    fn a_reduced_assurance_statement_out_of_form_is_refused() {
        let review = "Claude Opus 5.5 subagent at medium, whole unit";
        for (stated, why) in [
            // Names only: the other lineages are named, never recorded as
            // unavailable (Codex security review R1).
            (
                "Codex and Grok are available; neither was probed",
                "the item `Codex and Grok are available` does not start with a lineage id",
            ),
            (
                "Codex and Grok probed live at 16:24 on 2026-11-05, both unavailable",
                "does not start with a lineage id",
            ),
            (
                "codex available; grok probed unavailable",
                "the item `codex available` is not exactly `codex probed unavailable`",
            ),
            (
                "codex; grok probed unavailable",
                "the item `codex` is not exactly `codex probed unavailable`",
            ),
            // The probe left out (Codex confirm R1 at 594c804fe).
            (
                "codex unavailable; grok unavailable",
                "the item `codex unavailable` is not exactly `codex probed unavailable`",
            ),
            // A valid prefix, then words that deny the probe.
            (
                "codex probed unavailable, not probed; grok probed unavailable",
                "the item `codex probed unavailable, not probed` is not exactly `codex probed unavailable`",
            ),
            (
                "codex unavailable, not probed; grok unavailable, not probed",
                "the item `codex unavailable, not probed` is not exactly `codex probed unavailable`",
            ),
            // Any trailing word.
            (
                "codex probed unavailable today; grok probed unavailable",
                "the item `codex probed unavailable today` is not exactly `codex probed unavailable`",
            ),
            (
                "codex probed unavailable.; grok probed unavailable",
                "is not exactly `codex probed unavailable`",
            ),
            (
                "codex Probed unavailable; grok probed unavailable",
                "is not exactly `codex probed unavailable`",
            ),
            (
                "codex unavailable probed; grok probed unavailable",
                "is not exactly `codex probed unavailable`",
            ),
            (
                "codex probed; grok probed unavailable",
                "is not exactly `codex probed unavailable`",
            ),
            (
                "codex probed unavailable",
                "no item records grok as probed and unavailable",
            ),
            (
                "codex probed unavailable; codex probed unavailable; grok probed unavailable",
                "codex has more than one item",
            ),
            (
                "claude probed unavailable; codex probed unavailable; grok probed unavailable",
                "the item `claude probed unavailable` names claude, the builder's own lineage",
            ),
            (
                "gemini probed unavailable; codex probed unavailable; grok probed unavailable",
                "the item `gemini probed unavailable` does not start with a lineage id",
            ),
            // A lineage id is exact, as `built_by`'s is.
            (
                "Codex probed unavailable; grok probed unavailable",
                "the item `Codex probed unavailable` does not start with a lineage id",
            ),
            (
                "gpt probed unavailable; grok probed unavailable",
                "the item `gpt probed unavailable` does not start with a lineage id",
            ),
            (
                "codex probed unavailable; grok probed unavailable;",
                "the item `` does not start with a lineage id",
            ),
            // Only ASCII space and tab separate words: a non-breaking space
            // leaves `codex\u{a0}probed` one word, which is no lineage id.
            (
                "codex\u{a0}probed unavailable; grok probed unavailable",
                "does not start with a lineage id",
            ),
        ] {
            let found = judge(Some("claude"), review, Some(stated)).expect(stated);
            assert!(found.contains(why), "{stated}: {found}");
            assert!(found.contains(LANDED_REVIEW), "{found}");
            assert!(
                found.contains(
                    "needs `reduced_assurance` recording each other lineage as probed and unavailable"
                ),
                "{found}"
            );
        }
    }

    #[test]
    fn built_by_names_exactly_one_lineage_id() {
        let review = "Grok 4.7, whole unit";
        let found = judge(None, review, None).expect("missing");
        assert!(found.contains("has no `built_by`"), "{found}");
        for value in ["codex, claude", "Codex", "codex claude", "gpt"] {
            let found = judge(Some(value), review, None).expect(value);
            assert!(
                found.contains("is not one lineage id (claude, codex, grok)"),
                "{found}"
            );
        }
    }

    #[test]
    fn a_review_that_names_no_known_reviewer_is_refused() {
        let found =
            judge(Some("claude"), "https://example.test/pr/1#review", None).expect("refused");
        assert!(
            found.contains("names no reviewer of a known lineage (claude, codex, grok)"),
            "{found}"
        );
        // A lineage named after the first comma does not name the reviewer.
        assert!(judge(Some("claude"), "Reduced assurance, Codex was out", None).is_some());
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
