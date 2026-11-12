//! TSK-1378 AC-11 and AC-13: one definition of the path classes a planning
//! landing may carry, tested under the live set and the aligned one, and
//! the home anchor that a line's own records-only adoption sets.

use super::home_tests::{
    docs_move, moved, passes, path_of, refused, task, todo, Fixture, LINE, MOVED, OTHER,
};
use super::{admissible_planning_landing_with, Repository, PLANNING_LANDING_PATHS};
use crate::workgraph::classify::AmendmentPath;

const NARROW: &[AmendmentPath] = &[AmendmentPath::Record];
const ALIGNED: &[AmendmentPath] = &[AmendmentPath::Record, AmendmentPath::Doc];

fn docs_admitted() -> bool {
    PLANNING_LANDING_PATHS.contains(&AmendmentPath::Doc)
}

/// AC-11: production admits records only until the operator decides.
#[test]
fn the_live_set_is_records_only() {
    assert_eq!(PLANNING_LANDING_PATHS, NARROW);
}

/// AC-11: each landing moves the record beside one other path. Records
/// with docs pass only under the aligned set; every other class is refused
/// under both, and records alone pass under both.
#[test]
fn planning_landing_paths_narrow_and_aligned() {
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    let record = path_of("TSK-001");
    let cases: [(&str, Option<(&str, &str)>, bool, bool); 8] = [
        ("records only", None, true, true),
        (
            "records and docs",
            Some(("docs/note.md", "A note.\n")),
            false,
            true,
        ),
        (
            "instructions",
            Some(("AGENTS.md", "# Agents\n")),
            false,
            false,
        ),
        (
            "product",
            Some(("src/planned.rs", "pub fn planned() {}\n")),
            false,
            false,
        ),
        (
            "hooks",
            Some((".githooks/pre-push", "#!/bin/sh\nexit 0\n")),
            false,
            false,
        ),
        (
            "policy",
            Some((".codeflow/policy.json", "{\n  \"schema_version\": 1\n}\n")),
            false,
            false,
        ),
        (
            "workflow",
            Some((".github/workflows/ci.yml", "on: push\n")),
            false,
            false,
        ),
        (
            "record template",
            Some(("project-management/templates/task.md", "# Task\n")),
            false,
            false,
        ),
    ];
    let mut landings = Vec::new();
    for (case, extra, _, _) in &cases {
        let note = task("TSK-001", OTHER, "todo", case, "Pending.");
        let mut files = vec![(record.as_str(), note.as_str())];
        files.extend(extra.iter().copied());
        landings.push(fixture.land("main", &files));
    }
    let repo = Repository::open(fixture.root()).unwrap();
    let mut failures = Vec::new();
    for ((case, _, narrow, aligned), landing) in cases.iter().zip(landings) {
        for (set, name, expected) in [(NARROW, "narrow", *narrow), (ALIGNED, "aligned", *aligned)] {
            match admissible_planning_landing_with(&repo, landing, set) {
                Ok(found) if found.is_some() == expected => {}
                other => failures.push(format!("{case} under {name}: {other:?}")),
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// AC-11 and AC-13: a move on `main` in a merge that also carries a doc,
/// synced directly, follows the live set: refused under records only.
#[test]
fn a_main_move_in_a_records_and_docs_merge_follows_the_live_set() {
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    docs_move(&fixture, &moved("TSK-001", "main"));
    fixture.sync();
    if docs_admitted() {
        passes(&fixture, "a docs-bearing move under the aligned set");
    } else {
        refused(&fixture, "a docs-bearing move", &["TSK-001", MOVED]);
    }
}

/// AC-13: the line's records-only adoption of `main`'s exact version
/// leaves the record unchanged by the next sync, and sets the anchor that a
/// later sync carrying a reviewed note on `main` starts its walk from.
#[test]
fn a_line_adoption_then_a_sync_leaves_the_records_unchanged() {
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    let path = path_of("TSK-001");
    let after = moved("TSK-001", "main");
    let source = docs_move(&fixture, &after);
    let adoption = fixture.land(LINE, &[(&path, &after)]);
    fixture.sync();
    let found = passes(&fixture, "a sync after the adoption");
    assert!(!found.contains_key("TSK-001"), "{found:?}");

    // The note goes in the closeout, so the sync merges cleanly over the
    // line's adopted copy.
    fixture.land(
        "main",
        &[(
            &path,
            &task("TSK-001", "main", "todo", "Work moved.", "Noted."),
        )],
    );
    fixture.sync();
    let found = passes(&fixture, "a note after the adoption");
    let anchor = found["TSK-001"]
        .home_anchor
        .as_ref()
        .expect("the adoption sets the anchor");
    assert_eq!(anchor.line_landing, adoption);
    assert_eq!(anchor.source_version, source);
}

/// AC-13 (4): a copy that differs from `main`'s version by one comment, in
/// a landing that also carries a doc, gains no anchor and no reset: under
/// records only it is refused, and under any set it is no anchor.
#[test]
fn a_copy_with_a_comment_in_a_docs_bearing_landing_gains_no_anchor() {
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    let path = path_of("TSK-001");
    let after = moved("TSK-001", "main");
    docs_move(&fixture, &after);
    let copy = after.replace("Pending.", "Pending.\n\n<!-- copied from main -->");
    fixture.land(
        LINE,
        &[(&path, &copy), ("docs/line.md", "The line's note.\n")],
    );
    fixture.sync();
    if docs_admitted() {
        let found = passes(&fixture, "an own docs-bearing move under the aligned set");
        assert!(found
            .get("TSK-001")
            .is_none_or(|record| record.home_anchor.is_none()));
    } else {
        refused(&fixture, "a commented copy", &["TSK-001"]);
    }
}
