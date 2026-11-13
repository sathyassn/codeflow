//! TSK-1378 AC-11 and AC-13: one definition of the path classes a planning
//! landing may carry, tested under the live set and the aligned one, and
//! the home anchor that a line's own records-only adoption sets.

use super::home::line_landing;
use super::home_tests::{
    docs_move, moved, passes, path_of, refused, task, todo, Fixture, LINE, MOVED, OTHER, THIRD,
};
use super::{
    admissible_planning_landing_with, Graph, HomeAnchor, Repository, PLANNING_LANDING_PATHS,
};
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
/// A case name, the path landed beside the record, and whether the narrow
/// and the aligned set admit the landing.
type SetCase<'a> = (&'a str, Option<(&'a str, &'a str)>, bool, bool);

#[test]
fn planning_landing_paths_narrow_and_aligned() {
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    let record = path_of("TSK-001");
    let cases: [SetCase; 8] = [
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

/// The task as `main` notes it after its move, in the closeout, so a sync
/// merges the note cleanly over a line copy that took `main`'s version.
fn noted() -> String {
    task("TSK-001", "main", "todo", "Work moved.", "Noted.")
}

/// Why an expected refusal did not happen, or `None` when the judgment
/// refuses with every needle. Lets one test report each of its stages.
fn not_refused(fixture: &Fixture, what: &str, needles: &[&str]) -> Option<String> {
    match fixture.judge() {
        Ok(found) => Some(format!(
            "{what}: accepted, importing {:?}",
            found.keys().collect::<Vec<_>>()
        )),
        Err(said) => needles
            .iter()
            .find(|needle| !said.contains(**needle))
            .map(|needle| format!("{what}: missing {needle:?} in {said}")),
    }
}

/// AC-13 and AC-10 (review finding 1 at bf25c232f): the anchor's source is
/// the version the line adopted, where it first stood on `main`. A later
/// move away and an exact restoration of those bytes on `main` stay in the
/// walk, whether the sync leaves the record unchanged or imports a later
/// note.
#[test]
fn an_exact_restoration_on_main_never_advances_the_anchor() {
    let path = path_of("TSK-001");
    let after = moved("TSK-001", "main");
    let mut failures = Vec::new();
    for note in [false, true] {
        let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
        docs_move(&fixture, &after);
        fixture.land(LINE, &[(&path, &after)]);
        fixture.direct("main", &[(&path, &moved("TSK-001", THIRD))]);
        let away = format!("the move at {}", fixture.oid("main"));
        fixture.direct("main", &[(&path, &after)]);
        if note {
            fixture.land("main", &[(&path, &noted())]);
        }
        fixture.sync();
        let what = if note {
            "a restoration, then a note"
        } else {
            "a restoration the sync leaves unchanged"
        };
        failures.extend(not_refused(&fixture, what, &["TSK-001", MOVED, &away]));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// AC-13 (review finding 1): with a valid move away and back on `main`,
/// the anchor stays at the version the line adopted, never the restoration.
#[test]
fn a_valid_move_away_and_back_keeps_the_anchor_at_the_adopted_version() {
    let path = path_of("TSK-001");
    let after = moved("TSK-001", "main");
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    let source = docs_move(&fixture, &after);
    let adoption = fixture.land(LINE, &[(&path, &after)]);
    fixture.land("main", &[(&path, &moved("TSK-001", THIRD))]);
    let restored = fixture.land("main", &[(&path, &after)]);
    fixture.land("main", &[(&path, &noted())]);
    fixture.sync();
    let found = passes(&fixture, "a valid move away and back, then a note");
    let anchor = found["TSK-001"]
        .home_anchor
        .expect("the adoption sets the anchor");
    assert_eq!(anchor.line_landing, adoption);
    assert_eq!(anchor.source_version, source);
    assert_ne!(anchor.source_version, restored);
}

/// AC-13 (review finding 2, history a): the line takes `main`'s home in a
/// docs-bearing landing with one comment added, then removes the comment
/// in a records-only landing. The anchor is bound to the landing that
/// changed the home, so the later edit makes no anchor, and `main`'s
/// docs-bearing move is judged whether the sync leaves the record
/// unchanged or imports a later note.
#[test]
fn a_comment_removed_after_a_docs_bearing_adoption_gains_no_anchor() {
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    let path = path_of("TSK-001");
    let after = moved("TSK-001", "main");
    docs_move(&fixture, &after);
    let copy = after.replace("Pending.", "Pending.\n\n<!-- copied from main -->");
    let home_change = fixture.land(
        LINE,
        &[(&path, &copy), ("docs/line.md", "The line's note.\n")],
    );
    fixture.land(LINE, &[(&path, &after)]);
    fixture.sync();
    if docs_admitted() {
        passes(&fixture, "an own docs-bearing move under the aligned set");
        fixture.land("main", &[(&path, &noted())]);
        fixture.sync();
        let found = passes(&fixture, "a note under the aligned set");
        assert_eq!(found["TSK-001"].home_anchor, None);
        return;
    }
    let mut failures = Vec::new();
    failures.extend(not_refused(
        &fixture,
        "a comment removed after a docs-bearing adoption",
        &["TSK-001", MOVED, &home_change.to_string()],
    ));
    fixture.land("main", &[(&path, &noted())]);
    fixture.sync();
    failures.extend(not_refused(
        &fixture,
        "a note after a comment removed from a docs-bearing adoption",
        &["TSK-001", MOVED],
    ));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// AC-13 (review finding 2, history b): the line moves the task's home by
/// its own records-only planning merge, then a docs-and-records amendment
/// makes its text `main`'s version without changing the home. The own move
/// admits the record with no anchor, and every move `main` made is still
/// judged: a planning move passes, a direct one refuses.
#[test]
fn an_own_move_then_an_amendment_matching_main_passes_without_an_anchor() {
    let path = path_of("TSK-001");
    let main_version = moved("TSK-001", "main");
    let own = task("TSK-001", "main", "todo", "Work.", "Pending on the line.");
    let amendment = [
        (path.as_str(), main_version.as_str()),
        ("docs/line.md", "The line's note.\n"),
    ];

    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    fixture.land("main", &[(&path, &main_version)]);
    fixture.land(LINE, &[(&path, &own)]);
    fixture.land(LINE, &amendment);
    fixture.sync();
    let found = passes(&fixture, "an own move, then an amendment matching main");
    assert!(!found.contains_key("TSK-001"), "{found:?}");
    fixture.land("main", &[(&path, &noted())]);
    fixture.sync();
    let found = passes(&fixture, "a note after the own move and the amendment");
    assert_eq!(found["TSK-001"].home_anchor, None);

    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    fixture.direct("main", &[(&path, &main_version)]);
    let direct = fixture.oid("main");
    fixture.land(LINE, &[(&path, &own)]);
    fixture.land(LINE, &amendment);
    fixture.sync();
    refused(
        &fixture,
        "a direct move on main beside an own move",
        &[
            "TSK-001",
            MOVED,
            &format!("the move at {direct} is not a reviewed planning merge of records"),
        ],
    );
}

/// `main` moves the task away and back to its merge-base bytes by planning
/// merges, and lands product work beside them.
fn main_moves_away_and_back(fixture: &Fixture, base: &str) {
    let path = path_of("TSK-001");
    fixture.land("main", &[(&path, &moved("TSK-001", THIRD))]);
    fixture.land("main", &[("src/feature.rs", "pub fn feature() {}\n")]);
    fixture.land("main", &[(&path, base)]);
}

/// AC-13 (review finding 1 at b56cbb783): a home the line changed and then
/// restored is examined by its history, not by its net home or bytes. The
/// restoration is the line's last home change: in a merge that also
/// carries a doc it is refused, and in a records-only planning merge it
/// passes, with exact restored bytes and with a retained comment.
#[test]
fn a_home_the_line_changed_and_restored_is_examined() {
    let path = path_of("TSK-001");
    let base = todo("TSK-001", OTHER);
    let comment = "Pending.\n\n<!-- a line note -->";
    let cases = [
        ("exact restored bytes", todo("TSK-001", THIRD), base.clone()),
        (
            "a retained comment",
            todo("TSK-001", THIRD).replace("Pending.", comment),
            base.replace("Pending.", comment),
        ),
    ];
    let mut failures = Vec::new();
    for (case, away, restored) in &cases {
        let fixture = Fixture::new(&[("TSK-001", base.clone())]);
        main_moves_away_and_back(&fixture, &base);
        fixture.land(LINE, &[(&path, away)]);
        let restoration = fixture.land(
            LINE,
            &[(&path, restored), ("docs/line.md", "The line's note.\n")],
        );
        fixture.sync();
        if !docs_admitted() {
            failures.extend(not_refused(
                &fixture,
                &format!("a docs-bearing restoration, {case}"),
                &["TSK-001", MOVED, &restoration.to_string()],
            ));
        }

        let fixture = Fixture::new(&[("TSK-001", base.clone())]);
        main_moves_away_and_back(&fixture, &base);
        fixture.land(LINE, &[(&path, away)]);
        fixture.land(LINE, &[(&path, restored)]);
        fixture.sync();
        if let Err(said) = fixture.judge() {
            failures.push(format!("a records-only restoration, {case}: {said}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// AC-13 and AC-10: a line adoption of the bytes the merge base already
/// held sets no anchor, so `main`'s unauthorized move away and its
/// restoration of those bytes stay in the walk.
#[test]
fn an_adoption_of_the_merge_base_version_sets_no_anchor() {
    let path = path_of("TSK-001");
    let base = todo("TSK-001", OTHER);
    let fixture = Fixture::new(&[("TSK-001", base.clone())]);
    fixture.direct("main", &[(&path, &moved("TSK-001", THIRD))]);
    let away = format!("the move at {}", fixture.oid("main"));
    fixture.land("main", &[(&path, &base)]);
    fixture.land(LINE, &[(&path, &todo("TSK-001", THIRD))]);
    fixture.land(LINE, &[(&path, &base)]);
    fixture.sync();
    refused(
        &fixture,
        "a restoration of the merge-base version",
        &["TSK-001", MOVED, &away],
    );
}

/// AC-13 control: a line that edits the task but never changes its home
/// is not examined, so an ordinary sync passes even though `main` moved
/// the task away and back.
#[test]
fn a_line_that_never_changed_the_home_is_not_examined() {
    let path = path_of("TSK-001");
    let base = todo("TSK-001", OTHER);
    let fixture = Fixture::new(&[("TSK-001", base.clone())]);
    fixture.direct("main", &[(&path, &moved("TSK-001", THIRD))]);
    fixture.land("main", &[(&path, &base)]);
    fixture.land(
        LINE,
        &[(
            &path,
            &task("TSK-001", OTHER, "todo", "Work.", "Noted on the line."),
        )],
    );
    fixture.sync();
    let found = passes(&fixture, "a line that never changed the home");
    assert!(!found.contains_key("TSK-001"), "{found:?}");
}

/// AC-13 control: an earlier sync that imported a move `main` made, already
/// judged when it landed, is not the line's own home change. The next sync
/// passes, whether it leaves the record unchanged or imports a later note.
#[test]
fn an_earlier_sync_that_imported_a_judged_move_is_not_the_lines_own_change() {
    let path = path_of("TSK-001");
    let homed = moved("TSK-001", "main");
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    fixture.land("main", &[("src/feature.rs", "pub fn feature() {}\n")]);
    fixture.land("main", &[(&path, &homed)]);
    fixture.sync();
    passes(&fixture, "the first sync");
    fixture.git(&["switch", "-q", LINE]);
    fixture.git(&["merge", "--ff-only", "-q", "sync"]);

    fixture.land("main", &[(&path, &moved("TSK-001", THIRD))]);
    fixture.land("main", &[(&path, &homed)]);
    fixture.sync();
    let found = passes(&fixture, "a later sync leaving the record unchanged");
    assert!(!found.contains_key("TSK-001"), "{found:?}");

    fixture.land("main", &[(&path, &noted())]);
    fixture.sync();
    let found = passes(&fixture, "a later sync importing a note");
    assert_eq!(found["TSK-001"].home_anchor, None);
}

/// The line landing the walk binds for TSK-001 at the sync, and the anchor
/// it sets, apart from whether the record is admitted.
fn bound(fixture: &Fixture) -> Result<(Option<git2::Oid>, Option<HomeAnchor>), String> {
    let repo = Repository::open(fixture.root()).unwrap();
    let graph = Graph::from_revision(&repo, "sync").unwrap();
    line_landing(
        &repo,
        &graph.records["TSK-001"],
        fixture.oid("main"),
        fixture.oid(LINE),
        fixture.oid("sync"),
    )
}

/// Take the sync onto the line, as a landed sync pull request does.
fn take_the_sync(fixture: &Fixture) {
    fixture.git(&["switch", "-q", LINE]);
    fixture.git(&["merge", "--ff-only", "-q", "sync"]);
}

/// The history of review finding 1 at 94c5eaae1. The line moves the task
/// to `main` in its own landing, with a doc when `docs`. `main` lands an
/// unrelated change, a note on another record or product work when
/// `product`, and the line takes it by a clean sync that keeps the task as
/// the line holds it. `main` then moves the task to the line's exact bytes
/// and lands product work, and the next sync leaves the record unchanged.
/// Returns the line's own move.
fn own_move_kept_across_a_sync(docs: bool, product: bool) -> (Fixture, git2::Oid) {
    let path = path_of("TSK-001");
    let homed = moved("TSK-001", "main");
    let fixture = Fixture::new(&[
        ("TSK-001", todo("TSK-001", OTHER)),
        ("TSK-002", todo("TSK-002", "main")),
    ]);
    let mut own = vec![(path.as_str(), homed.as_str())];
    if docs {
        own.push(("docs/line.md", "The line's note.\n"));
    }
    let landing = fixture.land(LINE, &own);
    let other = path_of("TSK-002");
    let note = task("TSK-002", "main", "todo", "Work.", "Noted on main.");
    let unrelated = if product {
        ("src/feature.rs", "pub fn feature() {}\n")
    } else {
        (other.as_str(), note.as_str())
    };
    fixture.land("main", &[unrelated]);
    fixture.sync();
    passes(&fixture, "the earlier sync");
    take_the_sync(&fixture);
    fixture.land("main", &[(&path, &homed)]);
    fixture.land("main", &[("src/later.rs", "pub fn later() {}\n")]);
    fixture.sync();
    (fixture, landing)
}

/// AC-13 (review finding 1 at 94c5eaae1): when the line's last sync keeps
/// a home the line set before it, the walk binds the line's own move, never
/// the sync, whose own first-parent diff did not change the home. A
/// records-only own move passes and a docs-bearing one is refused, after a
/// records-only or a product-bearing earlier sync, with no anchor either way.
#[test]
fn a_home_kept_across_a_sync_binds_the_lines_own_move() {
    let mut failures = Vec::new();
    for docs in [false, true] {
        for product in [false, true] {
            let (fixture, landing) = own_move_kept_across_a_sync(docs, product);
            let case = format!(
                "{} own move, {} earlier sync",
                if docs {
                    "a docs-bearing"
                } else {
                    "a records-only"
                },
                if product {
                    "a product-bearing"
                } else {
                    "a records-only"
                },
            );
            match bound(&fixture) {
                Ok((at, anchor)) => {
                    if at != Some(landing) {
                        failures.push(format!("{case}: bound {at:?}, not the own move {landing}"));
                    }
                    if anchor.is_some() {
                        failures.push(format!("{case}: anchor {anchor:?}"));
                    }
                }
                Err(said) => failures.push(format!("{case}: no landing bound: {said}")),
            }
            if docs && !docs_admitted() {
                let named = format!("own home change at {landing}");
                failures.extend(not_refused(&fixture, &case, &["TSK-001", MOVED, &named]));
            } else if let Err(said) = fixture.judge() {
                failures.push(format!("{case}: refused: {said}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// AC-13: a line whose last sync kept a home that no commit of the line
/// set, by resolving the sync against `main`'s move, binds no landing. A
/// next sync that leaves the record unchanged is refused rather than
/// proving the sync itself; one that imports a later note sets no anchor
/// and judges every move `main` made, which pass here.
#[test]
fn a_kept_home_no_line_commit_set_is_refused() {
    let path = path_of("TSK-001");
    let base = todo("TSK-001", "main");
    let fixture = Fixture::new(&[("TSK-001", base.clone())]);
    fixture.land("main", &[(&path, &moved("TSK-001", THIRD))]);
    fixture.git(&["switch", "-q", "-C", "sync", LINE]);
    fixture.git(&["merge", "--no-ff", "-q", "--no-commit", "main"]);
    fixture.git(&["checkout", LINE, "--", &path]);
    fixture.git(&["commit", "-qm", "chore(merge): take main"]);
    let kept = fixture.oid("sync");
    take_the_sync(&fixture);
    fixture.land("main", &[(&path, &base)]);
    fixture.sync();
    let mut failures = Vec::new();
    match bound(&fixture) {
        Ok((at, _)) => failures.push(format!("bound {at:?} for a home no line commit set")),
        Err(said) if !said.contains(&kept.to_string()) => {
            failures.push(format!("the refusal does not name the sync {kept}: {said}"));
        }
        Err(_) => {}
    }
    failures.extend(not_refused(
        &fixture,
        "a home the sync kept against main",
        &[
            "TSK-001",
            MOVED,
            "no commit on the line's first-parent history changed it",
        ],
    ));
    fixture.land(
        "main",
        &[(&path, &task("TSK-001", "main", "todo", "Work.", "Noted."))],
    );
    fixture.sync();
    match fixture.judge() {
        Ok(found) if found["TSK-001"].home_anchor.is_none() => {}
        Ok(found) => failures.push(format!("a later note set an anchor: {found:?}")),
        Err(said) => failures.push(format!("a later note: refused: {said}")),
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// AC-13 negative (1): a reviewed records-only merge that adopts `main`'s
/// exact version but never landed on the line sets no anchor, so `main`'s
/// docs-bearing move is judged and refused.
#[test]
fn a_claimed_adoption_with_no_landing_is_refused() {
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    let path = path_of("TSK-001");
    let after = moved("TSK-001", "main");
    docs_move(&fixture, &after);
    fixture.git(&["branch", "plan/unlanded", LINE]);
    fixture.land("plan/unlanded", &[(&path, &after)]);
    fixture.sync();
    refused(
        &fixture,
        "an adoption that never landed on the line",
        &["TSK-001", MOVED],
    );
}

/// AC-13 negative (2) and AC-7: a records-only planning merge reachable
/// only through another merge's second parent proves nothing. On the line
/// it sets no anchor, and on `main` it is no move the walk accepts.
#[test]
fn an_adoption_or_move_reachable_only_by_a_second_parent_is_refused() {
    let path = path_of("TSK-001");
    let after = moved("TSK-001", "main");

    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    docs_move(&fixture, &after);
    fixture.git(&["branch", "plan/adopt", LINE]);
    fixture.land("plan/adopt", &[(&path, &after)]);
    fixture.direct("plan/adopt", &[("docs/line.md", "The line's note.\n")]);
    fixture.git(&["switch", "-q", LINE]);
    fixture.git(&[
        "merge",
        "--no-ff",
        "-q",
        "-m",
        "chore: land the adoption",
        "plan/adopt",
    ]);
    let landing = fixture.oid(LINE).to_string();
    fixture.sync();
    refused(
        &fixture,
        "an adoption behind a second parent",
        &["TSK-001", MOVED, &landing],
    );

    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    fixture.git(&["branch", "feature", "main"]);
    fixture.land("feature", &[(&path, &after)]);
    fixture.direct("feature", &[("src/feature.rs", "pub fn feature() {}\n")]);
    fixture.git(&["switch", "-q", "main"]);
    fixture.git(&[
        "merge",
        "--no-ff",
        "-q",
        "-m",
        "feat: land the feature",
        "feature",
    ]);
    let landing = fixture.oid("main").to_string();
    fixture.sync();
    refused(
        &fixture,
        "a main move behind a second parent",
        &["TSK-001", MOVED, &landing],
    );
}
