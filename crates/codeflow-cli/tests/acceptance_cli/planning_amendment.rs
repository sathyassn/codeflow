//! A task waiver names a planning amendment that changes more than the
//! task's record (issue 465). On the target, an epic amendment that also
//! changes the epic record, the epic plan and other task records waives
//! each criterion it amends, as the epic waiver rule allows. In the task's
//! own range, an amendment of its own criterion may update the plan beside
//! it. Either way the commit changes planning records only and amends that
//! criterion of this task.
use super::*;

/// TSK-001 targeting [`LINE`] with AC-1 removed, as an amendment leaves it.
fn removed_task(status: &str, closeout: &str) -> String {
    line_task("TSK-001", status, closeout).replace("- AC-1 When run, the system shall work.\n", "")
}

/// How the amendment leaves TSK-001's AC-1.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Own {
    /// AC-1 reworded.
    Amended,
    /// AC-1 removed.
    Removed,
    /// TSK-001 untouched.
    Untouched,
}

/// What the amendment changes besides TSK-001.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Extra {
    /// The epic plan only.
    Plan,
    /// The epic record, the epic plan and TSK-002's record.
    EpicPlanAndTask,
    /// The epic plan and a product file.
    Product,
    /// The epic plan and the policy file.
    Policy,
    /// The epic plan and a harness instruction file beside it.
    Instructions,
    /// The epic plan and a hidden settings file beside it.
    Hidden,
    /// The epic plan and a watched contract beside it ([`WATCHED`]).
    Watched,
}

/// A breaking-change watch path beneath the plan folder, so a planning
/// directory holds an adopter-facing file.
const WATCHED: &str = ", \"breaking_watch_paths\": [\"docs/plan/contract.md\"]";

/// Write TSK-001 as `status` with `closeout`, its AC-1 as `own` leaves it.
fn write_own(root: &Path, own: Own, status: &str, closeout: &str) {
    let text = match own {
        Own::Amended => amended_task("TSK-001", status, closeout),
        Own::Removed => removed_task(status, closeout),
        Own::Untouched => line_task("TSK-001", status, closeout),
    };
    write(root, &record_path("TSK-001"), &text);
}

/// Write the amendment's changes on the current branch and commit them.
fn amendment(root: &Path, own: Own, extra: Extra) -> String {
    if own != Own::Untouched {
        write_own(root, own, "todo", "Pending.\n");
    }
    write(
        root,
        "docs/plan/EPC-001.md",
        "# EPC-001 plan\n\nTSK-001 AC-1 narrowed to Linux.\n",
    );
    match extra {
        Extra::Plan => {}
        Extra::EpicPlanAndTask => {
            write(
                root,
                "project-management/epics/EPC-001.md",
                &EPIC.replace("An outcome.", "An outcome, narrowed for Linux."),
            );
            write(
                root,
                &record_path("TSK-002"),
                &line_task("TSK-002", "todo", "Pending.\n")
                    .replace("shall work.", "shall still work."),
            );
        }
        Extra::Product => write(root, "src/sneak.rs", "pub fn sneak() {}\n"),
        Extra::Policy => write(
            root,
            ".codeflow/policy.json",
            "{\n  \"schema_version\": 1,\n  \"git\": {\"product_paths\": []}\n}\n",
        ),
        Extra::Instructions => write(root, "docs/plan/AGENTS.md", "# Plan agents\n"),
        Extra::Hidden => write(root, "docs/plan/.claude/settings.json", "{}\n"),
        Extra::Watched => write(root, "docs/plan/contract.md", "# Contract\n"),
    }
    commit(root, "docs(plan): narrow EPC-001 for Linux")
}

/// Complete TSK-001 with AC-1 waived by `by`, reviewed at `reviewed`, and
/// run `codeflow ci` for the task pull request into [`LINE`].
fn complete_and_judge(root: &Path, own: Own, reviewed: &str, by: &str) -> (i32, String) {
    let own = if own == Own::Untouched {
        Own::Amended
    } else {
        own
    };
    // A removed criterion leaves the record, so the block has no row for
    // it to waive (`work.records` refuses a row the record does not have).
    let closeout = if own == Own::Removed {
        block(
            reviewed,
            &["AC-2: verified | journey ran"],
            "verified | tests/journey.rs",
            "none: nothing deferred",
        )
    } else {
        waived_block(reviewed, by)
    };
    write_own(root, own, "complete", &closeout);
    commit(root, "docs(records): complete TSK-001");
    ci_on(root, LINE, "task/TSK-001-work", "Task: TSK-001")
}

/// The amendment lands on [`LINE`] through a planning pull request; the
/// task branch, built before it, merges the line before the review. The
/// waiver names the amendment, or the landing merge when `name_merge`.
fn on_target(own: Own, extra: Extra, name_merge: bool) -> (i32, String) {
    let dir = line_repo_with(&["TSK-001", "TSK-002"], WATCHED);
    let root = dir.path();
    build(root, "task/TSK-001-work", "src/work.rs");
    git(root, &["switch", "-C", "plan/amend", LINE]);
    let amended = amendment(root, own, extra);
    let landed = land(root, "plan/amend");
    git(root, &["switch", "task/TSK-001-work"]);
    git(
        root,
        &["merge", "--no-ff", "-m", "chore: merge the line", LINE],
    );
    write(root, "src/work.rs", "pub fn work_on_linux() {}\n");
    let reviewed = commit(root, "feat: work on Linux");
    let by = if name_merge { landed } else { amended };
    complete_and_judge(root, own, &reviewed, &by)
}

/// The task amends its own criterion in its own range, before the review.
fn own_range(own: Own, extra: Extra) -> (i32, String) {
    let dir = line_repo_with(&["TSK-001", "TSK-002"], WATCHED);
    let root = dir.path();
    build(root, "task/TSK-001-work", "src/work.rs");
    let amended = amendment(root, own, extra);
    write(root, "src/work.rs", "pub fn work_on_linux() {}\n");
    let reviewed = commit(root, "feat: work on Linux");
    complete_and_judge(root, own, &reviewed, &amended)
}

/// Issue 465: an epic amendment on the target that also changes the epic
/// record, the plan and another task's record waives the criterion it
/// amends or removes, named by its commit or by its landing merge.
#[test]
fn a_task_waiver_names_an_epic_amendment_on_the_target() {
    for own in [Own::Amended, Own::Removed] {
        for name_merge in [false, true] {
            assert_passes(
                &on_target(own, Extra::EpicPlanAndTask, name_merge),
                &format!("{own:?}, named by the merge: {name_merge}"),
            );
        }
    }
}

/// Issue 465: in the task's own range, an amendment of its own criterion
/// may update the epic plan in the same commit. A criterion the task's own
/// pull request removes needs no waiver; `work.criteria_removal` asks the
/// pull request body for its reason instead.
#[test]
fn an_own_range_waiver_may_update_the_plan_beside_the_record() {
    assert_passes(&own_range(Own::Amended, Extra::Plan), "amended");
}

/// Fail closed on both routes: the amendment changes planning records only,
/// as the hardened amendment classification reads them (no instruction,
/// hidden or adopter-facing file even beneath a planning folder), and
/// amends this task's criterion.
#[test]
fn a_task_waiver_refuses_a_mixed_or_unrelated_amendment() {
    let refusals = [
        (Own::Amended, Extra::Product, "also changes src/sneak.rs"),
        (
            Own::Amended,
            Extra::Policy,
            "also changes .codeflow/policy.json",
        ),
        (
            Own::Amended,
            Extra::Instructions,
            "also changes docs/plan/AGENTS.md",
        ),
        (
            Own::Amended,
            Extra::Hidden,
            "also changes docs/plan/.claude/settings.json",
        ),
        (
            Own::Amended,
            Extra::Watched,
            "also changes docs/plan/contract.md",
        ),
        (
            Own::Untouched,
            Extra::Plan,
            "which does not amend AC-1 of TSK-001",
        ),
    ];
    for (own, extra, needle) in refusals {
        assert_blocks(
            &on_target(own, extra, false),
            &format!("on the target: {own:?}, {extra:?}"),
            &["AC-1 waiver", needle],
        );
        assert_blocks(
            &own_range(own, extra),
            &format!("own range: {own:?}, {extra:?}"),
            &["AC-1 waiver", needle],
        );
    }
}
