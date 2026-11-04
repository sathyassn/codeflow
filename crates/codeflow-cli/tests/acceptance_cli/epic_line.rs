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
    planned_line_with("")
}

/// [`planned_line`] with `extra_policy` inside the policy's `git` object.
fn planned_line_with(extra_policy: &str) -> tempfile::TempDir {
    let dir = line_repo_with(&["TSK-002"], extra_policy);
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
    reopened_on(
        root,
        LINE,
        "task/TSK-001-work",
        "TSK-001",
        &|status, criteria, closeout| own_task(status, criteria, closeout),
    )
}

/// [`reopened_in_its_own_pull_request`] for the task `id`, written by
/// `record`, on `branch` cut from `line` and landed there.
fn reopened_on(
    root: &Path,
    line: &str,
    branch: &str,
    id: &str,
    record: &dyn Fn(&str, &str, &str) -> String,
) -> (i32, String) {
    reopened_from(root, line, (line, None), branch, id, record)
}

/// [`reopened_on`] with the task branch cut from `from.0` and, after its
/// first commit, merging `from.1` when given, as TSK-422's branch was cut
/// from the 3.2.0 line and then took the EPC-025 line.
fn reopened_from(
    root: &Path,
    line: &str,
    from: (&str, Option<&str>),
    branch: &str,
    id: &str,
    record: &dyn Fn(&str, &str, &str) -> String,
) -> (i32, String) {
    git(root, &["switch", "-q", "-c", branch, from.0]);
    let code = format!("src/{}.rs", id.to_lowercase().replace('-', "_"));
    write(root, &code, &format!("// {branch}: initial\n"));
    let mut first = commit(root, "feat: initial work");
    if let Some(sync) = from.1 {
        git(
            root,
            &[
                "merge",
                "--no-ff",
                "-m",
                "chore(merge): take the task's line",
                sync,
            ],
        );
        first = head(root);
    }
    let old = fix_block(&first);
    write(
        root,
        &record_path(id),
        &record("complete", REOPEN_ADDS, &old),
    );
    commit(root, "docs(records): complete the task");
    let archived = old.replace(
        "acceptance:\n",
        "acceptance_superseded:\n  reason: one more case\n",
    );
    write(
        root,
        &record_path(id),
        &record("todo", REOPEN_ADDS, &archived),
    );
    commit(root, "docs(records): reopen in the task range");
    write(root, &code, &format!("// {branch}: fixed\n"));
    let reviewed = commit(root, "fix: keep working when rerun");
    let closeout = format!("{archived}{}", three_criteria_block(&reviewed));
    write(
        root,
        &record_path(id),
        &record("complete", REOPEN_ADDS, &closeout),
    );
    commit(root, "docs(records): complete the task again");
    let into_line = ci_on(root, line, branch, &format!("Task: {id}"));
    land_on(root, line, branch);
    into_line
}

/// Land `branch` on `line` with a merge commit.
fn land_on(root: &Path, line: &str, branch: &str) {
    git(root, &["switch", "-q", line]);
    git(
        root,
        &[
            "merge",
            "--no-ff",
            "-m",
            &format!("merge: {branch}"),
            branch,
        ],
    );
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
    let completion = git_out(root, &["rev-parse", "task/TSK-001-work"]);
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
    assert_bound_at(&into_main, "TSK-001", &completion);
}

/// The two binding refusals TSK-433 removes for the line's own completions.
const OUTSIDE_TIP: &str =
    "records a completion of this task, but it is not the target this run is judged against";

/// Assert that `result` bound `id`'s completion at `at`, the commit on its
/// task branch that made it, which its line landing carried.
fn assert_bound_at(result: &(i32, String), id: &str, at: &str) {
    let note = format!("{id} completion bound at {at}");
    assert!(result.1.contains(&note), "missing {note:?} in {}", result.1);
}

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
    assert_blocks(
        &result,
        "a forged completion on the line",
        &[
            "work.classification",
            &format!("'{LINE}' is not a verified epic line"),
            "has a commit made directly on the line",
            "work.acceptance_binding",
            "TSK-001: the history after the reviewed commit",
        ],
    );
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
    let completion = git_out(root, &["rev-parse", "task/TSK-001-work"]);
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
    assert_bound_at(&into_main, "TSK-001", &completion);
}

/// A second epic's line, which joins [`LINE`] as EPC-025's line joined the
/// 3.2.0 line (TSK-422 through TSK-1000).
const JOINED: &str = "integration/EPC-002-joined";

/// `id` as a task of EPC-002 targeting `target`.
fn joined_task(id: &str, target: &str, status: &str, criteria: &str, closeout: &str) -> String {
    task(id, status, criteria, closeout)
        .replace("epic_id: EPC-001", "epic_id: EPC-002")
        .replace(
            "integration_target: main",
            &format!("integration_target: {target}"),
        )
}

/// [`planned_line`] where a planning merge on [`LINE`] adds EPC-002 with
/// TSK-003 and TSK-004 (targeting [`JOINED`], so the line names its epic's
/// task), then [`JOINED`] cut from [`LINE`], as EPC-025 was planned on the
/// 3.2.0 line and its line cut from it. TSK-003 first targets [`LINE`]; a
/// planning merge on [`JOINED`] moves it to `target`, as TSK-422 first
/// targeted the 3.2.0 line.
fn joined_repo(target: &str) -> tempfile::TempDir {
    let dir = planned_line();
    let root = dir.path();
    git(root, &["switch", "-q", "-c", "plan/EPC-002", LINE]);
    write(
        root,
        "project-management/epics/EPC-002.md",
        &EPIC.replace("EPC-001", "EPC-002"),
    );
    write(
        root,
        &record_path("TSK-003"),
        &joined_task("TSK-003", LINE, "todo", OWN_JOURNEY, "Pending.\n"),
    );
    write(
        root,
        &record_path("TSK-004"),
        &joined_task("TSK-004", JOINED, "todo", OWN_JOURNEY, "Pending.\n"),
    );
    commit(root, "docs(records): plan EPC-002 and its own line");
    land_on(root, LINE, "plan/EPC-002");
    git(root, &["branch", JOINED, LINE]);
    git(
        root,
        &["switch", "-q", "-c", "plan/EPC-002-own-line", JOINED],
    );
    write(
        root,
        &record_path("TSK-003"),
        &joined_task("TSK-003", target, "todo", OWN_JOURNEY, "Pending.\n"),
    );
    commit(root, "docs(records): move TSK-003 to its own line");
    land_on(root, JOINED, "plan/EPC-002-own-line");
    dir
}

/// TSK-003's own pull request on [`JOINED`], reopened and re-completed
/// before its first landing, landed there. Its branch is cut from [`LINE`]
/// and takes [`JOINED`], so its own range holds a version of the record
/// that names [`LINE`], as TSK-422's did.
fn reopened_on_joined(root: &Path, target: &'static str) {
    reopened_from(
        root,
        JOINED,
        (LINE, Some(JOINED)),
        "task/TSK-003-work",
        "TSK-003",
        &move |status, criteria, closeout| {
            joined_task("TSK-003", target, status, criteria, closeout)
        },
    );
}

/// [`JOINED`] joins [`LINE`] as TSK-1000 joined EPC-025's line. First a
/// planning merge on [`LINE`] rewords TSK-003's criteria to the text its
/// own pull request settled, as the 3.2.0 line did for TSK-422, so the
/// line's head owns the record; then a task branch cut from [`LINE`]
/// merges [`JOINED`] and lands on [`LINE`].
fn join(root: &Path, target: &str) {
    git(root, &["switch", "-q", "-c", "plan/EPC-001-amend", LINE]);
    write(
        root,
        &record_path("TSK-003"),
        &joined_task("TSK-003", target, "todo", REOPEN_ADDS, "Pending.\n"),
    );
    commit(root, "docs(plan): reword TSK-003 on the line");
    land_on(root, LINE, "plan/EPC-001-amend");
    git(root, &["switch", "-q", "-c", "task/TSK-002-join", LINE]);
    git(
        root,
        &[
            "merge",
            "--no-ff",
            "-m",
            "chore(merge): take the joined line",
            JOINED,
        ],
    );
    land_on(root, LINE, "task/TSK-002-join");
}

/// TSK-433 AC-9: a verified epic line that joined the judged line, and
/// whose tip the range head contains, is judged as the range too. Its own
/// task, reopened and re-completed inside its pull request before landing
/// on its line, binds at that landing on the judged line's pull request
/// into `main`. Before AC-9 the joined line's tip counted as a target the
/// run is not judged against, as TSK-422's did.
#[test]
fn a_joined_lines_reopened_completion_binds_at_its_landing() {
    let dir = joined_repo(JOINED);
    let root = dir.path();
    reopened_on_joined(root, JOINED);
    let completion = git_out(root, &["rev-parse", "task/TSK-003-work"]);
    join(root, JOINED);
    let into_main = ci_on(root, "main", LINE, "");
    for refusal in [OUTSIDE_TIP, "work.acceptance_binding (block)"] {
        assert!(
            !into_main.1.contains(refusal),
            "the line into main with a joined line: {refusal}: {}",
            into_main.1
        );
    }
    assert_bound_at(&into_main, "TSK-003", &completion);
}

/// TSK-433 AC-9: a joined line that fails the epic-line proof, here for a
/// direct code commit with no landed adoption, keeps today's refusal: the
/// import check refuses the unverified home, and the joined tip is not
/// judged as the range.
#[test]
fn an_unverified_joined_line_stays_outside_evidence() {
    let dir = joined_repo(JOINED);
    let root = dir.path();
    reopened_on_joined(root, JOINED);
    git(root, &["switch", "-q", JOINED]);
    write(root, "src/direct.rs", "pub fn direct() {}\n");
    commit(root, "feat: a direct commit on the joined line");
    join(root, JOINED);
    assert_blocks(
        &ci_on(root, "main", LINE, ""),
        "an unverified joined line",
        &[
            "work.import_authority",
            "TSK-003",
            &format!("unverified home `{JOINED}`"),
        ],
    );
}

/// TSK-433 AC-9: a joined line's tip beyond the range head, here a later
/// landing on the joined line the judged line never merged, stays a target
/// the run is not judged against.
#[test]
fn a_joined_tip_beyond_the_head_stays_outside_evidence() {
    let dir = joined_repo(JOINED);
    let root = dir.path();
    reopened_on_joined(root, JOINED);
    join(root, JOINED);
    git(root, &["switch", "-q", "-c", "task/TSK-004-later", JOINED]);
    write(root, "src/later.rs", "pub fn later() {}\n");
    commit(root, "feat: later work on the joined line");
    land_on(root, JOINED, "task/TSK-004-later");
    git(root, &["switch", "-q", LINE]);
    assert_blocks(
        &ci_on(root, "main", LINE, ""),
        "a joined tip beyond the head",
        &["work.acceptance_binding", "TSK-003", OUTSIDE_TIP],
    );
}

/// TSK-433 AC-9: a task the joined line carries that declares `main` keeps
/// today's judgment; only a task of the joined line itself is judged as the
/// range.
#[test]
fn a_joined_lines_task_targeting_main_keeps_its_refusal() {
    let dir = joined_repo("main");
    let root = dir.path();
    reopened_on_joined(root, "main");
    join(root, "main");
    assert_blocks(
        &ci_on(root, "main", LINE, ""),
        "a task of main on the joined line",
        &["work.acceptance_binding", "TSK-003"],
    );
}

/// TSK-433 AC-9: a completion written straight onto the joined line, with
/// no task pull request and no landed adoption, leaves the joined line
/// unverified, so the import check refuses it and the joined tip is not
/// judged as the range.
#[test]
fn a_completion_forged_on_the_joined_line_stays_refused() {
    let dir = joined_repo(JOINED);
    let root = dir.path();
    git(root, &["switch", "-q", "-c", "task/TSK-003-work", JOINED]);
    write(root, "src/joined.rs", "pub fn joined() {}\n");
    let reviewed = commit(root, "feat: joined work");
    land_on(root, JOINED, "task/TSK-003-work");
    write(
        root,
        &record_path("TSK-003"),
        &joined_task(
            "TSK-003",
            JOINED,
            "complete",
            REOPEN_ADDS,
            &three_criteria_block(&reviewed),
        ),
    );
    commit(root, "docs(records): complete TSK-003 on the joined line");
    join(root, JOINED);
    assert_blocks(
        &ci_on(root, "main", LINE, ""),
        "a completion forged on the joined line",
        &[
            "work.import_authority",
            "TSK-003",
            &format!("unverified home `{JOINED}`"),
        ],
    );
}

/// TSK-433 AC-9: a joined line's task whose record once named the judged
/// line keeps today's refusal when the judged line itself fails the
/// epic-line proof, here for a direct code commit on it after the join:
/// the judged line is marked only on a proof of its own history.
#[test]
fn a_joined_task_on_an_unverified_judged_line_stays_refused() {
    let dir = joined_repo(JOINED);
    let root = dir.path();
    reopened_on_joined(root, JOINED);
    join(root, JOINED);
    write(root, "src/direct.rs", "pub fn direct() {}\n");
    commit(root, "feat: a direct commit on the judged line");
    assert_blocks(
        &ci_on(root, "main", LINE, ""),
        "a joined task on an unverified judged line",
        &["work.acceptance_binding", "TSK-003"],
    );
}

/// TSK-433 AC-9: for a verified joined line's task, a tip of the judged
/// line beyond the range head, here a later landing the run is not given,
/// stays a target the run is not judged against.
#[test]
fn a_judged_line_tip_beyond_the_head_stays_outside_for_a_joined_task() {
    let dir = joined_repo(JOINED);
    let root = dir.path();
    reopened_on_joined(root, JOINED);
    join(root, JOINED);
    let judged = head(root);
    build(root, "task/TSK-002-later", "src/later.rs");
    land(root, "task/TSK-002-later");
    git(root, &["switch", "-q", "--detach", &judged]);
    assert_blocks(
        &ci_on(root, "main", LINE, ""),
        "a judged line tip beyond the head for a joined task",
        &["work.acceptance_binding", "TSK-003", OUTSIDE_TIP, LINE],
    );
}

/// Review round 01, finding 1: a workspace root branch is judged with
/// amendable criteria without the epic-line proof. One on an epic line's
/// name that fails the proof, here for a direct code commit with no landed
/// adoption, keeps the binding refusal: the line is marked judged only on
/// a proof of its own history, whatever the range's class.
#[test]
fn an_unverified_root_branch_line_keeps_its_refusal() {
    let dir = planned_line_with(&format!(", \"root_branch\": \"{LINE}\""));
    let root = dir.path();
    reopened_in_its_own_pull_request(root);
    write(root, "src/direct.rs", "pub fn direct() {}\n");
    commit(root, "feat: a direct commit on the line");
    assert_blocks(
        &ci_on(root, "main", LINE, ""),
        "an unverified root branch line",
        &["work.acceptance_binding", "TSK-001", OUTSIDE_TIP],
    );
}

/// Review round 01, finding 2: a mark covers the exact tips its proof
/// covered. TSK-003's record once named its line by a remote-qualified
/// spelling, which resolves to a commit beside the joined line's tip: a
/// direct commit with no landed adoption, which the joined line then
/// merged. The head contains it and the canonical tip is proved, but that
/// commit is not, so it stays a target the run is not judged against.
#[test]
fn a_joined_line_spelling_at_an_unproved_tip_stays_outside_evidence() {
    let dir = joined_repo(JOINED);
    let root = dir.path();
    let other = format!("refs/remotes/other/{JOINED}");
    reopened_from(
        root,
        JOINED,
        (LINE, Some(JOINED)),
        "task/TSK-003-work",
        "TSK-003",
        &|status, criteria, closeout| {
            let first = status == "complete" && !closeout.contains("acceptance_superseded");
            let target = if first { other.as_str() } else { JOINED };
            joined_task("TSK-003", target, status, criteria, closeout)
        },
    );
    git(root, &["switch", "-q", "-c", "side", JOINED]);
    write(root, "src/side.rs", "pub fn side() {}\n");
    let unproved = commit(root, "feat: a direct commit beside the joined line");
    git(root, &["update-ref", &other, &unproved]);
    land_on(root, JOINED, "side");
    join(root, JOINED);
    assert_blocks(
        &ci_on(root, "main", LINE, ""),
        "a joined line spelling at an unproved tip",
        &[
            "work.acceptance_binding",
            "TSK-003",
            OUTSIDE_TIP,
            &format!("`{other}` (commit {})", &unproved[..9]),
        ],
    );
}
