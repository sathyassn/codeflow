//! TSK-1378: the home history of a task that a sync of the default branch
//! imports into a line. Each fixture builds `main` and the line, lands
//! changes on either, merges `main` into a sync branch made from the line,
//! and judges the imports of that sync as `codeflow ci` does for a pull
//! request into the line.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::Path;

use super::{imported_records, Graph, ImportedRecord, Repository};

pub(super) const LINE: &str = "integration/EPC-001-outcome";
pub(super) const OTHER: &str = "integration/EPC-002-other";
pub(super) const THIRD: &str = "integration/EPC-003-third";
pub(super) const MOVED: &str = "the landing changes the task's declared home";
pub(super) const NO_SOURCE: &str =
    "the record arrives on its home line by a sync with no proven source history";

const POLICY: &str =
    "{\n  \"schema_version\": 1,\n  \"git\": {\"product_paths\": [\"src/**\"]}\n}\n";
const BLOCK: &str = "```yaml\nacceptance:\n  reviewed: 0123456789abcdef0123456789abcdef01234567\n  review: https://example.test/review/1\n  criteria:\n    AC-1: verified | test ran\n  journey: verified | fixture\n  not_verified: none\n  follow_ups: none: fixture\n  verdict: approved\n```";
const MALFORMED: &str = "```yaml\nacceptance:\n  reviewed: [\n```";

pub(super) fn path_of(id: &str) -> String {
    format!("project-management/tasks/{id}.md")
}

/// A task record declaring `home`. The home, the `note` and the `closeout`
/// sit far enough apart that git merges a change to each on its own.
pub(super) fn task(id: &str, home: &str, status: &str, note: &str, closeout: &str) -> String {
    format!("---\nid: {id}\nuid: uid-{id}\nstatus: {status}\nintegration_target: {home}\n---\n\n# {id}: work\n\n## Description\n\n{note}\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall work.\n\n## Closeout\n\n{closeout}\n")
}

pub(super) fn todo(id: &str, home: &str) -> String {
    task(id, home, "todo", "Work.", "Pending.")
}

/// The task as `main` moves it to `home`, reworded so the sync changes it.
pub(super) fn moved(id: &str, home: &str) -> String {
    task(id, home, "todo", "Work moved.", "Pending.")
}

fn complete(id: &str, home: &str, note: &str) -> String {
    task(id, home, "complete", note, BLOCK)
}

/// A repository whose `main` holds the policy and the given records, with
/// the line branched from it.
pub(super) struct Fixture {
    dir: tempfile::TempDir,
    branches: Cell<u32>,
}

impl Fixture {
    pub(super) fn new(records: &[(&str, String)]) -> Self {
        let fixture = Self {
            dir: tempfile::tempdir().unwrap(),
            branches: Cell::new(0),
        };
        fixture.git(&["init", "-q", "-b", "main"]);
        fixture.git(&["config", "user.email", "t@example.com"]);
        fixture.git(&["config", "user.name", "t"]);
        fixture.write(".codeflow/policy.json", POLICY);
        fixture.write("README.md", "A fixture.\n");
        for (id, text) in records {
            fixture.write(&path_of(id), text);
        }
        fixture.commit("chore: plan the work");
        fixture.git(&["branch", LINE]);
        fixture
    }

    pub(super) fn root(&self) -> &Path {
        self.dir.path()
    }

    pub(super) fn git(&self, args: &[&str]) -> String {
        let out = crate::git::command()
            .args(args)
            .current_dir(self.root())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    pub(super) fn write(&self, path: &str, text: &str) {
        let full = self.root().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, text).unwrap();
    }

    fn commit(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-qm", message]);
    }

    pub(super) fn oid(&self, revision: &str) -> git2::Oid {
        git2::Oid::from_str(&self.git(&["rev-parse", revision])).unwrap()
    }

    /// Commit `files` directly on `branch`.
    pub(super) fn direct(&self, branch: &str, files: &[(&str, &str)]) {
        self.git(&["switch", "-q", branch]);
        for (path, text) in files {
            self.write(path, text);
        }
        self.commit("docs: a direct change");
    }

    /// Land `files` on `branch` as a reviewed merge of a side branch, and
    /// return the merge.
    pub(super) fn land(&self, branch: &str, files: &[(&str, &str)]) -> git2::Oid {
        self.land_with(branch, files, false)
    }

    /// Land `files` on `branch` as a merge whose tree also carries a file
    /// neither parent holds.
    pub(super) fn land_evil(&self, branch: &str, files: &[(&str, &str)]) -> git2::Oid {
        self.land_with(branch, files, true)
    }

    fn land_with(&self, branch: &str, files: &[(&str, &str)], evil: bool) -> git2::Oid {
        let count = self.branches.get() + 1;
        self.branches.set(count);
        let side = format!("plan/change-{count}");
        self.git(&["switch", "-q", "-c", &side, branch]);
        for (path, text) in files {
            self.write(path, text);
        }
        self.commit("docs: plan a change");
        self.git(&["switch", "-q", branch]);
        if evil {
            self.git(&["merge", "--no-ff", "-q", "--no-commit", &side]);
            self.write("project-management/plans/evil.md", "Unreviewed.\n");
            self.commit("chore: land reviewed planning");
        } else {
            self.git(&[
                "merge",
                "--no-ff",
                "-q",
                "-m",
                "chore: land reviewed planning",
                &side,
            ]);
        }
        self.oid("HEAD")
    }

    /// The sync branch: the line with `main` merged in by git's own merge.
    pub(super) fn sync(&self) {
        self.git(&["switch", "-q", "-C", "sync", LINE]);
        self.git(&[
            "merge",
            "--no-ff",
            "-q",
            "-m",
            "chore(merge): take main",
            "main",
        ]);
    }

    /// The imports of the sync, judged against the line.
    pub(super) fn judge(&self) -> Result<BTreeMap<String, ImportedRecord>, String> {
        let repo = Repository::open(self.root()).unwrap();
        let graph = Graph::from_revision(&repo, "sync").unwrap();
        imported_records(&repo, self.oid(LINE), self.oid("sync"), &graph)
    }
}

pub(super) fn passes(fixture: &Fixture, what: &str) -> BTreeMap<String, ImportedRecord> {
    match fixture.judge() {
        Ok(found) => found,
        Err(said) => panic!("{what}: refused: {said}"),
    }
}

pub(super) fn refused(fixture: &Fixture, what: &str, needles: &[&str]) {
    match fixture.judge() {
        Ok(found) => panic!(
            "{what}: accepted, importing {:?}",
            found.keys().collect::<Vec<_>>()
        ),
        Err(said) => {
            for needle in needles {
                assert!(
                    said.contains(needle),
                    "{what}: missing {needle:?} in {said}"
                );
            }
        }
    }
}

/// AC-1: `main` moves a task to itself by a reviewed planning merge whose
/// diff holds only records, and the line already names that home through
/// its own planning merge. The sync carries `main`'s reworded version.
#[test]
fn a_main_planning_move_the_line_already_names_passes() {
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    let path = path_of("TSK-001");
    fixture.land("main", &[(&path, &moved("TSK-001", "main"))]);
    fixture.land(LINE, &[(&path, &todo("TSK-001", "main"))]);
    fixture.sync();
    let found = passes(&fixture, "a move the line names");
    assert_eq!(found["TSK-001"].home, "main");
}

/// AC-2: the line names the old home and changed its copy since the merge
/// base, so the move is the line's disagreement, not an agreed home.
#[test]
fn a_main_move_the_line_changed_since_stays_refused() {
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    let path = path_of("TSK-001");
    fixture.land("main", &[(&path, &moved("TSK-001", "main"))]);
    // The line rewords its copy as main does, and keeps the old home, so
    // the merge result is main's exact version.
    fixture.land(LINE, &[(&path, &moved("TSK-001", OTHER))]);
    fixture.sync();
    refused(
        &fixture,
        "a line that changed its copy",
        &["TSK-001", MOVED],
    );
}

/// AC-2: a home change made by a direct commit on the default branch.
#[test]
fn a_main_move_by_a_direct_commit_stays_refused() {
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    fixture.direct("main", &[(&path_of("TSK-001"), &moved("TSK-001", "main"))]);
    fixture.sync();
    refused(&fixture, "a direct move", &["TSK-001", MOVED]);
}

/// AC-2: a home change in a merge whose diff carries more than records.
#[test]
fn a_main_move_with_product_paths_stays_refused() {
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    fixture.land(
        "main",
        &[
            (&path_of("TSK-001"), &moved("TSK-001", "main")),
            ("src/planned.rs", "pub fn planned() {}\n"),
        ],
    );
    fixture.sync();
    refused(&fixture, "a move beside product code", &["TSK-001", MOVED]);
}

/// AC-2: a local branch named like the default branch that holds the move
/// grants nothing when the published default tip does not hold it.
#[test]
fn a_local_main_holding_the_move_grants_nothing() {
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    let published = fixture.oid("main");
    fixture.land("main", &[(&path_of("TSK-001"), &moved("TSK-001", "main"))]);
    // A remote whose `main` is the commit before the move; nothing is fetched.
    fixture.git(&["remote", "add", "origin", "file:///nonexistent/repo.git"]);
    fixture.git(&[
        "update-ref",
        "refs/remotes/origin/main",
        &published.to_string(),
    ]);
    fixture.sync();
    match fixture.judge() {
        Ok(found) => assert!(
            !found.contains_key("TSK-001"),
            "a local move became an import: {found:?}"
        ),
        Err(said) => assert!(said.contains("TSK-001"), "{said}"),
    }
}

/// AC-6 (a): a todo task homed on another line, moved to `main` by a
/// planning merge, while the line's copy is unchanged since the merge base.
#[test]
fn a_main_move_of_a_todo_task_homed_on_another_line_passes() {
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    fixture.land("main", &[(&path_of("TSK-001"), &moved("TSK-001", "main"))]);
    fixture.sync();
    let found = passes(&fixture, "an unchanged copy of another line's task");
    assert_eq!(found["TSK-001"].home, "main");
}

/// AC-6 (b): a task the line never held, planned on `main` for another
/// line and then moved to `main`, arrives as a new import.
#[test]
fn a_main_move_of_a_task_absent_on_the_line_passes() {
    let fixture = Fixture::new(&[]);
    let path = path_of("TSK-005");
    fixture.land("main", &[(&path, &todo("TSK-005", OTHER))]);
    fixture.land("main", &[(&path, &moved("TSK-005", "main"))]);
    fixture.sync();
    passes(&fixture, "a task absent on the line");
}

/// A case name, the records `main` starts with, and the record its move
/// lands.
type StateCase<'a> = (&'a str, Vec<(&'a str, String)>, String);

/// AC-7: AC-6 fails closed for every condition it cannot prove.
#[test]
fn a_main_move_outside_a_permitted_state_stays_refused() {
    let shared = todo("TSK-003", OTHER).replace("uid: uid-TSK-003", "uid: uid-TSK-001");
    let cases: Vec<StateCase> = vec![
        (
            "complete at the move",
            vec![("TSK-001", todo("TSK-001", OTHER))],
            complete("TSK-001", "main", "Work moved."),
        ),
        (
            "complete before",
            vec![("TSK-001", complete("TSK-001", OTHER, "Work."))],
            complete("TSK-001", "main", "Work moved."),
        ),
        (
            "block before",
            vec![("TSK-001", task("TSK-001", OTHER, "todo", "Work.", BLOCK))],
            task("TSK-001", "main", "todo", "Work moved.", BLOCK),
        ),
        (
            "malformed before",
            vec![(
                "TSK-001",
                task("TSK-001", OTHER, "todo", "Work.", MALFORMED),
            )],
            task("TSK-001", "main", "todo", "Work moved.", MALFORMED),
        ),
        (
            "shared uid",
            vec![("TSK-001", todo("TSK-001", OTHER)), ("TSK-003", shared)],
            moved("TSK-001", "main"),
        ),
    ];
    let mut failures = Vec::new();
    for (case, records, after) in cases {
        let fixture = Fixture::new(&records);
        fixture.land("main", &[(&path_of("TSK-001"), &after)]);
        fixture.sync();
        match fixture.judge() {
            Ok(_) => failures.push(format!("{case}: accepted")),
            Err(said) if !said.contains(MOVED) || !said.contains("TSK-001") => {
                failures.push(format!("{case}: {said}"));
            }
            Err(_) => {}
        }
    }
    // A merge whose tree is not the clean merge of its parents.
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    fixture.land_evil("main", &[(&path_of("TSK-001"), &moved("TSK-001", "main"))]);
    fixture.sync();
    match fixture.judge() {
        Ok(_) => failures.push("evil: accepted".into()),
        Err(said) if !said.contains("TSK-001") => failures.push(format!("evil: {said}")),
        Err(_) => {}
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// AC-8 and AC-9: every refused record is named with its own cause and
/// the routes that clear it; a later change on `main` is not one.
#[test]
fn two_refused_records_are_both_named() {
    let fixture = Fixture::new(&[
        ("TSK-001", todo("TSK-001", OTHER)),
        ("TSK-002", todo("TSK-002", OTHER)),
    ]);
    fixture.direct(
        "main",
        &[
            (&path_of("TSK-001"), &moved("TSK-001", "main")),
            (&path_of("TSK-002"), &moved("TSK-002", "main")),
        ],
    );
    fixture.sync();
    refused(
        &fixture,
        "two direct moves",
        &[
            "TSK-001: ",
            "TSK-002: ",
            MOVED,
            "reviewed planning merge",
            "does not clear the move",
        ],
    );
}

/// AC-10: a later reviewed note on `main` does not clear an earlier
/// refused move.
#[test]
fn a_refused_move_then_a_reviewed_note_stays_refused() {
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    let path = path_of("TSK-001");
    fixture.direct("main", &[(&path, &moved("TSK-001", "main"))]);
    fixture.land(
        "main",
        &[(
            &path,
            &task("TSK-001", "main", "todo", "Work noted.", "Pending."),
        )],
    );
    fixture.sync();
    refused(&fixture, "a note after a direct move", &["TSK-001", MOVED]);
}

/// AC-10: of two moves on `main`, the second is refused, and a later note
/// does not clear it.
#[test]
fn a_second_refused_move_stays_refused() {
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    let path = path_of("TSK-001");
    fixture.land("main", &[(&path, &todo("TSK-001", THIRD))]);
    fixture.direct("main", &[(&path, &moved("TSK-001", "main"))]);
    fixture.land(
        "main",
        &[(
            &path,
            &task("TSK-001", "main", "todo", "Work noted.", "Pending."),
        )],
    );
    fixture.sync();
    refused(&fixture, "a direct second move", &["TSK-001", MOVED]);
}

/// AC-10: a valid move and its valid revert pass; an inadmissible move is
/// not undone by a revert, because the walk never resets.
#[test]
fn a_move_and_its_revert() {
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", "main"))]);
    let path = path_of("TSK-001");
    fixture.land("main", &[(&path, &todo("TSK-001", OTHER))]);
    fixture.land("main", &[(&path, &moved("TSK-001", "main"))]);
    fixture.sync();
    passes(&fixture, "a planning move and its planning revert");

    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", "main"))]);
    fixture.direct("main", &[(&path, &todo("TSK-001", OTHER))]);
    fixture.land("main", &[(&path, &moved("TSK-001", "main"))]);
    fixture.sync();
    refused(
        &fixture,
        "a direct move and its revert",
        &["TSK-001", MOVED],
    );
}

/// AC-10: a move before the last one must be in a permitted state at both
/// ends, even when the line names the final home.
#[test]
fn a_move_while_complete_before_the_last_move_is_refused() {
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    let path = path_of("TSK-001");
    fixture.land("main", &[(&path, &complete("TSK-001", THIRD, "Work."))]);
    fixture.land(
        "main",
        &[(&path, &complete("TSK-001", "main", "Work moved."))],
    );
    // The line names the final home and matches main's status and closeout
    // lines, so git merges the note change on its own.
    fixture.land(LINE, &[(&path, &complete("TSK-001", "main", "Work."))]);
    fixture.sync();
    refused(
        &fixture,
        "a complete intermediate move",
        &["TSK-001", MOVED],
    );
}

/// AC-12: a record homed on the receiving line that the line never held
/// arrives only with its source history proven.
#[test]
fn a_line_homed_record_absent_on_the_line() {
    let path = path_of("TSK-005");
    let fixture = Fixture::new(&[]);
    fixture.direct("main", &[(&path, &todo("TSK-005", LINE))]);
    fixture.sync();
    refused(&fixture, "a direct introduction", &["TSK-005", NO_SOURCE]);

    let fixture = Fixture::new(&[]);
    fixture.land("main", &[(&path, &todo("TSK-005", LINE))]);
    fixture.sync();
    passes(&fixture, "a planning introduction");
}

/// AC-12: a record the line holds for another line, which `main` moves to
/// the receiving line, passes only on the move's proof.
#[test]
fn a_record_moved_to_the_receiving_line_by_main() {
    let path = path_of("TSK-001");
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    fixture.land("main", &[(&path, &moved("TSK-001", LINE))]);
    fixture.sync();
    passes(&fixture, "a planning move to the line");

    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    fixture.direct("main", &[(&path, &moved("TSK-001", LINE))]);
    fixture.sync();
    refused(&fixture, "a direct move to the line", &["TSK-001", MOVED]);
}

/// The record as `main` moves it in a merge that also carries a doc.
pub(super) fn docs_move(fixture: &Fixture, after: &str) -> git2::Oid {
    fixture.land(
        "main",
        &[
            (&path_of("TSK-001"), after),
            ("docs/decision.md", "A decision.\n"),
        ],
    )
}

/// AC-13 (2): an adoption by a direct commit on the line sets no anchor,
/// and the sync then judges `main`'s move.
#[test]
fn an_adoption_by_a_direct_commit_is_refused() {
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    let after = moved("TSK-001", "main");
    docs_move(&fixture, &after);
    fixture.direct(LINE, &[(&path_of("TSK-001"), &after)]);
    fixture.sync();
    refused(&fixture, "a direct adoption", &["TSK-001"]);
}

/// AC-13 (3): an adoption in a landing that also carries a doc.
#[test]
fn an_adoption_in_a_docs_bearing_landing_is_refused() {
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    let after = moved("TSK-001", "main");
    docs_move(&fixture, &after);
    fixture.land(
        LINE,
        &[
            (&path_of("TSK-001"), &after),
            ("docs/line.md", "The line's note.\n"),
        ],
    );
    fixture.sync();
    refused(&fixture, "a docs-bearing adoption", &["TSK-001"]);
}

/// AC-13 (5): an adoption of a completed task.
#[test]
fn an_adoption_of_a_completed_task_is_refused() {
    let fixture = Fixture::new(&[("TSK-001", complete("TSK-001", OTHER, "Work."))]);
    let after = complete("TSK-001", "main", "Work moved.");
    docs_move(&fixture, &after);
    fixture.land(LINE, &[(&path_of("TSK-001"), &after)]);
    fixture.sync();
    refused(&fixture, "a completed adoption", &["TSK-001"]);
}

/// AC-13 (6): a line introduction of `main`'s exact bytes is admitted as
/// shared planning, never as an adoption, so `main`'s move is judged.
#[test]
fn an_introduction_used_as_an_adoption_is_refused() {
    let fixture = Fixture::new(&[]);
    let path = path_of("TSK-006");
    fixture.land("main", &[(&path, &todo("TSK-006", OTHER))]);
    let after = moved("TSK-006", "main");
    fixture.land(
        "main",
        &[(&path, &after), ("docs/decision.md", "A decision.\n")],
    );
    fixture.land(LINE, &[(&path, &after)]);
    fixture.sync();
    refused(
        &fixture,
        "an introduction claimed as an adoption",
        &["TSK-006"],
    );
}

/// AC-13 (7): after an adoption, a later unauthorized move on `main` is
/// still judged.
#[test]
fn a_later_unauthorized_move_after_an_adoption_is_refused() {
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    let path = path_of("TSK-001");
    let after = moved("TSK-001", "main");
    docs_move(&fixture, &after);
    fixture.land(LINE, &[(&path, &after)]);
    fixture.direct("main", &[(&path, &moved("TSK-001", THIRD))]);
    // The later note goes in the closeout, so the sync merges cleanly over
    // the line's adopted note.
    fixture.land(
        "main",
        &[(
            &path,
            &task("TSK-001", "main", "todo", "Work moved.", "Noted."),
        )],
    );
    fixture.sync();
    refused(&fixture, "a later direct move", &["TSK-001", MOVED]);
}

/// AC-13 (b): the line's own planning move to the home `main` also chose
/// admits the line's change but removes none of `main`'s moves: a refused
/// move on `main` still refuses, and a valid one passes.
#[test]
fn a_same_destination_own_move_still_judges_mains_moves() {
    let path = path_of("TSK-001");
    let own = task("TSK-001", "main", "todo", "Work.", "Pending on the line.");

    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    fixture.direct("main", &[(&path, &todo("TSK-001", "main"))]);
    fixture.land(LINE, &[(&path, &own)]);
    fixture.sync();
    refused(
        &fixture,
        "a direct move beside the line's own",
        &["TSK-001", MOVED],
    );

    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", OTHER))]);
    fixture.land("main", &[(&path, &todo("TSK-001", "main"))]);
    fixture.land(LINE, &[(&path, &own)]);
    fixture.sync();
    passes(&fixture, "a planning move beside the line's own");
}

/// TSK-1605 Part 1: when a receiving line carries the same criteria signature
/// as its unique merge base with `main` (for example after re-homing the task
/// to `main` or merging a branch that lacked the task), it has not amended the
/// criteria and does not compete against `main`'s settled criteria.
#[test]
fn line_unchanged_criteria_yields_to_main_and_amended_criteria_still_competes() {
    let path = path_of("TSK-001");
    let main_amended = "---\nid: TSK-001\nuid: uid-TSK-001\nstatus: todo\nintegration_target: main\n---\n\n# TSK-001: work\n\n## Description\n\nRe-scoped on main.\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall work.\n- AC-2 Main added a second requirement.\n\n## Closeout\n\nPending.\n";

    // Case 1: LINE re-homes TSK-001 to main without changing its criteria,
    // while main amends the criteria and moves it to main. LINE's criteria
    // match the merge base, so sync passes.
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", LINE))]);
    // Create a branch from before TSK-001 existed (empty root) or a side merge
    // on LINE that re-homes TSK-001 without touching AC-1.
    fixture.land("main", &[(&path, main_amended)]);
    fixture.land(LINE, &[(&path, &todo("TSK-001", "main"))]);
    fixture.sync();
    passes(&fixture, "line re-homed without criteria change");

    // Case 2: LINE independently amends AC-1 on its own branch. Its criteria
    // now differ from the merge base, so it still competes and refuses.
    let line_amended = "---\nid: TSK-001\nuid: uid-TSK-001\nstatus: todo\nintegration_target: main\n---\n\n# TSK-001: work\n\n## Description\n\nWork.\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall work.\n- AC-9 Line added a competing requirement.\n\n## Closeout\n\nPending.\n";
    let fixture = Fixture::new(&[("TSK-001", todo("TSK-001", LINE))]);
    fixture.land("main", &[(&path, main_amended)]);
    fixture.land(LINE, &[(&path, line_amended)]);
    fixture.sync();
    refused(
        &fixture,
        "line independently amended criteria",
        &["TSK-001", "accepted criteria cannot be resolved"],
    );
}

/// TSK-1605 Part 2 (AC-2): a task filed on `main` via a reviewed mixed
/// planning merge carrying `docs/` and an ADR alongside task records (the
/// PR #925 shape) syncs cleanly onto its declared home line, while a merge
/// carrying product code still refuses with NO_SOURCE.
#[test]
fn main_filed_mixed_planning_merge_syncs_to_declared_home_line() {
    let path = path_of("TSK-009");
    let record = todo("TSK-009", LINE);

    let fixture = Fixture::new(&[]);
    fixture.land(
        "main",
        &[
            (&path, &record),
            ("docs/delivery.md", "Delivery update.\n"),
            ("docs/decisions/ADR-0102.md", "# ADR-0102\n"),
        ],
    );
    fixture.sync();
    passes(&fixture, "PR #925 mixed planning merge syncs to home line");

    let fixture_bad = Fixture::new(&[]);
    fixture_bad.land(
        "main",
        &[(&path, &record), ("src/lib.rs", "pub fn sneaky() {}\n")],
    );
    fixture_bad.sync();
    refused(
        &fixture_bad,
        "product-bearing arrival on main still refuses",
        &["TSK-009", NO_SOURCE],
    );
}

/// TSK-1605 Part 3: `git.import_authority_settled_before` in trusted `main`'s
/// policy marks historical default-branch completions at or before that exact
/// first-parent SHA as settled on a sync, while completions after it still
/// undergo full completion proof, and non-first-parent values fail closed.
#[test]
fn settled_before_cutover_covers_prior_main_completions_only() {
    let p1 = path_of("TSK-010");
    let p2 = path_of("TSK-011");

    // Without cutover: a completion on main whose acceptance block cannot be
    // proven (synthetic unreviewed SHA) is refused on sync.
    let fixture = Fixture::new(&[
        ("TSK-010", todo("TSK-010", "main")),
        ("TSK-011", todo("TSK-011", "main")),
    ]);
    let settled_merge = fixture.land("main", &[(&p1, &complete("TSK-010", "main", "Settled."))]);
    fixture.sync();
    refused(&fixture, "unset cutover still refuses", &["TSK-010"]);

    // With cutover set in trusted main policy to `settled_merge`: TSK-010 (at
    // cutover) passes on sync, while TSK-011 (landed AFTER cutover) still refuses.
    let policy_with_cutover = format!(
        "{{\n  \"schema_version\": 1,\n  \"git\": {{\"product_paths\": [\"src/**\"], \"import_authority_settled_before\": \"{settled_merge}\"}}\n}}\n"
    );
    fixture.land("main", &[(".codeflow/policy.json", &policy_with_cutover)]);
    fixture.sync();
    passes(&fixture, "completion at cutover passes with trusted policy");

    fixture.land("main", &[(&p2, &complete("TSK-011", "main", "After cutover."))]);
    fixture.sync();
    refused(&fixture, "completion after cutover still refuses", &["TSK-011"]);

    // Non-first-parent / unknown SHA fails closed.
    let bad_policy = "{\n  \"schema_version\": 1,\n  \"git\": {\"product_paths\": [\"src/**\"], \"import_authority_settled_before\": \"0123456789abcdef0123456789abcdef01234567\"}\n}\n";
    fixture.land("main", &[(".codeflow/policy.json", bad_policy)]);
    fixture.sync();
    refused(
        &fixture,
        "non-first-parent cutover fails closed",
        &["import_authority_settled_before", "first-parent"],
    );
}
