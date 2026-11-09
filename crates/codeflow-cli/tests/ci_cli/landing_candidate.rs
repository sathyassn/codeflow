//! TSK-1154 (issue #488, and TSK-1277's c2 shape): `codeflow ci` judges
//! each member merge of a landing candidate at its own reviewed head,
//! against the tip it merged onto, so a task's own criteria change that
//! its pull request already judged lands through the candidate, and a
//! clean batch of reviewed members passes. Every defect a member's own
//! pull request would refuse is still refused, naming the member.
use std::process::Output;

use super::line_adoption::fixture::{blocks, output, passes, Line, LINE, ZERO};

const CANDIDATE: &str = "candidate/epc-001-c1";

fn record_path(task: &str) -> String {
    format!("project-management/tasks/{task}.md")
}

/// A todo task of EPC-001 that targets the line, with AC-1 to AC-4.
fn planned(task: &str) -> String {
    format!("---\nid: {task}\nepic_id: EPC-001\ntitle: Work {task}\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: []\nintegration_target: {LINE}\ncreated: 2026-10-06\n---\n\n# Work {task}\n\n## Description\n\nDeliver work.\n\n## Acceptance Criteria\n\n- AC-1 Work runs.\n- AC-2 The command succeeds. (journey)\n- AC-3 The output is plain.\n- AC-4 The text is A.\n")
}

/// A line published at its tip with TSK-001 landed and `tasks` planned
/// there by a reviewed planning merge; the candidate branch is checked out
/// at the tip.
fn published(tasks: &[&str]) -> (Line, String) {
    published_with(tasks, false)
}

/// [`published`], with the push check on (`test_gate_on_push: block`) at
/// the destination's default branch, the policy that judges a push.
fn published_with(tasks: &[&str], gated: bool) -> (Line, String) {
    let f = Line::new();
    if gated {
        f.git(&["switch", "-q", "main"]);
        f.write(
            ".codeflow/policy.json",
            "{\"schema_version\":1,\"git\":{\"product_paths\":[\"src/**\"],\"test_gate_on_push\":\"block\"}}\n",
        );
        f.commit("chore: check pushes");
        f.git(&["switch", "-q", LINE]);
        f.git(&["merge", "-q", "--ff-only", "main"]);
    }
    f.git(&["push", "-q", "origin", "main"]);
    f.land_task();
    if !tasks.is_empty() {
        f.git(&["switch", "-qc", "plan/EPC-001-members"]);
        for task in tasks {
            f.write(&record_path(task), &planned(task));
        }
        f.commit("docs: plan the members");
        f.git(&["switch", "-q", LINE]);
        f.merge("plan/EPC-001-members");
    }
    f.git(&["push", "-q", "origin", LINE]);
    let tip = f.git(&["rev-parse", "HEAD"]);
    f.git(&["switch", "-qc", CANDIDATE]);
    (f, tip)
}

fn read(f: &Line, path: &str) -> String {
    std::fs::read_to_string(f.root().join(path)).unwrap()
}

/// Complete `task` at the current head, as its reviewed pull request does.
fn complete(f: &Line, task: &str) -> String {
    let reviewed = f.git(&["rev-parse", "HEAD"]);
    let path = record_path(task);
    let record = read(f, &path).replace("status: todo", "status: complete");
    f.write(&path, &format!("{record}\n## Closeout\n\n```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: https://example.test/review/{task}\n  criteria:\n    AC-1: verified | focused test\n    AC-2: verified | journey\n    AC-3: verified | focused test\n    AC-4: verified | focused test\n  journey: verified | exercised command\n  not_verified: none\n  follow_ups: none: fixture\n  verdict: approved\n```\n"));
    f.commit(&format!("docs: complete {task}"));
    reviewed
}

/// What a member's reviewed task branch changes beyond its own code.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Change {
    /// Code only.
    Code,
    /// Its own AC-4 rewritten with a dated amendment note (issue #488).
    OwnCriterion,
    /// Its own AC-4 rewritten with no amendment note.
    OwnCriterionUndated,
    /// Another task's (TSK-001's) AC-1 rewritten.
    ForeignCriterion,
    /// The epic's AC-1 rewritten.
    EpicCriterion,
}

const AMENDED: &str = "- AC-4 The text is B.\n  - Amended 2026-11-01: reworded after review.\n";

/// Build `task`'s reviewed head from `from` with `change`, completed at its
/// last commit, and return the head. The candidate branch is left as it was.
fn member_head(f: &Line, task: &str, from: &str, change: Change) -> String {
    let back = f.git(&["rev-parse", "--abbrev-ref", "HEAD"]);
    let branch = format!("task/{task}-work");
    f.git(&["switch", "-qc", &branch, from]);
    f.write(
        &format!("src/{}.rs", task.to_lowercase().replace('-', "_")),
        "pub fn member() {}\n",
    );
    let path = record_path(task);
    match change {
        Change::Code => {}
        Change::OwnCriterion => {
            let record = read(f, &path).replace("- AC-4 The text is A.\n", AMENDED);
            f.write(&path, &record);
        }
        Change::OwnCriterionUndated => {
            let record =
                read(f, &path).replace("- AC-4 The text is A.\n", "- AC-4 The text is B.\n");
            f.write(&path, &record);
        }
        Change::ForeignCriterion => {
            let other = record_path("TSK-001");
            let record = read(f, &other).replace(
                "AC-1 The planned work runs.",
                "AC-1 The planned work runs fast.",
            );
            f.write(&other, &record);
        }
        Change::EpicCriterion => {
            let epic = super::line_adoption::fixture::EPIC_PATH;
            let record = read(f, epic).replace("- AC-1 Work runs.", "- AC-1 Work runs well.");
            f.write(epic, &record);
        }
    }
    f.commit(&format!("feat: implement {task}"));
    complete(f, task);
    let head = f.git(&["rev-parse", "HEAD"]);
    f.git(&["switch", "-q", &back]);
    head
}

/// Merge `head` onto the candidate by a no-ff merge, as the primary does.
fn land(f: &Line, task: &str, head: &str) -> String {
    f.git(&[
        "merge",
        "--no-ff",
        "-qm",
        &format!("chore: land {task} on the candidate"),
        head,
    ]);
    f.git(&["rev-parse", "HEAD"])
}

fn ci(f: &Line, args: &[&str]) -> Output {
    f.command(env!("CARGO_BIN_EXE_codeflow"))
        .arg("ci")
        .args(args)
        .output()
        .unwrap()
}

fn candidate_ci(f: &Line, tip: &str) -> Output {
    ci(f, &["--base", tip, "--head", "HEAD", "--line-tip", LINE])
}

fn lacks(o: &Output, text: &str) {
    assert!(
        !output(o).contains(text),
        "unexpected {text}: {}",
        output(o)
    );
}

fn has(o: &Output, text: &str) {
    assert!(output(o).contains(text), "missing {text}: {}", output(o));
}

/// AC-2 (issue #488's four steps) and AC-3's landing form: the candidate of
/// a task head that rewrites its own AC-4 with a dated note and completes
/// passes, with a note naming the task's criterion, both with
/// `--line-tip` and with the issue's own invocation, which finds the line
/// from the base.
#[test]
fn a_members_own_criteria_change_lands_through_the_candidate() {
    let (f, tip) = published(&["TSK-002"]);
    let head = member_head(&f, "TSK-002", &tip, Change::OwnCriterion);
    land(&f, "TSK-002", &head);
    for result in [
        candidate_ci(&f, &tip),
        ci(&f, &["--base", &tip, "--head", "HEAD"]),
    ] {
        passes(&result);
        lacks(&result, "work.criteria_frozen (block)");
        has(&result, "TSK-002 AC-4");
        has(&result, &format!("member {:.9}", head));
    }
}

/// AC-3: the same task head gets the same verdict through its own pull
/// request range into the line.
#[test]
fn the_task_pull_request_and_the_candidate_agree() {
    let (f, tip) = published(&["TSK-002"]);
    let head = member_head(&f, "TSK-002", &tip, Change::OwnCriterion);
    f.git(&["switch", "-q", "task/TSK-002-work"]);
    let own = ci(
        &f,
        &[
            "--base",
            &tip,
            "--head",
            &head,
            "--into",
            LINE,
            "--branch",
            "task/TSK-002-work",
        ],
    );
    passes(&own);
    lacks(&own, "work.criteria_frozen (block)");
    f.git(&["switch", "-q", CANDIDATE]);
    land(&f, "TSK-002", &head);
    let landed = candidate_ci(&f, &tip);
    passes(&landed);
    lacks(&landed, "work.criteria_frozen (block)");
}

/// AC-8 and AC-9 (the c2 shape): a line tip plus no-ff merges of three
/// reviewed member heads, each with its own record and completion, two of
/// them changing their own criteria. Each member is judged at its own
/// reviewed head against the tip it merged onto, so the clean batch passes;
/// before this change the candidate was judged as one pull request, so
/// every completion was bound at the candidate head and every own criteria
/// change was frozen.
#[test]
fn a_clean_batch_of_reviewed_members_passes() {
    let (f, tip) = published(&["TSK-002", "TSK-003", "TSK-004"]);
    let heads = [
        (
            "TSK-002",
            member_head(&f, "TSK-002", &tip, Change::OwnCriterion),
        ),
        ("TSK-003", member_head(&f, "TSK-003", &tip, Change::Code)),
        (
            "TSK-004",
            member_head(&f, "TSK-004", &tip, Change::OwnCriterion),
        ),
    ];
    for (task, head) in &heads {
        land(&f, task, head);
    }
    let result = candidate_ci(&f, &tip);
    passes(&result);
    lacks(&result, "work.acceptance_binding (block)");
    lacks(&result, "work.criteria_frozen (block)");
    for (task, head) in &heads {
        has(&result, &format!("member {head:.9} ({task})"));
    }
}

/// AC-10: the `candidate/` branch name raises no branch-naming finding.
#[test]
fn the_candidate_branch_name_is_accepted() {
    let (f, tip) = published(&["TSK-002"]);
    let head = member_head(&f, "TSK-002", &tip, Change::Code);
    land(&f, "TSK-002", &head);
    let result = candidate_ci(&f, &tip);
    passes(&result);
    lacks(&result, "git.branch_naming");
}

/// AC-4 (a) and AC-12: a member that changes another task's criteria is
/// refused, naming the member.
#[test]
fn a_member_changing_another_tasks_criteria_is_refused() {
    let (f, tip) = published(&["TSK-002"]);
    let head = member_head(&f, "TSK-002", &tip, Change::ForeignCriterion);
    land(&f, "TSK-002", &head);
    let result = candidate_ci(&f, &tip);
    blocks(&result, "work.criteria_frozen");
    has(&result, "TSK-001");
    has(&result, &format!("member {head:.9}"));
}

/// AC-4 (b) and AC-12: a criteria change made by a non-merge commit on the
/// candidate is refused, and so is the direct commit itself.
#[test]
fn a_criteria_change_by_a_direct_commit_is_refused() {
    let (f, tip) = published(&["TSK-002"]);
    let head = member_head(&f, "TSK-002", &tip, Change::Code);
    land(&f, "TSK-002", &head);
    let path = record_path("TSK-002");
    f.write(
        &path,
        &read(&f, &path).replace("AC-3 The output is plain.", "AC-3 The output is bold."),
    );
    let direct = f.commit("docs: reword a criterion on the candidate");
    let result = candidate_ci(&f, &tip);
    blocks(&result, "work.criteria_frozen");
    has(&result, &format!("{direct:.9}"));
    has(&result, "work.epic_line");
}

/// AC-4 (c) and AC-12: a merge whose result differs from the clean remerge
/// of its parents is refused, naming the member and the hunk's path, and
/// saying the member's whole-unit review reopens.
#[test]
fn a_hand_resolved_merge_is_refused() {
    let (f, tip) = published(&["TSK-002"]);
    let head = member_head(&f, "TSK-002", &tip, Change::OwnCriterion);
    f.git(&["merge", "--no-ff", "--no-commit", "-q", &head]);
    let path = record_path("TSK-002");
    f.write(
        &path,
        &read(&f, &path).replace("AC-4 The text is B.", "AC-4 The text is C."),
    );
    f.git(&["add", "-A"]);
    f.git(&["commit", "-qm", "chore: land TSK-002 on the candidate"]);
    let result = candidate_ci(&f, &tip);
    blocks(&result, "differs from the clean remerge");
    has(&result, &format!("member {head:.9}"));
    has(&result, "project-management/tasks/TSK-002.md");
    has(&result, "whole-unit review reopens");
}

/// AC-4 (d): a task whose record does not declare the line at the line's
/// tip keeps its criteria frozen on the candidate.
#[test]
fn a_member_that_does_not_declare_the_line_is_refused() {
    let (f, tip) = published(&["TSK-002"]);
    // TSK-005 is planned only in the member, never at the line's tip.
    let back = f.git(&["rev-parse", "--abbrev-ref", "HEAD"]);
    f.git(&["switch", "-qc", "plan/TSK-005-own", &tip]);
    f.write(&record_path("TSK-005"), &planned("TSK-005"));
    f.commit("docs: plan TSK-005 in its own branch");
    f.git(&["switch", "-q", &back]);
    let head = member_head(&f, "TSK-005", "plan/TSK-005-own", Change::OwnCriterion);
    land(&f, "TSK-005", &head);
    let result = candidate_ci(&f, &tip);
    blocks(&result, "work.criteria_frozen");
    has(&result, "TSK-005");
    has(&result, "does not declare this line");
}

/// AC-4 (e): a task already complete on the line keeps its criteria
/// (the reopen freeze).
#[test]
fn a_member_reopening_a_task_complete_on_the_line_is_refused() {
    let (f, tip) = published(&[]);
    let back = f.git(&["rev-parse", "--abbrev-ref", "HEAD"]);
    f.git(&["switch", "-qc", "task/TSK-001-again", &tip]);
    f.write("src/again.rs", "pub fn again() {}\n");
    let path = record_path("TSK-001");
    f.write(
        &path,
        &read(&f, &path).replace(
            "- AC-1 The planned work runs.\n",
            "- AC-1 The planned work runs again.\n  - Amended 2026-11-01: reworded.\n",
        ),
    );
    f.commit("fix: rework TSK-001");
    let head = f.git(&["rev-parse", "HEAD"]);
    f.git(&["switch", "-q", &back]);
    land(&f, "TSK-001", &head);
    let result = candidate_ci(&f, &tip);
    blocks(&result, "work.criteria_frozen");
    has(&result, "TSK-001");
}

/// AC-4 (f): a changed criterion with no dated amendment note is refused.
#[test]
fn an_undated_own_criteria_change_is_refused() {
    let (f, tip) = published(&["TSK-002"]);
    let head = member_head(&f, "TSK-002", &tip, Change::OwnCriterionUndated);
    land(&f, "TSK-002", &head);
    let result = candidate_ci(&f, &tip);
    blocks(&result, "work.criteria_frozen");
    has(
        &result,
        "TSK-002 AC-4 changes without a dated amendment note",
    );
}

/// AC-4 (g): a member that changes an epic record's criteria is refused.
#[test]
fn a_member_changing_epic_criteria_is_refused() {
    let (f, tip) = published(&["TSK-002"]);
    let head = member_head(&f, "TSK-002", &tip, Change::EpicCriterion);
    land(&f, "TSK-002", &head);
    let result = candidate_ci(&f, &tip);
    blocks(&result, "EPC-001");
    has(&result, &format!("member {head:.9}"));
}

/// AC-12: a member head with an unbound completion (more work after its
/// review) is refused, naming the member.
#[test]
fn a_member_with_an_unbound_completion_is_refused() {
    let (f, tip) = published(&["TSK-002"]);
    let head = member_head(&f, "TSK-002", &tip, Change::Code);
    let back = f.git(&["rev-parse", "--abbrev-ref", "HEAD"]);
    f.git(&["switch", "-q", "task/TSK-002-work"]);
    f.write("src/later.rs", "pub fn later() {}\n");
    let later = f.commit("feat: add work after the review");
    f.git(&["switch", "-q", &back]);
    assert_ne!(head, later);
    land(&f, "TSK-002", &later);
    let result = candidate_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding");
    has(&result, &format!("member {later:.9}"));
}

/// AC-4 (h): a shallow history refuses, whatever the members hold.
#[test]
fn a_shallow_history_refuses() {
    let (f, tip) = published(&["TSK-002"]);
    let head = member_head(&f, "TSK-002", &tip, Change::OwnCriterion);
    land(&f, "TSK-002", &head);
    let first = f.git(&["rev-list", "--max-parents=0", "HEAD"]);
    std::fs::write(f.root().join(".git/shallow"), format!("{first}\n")).unwrap();
    let result = candidate_ci(&f, &tip);
    assert_ne!(result.status.code(), Some(0), "{}", output(&result));
}

/// AC-5: a supplied base that skips commits between the pinned tip and the
/// head grants nothing: the whole chain from the fetched tip is judged, so
/// the skipped member's undated criteria change is still refused.
#[test]
fn a_base_after_the_tip_does_not_shorten_the_candidate() {
    let (f, tip) = published(&["TSK-002", "TSK-003"]);
    let undated = member_head(&f, "TSK-002", &tip, Change::OwnCriterionUndated);
    let first = land(&f, "TSK-002", &undated);
    let head = member_head(&f, "TSK-003", &tip, Change::OwnCriterion);
    land(&f, "TSK-003", &head);
    let result = ci(
        &f,
        &["--base", &first, "--head", "HEAD", "--line-tip", LINE],
    );
    blocks(
        &result,
        "TSK-002 AC-4 changes without a dated amendment note",
    );
    has(&result, &format!("fetched tip {tip:.9}"));
}

/// AC-5: a fabricated local line ref grants nothing; the line's authority
/// is its fetched tip only.
#[test]
fn a_fabricated_local_line_grants_nothing() {
    let (f, tip) = published(&["TSK-002"]);
    let head = member_head(&f, "TSK-002", &tip, Change::OwnCriterion);
    land(&f, "TSK-002", &head);
    f.git(&["branch", "integration/EPC-001-local"]);
    let result = ci(
        &f,
        &[
            "--base",
            &tip,
            "--head",
            "HEAD",
            "--line-tip",
            "integration/EPC-001-local",
        ],
    );
    assert_eq!(result.status.code(), Some(1), "{}", output(&result));
    has(&result, "has no fetched tip");
}

/// AC-5: a head branch named like the member's task, or like an
/// `integration/` line, on a destination that is not a confirmed line,
/// grants nothing: without a landing candidate's line the candidate is
/// judged as one range and the own criteria change is frozen.
#[test]
fn branch_names_grant_nothing() {
    let (f, tip) = published(&["TSK-002"]);
    let head = member_head(&f, "TSK-002", &tip, Change::OwnCriterion);
    land(&f, "TSK-002", &head);
    for branch in ["task/TSK-002-work", "integration/EPC-009-decoy"] {
        let result = ci(
            &f,
            &[
                "--base", "main", "--head", "HEAD", "--into", "main", "--branch", branch,
            ],
        );
        assert_ne!(result.status.code(), Some(0), "{}", output(&result));
        lacks(&result, "member ");
    }
}

/// AC-3 and AC-15: the same members get the same verdict through a real
/// push of the candidate to a `candidate/` branch and of the candidate to
/// the line. Before this change the candidate branch's push was judged as
/// one range and refused with `work.criteria_frozen` for each member.
#[test]
fn a_pushed_candidate_gets_the_landing_verdict() {
    let (f, tip) = published_with(&["TSK-002", "TSK-003"], true);
    let mut heads = Vec::new();
    for task in ["TSK-002", "TSK-003"] {
        let head = member_head(&f, task, &tip, Change::OwnCriterion);
        land(&f, task, &head);
        heads.push(head);
    }
    let pushed = f.push_branch(CANDIDATE, ZERO);
    passes(&pushed);
    lacks(&pushed, "work.criteria_frozen (block)");
    for head in &heads {
        has(&pushed, &format!("member {head:.9}"));
    }
    passes(&candidate_ci(&f, &tip));
    passes(&f.push_branch(LINE, &tip));
}

/// AC-15: a doctored pushed candidate, whose member changes another
/// task's criteria, is refused naming the member.
#[test]
fn a_doctored_pushed_candidate_is_refused() {
    let (f, tip) = published_with(&["TSK-002", "TSK-003"], true);
    let own = member_head(&f, "TSK-002", &tip, Change::OwnCriterion);
    land(&f, "TSK-002", &own);
    let foreign = member_head(&f, "TSK-003", &tip, Change::ForeignCriterion);
    land(&f, "TSK-003", &foreign);
    let pushed = f.push_branch(CANDIDATE, ZERO);
    blocks(&pushed, "work.criteria_frozen");
    has(&pushed, &format!("member {foreign:.9}"));
}

/// AC-4 (e): a task whose completion already landed on the default target
/// keeps its criteria on the candidate (the reopen freeze), even though the
/// line's tip still holds it as todo.
#[test]
fn a_member_completed_on_the_default_target_is_refused() {
    let (f, tip) = published(&["TSK-002"]);
    let back = f.git(&["rev-parse", "--abbrev-ref", "HEAD"]);
    f.git(&["switch", "-q", "main"]);
    f.write(&record_path("TSK-002"), &planned("TSK-002"));
    f.commit("docs: plan TSK-002 on main");
    complete(&f, "TSK-002");
    f.git(&["push", "-q", "origin", "main"]);
    f.git(&["switch", "-q", &back]);
    let head = member_head(&f, "TSK-002", &tip, Change::OwnCriterion);
    land(&f, "TSK-002", &head);
    let result = candidate_ci(&f, &tip);
    blocks(&result, "work.criteria_frozen");
    has(&result, &format!("member {head:.9}"));
}

/// A merge of the published default target into the candidate is a sync,
/// not a member: it is judged as a range of its own and named as a sync,
/// while the member beside it is judged at its own head. (An imported
/// completion through such a sync is pinned by
/// `range_context::explicit_head_reports_context_without_blame_for_imported_work`.)
#[test]
fn a_sync_of_the_default_target_is_not_judged_as_a_member() {
    let (f, tip) = published(&["TSK-002"]);
    let back = f.git(&["rev-parse", "--abbrev-ref", "HEAD"]);
    f.git(&["switch", "-q", "main"]);
    f.write("docs/main-notes.md", "Notes made on main.\n");
    let synced = f.commit("docs: add main notes");
    f.git(&["push", "-q", "origin", "main"]);
    f.git(&["switch", "-q", &back]);
    land(&f, "main", &synced);
    let head = member_head(&f, "TSK-002", &tip, Change::OwnCriterion);
    land(&f, "TSK-002", &head);
    let result = candidate_ci(&f, &tip);
    passes(&result);
    has(&result, &format!("sync {synced:.9}"));
    lacks(&result, &format!("member {synced:.9}"));
    has(&result, &format!("member {head:.9}"));
}
