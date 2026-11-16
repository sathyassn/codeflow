//! TSK-1185 AC-16: the home moves of a record imported from a home other
//! than the default branch, where no default-branch walk runs. Every earlier
//! move on the home's first-parent history is judged in its own right, and
//! the history is followed by identity: a later accepted move or correction
//! does not clear an earlier one, and a changed path, a record not proved to
//! be this task, or a side that declares no home is refused, never read as
//! the record's introduction.

use std::path::Path;

use super::home_tests::{path_of, task, todo, Fixture, LINE, MOVED, OTHER, THIRD};
use super::{
    verify_earlier_home_move, verify_landing_home, Oid, RecordKind, RecordView, Repository,
};

const PRODUCT: (&str, &str) = ("src/planned.rs", "pub fn planned() {}\n");
const NO_HOME: &str = "declares no home on one side of the change";
const PATH_CHANGED: &str = "the task's record cannot be identified";

/// The task record at `path` in the tree of `at`.
fn record_in(repo: &Repository, at: Oid, path: &str) -> RecordView {
    let tree = repo.find_commit(at).unwrap().tree().unwrap();
    let blob = tree
        .get_path(Path::new(path))
        .unwrap()
        .to_object(repo)
        .unwrap()
        .peel_to_blob()
        .unwrap();
    RecordView::parse(
        RecordKind::Task,
        path,
        std::str::from_utf8(blob.content()).unwrap(),
    )
    .unwrap()
}

/// A fixture whose home `OTHER` branches from `main`, which holds TSK-001
/// homed on `LINE`.
fn on_other() -> Fixture {
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", LINE))]);
    fixture.git(&["branch", OTHER, "main"]);
    fixture
}

/// The earlier-move walk for the record at `path` whose landing is the tip
/// of `OTHER`, received past `main`, on a run that verified no default tip.
fn walk(fixture: &Fixture, path: &str) -> Result<(), String> {
    let repo = Repository::open(fixture.root()).unwrap();
    let tip = fixture.oid(OTHER);
    let record = record_in(&repo, tip, path);
    verify_earlier_home_move(
        &repo,
        &record,
        (OTHER, tip),
        tip,
        (fixture.oid("main"), tip),
        None,
    )
}

/// The home checks of the landing of the record at `path`, the tip of
/// `OTHER`, with no default-branch walk and no verified tip: the landing's
/// own change, then the earlier moves.
fn landing(fixture: &Fixture, path: &str) -> Result<(), String> {
    let repo = Repository::open(fixture.root()).unwrap();
    let tip = fixture.oid(OTHER);
    let record = record_in(&repo, tip, path);
    let parents = [
        fixture.oid(&format!("{OTHER}^1")),
        fixture.oid(&format!("{OTHER}^2")),
    ];
    verify_landing_home(
        &repo,
        &record,
        (OTHER, tip),
        tip,
        &parents,
        (fixture.oid("main"), tip),
        None,
        None,
    )
    .map(|_| ())
}

fn refuses(result: &Result<(), String>, what: &str, needle: &str) {
    match result {
        Ok(()) => panic!("{what}: accepted"),
        Err(said) => assert!(
            said.contains(needle),
            "{what}: missing {needle:?} in {said}"
        ),
    }
}

fn edited(home: &str) -> String {
    task("TSK-001", home, "todo", "Work, with notes.", "Pending.")
}

/// A move with a product file, then a reviewed records-only move: the second
/// move is accepted and the walk still reaches the first, whether the second
/// move is the record's landing or a later edit is.
#[test]
fn an_accepted_move_does_not_clear_an_earlier_refused_one() {
    let path = path_of("TSK-001");
    for later_edit in [false, true] {
        let fixture = on_other();
        fixture.land(OTHER, &[(&path, &todo("TSK-001", THIRD)), PRODUCT]);
        fixture.land(OTHER, &[(&path, &todo("TSK-001", OTHER))]);
        if later_edit {
            fixture.land(OTHER, &[(&path, &edited(OTHER))]);
        }
        refuses(
            &walk(&fixture, &path),
            &format!("refused move, then an accepted one (later edit: {later_edit})"),
            MOVED,
        );
    }
}

/// Two reviewed records-only moves, then a later edit, are all taken.
#[test]
fn two_accepted_moves_and_a_later_edit_are_taken() {
    let path = path_of("TSK-001");
    let fixture = on_other();
    fixture.land(OTHER, &[(&path, &todo("TSK-001", THIRD))]);
    fixture.land(OTHER, &[(&path, &todo("TSK-001", OTHER))]);
    fixture.land(OTHER, &[(&path, &edited(OTHER))]);
    walk(&fixture, &path).unwrap();
}

/// The record completed under `home`, reviewed at `reviewed`.
fn complete(home: &str, note: &str, reviewed: Oid) -> String {
    let block = format!("```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: https://example.test/review/1\n  criteria:\n    AC-1: verified | test ran\n  journey: verified | fixture\n  not_verified: none\n  follow_ups: none: fixture\n  verdict: approved\n```");
    task("TSK-001", home, "complete", note, &block)
}

/// A move to `THIRD` (with a product file when `bad`), the task completed
/// there, and a proved correction of its home to `OTHER` (TSK-1185 AC-10),
/// then a later edit when `later_edit`.
fn corrected(bad: bool, later_edit: bool) -> Fixture {
    let path = path_of("TSK-001");
    let fixture = on_other();
    let moved = todo("TSK-001", THIRD);
    if bad {
        fixture.land(OTHER, &[(&path, &moved), PRODUCT]);
    } else {
        fixture.land(OTHER, &[(&path, &moved)]);
    }
    let reviewed = fixture.oid(OTHER);
    fixture.land(OTHER, &[(&path, &complete(THIRD, "Work.", reviewed))]);
    fixture.land(OTHER, &[(&path, &complete(OTHER, "Work.", reviewed))]);
    if later_edit {
        fixture.land(
            OTHER,
            &[(&path, &complete(OTHER, "Work, with notes.", reviewed))],
        );
    }
    fixture
}

/// A proved correction on a home other than the default branch clears only
/// itself: the refused move before it is still judged.
#[test]
fn a_correction_on_another_home_does_not_clear_an_earlier_refused_move() {
    for later_edit in [false, true] {
        refuses(
            &walk(&corrected(true, later_edit), &path_of("TSK-001")),
            &format!("refused move, then a correction (later edit: {later_edit})"),
            MOVED,
        );
    }
}

/// The same correction after a reviewed records-only move is taken.
#[test]
fn a_correction_after_an_accepted_move_is_taken() {
    for later_edit in [false, true] {
        walk(&corrected(false, later_edit), &path_of("TSK-001")).unwrap();
    }
}

/// A refused move, then planning landings that drop the declared home and
/// restore it, then a later edit: the side with no home is refused, both in
/// the walk and as the landing's own change.
#[test]
fn a_record_that_declares_no_home_does_not_end_the_history() {
    let path = path_of("TSK-001");
    let fixture = on_other();
    fixture.land(OTHER, &[(&path, &todo("TSK-001", OTHER)), PRODUCT]);
    let undeclared = todo("TSK-001", OTHER).replace(&format!("integration_target: {OTHER}\n"), "");
    fixture.land(OTHER, &[(&path, &undeclared)]);
    fixture.land(OTHER, &[(&path, &todo("TSK-001", OTHER))]);
    refuses(
        &landing(&fixture, &path),
        "home restored at the landing",
        NO_HOME,
    );
    fixture.land(OTHER, &[(&path, &edited(OTHER))]);
    refuses(
        &walk(&fixture, &path),
        "home restored before an edit",
        NO_HOME,
    );
}

/// A refused move, then a planning landing that moves the record to another
/// path with the same identity, then a later edit: the history is not read
/// as starting at the new path, both in the walk and as the landing's own
/// change.
#[test]
fn a_record_moved_to_another_path_does_not_end_the_history() {
    let path = path_of("TSK-001");
    let renamed = "project-management/epics/EPC-002/tasks/TSK-001.md";
    let fixture = on_other();
    fixture.land(OTHER, &[(&path, &todo("TSK-001", OTHER)), PRODUCT]);
    fixture.git(&["switch", "-q", "-c", "plan/rename", OTHER]);
    std::fs::create_dir_all(
        fixture
            .root()
            .join("project-management/epics/EPC-002/tasks"),
    )
    .unwrap();
    fixture.git(&["mv", &path, renamed]);
    fixture.git(&["commit", "-qm", "docs: file the task under its epic"]);
    fixture.git(&["switch", "-q", OTHER]);
    fixture.git(&[
        "merge",
        "--no-ff",
        "-q",
        "-m",
        "chore: land reviewed planning",
        "plan/rename",
    ]);
    refuses(
        &landing(&fixture, renamed),
        "renamed at the landing",
        PATH_CHANGED,
    );
    fixture.land(OTHER, &[(renamed, &edited(OTHER))]);
    refuses(
        &walk(&fixture, renamed),
        "renamed before an edit",
        PATH_CHANGED,
    );
}

/// A record introduced on the home by a planning landing is taken: absent at
/// its path and by identity before it.
#[test]
fn a_record_introduced_on_the_home_is_taken() {
    let path = path_of("TSK-002");
    let fixture = on_other();
    fixture.land(OTHER, &[(&path, &todo("TSK-002", OTHER))]);
    fixture.land(
        OTHER,
        &[(&path, &edited(OTHER).replace("TSK-001", "TSK-002"))],
    );
    walk(&fixture, &path).unwrap();
}

/// A fixture whose `main` holds TSK-001 homed on `LINE` and `occupant` at
/// `path_of(occupant_file)`, with `OTHER` branched from it.
fn with_occupant(occupant_file: &str, occupant: String) -> Fixture {
    let fixture = Fixture::new(&[
        ("TSK-001", todo("TSK-001", LINE)),
        (occupant_file, occupant),
    ]);
    fixture.git(&["branch", OTHER, "main"]);
    fixture
}

/// Land on `OTHER` a planning change that removes `from` and writes `text`
/// at `to`, so the task's record takes over the path another record held.
fn land_over(fixture: &Fixture, from: &str, to: &str, text: &str) {
    fixture.git(&["switch", "-q", "-c", "plan/over", OTHER]);
    fixture.git(&["rm", "-q", from]);
    fixture.write(to, text);
    fixture.git(&["add", "-A"]);
    fixture.git(&["commit", "-qm", "docs: file the task at another path"]);
    fixture.git(&["switch", "-q", OTHER]);
    fixture.git(&[
        "merge",
        "--no-ff",
        "-q",
        "-m",
        "chore: land reviewed planning",
        "plan/over",
    ]);
}

/// A refused move of TSK-001, then a planning landing that puts its record
/// at `path_of(occupant_file)`, which `occupant` held until then. Returns
/// the fixture and that path.
fn taken_over(occupant_file: &str, occupant: String) -> (Fixture, String) {
    let path = path_of("TSK-001");
    let occupied = path_of(occupant_file);
    let fixture = with_occupant(occupant_file, occupant);
    fixture.land(OTHER, &[(&path, &todo("TSK-001", OTHER)), PRODUCT]);
    land_over(&fixture, &path, &occupied, &todo("TSK-001", OTHER));
    (fixture, occupied)
}

/// The record lands at a path another task's record held, on the same
/// home: that record's history is not read as this task's at the landing.
#[test]
fn a_record_at_a_path_another_task_held_is_refused_at_the_landing() {
    let (fixture, occupied) = taken_over("TSK-002", todo("TSK-002", OTHER));
    refuses(
        &landing(&fixture, &occupied),
        "another task's path at the landing",
        PATH_CHANGED,
    );
}

/// The same, with a later edit as the landing: the walk does not follow the
/// other record's history either.
#[test]
fn a_record_at_a_path_another_task_held_is_refused_under_a_later_edit() {
    let (fixture, occupied) = taken_over("TSK-002", todo("TSK-002", OTHER));
    fixture.land(OTHER, &[(&occupied, &edited(OTHER))]);
    refuses(
        &walk(&fixture, &occupied),
        "another task's path before an edit",
        PATH_CHANGED,
    );
}

/// The record it replaced shares the task's id under another uid: two
/// records match the task before the landing, so it is refused.
#[test]
fn a_record_whose_id_another_record_shares_is_refused() {
    let occupant = todo("TSK-001", OTHER).replace("uid: uid-TSK-001", "uid: uid-other");
    let (fixture, occupied) = taken_over("TSK-009", occupant);
    refuses(
        &landing(&fixture, &occupied),
        "a shared id at the landing",
        PATH_CHANGED,
    );
}

/// The record it replaced shares the task's uid under another id.
#[test]
fn a_record_whose_uid_another_record_shares_is_refused() {
    let occupant = todo("TSK-009", OTHER).replace("uid: uid-TSK-009", "uid: uid-TSK-001");
    let (fixture, occupied) = taken_over("TSK-009", occupant);
    refuses(
        &landing(&fixture, &occupied),
        "a shared uid at the landing",
        PATH_CHANGED,
    );
}

/// An unreadable task record before the landing may be a copy of the task,
/// so the task's earlier version cannot be identified: the landing's own
/// home check refuses it, and so does the walk.
#[test]
fn an_unreadable_task_record_is_refused() {
    let path = path_of("TSK-001");
    let fixture = with_occupant("TSK-005", "---\nid: [\n---\n".to_string());
    fixture.land(OTHER, &[(&path, &edited(LINE))]);
    refuses(
        &landing(&fixture, &path),
        "an unreadable task record at the landing",
        "identity is ambiguous",
    );
    refuses(
        &walk(&fixture, &path),
        "an unreadable task record in the walk",
        PATH_CHANGED,
    );
}

/// The uid the fixture's id registry binds TSK-001 to.
const REGISTERED: &str = "8c0e3f2a-5b1d-4c7e-9a3f-2d6b8e1f4a70";

/// TSK-001 homed on `OTHER` with `uid` in place of the fixture's uid, or
/// with no uid line (a legacy record) when `uid` is `None`.
fn legacy(uid: Option<&str>, note: &str) -> String {
    let text = task("TSK-001", OTHER, "todo", note, "Pending.");
    match uid {
        Some(uid) => text.replace("uid: uid-TSK-001", &format!("uid: {uid}")),
        None => text.replace("uid: uid-TSK-001\n", ""),
    }
}

/// A fixture whose `main` holds TSK-001 as a legacy record without a uid,
/// homed on `home`, with `OTHER` branched from it and a local id registry
/// that binds TSK-001 to [`REGISTERED`].
fn with_registry(home: &str) -> Fixture {
    let fixture = Fixture::new(&[]);
    fixture.write(
        &path_of("TSK-001"),
        &legacy(None, "Work.").replace(OTHER, home),
    );
    fixture.git(&["add", "-A"]);
    fixture.git(&["commit", "-qm", "docs: plan a legacy task"]);
    fixture.git(&["branch", OTHER, "main"]);
    let id = crate::ids::entry::RegId::parse("TSK-001").unwrap();
    let entry =
        crate::ids::entry::Entry::issued(id.clone(), REGISTERED.into(), "work", "t", "main");
    fixture.git(&["switch", "-q", "--orphan", "codeflow/registry"]);
    fixture.write(&id.registry_path(), &entry.render());
    fixture.git(&["add", "-A"]);
    fixture.git(&["commit", "-qm", "ids: issue TSK-001"]);
    fixture.git(&["switch", "-q", "main"]);
    fixture
}

/// [`with_registry`] homed on `OTHER`, then a planning landing on `OTHER`
/// that adds the uid `backfill` to the record, as `ids backfill` does.
fn backfilled(backfill: &str) -> Fixture {
    let fixture = with_registry(OTHER);
    let path = path_of("TSK-001");
    fixture.land(OTHER, &[(&path, &legacy(Some(backfill), "Work."))]);
    fixture
}

/// The registered uid backfilled onto a legacy record keeps its identity:
/// the landing that adds it is taken.
#[test]
fn a_registered_uid_backfill_is_taken_at_the_landing() {
    landing(&backfilled(REGISTERED), &path_of("TSK-001")).unwrap();
}

/// The same backfill under a later edit: the walk crosses it and goes on.
#[test]
fn a_registered_uid_backfill_is_taken_under_a_later_edit() {
    let path = path_of("TSK-001");
    let fixture = backfilled(REGISTERED);
    fixture.land(
        OTHER,
        &[(&path, &legacy(Some(REGISTERED), "Work, with notes."))],
    );
    walk(&fixture, &path).unwrap();
}

/// A first uid other than the one the registry binds is refused.
#[test]
fn an_unregistered_uid_backfill_is_refused() {
    let fixture = backfilled("1f2e3d4c-6b5a-4978-8a6b-5c4d3e2f1a0b");
    refuses(
        &landing(&fixture, &path_of("TSK-001")),
        "an unregistered backfill at the landing",
        PATH_CHANGED,
    );
}

/// A refused move before a registered backfill is still judged: the
/// walk crosses the backfill and reaches the move.
#[test]
fn a_registered_backfill_does_not_clear_an_earlier_refused_move() {
    let path = path_of("TSK-001");
    let fixture = with_registry(LINE);
    fixture.land(OTHER, &[(&path, &legacy(None, "Work.")), PRODUCT]);
    fixture.land(OTHER, &[(&path, &legacy(Some(REGISTERED), "Work."))]);
    refuses(
        &walk(&fixture, &path),
        "a refused move, then a registered backfill",
        MOVED,
    );
}
