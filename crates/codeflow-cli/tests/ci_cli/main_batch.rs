//! TSK-1154 AC-16: a batch pull request into `main` is judged member by
//! member from `main`'s fetched tip. Each member binds as a single-task
//! pull request into `main` does: the task record's acceptance block names
//! a reviewed commit that the member head is, or holds with only clean
//! merges of `main` after it, with the verdict at that head; or the member
//! is a filing that only adds new record files. No existing criterion may
//! change, the bound task's own included, and records are matched across a
//! member's two parents by id and uid, never by path.
use std::process::Output;

use super::landing_candidate::{ci, has, lacks, land, read, record_path};
use super::line_adoption::fixture::{blocks, output, passes, Line, LINE, ZERO};

const BATCH: &str = "candidate/main-c1";
const BODY: &str = "## Summary\nLand the reviewed members on main.\n\n- one batch\n\n## Changes\n- members\n\n## Testing\n- fixture\n";

/// A todo task that targets `main`: standalone with a reason, or of `epic`.
fn main_task(task: &str, epic: Option<&str>) -> String {
    let owner = match epic {
        Some(epic) => format!("epic_id: {epic}"),
        None => "epic_id: null\nstandalone_reason: One bounded fix".to_string(),
    };
    format!("---\nid: {task}\n{owner}\ntitle: Work {task}\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: []\nintegration_target: main\ncreated: 2026-10-06\n---\n\n# Work {task}\n\n## Description\n\nDeliver work.\n\n## Acceptance Criteria\n\n- AC-1 Work runs.\n- AC-2 The command succeeds. (journey)\n- AC-3 The output is plain.\n- AC-4 The text is A.\n")
}

/// [`main_task`] with a `uid` line.
fn with_uid(record: &str, uid: &str) -> String {
    record.replacen("---\n", &format!("---\nuid: {uid}\n"), 1)
}

/// `main` published with `records` planned on it; the batch branch is
/// checked out at that tip, which is returned.
fn planned(records: &[(&str, String)]) -> (Line, String) {
    planned_with(records, false)
}

/// [`planned`], with the push check on (`test_gate_on_push: block`) at
/// `main`, the policy that judges a push, when `gated`.
fn planned_with(records: &[(&str, String)], gated: bool) -> (Line, String) {
    let f = Line::new();
    f.git(&["switch", "-q", "main"]);
    if gated {
        f.write(
            ".codeflow/policy.json",
            "{\"schema_version\":1,\"git\":{\"product_paths\":[\"src/**\"],\"test_gate_on_push\":\"block\"}}\n",
        );
    }
    for (task, record) in records {
        f.write(&record_path(task), record);
    }
    f.commit("docs: plan main work");
    f.git(&["push", "-q", "origin", "main"]);
    let tip = f.git(&["rev-parse", "HEAD"]);
    f.git(&["switch", "-qc", BATCH]);
    (f, tip)
}

/// The usual plan: TSK-010 standalone and TSK-011 of EPC-001, both into
/// `main`.
fn batch() -> (Line, String) {
    planned(&[
        ("TSK-010", main_task("TSK-010", None)),
        ("TSK-011", main_task("TSK-011", Some("EPC-001"))),
    ])
}

/// Complete `task` at the current head with `verdict`, writing `extra`
/// (a path and its text) in the same commit when given; returns the
/// reviewed commit.
fn complete(f: &Line, task: &str, verdict: &str, extra: Option<(&str, &str)>) -> String {
    let reviewed = f.git(&["rev-parse", "HEAD"]);
    let path = record_path(task);
    let record = read(f, &path).replace("status: todo", "status: complete");
    f.write(&path, &format!("{record}\n## Closeout\n\n```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: https://example.test/review/{task}\n  criteria:\n    AC-1: verified | focused test\n    AC-2: verified | journey\n    AC-3: verified | focused test\n    AC-4: verified | focused test\n  journey: verified | exercised command\n  not_verified: none\n  follow_ups: none: fixture\n  verdict: {verdict}\n```\n"));
    if let Some((path, text)) = extra {
        f.write(path, text);
    }
    f.commit(&format!("docs: complete {task}"));
    reviewed
}

/// `task`'s reviewed head on `branch` from `from`: its code, then `edit`,
/// in one commit, then its approved completion. The batch branch stays
/// checked out.
fn member_on(f: &Line, branch: &str, task: &str, from: &str, edit: impl Fn(&Line)) -> String {
    let back = f.git(&["rev-parse", "--abbrev-ref", "HEAD"]);
    f.git(&["switch", "-qc", branch, from]);
    f.write(
        &format!("src/{}.rs", task.to_lowercase().replace('-', "_")),
        "pub fn member() {}\n",
    );
    edit(f);
    f.commit(&format!("feat: implement {task}"));
    complete(f, task, "approved", None);
    let head = f.git(&["rev-parse", "HEAD"]);
    f.git(&["switch", "-q", &back]);
    head
}

fn member(f: &Line, task: &str, from: &str, edit: impl Fn(&Line)) -> String {
    member_on(f, &format!("task/{task}-work"), task, from, edit)
}

fn untouched(_: &Line) {}

/// A filing from `from`: a planning branch that adds `task`'s new record.
fn filing(f: &Line, task: &str, from: &str) -> String {
    let back = f.git(&["rev-parse", "--abbrev-ref", "HEAD"]);
    f.git(&["switch", "-qc", &format!("plan/{task}-file"), from]);
    f.write(&record_path(task), &main_task(task, None));
    let head = f.commit(&format!("docs: file {task}"));
    f.git(&["switch", "-q", &back]);
    head
}

/// A planning branch from `from` with `edit` and no completion.
fn records_only(f: &Line, name: &str, from: &str, edit: impl Fn(&Line)) -> String {
    let back = f.git(&["rev-parse", "--abbrev-ref", "HEAD"]);
    f.git(&["switch", "-qc", name, from]);
    edit(f);
    let head = f.commit("docs: change records");
    f.git(&["switch", "-q", &back]);
    head
}

/// The hosted form: a pull request into `main` with a body.
fn batch_ci(f: &Line, tip: &str) -> Output {
    batch_ci_with(f, tip, BODY)
}

fn batch_ci_with(f: &Line, tip: &str, body: &str) -> Output {
    ci(
        f,
        &[
            "--base",
            tip,
            "--head",
            "HEAD",
            "--into",
            "main",
            "--branch",
            BATCH,
            "--pr-body",
            body,
        ],
    )
}

/// Seed the id registry from the checked-out batch, which holds every
/// record its members add, and publish it, as the hosted run fetches it.
fn seed(f: &Line) {
    passes(
        &f.command(env!("CARGO_BIN_EXE_codeflow"))
            .args(["ids", "seed"])
            .output()
            .unwrap(),
    );
    f.git(&["push", "-q", "origin", "codeflow/registry"]);
}

/// Advance `main` by `edit` (one commit) and publish it; returns the tip.
fn advance_main(f: &Line, path: &str, text: &str) -> String {
    let back = f.git(&["rev-parse", "--abbrev-ref", "HEAD"]);
    f.git(&["switch", "-q", "main"]);
    f.write(path, text);
    f.commit("docs: advance main");
    f.git(&["push", "-q", "origin", "main"]);
    let tip = f.git(&["rev-parse", "HEAD"]);
    f.git(&["switch", "-q", &back]);
    tip
}

fn criterion(f: &Line, task: &str, from: &str, to: &str) {
    let path = record_path(task);
    f.write(&path, &read(f, &path).replace(from, to));
}

/// The clean batch: a standalone task member, an epic task member that
/// targets `main` and a filing of a new record pass the hosted form, each
/// judged at its own head.
#[test]
fn a_clean_main_batch_passes_the_hosted_form() {
    let (f, tip) = batch();
    let standalone = member(&f, "TSK-010", &tip, untouched);
    // The epic member also adds a record, which targets the line and so
    // binds nothing, and changes no existing criterion.
    let epic = member(&f, "TSK-011", &tip, |f| {
        f.write(
            &record_path("TSK-013"),
            &main_task("TSK-013", Some("EPC-001")).replace(
                "integration_target: main",
                &format!("integration_target: {LINE}"),
            ),
        );
    });
    let filed = filing(&f, "TSK-012", &tip);
    land(&f, "TSK-010", &standalone);
    land(&f, "TSK-011", &epic);
    land(&f, "TSK-012", &filed);
    seed(&f);
    // No forge lookup takes part, so no token is needed.
    let result = f
        .command(env!("CARGO_BIN_EXE_codeflow"))
        .env_remove("GH_TOKEN")
        .env_remove("GITHUB_TOKEN")
        .args([
            "ci",
            "--base",
            &tip,
            "--head",
            "HEAD",
            "--into",
            "main",
            "--branch",
            BATCH,
            "--pr-body",
            BODY,
        ])
        .output()
        .unwrap();
    passes(&result);
    has(&result, "main-batch-filings");
    has(&result, &format!("filing member {filed:.9}"));
    lacks(&result, "work.criteria_frozen (block)");
    lacks(&result, "work.acceptance_binding (block)");
    lacks(&result, "work.classification");
    has(&result, &format!("member {standalone:.9} (TSK-010)"));
    has(&result, &format!("member {epic:.9} (TSK-011)"));
    has(
        &result,
        &format!("member {filed:.9} (a filing of new records)"),
    );
    // The filing's diff is in the batch range, which gets the range checks
    // a filing pull request into `main` gets.
    for check in [
        "workflow-change",
        "path-policy",
        "work-records",
        "id-registry",
        "work.valid_graph",
        "main-batch",
        "classification",
        "acceptance",
    ] {
        has(&result, check);
    }
}

/// Binding: a member head that holds its reviewed commit with only a clean
/// merge of `main` after it binds, as a single-task pull request does.
#[test]
fn a_member_with_only_main_merges_after_its_review_passes() {
    let (f, tip) = batch();
    let reviewed = member(&f, "TSK-010", &tip, untouched);
    let moved = advance_main(&f, "docs/notes.md", "Main moved.\n");
    f.git(&["switch", "-q", "task/TSK-010-work"]);
    f.git(&["merge", "--no-ff", "-qm", "chore: merge main", "main"]);
    let head = f.git(&["rev-parse", "HEAD"]);
    f.git(&["switch", "-qc", "candidate/main-c2", &moved]);
    land(&f, "TSK-010", &head);
    let result = ci(
        &f,
        &[
            "--base",
            &moved,
            "--head",
            "HEAD",
            "--into",
            "main",
            "--branch",
            "candidate/main-c2",
            "--pr-body",
            BODY,
        ],
    );
    passes(&result);
    has(&result, &format!("member {head:.9} (TSK-010)"));
    assert_ne!(reviewed, head);
}

/// Binding: a completion commit that also changes another path is a
/// non-merge commit after the review that changes more than the record's
/// status and Closeout, so the member is refused.
#[test]
fn a_completion_that_changes_another_path_is_refused() {
    let (f, tip) = batch();
    f.git(&["switch", "-qc", "task/TSK-010-work", &tip]);
    f.write("src/tsk_010.rs", "pub fn member() {}\n");
    f.commit("feat: implement TSK-010");
    complete(
        &f,
        "TSK-010",
        "approved",
        Some(("src/tsk_010.rs", "pub fn changed_after_review() {}\n")),
    );
    let head = f.git(&["rev-parse", "HEAD"]);
    f.git(&["switch", "-q", BATCH]);
    land(&f, "TSK-010", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding");
    has(&result, &format!("member {head:.9} (TSK-010)"));
}

/// Binding: a hand-resolved merge of `main` after the review refuses.
#[test]
fn a_hand_resolved_merge_of_main_after_the_review_is_refused() {
    let (f, tip) = batch();
    member(&f, "TSK-010", &tip, untouched);
    let moved = advance_main(&f, "src/lib.rs", "pub fn moved() {}\n");
    f.git(&["switch", "-q", "task/TSK-010-work"]);
    f.git(&["merge", "--no-ff", "--no-commit", "-q", "main"]);
    f.write("src/lib.rs", "pub fn resolved_by_hand() {}\n");
    f.git(&["add", "-A"]);
    f.git(&["commit", "-qm", "chore: merge main"]);
    let head = f.git(&["rev-parse", "HEAD"]);
    f.git(&["switch", "-qc", "candidate/main-c2", &moved]);
    land(&f, "TSK-010", &head);
    let result = ci(
        &f,
        &[
            "--base",
            &moved,
            "--head",
            "HEAD",
            "--into",
            "main",
            "--branch",
            "candidate/main-c2",
            "--pr-body",
            BODY,
        ],
    );
    blocks(&result, "work.acceptance_binding");
    has(&result, &format!("member {head:.9} (TSK-010)"));
    has(&result, "clean re-merge");
}

/// Binding: a member whose acceptance block has no approved verdict at
/// that head is refused.
#[test]
fn a_member_with_no_approved_verdict_is_refused() {
    let (f, tip) = batch();
    f.git(&["switch", "-qc", "task/TSK-010-work", &tip]);
    f.write("src/tsk_010.rs", "pub fn member() {}\n");
    f.commit("feat: implement TSK-010");
    complete(&f, "TSK-010", "changes-requested", None);
    let head = f.git(&["rev-parse", "HEAD"]);
    f.git(&["switch", "-q", BATCH]);
    land(&f, "TSK-010", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "TSK-010");
    has(&result, &format!("member {head:.9} (TSK-010)"));
    lacks(&result, "codeflow ci: passed");
}

/// A commit made directly on the batch refuses, naming it.
#[test]
fn a_direct_commit_on_the_batch_is_refused() {
    let (f, tip) = batch();
    let head = member(&f, "TSK-010", &tip, untouched);
    land(&f, "TSK-010", &head);
    f.write("src/direct.rs", "pub fn direct() {}\n");
    let direct = f.commit("fix: direct on the batch");
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding");
    has(
        &result,
        &format!("candidate commit {direct:.9} is not a merge"),
    );
}

/// A member merge that differs from the clean remerge of its parents
/// refuses, naming the hunk's path.
#[test]
fn a_hand_resolved_member_merge_is_refused() {
    let (f, tip) = batch();
    let head = member(&f, "TSK-010", &tip, untouched);
    f.git(&["merge", "--no-ff", "--no-commit", "-q", &head]);
    f.write("src/tsk_010.rs", "pub fn changed_in_the_merge() {}\n");
    f.git(&["add", "-A"]);
    f.git(&["commit", "-qm", "chore: land TSK-010"]);
    let result = batch_ci(&f, &tip);
    blocks(&result, "differs from the clean remerge");
    has(&result, "src/tsk_010.rs");
}

/// A forged member, code with no completion, is refused.
#[test]
fn a_forged_member_is_refused() {
    let (f, tip) = batch();
    let head = records_only(&f, "task/TSK-010-work", &tip, |f| {
        f.write("src/forged.rs", "pub fn forged() {}\n");
    });
    land(&f, "TSK-010", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding");
    has(&result, &format!("member {head:.9}"));
    has(&result, "adds no task completion");
}

/// A member branch and merge message that name a task grant nothing: the
/// member is still refused for what its records say.
#[test]
fn a_branch_name_naming_another_task_grants_nothing() {
    let (f, tip) = batch();
    // TSK-011's branch carries TSK-010's completion.
    let head = member_on(&f, "task/TSK-011-work", "TSK-010", &tip, |f| {
        f.write("src/tsk_011.rs", "pub fn other() {}\n");
    });
    land(&f, "TSK-011", &head);
    let result = batch_ci(&f, &tip);
    passes(&result);
    has(&result, &format!("member {head:.9} (TSK-010)"));
    // And a code-only head on a branch named for TSK-010 binds nothing.
    let forged = records_only(&f, "task/TSK-010-again", &tip, |f| {
        f.write("src/forged.rs", "pub fn forged() {}\n");
    });
    f.git(&[
        "merge",
        "--no-ff",
        "-qm",
        "Merge branch 'task/TSK-010-again' (TSK-010)",
        &forged,
    ]);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding");
    has(&result, &format!("member {forged:.9}"));
}

/// The bound task's own criteria change, even with a dated note, refuses
/// in a batch into `main`.
#[test]
fn the_bound_tasks_own_criteria_change_is_refused() {
    let (f, tip) = batch();
    let head = member(&f, "TSK-010", &tip, |f| {
        criterion(
            f,
            "TSK-010",
            "- AC-4 The text is A.\n",
            "- AC-4 The text is B.\n  - Amended 2026-11-09: reworded.\n",
        );
    });
    land(&f, "TSK-010", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.criteria_frozen");
    has(&result, "TSK-010 changes its criteria");
}

/// Another task's criteria change refuses the member.
#[test]
fn another_tasks_criteria_change_is_refused() {
    let (f, tip) = batch();
    let head = member(&f, "TSK-010", &tip, |f| {
        criterion(
            f,
            "TSK-011",
            "AC-3 The output is plain.",
            "AC-3 The output is bold.",
        );
    });
    land(&f, "TSK-010", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.criteria_frozen");
    has(&result, "TSK-011 changes its criteria");
}

/// A record deleted and added back with its id kept and a criterion
/// changed is judged against its first-parent criteria.
#[test]
fn a_record_deleted_and_added_back_is_refused() {
    let (f, tip) = batch();
    let head = member(&f, "TSK-010", &tip, |f| {
        let path = record_path("TSK-011");
        let record = read(f, &path);
        std::fs::remove_file(f.root().join(&path)).unwrap();
        f.commit("docs: drop TSK-011");
        f.write(
            &path,
            &record.replace("AC-1 Work runs.", "AC-1 Work runs again."),
        );
    });
    land(&f, "TSK-010", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.criteria_frozen");
    has(&result, "TSK-011 changes its criteria");
}

/// A record renamed to another record path with its id kept and a
/// criterion changed is matched by id, never by path.
#[test]
fn a_renamed_record_is_matched_by_id() {
    let (f, tip) = batch();
    let head = member(&f, "TSK-010", &tip, |f| {
        let path = record_path("TSK-011");
        let record = read(f, &path);
        std::fs::remove_file(f.root().join(&path)).unwrap();
        f.write(
            "project-management/epics/EPC-001/tasks/TSK-011.md",
            &record.replace("AC-1 Work runs.", "AC-1 Work runs elsewhere."),
        );
    });
    land(&f, "TSK-010", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.criteria_frozen");
    has(&result, "TSK-011 changes its criteria");
}

/// A record split in two, one keeping the id with the old criteria and
/// one keeping the uid under a new id with a changed criterion, refuses.
#[test]
fn a_split_record_is_refused() {
    let (f, tip) = planned(&[
        ("TSK-010", main_task("TSK-010", None)),
        (
            "TSK-011",
            with_uid(&main_task("TSK-011", Some("EPC-001")), "u-tsk-011"),
        ),
    ]);
    let head = member(&f, "TSK-010", &tip, |f| {
        let record = read(f, &record_path("TSK-011"));
        f.write(
            &record_path("TSK-011"),
            &record.replace("uid: u-tsk-011\n", ""),
        );
        f.write(
            &record_path("TSK-017"),
            &record
                .replace("TSK-011", "TSK-017")
                .replace("AC-1 Work runs.", "AC-1 Work runs split."),
        );
    });
    land(&f, "TSK-010", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.criteria_frozen");
    has(&result, "TSK-011 matches TSK-011 and TSK-017");
}

/// A uid on two records in one tree refuses.
#[test]
fn a_uid_on_two_records_is_refused() {
    let (f, tip) = planned(&[
        ("TSK-010", main_task("TSK-010", None)),
        (
            "TSK-011",
            with_uid(&main_task("TSK-011", Some("EPC-001")), "u-tsk-011"),
        ),
    ]);
    let head = member(&f, "TSK-010", &tip, |f| {
        f.write(
            &record_path("TSK-018"),
            &with_uid(&main_task("TSK-018", Some("EPC-001")), "u-tsk-011"),
        );
    });
    land(&f, "TSK-010", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.criteria_frozen");
    has(
        &result,
        "uid u-tsk-011 is on two records at the member head",
    );
}

/// An id on two records in one tree refuses.
#[test]
fn an_id_on_two_records_is_refused() {
    let (f, tip) = batch();
    let head = member(&f, "TSK-010", &tip, |f| {
        f.write(
            "project-management/epics/EPC-001/tasks/TSK-011.md",
            &read(f, &record_path("TSK-011")),
        );
    });
    land(&f, "TSK-010", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding");
    has(&result, &format!("member {head:.9}"));
    has(&result, "TSK-011");
}

/// An unreadable task record at the member head refuses the member.
#[test]
fn an_unreadable_task_record_is_refused() {
    let (f, tip) = batch();
    let head = member(&f, "TSK-010", &tip, |f| {
        f.write(
            &record_path("TSK-011"),
            "---\nid: TSK-011\nstatus: [unclosed\n---\n",
        );
    });
    land(&f, "TSK-010", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding");
    has(&result, &format!("member {head:.9}"));
}

/// A task member whose range has two records that both bind it to `main`
/// refuses: a task member binds exactly one task.
#[test]
fn two_records_binding_one_member_are_refused() {
    let (f, tip) = batch();
    let head = member(&f, "TSK-010", &tip, |f| {
        f.write(&record_path("TSK-019"), &main_task("TSK-019", None));
    });
    land(&f, "TSK-010", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding");
    has(&result, "TSK-010, TSK-019 each declare 'main'");
}

/// A records-only member that amends another task's criteria is no filing
/// and refuses.
#[test]
fn a_records_only_member_amending_criteria_is_refused() {
    let (f, tip) = batch();
    let head = records_only(&f, "plan/amend", &tip, |f| {
        criterion(
            f,
            "TSK-011",
            "AC-3 The output is plain.",
            "AC-3 The output is bold.",
        );
    });
    land(&f, "plan", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding");
    has(
        &result,
        "it changes or removes project-management/tasks/TSK-011.md",
    );
    has(&result, "TSK-011 changes its criteria");
}

/// A records-only member that changes an existing record without touching
/// criteria is still no filing, and refuses.
#[test]
fn a_records_only_member_changing_a_record_is_refused() {
    let (f, tip) = batch();
    let head = records_only(&f, "plan/retitle", &tip, |f| {
        criterion(f, "TSK-011", "title: Work TSK-011", "title: Work renamed");
    });
    land(&f, "plan", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding");
    has(
        &result,
        "it changes or removes project-management/tasks/TSK-011.md",
    );
}

/// An epic task member whose epic is cancelled at the anchor refuses.
#[test]
fn a_cancelled_epic_refuses_its_member() {
    let f = Line::new();
    f.git(&["switch", "-q", "main"]);
    let epic = super::line_adoption::fixture::EPIC_PATH;
    f.write(
        epic,
        &read(&f, epic).replace("status: planning", "status: cancelled"),
    );
    f.write(
        &record_path("TSK-011"),
        &main_task("TSK-011", Some("EPC-001")),
    );
    f.commit("docs: plan main work under a cancelled epic");
    f.git(&["push", "-q", "origin", "main"]);
    let tip = f.git(&["rev-parse", "HEAD"]);
    f.git(&["switch", "-qc", BATCH]);
    let head = member(&f, "TSK-011", &tip, untouched);
    land(&f, "TSK-011", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding");
    has(
        &result,
        "names EPC-001, which is cancelled at the batch's anchor",
    );
}

/// A task member whose record does not declare `main` refuses.
#[test]
fn a_member_that_does_not_declare_main_is_refused() {
    let line_task = main_task("TSK-020", Some("EPC-001")).replace(
        "integration_target: main",
        &format!("integration_target: {LINE}"),
    );
    let (f, tip) = planned(&[("TSK-020", line_task)]);
    let head = member(&f, "TSK-020", &tip, untouched);
    land(&f, "TSK-020", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding");
    has(&result, "does not declare 'main' as its integration_target");
}

/// The batch's body names no `Task:` line: its members bind from their
/// own records.
#[test]
fn a_task_line_in_the_batch_body_is_refused() {
    let (f, tip) = batch();
    let head = member(&f, "TSK-010", &tip, untouched);
    land(&f, "TSK-010", &head);
    let result = batch_ci_with(&f, &tip, &format!("Task: TSK-010\n\n{BODY}"));
    blocks(&result, "work.classification");
    has(&result, "carries no `Task:` line");
}

/// The anchor is `main`'s fetched tip, never a supplied base: a base after
/// a refused member hides nothing.
#[test]
fn a_base_after_the_anchor_does_not_shorten_the_batch() {
    let (f, tip) = batch();
    let forged = records_only(&f, "task/TSK-010-work", &tip, |f| {
        f.write("src/forged.rs", "pub fn forged() {}\n");
    });
    let after = land(&f, "TSK-010", &forged);
    let head = member(&f, "TSK-011", &tip, untouched);
    land(&f, "TSK-011", &head);
    let result = batch_ci(&f, &after);
    blocks(&result, "work.acceptance_binding");
    has(&result, &format!("member {forged:.9}"));
    assert_ne!(tip, after);
}

/// A pushed batch gets the batch verdict: the clean batch passes the
/// pre-push hook, and a forged member is refused there too.
#[test]
fn a_pushed_main_batch_gets_the_batch_verdict() {
    let (f, tip) = planned_with(
        &[
            ("TSK-010", main_task("TSK-010", None)),
            ("TSK-011", main_task("TSK-011", Some("EPC-001"))),
        ],
        true,
    );
    let standalone = member(&f, "TSK-010", &tip, untouched);
    let filed = filing(&f, "TSK-012", &tip);
    land(&f, "TSK-010", &standalone);
    land(&f, "TSK-012", &filed);
    seed(&f);
    let pushed = f.push_branch(BATCH, ZERO);
    passes(&pushed);
    let forged = records_only(&f, "task/TSK-011-work", &tip, |f| {
        f.write("src/forged.rs", "pub fn forged() {}\n");
    });
    land(&f, "TSK-011", &forged);
    let pushed = f.push_branch(BATCH, ZERO);
    assert!(!pushed.status.success(), "{}", output(&pushed));
    assert!(
        output(&pushed).contains(&format!("{forged:.9}")),
        "{}",
        output(&pushed)
    );
}

/// A filing adds only new files at record paths: any other added path
/// refuses the member.
#[test]
fn a_filing_that_adds_another_path_is_refused() {
    let (f, tip) = batch();
    let head = records_only(&f, "plan/file-and-note", &tip, |f| {
        f.write(&record_path("TSK-012"), &main_task("TSK-012", None));
        f.write("docs/plan/note.md", "A note.\n");
    });
    land(&f, "plan", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding");
    has(
        &result,
        "it adds docs/plan/note.md, which is not a record file",
    );
}

/// A new record that carries an acceptance block, without being a
/// completion the member adds, is no filing: it would bind as a task
/// member, so the member refuses.
#[test]
fn a_filing_of_a_record_with_an_acceptance_block_is_refused() {
    let (f, tip) = batch();
    let head = records_only(&f, "plan/file-reviewed", &tip, |f| {
        f.write(
            &record_path("TSK-012"),
            &format!("{}\n## Closeout\n\n```yaml\nacceptance:\n  reviewed: {tip}\n  review: https://example.test/review/TSK-012\n  criteria:\n    AC-1: verified | focused test\n    AC-2: verified | journey\n    AC-3: verified | focused test\n    AC-4: verified | focused test\n  journey: verified | exercised command\n  not_verified: none\n  follow_ups: none: fixture\n  verdict: approved\n```\n", main_task("TSK-012", None)),
        );
    });
    land(&f, "plan", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding");
    has(&result, "which shows a completion");
}

/// `task`'s head on `task/<task>-work` from `from`: its code, then a
/// completion that also writes `blocker` before the Closeout.
fn member_with_blocker(f: &Line, task: &str, from: &str, blocker: &str) -> String {
    f.git(&["switch", "-qc", &format!("task/{task}-work"), from]);
    f.write("src/tsk_010.rs", "pub fn member() {}\n");
    f.commit(&format!("feat: implement {task}"));
    let path = record_path(task);
    let record = read(f, &path);
    f.write(&path, &format!("{record}\n{blocker}"));
    complete(f, task, "approved", None);
    let head = f.git(&["rev-parse", "HEAD"]);
    f.git(&["switch", "-q", BATCH]);
    head
}

/// Binding: a completion after the reviewed commit that changes only the
/// record's status, its Closeout and a Blocker of the generated shape
/// binds, as the one-task binding leaves those out of reviewed scope.
#[test]
fn a_completion_with_a_generated_blocker_passes() {
    let (f, tip) = batch();
    let head = member_with_blocker(
        &f,
        "TSK-010",
        &tip,
        "## Blocker\n\n- reason: waited on review\n- owner: coord\n- revisit: 2026-11-10\n",
    );
    land(&f, "TSK-010", &head);
    let result = batch_ci(&f, &tip);
    passes(&result);
    has(&result, &format!("member {head:.9} (TSK-010)"));
}

/// Binding: any other text in the Blocker section keeps it reviewed, so a
/// completion that writes it refuses the member.
#[test]
fn a_completion_with_other_blocker_text_is_refused() {
    let (f, tip) = batch();
    let head = member_with_blocker(
        &f,
        "TSK-010",
        &tip,
        "## Blocker\n\n- reason: waited on review\n- owner: coord\n- revisit: 2026-11-10\n\nAlso widened the scope.\n",
    );
    land(&f, "TSK-010", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding");
    has(&result, &format!("member {head:.9} (TSK-010)"));
}

/// A task member whose task is not complete at its head, though its record
/// carries an acceptance block, is no reviewed head and refuses.
#[test]
fn a_member_whose_task_is_not_complete_is_refused() {
    let (f, tip) = batch();
    f.git(&["switch", "-qc", "task/TSK-010-work", &tip]);
    f.write("src/tsk_010.rs", "pub fn member() {}\n");
    f.commit("feat: implement TSK-010");
    complete(&f, "TSK-010", "approved", None);
    let path = record_path("TSK-010");
    f.write(
        &path,
        &read(&f, &path).replace("status: complete", "status: todo"),
    );
    let head = f.commit("docs: leave TSK-010 open");
    f.git(&["switch", "-q", BATCH]);
    land(&f, "TSK-010", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding");
    has(&result, &format!("member {head:.9}"));
}

/// Binding: a merge after the reviewed commit whose other parent is not on
/// the anchor `main`'s first-parent line refuses the member.
#[test]
fn a_merge_of_a_side_branch_after_the_review_is_refused() {
    let (f, tip) = batch();
    member(&f, "TSK-010", &tip, untouched);
    let side = records_only(&f, "side/extra", &tip, |f| {
        f.write("src/side.rs", "pub fn side() {}\n");
    });
    f.git(&["switch", "-q", "task/TSK-010-work"]);
    f.git(&[
        "merge",
        "--no-ff",
        "-qm",
        "chore: merge a side branch",
        &side,
    ]);
    let head = f.git(&["rev-parse", "HEAD"]);
    f.git(&["switch", "-q", BATCH]);
    land(&f, "TSK-010", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding");
    has(&result, &format!("member {head:.9} (TSK-010)"));
    has(&result, "not on the first-parent line");
}

/// A batch that carries a changed policy is judged with the base's: a
/// member that lowers `work_records` to warn does not lower the refusal of
/// a forged member beside it.
#[test]
fn a_batch_carrying_a_changed_policy_is_judged_with_the_bases() {
    let (f, tip) = batch();
    let lowering = member(&f, "TSK-010", &tip, |f| {
        f.write(
            ".codeflow/policy.json",
            "{\"schema_version\":1,\"git\":{\"product_paths\":[\"src/**\"],\"test_gate_on_push\":\"off\",\"work_records\":\"warn\"}}\n",
        );
    });
    let forged = records_only(&f, "task/TSK-011-work", &tip, |f| {
        f.write("src/forged.rs", "pub fn forged() {}\n");
    });
    land(&f, "TSK-010", &lowering);
    land(&f, "TSK-011", &forged);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding (block)");
    has(&result, &format!("member {forged:.9}"));
}

/// A records-only member that changes an existing task's status is no
/// filing, and refuses.
#[test]
fn a_records_only_member_changing_a_status_is_refused() {
    let (f, tip) = batch();
    let head = records_only(&f, "plan/start", &tip, |f| {
        criterion(f, "TSK-011", "status: todo", "status: in_progress");
    });
    land(&f, "plan", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding");
    has(
        &result,
        "it changes or removes project-management/tasks/TSK-011.md",
    );
}

/// A records-only member that changes any other existing file is no
/// filing, and refuses.
#[test]
fn a_records_only_member_changing_another_file_is_refused() {
    let (f, tip) = batch();
    let epic = super::line_adoption::fixture::EPIC_PATH;
    let head = records_only(&f, "plan/summary", &tip, |f| {
        f.write(
            epic,
            &read(f, epic).replace("Deliver work.", "Deliver more work."),
        );
    });
    land(&f, "plan", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding");
    has(&result, &format!("it changes or removes {epic}"));
}

/// A filing whose new record breaks a record rule a one-pull-request filing
/// must pass refuses, naming the member: new records list criteria without
/// a checkbox.
#[test]
fn a_filing_whose_record_breaks_a_record_rule_is_refused() {
    let (f, tip) = batch();
    let head = records_only(&f, "plan/file-boxed", &tip, |f| {
        f.write(
            &record_path("TSK-012"),
            &main_task("TSK-012", None).replace("- AC-1 Work runs.", "- [ ] AC-1 Work runs."),
        );
    });
    land(&f, "plan", &head);
    seed(&f);
    let result = batch_ci(&f, &tip);
    blocks(&result, "new records list criteria");
    has(
        &result,
        &format!("member {head:.9} (a filing of new records), merged by"),
    );
}

/// A filing that adds a workflow path is no filing, and refuses.
#[test]
fn a_filing_that_adds_a_workflow_path_is_refused() {
    let (f, tip) = batch();
    let head = records_only(&f, "plan/file-workflow", &tip, |f| {
        f.write(&record_path("TSK-012"), &main_task("TSK-012", None));
        f.write(
            ".github/workflows/extra.yml",
            "name: extra\non: push\njobs: {}\n",
        );
    });
    land(&f, "plan", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding");
    has(
        &result,
        "it adds .github/workflows/extra.yml, which is not a record file",
    );
}

/// A filing whose new complete record carries an acceptance block binds
/// as a task member, so a reviewed commit that is not an ancestor of its
/// head refuses it.
#[test]
fn a_filing_of_a_complete_record_reviewed_elsewhere_is_refused() {
    let (f, tip) = batch();
    let elsewhere = records_only(&f, "side/elsewhere", &tip, |f| {
        f.write("src/elsewhere.rs", "pub fn elsewhere() {}\n");
    });
    let head = records_only(&f, "plan/file-complete", &tip, |f| {
        f.write(
            &record_path("TSK-012"),
            &format!("{}\n## Closeout\n\n```yaml\nacceptance:\n  reviewed: {elsewhere}\n  review: https://example.test/review/TSK-012\n  criteria:\n    AC-1: verified | focused test\n    AC-2: verified | journey\n    AC-3: verified | focused test\n    AC-4: verified | focused test\n  journey: verified | exercised command\n  not_verified: none\n  follow_ups: none: fixture\n  verdict: approved\n```\n", main_task("TSK-012", None).replace("status: todo", "status: complete")),
        );
    });
    land(&f, "plan", &head);
    seed(&f);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding");
    has(&result, &format!("member {head:.9} (TSK-012)"));
}

/// A filing whose new record carries a uid the id registry does not hold
/// refuses, naming the member.
#[test]
fn a_filing_with_an_unregistered_uid_is_refused() {
    let (f, tip) = batch();
    seed(&f);
    let head = records_only(&f, "plan/file-uid", &tip, |f| {
        f.write(
            &record_path("TSK-012"),
            &with_uid(&main_task("TSK-012", None), "u-not-registered"),
        );
    });
    land(&f, "plan", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.id_registry");
    has(
        &result,
        &format!("member {head:.9} (a filing of new records), merged by"),
    );
}

/// An unreadable task record on the tip a member merged onto refuses the
/// member: its records cannot be judged.
#[test]
fn an_unreadable_record_on_the_tip_refuses_the_member() {
    let (f, tip) = planned(&[
        ("TSK-010", main_task("TSK-010", None)),
        (
            "TSK-014",
            "---\nid: TSK-014\nstatus: [unclosed\n---\n".to_string(),
        ),
    ]);
    let head = member(&f, "TSK-010", &tip, untouched);
    land(&f, "TSK-010", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding");
    has(&result, &format!("member {head:.9}"));
}

/// Two members from the anchor: after the second's review it merges the
/// candidate's first member merge, then lands onto it. That merge is
/// unlanded work, not `main`, so it neither carries a post-review merge
/// nor serves as the member's base, and the member is refused.
#[test]
fn a_member_merging_an_earlier_candidate_merge_after_its_review_is_refused() {
    let (f, tip) = batch();
    let first = member(&f, "TSK-011", &tip, untouched);
    member(&f, "TSK-010", &tip, untouched);
    let c1 = land(&f, "TSK-011", &first);
    f.git(&["switch", "-q", "task/TSK-010-work"]);
    f.git(&["merge", "--no-ff", "-qm", "chore: merge the candidate", &c1]);
    let head = f.git(&["rev-parse", "HEAD"]);
    f.git(&["switch", "-q", BATCH]);
    land(&f, "TSK-010", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding");
    has(&result, &format!("member {head:.9}"));
    has(&result, "so it starts from unlanded work");
}

/// A member that branches from an earlier candidate merge carries that
/// member's unlanded work, which its own pull request into `main` would
/// carry too, and is refused.
#[test]
fn a_member_branched_from_an_earlier_candidate_merge_is_refused() {
    let (f, tip) = batch();
    let first = member(&f, "TSK-011", &tip, untouched);
    let c1 = land(&f, "TSK-011", &first);
    let head = member(&f, "TSK-010", &c1, untouched);
    land(&f, "TSK-010", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding");
    has(&result, &format!("member {head:.9}"));
    has(&result, "so it starts from unlanded work");
}

/// A filing that adds TSK-012, which depends on TSK-013, and another
/// filing that adds TSK-013. The batch head resolves the dependency, but
/// the first filing's own pull request into `main` does not, and the batch
/// refuses that member with the same finding its own run gives.
#[test]
fn a_filing_depending_on_a_task_another_filing_adds_is_refused_as_alone() {
    let (f, tip) = batch();
    let dependent = records_only(&f, "plan/TSK-012-file", &tip, |f| {
        f.write(
            &record_path("TSK-012"),
            &main_task("TSK-012", None).replace("depends_on: []", "depends_on: [TSK-013]"),
        );
    });
    let other = filing(&f, "TSK-013", &tip);
    land(&f, "TSK-012", &dependent);
    land(&f, "TSK-013", &other);
    seed(&f);
    let finding = "task TSK-012 depends on TSK-013, but no task with that id exists";
    let batched = batch_ci(&f, &tip);
    blocks(&batched, "work.valid_graph");
    has(&batched, &format!("member {dependent:.9}, merged by"));
    has(&batched, finding);
    // Its own pull request names the task it files.
    f.git(&["switch", "-q", "plan/TSK-012-file"]);
    let body = format!("Task: TSK-012\n\n{BODY}");
    let alone = ci(
        &f,
        &[
            "--base",
            &tip,
            "--head",
            "HEAD",
            "--into",
            "main",
            "--branch",
            "plan/TSK-012-file",
            "--pr-body",
            &body,
        ],
    );
    blocks(&alone, "work.valid_graph");
    has(&alone, finding);
}

/// An epic task that drops its epic and becomes standalone in its reviewed
/// change is refused: a member keeps the owner its anchor record names,
/// whatever that epic's state, and the anchor's cancelled or archived epic
/// still refuses it.
#[test]
fn an_epic_task_turned_standalone_is_refused() {
    let to_standalone = |f: &Line| {
        criterion(
            f,
            "TSK-011",
            "epic_id: EPC-001",
            "epic_id: null\nstandalone_reason: One bounded fix",
        );
    };
    for state in [None, Some("cancelled"), Some("archived")] {
        let f = Line::new();
        f.git(&["switch", "-q", "main"]);
        if let Some(state) = state {
            let epic = super::line_adoption::fixture::EPIC_PATH;
            f.write(
                epic,
                &read(&f, epic).replace("status: planning", &format!("status: {state}")),
            );
        }
        f.write(
            &record_path("TSK-011"),
            &main_task("TSK-011", Some("EPC-001")),
        );
        f.commit("docs: plan main work");
        f.git(&["push", "-q", "origin", "main"]);
        let tip = f.git(&["rev-parse", "HEAD"]);
        f.git(&["switch", "-qc", BATCH]);
        let head = member(&f, "TSK-011", &tip, to_standalone);
        land(&f, "TSK-011", &head);
        let result = batch_ci(&f, &tip);
        blocks(&result, "work.acceptance_binding");
        has(&result, &format!("member {head:.9}"));
        has(
            &result,
            "TSK-011 names no epic at the member head, but EPC-001 at the batch's anchor",
        );
    }
}

/// A task the anchor holds under an epic the anchor has no record of
/// cannot take that epic's record from the member head: a member that
/// adds the record itself is refused.
#[test]
fn an_epic_record_only_the_member_adds_does_not_bind_an_anchor_task() {
    let (f, tip) = planned(&[("TSK-011", main_task("TSK-011", Some("EPC-002")))]);
    let head = member(&f, "TSK-011", &tip, |f| {
        let epic = super::line_adoption::fixture::EPIC_PATH;
        f.write(
            "project-management/epics/EPC-002.md",
            &read(f, epic).replace("id: EPC-001", "id: EPC-002"),
        );
    });
    land(&f, "TSK-011", &head);
    seed(&f);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding");
    has(&result, &format!("member {head:.9}"));
    has(
        &result,
        "TSK-011 names EPC-002, which has no epic record at the batch's anchor",
    );
    lacks(&result, "work.id_registry (block)");
}

/// A standalone task that takes an epic in its reviewed change is refused
/// the same way; the clean batch keeps its standalone member passing.
#[test]
fn a_standalone_task_given_an_epic_is_refused() {
    let (f, tip) = batch();
    let head = member(&f, "TSK-010", &tip, |f| {
        criterion(
            f,
            "TSK-010",
            "epic_id: null\nstandalone_reason: One bounded fix",
            "epic_id: EPC-001",
        );
    });
    land(&f, "TSK-010", &head);
    let result = batch_ci(&f, &tip);
    blocks(&result, "work.acceptance_binding");
    has(
        &result,
        "TSK-010 names EPC-001 at the member head, but no epic at the batch's anchor",
    );
}
