//! TSK-1378: a sync of the default branch into a line keeps a home that
//! the default branch moved through a reviewed planning merge when the line
//! already names it, judges every home move since the merge base, and names
//! every record it refuses. The fixtures build on `main_sync`'s.

use std::path::Path;

use super::main_sync::{
    blocks, ci, commit, fixture, passes, stdout_of, sync_branch, task, text, write, FRAGMENT, LINE,
};

const RECORD: &str = "project-management/tasks/TSK-001.md";
const MOVED: &str = "the landing changes the task's declared home";

/// TSK-001 completed with `reviewed` as its reviewed commit.
fn completed(target: &str, reviewed: &str) -> String {
    format!(
        "{}\n## Closeout\n\n```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: https://example.test/review/1\n  criteria:\n    AC-1: verified | test ran\n    AC-2: verified | journey ran\n  journey: verified | CLI fixture\n  not_verified: none\n  follow_ups: none: fixture\n  verdict: approved\n```\n",
        task("TSK-001", target).replace("status: todo", "status: complete")
    )
}

/// Land `files` on `branch` as a reviewed merge of a side branch `side`.
fn land(root: &Path, branch: &str, side: &str, files: &[(&str, &str)]) {
    super::git(root, &["switch", "-q", "-c", side, branch]);
    for (path, content) in files {
        write(root, path, content);
    }
    commit(root, "docs: plan a change");
    super::git(root, &["switch", "-q", branch]);
    super::git(root, &["merge", "--no-ff", "-q", "--no-edit", side]);
}

/// AC-3 (journey), the shape found on 2026-11-07: `main` completes the line
/// task TSK-001, then retargets it to itself by a reviewed planning merge
/// whose diff touches only records; the line takes that home through its
/// own planning merge; the sync of `main` into the line is accepted.
#[test]
fn a_main_planning_move_the_line_already_names_passes() {
    let dir = fixture();
    let root = dir.path();
    super::git(root, &["switch", "-q", "-c", "task/TSK-001-work", "main"]);
    write(root, "src/work.rs", "pub fn work() {}\n");
    write(root, "changelog.d/TSK-001.md", FRAGMENT);
    commit(root, "feat: the work");
    let reviewed = stdout_of(root, &["rev-parse", "HEAD"]);
    write(root, RECORD, &completed(LINE, &reviewed));
    commit(root, "docs: complete TSK-001");
    super::git(root, &["switch", "-q", "main"]);
    super::git(
        root,
        &["merge", "--no-ff", "-q", "--no-edit", "task/TSK-001-work"],
    );
    land(
        root,
        "main",
        "plan/retarget",
        &[(RECORD, &completed("main", &reviewed))],
    );
    land(
        root,
        LINE,
        "plan/line-home",
        &[(RECORD, &task("TSK-001", "main"))],
    );
    sync_branch(root);
    let synced = ci(root, "Task: EPC-001", None);
    passes(&synced, "a move the line already names");
    assert!(
        !text(&synced).contains("work.import_authority"),
        "{}",
        text(&synced)
    );
}

/// Decision 1 of 2026-11-12: a todo task homed on the line that `main`
/// moves by a planning merge passes the import check (its line copy is
/// unchanged), and the criteria freeze still refuses the sync.
#[test]
fn a_line_homed_todo_task_moved_by_main_stays_frozen() {
    let dir = fixture();
    let root = dir.path();
    land(
        root,
        "main",
        "plan/move",
        &[(
            RECORD,
            &task("TSK-001", "main").replace("Work.", "Other work."),
        )],
    );
    sync_branch(root);
    let refused = ci(root, "Task: EPC-001", None);
    blocks(&refused, "a line-homed task", "work.criteria_frozen");
    blocks(&refused, "a line-homed task", "whose home is the line");
}

/// AC-8: the refusal names every refused record with its own cause.
#[test]
fn two_refused_records_are_both_named() {
    let dir = fixture();
    let root = dir.path();
    let second = "project-management/tasks/TSK-004.md";
    write(root, second, &task("TSK-004", LINE));
    commit(root, "docs: plan a second line task");
    super::git(root, &["switch", "-q", LINE]);
    super::git(root, &["merge", "--no-ff", "-q", "--no-edit", "main"]);
    super::git(root, &["switch", "-q", "main"]);
    for (path, id) in [(RECORD, "TSK-001"), (second, "TSK-004")] {
        write(
            root,
            path,
            &task(id, "main").replace("Work.", "Other work."),
        );
    }
    commit(root, "docs: main re-homes two line tasks");
    sync_branch(root);
    let refused = ci(root, "Task: EPC-001", None);
    blocks(&refused, "two re-homed tasks", "work.import_authority");
    let said = text(&refused);
    for id in ["TSK-001: ", "TSK-004: "] {
        assert!(said.contains(id), "missing {id:?} in {said}");
    }
    assert!(said.matches(MOVED).count() >= 2, "{said}");
}
