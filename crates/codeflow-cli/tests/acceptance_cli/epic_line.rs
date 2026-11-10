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

/// The refusal a completion outside the range gives (TSK-416 AC-20).
const OUTSIDE_RANGE: &str = "records a completion of this task outside this range";

/// TSK-416 AC-20, checkpoint 607's shape: a planning branch cut from
/// [`LINE`] amends TSK-001's record before the task lands; the task's own
/// pull request, reopened and re-completed, lands on the line; then the
/// planning branch takes the line, so its merge writes a record neither
/// parent holds that still shows the completion. Returns that merge, with
/// the planning branch checked out at it and not yet landed.
fn amended_by_a_planning_merge(root: &Path) -> String {
    git(root, &["switch", "-q", "-c", "plan/amend-seats", LINE]);
    let record = std::fs::read_to_string(root.join(record_path("TSK-001"))).unwrap();
    write(
        root,
        &record_path("TSK-001"),
        &record.replace(
            "# TSK-001: work\n",
            "# TSK-001: work\n\n- Reassigned: the reviewer is another seat.\n",
        ),
    );
    commit(root, "docs(records): reassign TSK-001's reviewer");
    reopened_in_its_own_pull_request(root);
    git(root, &["switch", "-q", "plan/amend-seats"]);
    git(
        root,
        &[
            "merge",
            "--no-ff",
            "-m",
            "chore(merge): take the line",
            LINE,
        ],
    );
    let merge = head(root);
    let merged = std::fs::read_to_string(root.join(record_path("TSK-001"))).unwrap();
    assert!(
        merged.contains("Reassigned") && merged.contains("status: complete"),
        "{merged}"
    );
    merge
}

/// TSK-416 AC-20: once the planning branch lands, its merge is in the range
/// the line's judged tip holds, so the line into `main` binds the
/// completion at its own commit. Before the fix the walk for a holder
/// outside the range hid only the completion's commit, reached the merge
/// through the line and the planning branch, and refused.
#[test]
fn tsk416_a_record_rewritten_by_an_in_range_merge_is_no_outside_completion() {
    let dir = planned_line();
    let root = dir.path();
    amended_by_a_planning_merge(root);
    let completion = git_out(root, &["rev-parse", "task/TSK-001-work"]);
    land(root, "plan/amend-seats");
    let result = ci_on(root, "main", LINE, "");
    assert!(!result.1.contains(OUTSIDE_RANGE), "{}", result.1);
    assert!(
        !result.1.contains("work.acceptance_binding (block)"),
        "{}",
        result.1
    );
    assert_bound_at(&result, "TSK-001", &completion);
}

/// TSK-416 AC-20: the same merge on a ref that is no judged tip, the
/// planning branch before it lands, is outside the range and still refuses.
#[test]
fn tsk416_the_same_merge_on_a_ref_that_is_no_judged_tip_still_refuses() {
    let dir = planned_line();
    let root = dir.path();
    let merge = amended_by_a_planning_merge(root);
    git(root, &["switch", "-q", LINE]);
    assert_blocks(
        &ci_on(root, "main", LINE, ""),
        "the merge on the planning branch only",
        &[
            "work.acceptance_binding",
            "TSK-001",
            &format!("`plan/amend-seats` {OUTSIDE_RANGE} (commit {}", &merge[..9]),
        ],
    );
}

/// TSK-416 AC-20: a ref named like the line at another commit is no judged
/// tip: judged tips bind by exact commit, never by name, so the merge it
/// holds still refuses.
#[test]
fn tsk416_a_ref_named_like_the_line_at_another_commit_still_refuses() {
    let dir = planned_line();
    let root = dir.path();
    let merge = amended_by_a_planning_merge(root);
    git(root, &["switch", "-q", LINE]);
    let named = format!("refs/heads/origin/{LINE}");
    git(root, &["update-ref", &named, &merge]);
    git(root, &["branch", "-q", "-D", "plan/amend-seats"]);
    assert_blocks(
        &ci_on(root, "main", LINE, ""),
        "a ref named like the line",
        &[
            "work.acceptance_binding",
            "TSK-001",
            &format!("`origin/{LINE}` {OUTSIDE_RANGE} (commit {}", &merge[..9]),
        ],
    );
}

/// Checkpoint 607's base, `main` when hosted job 127659313186 ran.
const CHECKPOINT_BASE: &str = "879a7a7fc83ae37ded08dcc730a283acebd1d6c2";
/// Checkpoint 607's head, the 3.2.0 line's tip it judged.
const CHECKPOINT_HEAD: &str = "2630d4854c47941af283df41591eab03f4e8407e";
/// The 3.2.0 line checkpoint 607 took into `main`.
const CHECKPOINT_LINE: &str = "integration/EPC-022-the-3-2-0-milestone-fixes";
/// Lines other than the 3.2.0 line that records changed in checkpoint 607
/// name as their home.
const CHECKPOINT_HOMES: [&str; 2] = [
    "integration/EPC-024-quality-defaults-local-scan-parity-and-seat-capa",
    "integration/EPC-026-the-capabilities-catalog",
];

/// TSK-416 AC-20 on the real commits: checkpoint 607 (`879a7a7f..2630d4854`,
/// 997 commits and 383 merges) runs as hosted job 127659313186 ran it, with
/// its pull request's `Task: EPC-022`. Before the fix it refused TSK-1062 as
/// a completion outside the range, reached through `c677596d6`, an in-range
/// merge rewriting the record; with the fix the line's completions are its
/// own. The judgement must run: the test also requires TSK-1062's binding
/// note and no import refusal, which would skip every task.
#[test]
#[ignore = "needs this repository's full history; CI's test checkout is shallow"]
fn tsk416_checkpoint_607_reports_no_completion_outside_its_range() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for commit in [CHECKPOINT_BASE, CHECKPOINT_HEAD] {
        git(
            &source,
            &["cat-file", "-e", &format!("{commit}^{{commit}}")],
        );
    }
    let scratch = tempfile::tempdir().unwrap();
    let remote = scratch.path().join("origin.git");
    git(
        scratch.path(),
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    let url = format!("file://{}", source.canonicalize().unwrap().display());
    // The refs `origin` holds, and why:
    // - `main` at the base: the default target the range lands on.
    // - The line at the head: the ref through which the walk for a holder
    //   outside the range, over every branch and remote-tracking ref
    //   (landing.rs:1022-1043), reached `c677596d6` before the fix.
    // - Each other home at the base: the import check reads a home's
    //   remote-tracking tip, refuses a missing one and skips every task's
    //   judgement (imports.rs:121-124), and passes a tip the anchor holds
    //   with no import (imports.rs:126).
    let mut refspecs = vec![
        format!("{CHECKPOINT_BASE}:refs/heads/main"),
        format!("{CHECKPOINT_HEAD}:refs/heads/{CHECKPOINT_LINE}"),
    ];
    refspecs.extend(
        CHECKPOINT_HOMES
            .iter()
            .map(|home| format!("{CHECKPOINT_BASE}:refs/heads/{home}")),
    );
    let mut fetch = vec![
        "-c",
        "uploadpack.allowAnySHA1InWant=true",
        "fetch",
        "-q",
        url.as_str(),
    ];
    fetch.extend(refspecs.iter().map(String::as_str));
    git(&remote, &fetch);
    let clone = scratch.path().join("clone");
    codeflow_fixture::clone(
        scratch.path(),
        format!("file://{}", remote.display()),
        &clone,
    )
    .no_checkout()
    .run();
    git(&clone, &["switch", "-q", "--detach", CHECKPOINT_HEAD]);
    let body = scratch.path().join("pr-body.md");
    std::fs::write(
        &body,
        "## Summary\nThe 3.2.0 line into main.\n\nTask: EPC-022\n\n## Changes\n- the line\n\n## Testing\n- test\n",
    )
    .unwrap();
    let out = codeflow()
        .args([
            "ci",
            "--base",
            CHECKPOINT_BASE,
            "--head",
            CHECKPOINT_HEAD,
            "--branch",
            CHECKPOINT_LINE,
            "--policy-from",
            CHECKPOINT_BASE,
            "--baseline-from",
            CHECKPOINT_BASE,
            "--pr-body-file",
            body.to_str().unwrap(),
        ])
        .current_dir(&clone)
        .output()
        .unwrap();
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!printed.contains("work.import_authority"), "{printed}");
    assert!(
        printed.contains("TSK-1062 completion bound at"),
        "{printed}"
    );
    let refused = printed
        .lines()
        .any(|line| line.contains("TSK-1062") && line.contains(OUTSIDE_RANGE));
    assert!(!refused, "TSK-1062 refused as outside the range: {printed}");
    assert!(
        !printed.contains("work.acceptance_binding (block)"),
        "{printed}"
    );
}
