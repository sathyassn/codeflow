//! The epic line's pull request into `main` judges the line's own
//! completions as its own (TSK-433, issues #320 and #341): a completion
//! landed on the line is bound at its line landing, as the task pull
//! request that landed it was judged.
use super::*;

/// TSK-001 as a task of EPC-001 with `criteria`, targeting [`LINE`].
fn own_task(status: &str, criteria: &str, closeout: &str) -> String {
    task("TSK-001", status, criteria, closeout).replace(
        "integration_target: main",
        &format!("integration_target: {LINE}"),
    )
}

/// A line cut from `main` that plans TSK-001 through a planning merge, so
/// `main` never holds the task, as on the 3.2.0 line. TSK-002 binds the
/// line to EPC-001.
fn planned_line() -> tempfile::TempDir {
    let dir = line_repo(&["TSK-002"]);
    let root = dir.path();
    git(root, &["switch", "-q", "-c", "plan/add-task", LINE]);
    write(
        root,
        &record_path("TSK-001"),
        &own_task("todo", OWN_JOURNEY, "Pending.\n"),
    );
    commit(root, "docs(records): plan TSK-001 on the line");
    land(root, "plan/add-task");
    dir
}

/// On a task branch cut from [`LINE`], TSK-001's own pull request adds a
/// criterion, completes, reopens and completes again before it lands, as
/// each of the line's eleven refused tasks did (TSK-275 and others); then
/// it lands on the line. Returns the task pull request's `codeflow ci`
/// into the line, run before the landing.
fn reopened_in_its_own_pull_request(root: &Path) -> (i32, String) {
    let branch = "task/TSK-001-work";
    git(root, &["switch", "-q", "-c", branch, LINE]);
    write(root, "src/lib.rs", "pub fn initial() {}\n");
    let first = commit(root, "feat: initial work");
    let old = fix_block(&first);
    write(
        root,
        &record_path("TSK-001"),
        &own_task("complete", REOPEN_ADDS, &old),
    );
    commit(root, "docs(records): complete the task");
    let archived = old.replace(
        "acceptance:\n",
        "acceptance_superseded:\n  reason: one more case\n",
    );
    write(
        root,
        &record_path("TSK-001"),
        &own_task("todo", REOPEN_ADDS, &archived),
    );
    commit(root, "docs(records): reopen in the task range");
    write(root, "src/lib.rs", "pub fn fixed() {}\n");
    let reviewed = commit(root, "fix: keep working when rerun");
    let closeout = format!("{archived}{}", three_criteria_block(&reviewed));
    write(
        root,
        &record_path("TSK-001"),
        &own_task("complete", REOPEN_ADDS, &closeout),
    );
    commit(root, "docs(records): complete the task again");
    let into_line = ci_on(root, LINE, branch, "Task: TSK-001");
    land(root, branch);
    into_line
}

/// TSK-433 AC-1: the line's own task, completed, reopened and completed
/// again inside its own pull request, which also changed its criteria
/// before its first landing, binds at its line landing on the line's pull
/// request into `main`, as the task pull request into the line did. Before
/// TSK-433 the line's tip, which is the head of this very range, counted as
/// a target the run is not judged against and refused it twice.
#[test]
fn the_lines_own_reopened_completion_binds_at_its_line_landing() {
    let dir = planned_line();
    let root = dir.path();
    let into_line = reopened_in_its_own_pull_request(root);
    for rule in ["work.acceptance_binding", "work.criteria_frozen (block)"] {
        assert!(
            !into_line.1.contains(rule),
            "the task pull request into the line: {rule}: {}",
            into_line.1
        );
    }
    let into_main = ci_on(root, "main", LINE, "");
    for refusal in [
        "records a completion of this task, but it is not the target this run is judged against",
        "a reopened task keeps its criteria as the anchored target has them",
    ] {
        assert!(
            !into_main.1.contains(refusal),
            "the line into main: {refusal}: {}",
            into_main.1
        );
    }
    assert!(
        !into_main.1.contains("work.acceptance_binding (block)"),
        "the line into main: {}",
        into_main.1
    );
}

/// The two binding refusals TSK-433 removes for the line's own completions.
const OUTSIDE_TIP: &str =
    "records a completion of this task, but it is not the target this run is judged against";

/// TSK-433 AC-2: the line's tip counts as the range only up to the range
/// head. A tip of the line beyond the head, here a later landing the run is
/// not given, stays a target the run is not judged against and refuses.
#[test]
fn a_line_tip_beyond_the_judged_head_stays_outside_evidence() {
    let dir = planned_line();
    let root = dir.path();
    reopened_in_its_own_pull_request(root);
    let judged = head(root);
    build(root, "task/TSK-002-later", "src/later.rs");
    land(root, "task/TSK-002-later");
    git(root, &["switch", "-q", "--detach", &judged]);
    assert_blocks(
        &ci_on(root, "main", LINE, ""),
        "the line into main at an older head",
        &["work.acceptance_binding", "TSK-001", OUTSIDE_TIP],
    );
}

/// TSK-433 AC-2: a task the line carries that declares `main` keeps today's
/// judgment. Its completion landed on `main` after the line forked, so the
/// reopen inside its pull request on the line keeps `main`'s criteria.
#[test]
fn a_task_targeting_main_keeps_its_landed_criteria() {
    let dir = planned_line();
    let root = dir.path();
    let to_main = |record: String| {
        record.replace(
            &format!("integration_target: {LINE}"),
            "integration_target: main",
        )
    };
    git(
        root,
        &["switch", "-q", "-c", "task/TSK-001-on-main", "main"],
    );
    write(root, "src/main.rs", "pub fn landed() {}\n");
    let landed = commit(root, "feat: land on main");
    write(
        root,
        &record_path("TSK-001"),
        &to_main(own_task("complete", OWN_JOURNEY, &fix_block(&landed))),
    );
    commit(root, "docs(records): complete on main");
    git(root, &["switch", "-q", "main"]);
    git(
        root,
        &[
            "merge",
            "--no-ff",
            "-m",
            "merge: main landing",
            "task/TSK-001-on-main",
        ],
    );
    git(root, &["switch", "-q", LINE]);
    let branch = "task/TSK-001-work";
    git(root, &["switch", "-q", "-c", branch, LINE]);
    write(root, "src/lib.rs", "pub fn initial() {}\n");
    let first = commit(root, "feat: initial work");
    let old = fix_block(&first);
    write(
        root,
        &record_path("TSK-001"),
        &to_main(own_task("complete", REOPEN_ADDS, &old)),
    );
    commit(root, "docs(records): complete the task");
    let archived = old.replace(
        "acceptance:\n",
        "acceptance_superseded:\n  reason: one more case\n",
    );
    write(
        root,
        &record_path("TSK-001"),
        &to_main(own_task("todo", REOPEN_ADDS, &archived)),
    );
    commit(root, "docs(records): reopen in the task range");
    write(root, "src/lib.rs", "pub fn fixed() {}\n");
    let reviewed = commit(root, "fix: keep working when rerun");
    let closeout = format!("{archived}{}", three_criteria_block(&reviewed));
    write(
        root,
        &record_path("TSK-001"),
        &to_main(own_task("complete", REOPEN_ADDS, &closeout)),
    );
    commit(root, "docs(records): complete the task again");
    land(root, branch);
    assert_blocks(
        &ci_on(root, "main", LINE, ""),
        "a task of main carried by the line",
        &["work.acceptance_binding", "TSK-001", "keeps its criteria"],
    );
}

/// TSK-433 AC-2: a completion written straight onto the line, with no task
/// pull request and no landed adoption, leaves the line unverified, so the
/// range is no epic line and the completion is refused.
#[test]
fn a_completion_forged_on_the_line_stays_refused() {
    let dir = planned_line();
    let root = dir.path();
    let reviewed = build(root, "task/TSK-001-work", "src/work.rs");
    land(root, "task/TSK-001-work");
    write_done(root, "TSK-001", "complete", &reviewed);
    commit(root, "docs(records): complete TSK-001 on the line");
    let result = ci_on(root, "main", LINE, "");
    assert_ne!(result.0, 0, "a forged completion on the line: {}", result.1);
}

/// TSK-433 AC-2: after the landing, a later merge on the line changes the
/// completed record's active acceptance block, so the record at the head
/// differs from the landed record and the binding refuses.
#[test]
fn a_record_changed_after_its_line_landing_stays_refused() {
    let dir = planned_line();
    let root = dir.path();
    reopened_in_its_own_pull_request(root);
    git(root, &["switch", "-q", "-c", "task/TSK-002-edit", LINE]);
    let record = std::fs::read_to_string(root.join(record_path("TSK-001"))).unwrap();
    let (archive, active) = record.rsplit_once("acceptance:\n").unwrap();
    write(
        root,
        &record_path("TSK-001"),
        &format!(
            "{archive}acceptance:\n{}",
            active.replace("AC-3: verified | unit", "AC-3: verified | a later claim")
        ),
    );
    write(root, "src/edit.rs", "pub fn edit() {}\n");
    commit(root, "feat: edit the completed record");
    land(root, "task/TSK-002-edit");
    let result = ci_on(root, "main", LINE, "");
    assert_blocks(
        &result,
        "a record changed after its landing",
        &["work.acceptance_binding", "TSK-001"],
    );
}

/// TSK-433 AC-2: replacement refs that overlay the line's history make the
/// history unreadable, which refuses rather than trusting the line.
#[test]
fn an_overlaid_line_history_stays_refused() {
    let dir = planned_line();
    let root = dir.path();
    reopened_in_its_own_pull_request(root);
    let tip = head(root);
    git(root, &["replace", &tip, "main"]);
    let result = ci_on(root, "main", LINE, "");
    assert_ne!(result.0, 0, "an overlaid line: {}", result.1);
    assert!(
        result.1.contains("overlays") || result.1.contains("replace"),
        "{}",
        result.1
    );
}

/// TSK-433 AC-8, issue #320's shape: after the line's own reopened and
/// re-completed task lands, `main` gains a file and the line merges `main`.
/// On the verified line the completion binds at its landing, so the file
/// the sync merge brought, equal to `main`'s, is not counted against the
/// task's reviewed commit.
#[test]
fn a_sync_merge_of_main_after_the_landing_is_not_counted() {
    let dir = planned_line();
    let root = dir.path();
    reopened_in_its_own_pull_request(root);
    git(root, &["switch", "-q", "main"]);
    write(root, "src/synced.rs", "pub fn synced() {}\n");
    commit(root, "feat: work on main");
    git(root, &["switch", "-q", LINE]);
    git(
        root,
        &["merge", "--no-ff", "-m", "chore(merge): sync main", "main"],
    );
    let into_main = ci_on(root, "main", LINE, "");
    assert!(
        !into_main.1.contains("work.acceptance_binding (block)"),
        "the line into main after a sync merge: {}",
        into_main.1
    );
    assert!(!into_main.1.contains("src/synced.rs"), "{}", into_main.1);
}
