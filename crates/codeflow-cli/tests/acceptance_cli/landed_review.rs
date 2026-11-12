//! A task that lands on a marked epic's line with no pull request is
//! admitted only when its completion names an approving review at the
//! commit the landing carries, the one lineage that built it (`built_by`)
//! and a reviewer of another lineage, or the recorded reduced-assurance
//! statement (ADR-0102, TSK-1433). The marker is the epic record's
//! `main_landing: whole-epic` on `main`. Each landing is judged in both
//! forms `codeflow ci` meets it: the line's push, with no pull request body,
//! and the epic's pull request into `main`. Both give the same verdict. An
//! unmarked line, and a task pull request into a line, are judged as before.
use super::*;

const LINE: &str = "integration/EPC-001-line";
const TASK_BRANCH: &str = "task/TSK-001-work";
/// A plan's Builder line that names a lineage and its fallback.
const TWO_FAMILY_PLAN: &str =
    "Codex GPT-6.1 Sol at medium, else a Claude Opus 5.5 medium subagent.";
const CODEX_REVIEW: &str = "Codex GPT-6.1 Sol at medium, whole unit, approved";
const CLAUDE_REVIEW: &str = "Claude Fable 5.1 as cf-reviewer, whole unit";
const WHOLE_EPIC: Option<&str> = Some("whole-epic");

/// TSK-001 targeting [`LINE`] as `status`, planned for [`TWO_FAMILY_PLAN`],
/// with `closeout`.
fn record(status: &str, closeout: &str) -> String {
    let planned = task("TSK-001", status, OWN_JOURNEY, "").replace(
        "integration_target: main",
        &format!("integration_target: {LINE}"),
    );
    let planned = planned.trim_end_matches("## Closeout\n\n");
    format!(
        "{planned}## Execution contract\n\n- Builder: {TWO_FAMILY_PLAN}\n\n## Closeout\n\n{closeout}"
    )
}

/// An acceptance block reviewed at `reviewed` by `review` with `verdict`,
/// with `fields` (whole `  key: value` lines) after `review`.
fn completion(reviewed: &str, review: &str, fields: &str, verdict: &str) -> String {
    format!(
        "```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: {review}\n{fields}  criteria:\n    AC-1: verified | cargo test | 3 passed\n    AC-2: verified | journey ran\n  journey: verified | tests/journey.rs\n  not_verified: none\n  follow_ups: none: nothing deferred\n  verdict: {verdict}\n```\n"
    )
}

/// One landing: the repository and the line's tip before it.
struct Landing {
    dir: tempfile::TempDir,
    before: String,
}

/// TSK-001 built on its branch in two commits, completed there with the
/// block `closeout` gives for (first commit, second commit), when it gives
/// one, then merged into [`LINE`] with no pull request. `marker` is the
/// epic record's `main_landing` line on `main`, when it has one.
fn landing(marker: Option<&str>, closeout: impl Fn(&str, &str) -> Option<String>) -> Landing {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    write(root, &record_path("TSK-001"), &record("todo", "Pending.\n"));
    if let Some(marker) = marker {
        write(
            root,
            "project-management/epics/EPC-001.md",
            &EPIC.replacen(
                "status: planning\n",
                &format!("status: planning\nmain_landing: {marker}\n"),
                1,
            ),
        );
    }
    commit(root, "docs(records): target the line");
    git(root, &["branch", LINE, "main"]);
    let before = git_out(root, &["rev-parse", LINE]);
    git(root, &["switch", "-C", TASK_BRANCH, LINE]);
    write(root, "src/work.rs", "pub fn work() {}\n");
    let first = commit(root, "feat: build the work");
    write(root, "src/more.rs", "pub fn more() {}\n");
    let head = commit(root, "feat: build more of the work");
    if let Some(done) = closeout(&first, &head) {
        write(root, &record_path("TSK-001"), &record("complete", &done));
        commit(root, "docs(records): complete TSK-001");
    }
    land(root, TASK_BRANCH);
    Landing { dir, before }
}

/// A landing on a line `marker` marks, completed at the head it carries
/// by `review` with `fields` and `verdict`.
fn reviewed_by(
    marker: Option<&'static str>,
    review: &'static str,
    fields: &'static str,
    verdict: &'static str,
) -> Landing {
    landing(marker, move |_, head| {
        Some(completion(head, review, fields, verdict))
    })
}

/// The line's push: `codeflow ci` over the landing with no pull request
/// body, as the pre-push hook runs it.
fn push_form(landing: &Landing) -> (i32, String) {
    push_with(landing, &[])
}

/// [`push_form`] with `extra` arguments.
fn push_with(landing: &Landing, extra: &[&str]) -> (i32, String) {
    let out = codeflow()
        .args([
            "ci",
            "--base",
            &landing.before,
            "--head",
            "HEAD",
            "--branch",
            LINE,
        ])
        .args(extra)
        .current_dir(landing.dir.path())
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

/// The epic's pull request: `codeflow ci` from `main` to the line's tip.
fn epic_form(landing: &Landing) -> (i32, String) {
    ci_on(landing.dir.path(), "main", LINE, "Task: EPC-001")
}

/// Both forms pass `landing`, and neither prints this check's findings.
fn passes_in_both_forms(what: &str, landing: &Landing) {
    for (form, result) in [
        ("push", push_form(landing)),
        ("epic PR", epic_form(landing)),
    ] {
        assert_passes(&result, &format!("{what} ({form})"));
        assert!(
            !result.1.contains("with no pull request") && !result.1.contains("main_landing"),
            "{what} ({form}): {}",
            result.1
        );
    }
}

/// Both forms refuse `landing`, each naming the cause by every needle.
fn refused_in_both_forms(what: &str, landing: &Landing, needles: &[&str]) {
    for (form, result) in [
        ("push", push_form(landing)),
        ("epic PR", epic_form(landing)),
    ] {
        assert_blocks(&result, &format!("{what} ({form})"), needles);
    }
}

/// AC-2, AC-4: on a marked line, a completion that names the commit the
/// landing carries, its builder's lineage and an approving reviewer of
/// another lineage passes in both forms.
#[test]
fn a_landing_reviewed_by_another_lineage_passes_in_both_forms() {
    for (review, fields) in [
        (CODEX_REVIEW, "  built_by: claude\n"),
        ("Grok 4.7 at high; whole unit", "  built_by: codex\n"),
    ] {
        let landed = reviewed_by(WHOLE_EPIC, review, fields, "approved");
        passes_in_both_forms(review, &landed);
    }
}

/// Batch 1 item 2: a Codex build reviewed by Claude passes, though the
/// plan's Builder line names Claude as Codex's fallback. Only `built_by`
/// says who built; the plan's line is not read.
#[test]
fn a_codex_build_reviewed_by_claude_passes_whatever_the_plan_names() {
    let landed = reviewed_by(WHOLE_EPIC, CLAUDE_REVIEW, "  built_by: codex\n", "approved");
    passes_in_both_forms("codex built, claude reviewed", &landed);
}

/// Batch 1 item 1: a line whose epic is unmarked, shaped like EPC-022's
/// (a two-family plan, a same-lineage review and no `built_by`), is judged
/// as before this check, and so is an epic marked `checkpoint-prs`.
#[test]
fn an_unmarked_line_is_judged_as_before() {
    for marker in [None, Some("checkpoint-prs")] {
        let landed = reviewed_by(marker, CLAUDE_REVIEW, "", "approved");
        passes_in_both_forms(&format!("{marker:?}"), &landed);
    }
}

/// Batch 1 item 1: the marker is read only at the default branch's commit:
/// the one `--policy-from` names, as the pre-push hook passes it, else the
/// `main` this clone resolves. A marker only the line's copy of the epic
/// record carries leaves the line unmarked.
#[test]
fn the_marker_is_read_only_at_the_default_branchs_commit() {
    let landed = reviewed_by(
        WHOLE_EPIC,
        CLAUDE_REVIEW,
        "  built_by: claude\n",
        "approved",
    );
    assert_blocks(
        &push_with(&landed, &["--policy-from", "main"]),
        "marked on main, named by --policy-from",
        &["lands on its marked epic's line with no pull request"],
    );

    let landed = landing(None, |_, head| {
        Some(completion(head, CLAUDE_REVIEW, "", "approved"))
    });
    let root = landed.dir.path();
    git(root, &["switch", LINE]);
    write(
        root,
        "project-management/epics/EPC-001.md",
        &EPIC.replacen(
            "status: planning\n",
            "status: planning\nmain_landing: whole-epic\n",
            1,
        ),
    );
    commit(root, "docs(records): mark the epic on its line");
    for (form, result) in [
        ("push", push_form(&landed)),
        (
            "push, --policy-from",
            push_with(&landed, &["--policy-from", "main"]),
        ),
        ("epic PR", epic_form(&landed)),
    ] {
        assert!(
            !result.1.contains("with no pull request") && !result.1.contains("main_landing"),
            "marked on the line only ({form}): {}",
            result.1
        );
    }
}

/// Batch 1 item 1: a marker the run cannot read refuses, naming the field
/// and the values it accepts, in both forms.
#[test]
fn a_malformed_marker_is_refused_in_both_forms() {
    for marker in ["whole_epic", "\"\"", "[whole-epic]", "null"] {
        let landed = reviewed_by(
            Some(marker),
            CODEX_REVIEW,
            "  built_by: claude\n",
            "approved",
        );
        refused_in_both_forms(
            marker,
            &landed,
            &[
                "work.acceptance_binding",
                "cannot read its epic's `main_landing` marker",
                "accepts `whole-epic` or `checkpoint-prs`",
                "set the epic record's `main_landing` on the default branch",
            ],
        );
    }
}

/// Batch 1 item 2: on a marked line, a block with no `built_by`, or one
/// that names other than exactly one lineage id, is refused in both forms.
/// An empty `built_by` is refused by the block's parser, as every empty
/// key is.
#[test]
fn a_missing_or_multi_valued_built_by_is_refused_in_both_forms() {
    let landed = reviewed_by(WHOLE_EPIC, CODEX_REVIEW, "", "approved");
    refused_in_both_forms(
        "no built_by",
        &landed,
        &[
            "lands on its marked epic's line with no pull request",
            "has no `built_by`, the one lineage that built it (claude, codex, grok)",
            "have a reviewer of another lineage",
        ],
    );
    for fields in [
        "  built_by: claude, codex\n",
        "  built_by: Claude Opus 5.5\n",
    ] {
        let landed = reviewed_by(WHOLE_EPIC, CODEX_REVIEW, fields, "approved");
        refused_in_both_forms(
            fields,
            &landed,
            &["is not one lineage id (claude, codex, grok)"],
        );
    }
    let landed = reviewed_by(WHOLE_EPIC, CODEX_REVIEW, "  built_by:\n", "approved");
    refused_in_both_forms("empty built_by", &landed, &["built_by is empty"]);
}

/// AC-3, AC-4: a task merged onto its line with no completion has no
/// review row; the product work it carries is refused in both forms.
#[test]
fn a_landing_with_no_review_row_is_refused_in_both_forms() {
    let landed = landing(WHOLE_EPIC, |_, _| None);
    refused_in_both_forms(
        "no review row",
        &landed,
        &[
            "work.acceptance_binding",
            "introduces src/work.rs through merge",
            "outside its verified source and reviewed task heads",
        ],
    );
}

/// AC-3, AC-4: a review of an earlier commit than the one the landing
/// carries is refused in both forms.
#[test]
fn a_review_of_another_commit_is_refused_in_both_forms() {
    let landed = landing(WHOLE_EPIC, |first, _| {
        Some(completion(
            first,
            CODEX_REVIEW,
            "  built_by: claude\n",
            "approved",
        ))
    });
    refused_in_both_forms(
        "another commit",
        &landed,
        &[
            "work.acceptance_binding",
            "changed between the reviewed commit",
            "src/more.rs",
        ],
    );
}

/// AC-3, AC-4, batch 1 item 3: a reviewer of the lineage `built_by` names,
/// with no reduced-assurance statement naming each other lineage, is
/// refused in both forms, with the remedy for this landing.
#[test]
fn a_reviewer_of_the_builders_lineage_is_refused_in_both_forms() {
    for (review, fields, missing) in [
        (CLAUDE_REVIEW, "  built_by: claude\n", "codex, grok"),
        (
            "Codex GPT-6 Astra at high; whole unit",
            "  built_by: codex\n",
            "claude, grok",
        ),
        (
            CLAUDE_REVIEW,
            "  built_by: claude\n  reduced_assurance: Codex was out at 16:24\n",
            "grok",
        ),
    ] {
        let landed = reviewed_by(WHOLE_EPIC, review, fields, "approved");
        let named = format!("does not name {missing} as probed and unavailable");
        refused_in_both_forms(
            fields,
            &landed,
            &[
                "work.acceptance_binding",
                "lands on its marked epic's line with no pull request",
                "is of the lineage `built_by` names",
                &named,
                "have a reviewer of another lineage",
            ],
        );
    }
}

/// Batch 1 item 3: a same-lineage review passes when the completion
/// records the reduced-assurance statement naming each other lineage.
#[test]
fn a_same_lineage_review_with_the_reduced_assurance_statement_passes() {
    let landed = reviewed_by(
        WHOLE_EPIC,
        CLAUDE_REVIEW,
        "  built_by: claude\n  reduced_assurance: Codex and Grok probed live at 16:24 on 2026-11-05, both unavailable\n",
        "approved",
    );
    passes_in_both_forms("reduced assurance", &landed);
}

/// AC-3, AC-4: a review that names no reviewer of a known lineage does not
/// show another lineage, in both forms.
#[test]
fn a_review_naming_no_known_reviewer_is_refused_in_both_forms() {
    let landed = reviewed_by(
        WHOLE_EPIC,
        "https://example.test/pr/1#review",
        "  built_by: claude\n",
        "approved",
    );
    refused_in_both_forms(
        "no known reviewer",
        &landed,
        &[
            "lands on its marked epic's line with no pull request",
            "names no reviewer of a known lineage (claude, codex, grok)",
        ],
    );
}

/// AC-3, AC-4: a verdict that does not approve is refused in both forms.
#[test]
fn a_verdict_that_does_not_approve_is_refused_in_both_forms() {
    let landed = reviewed_by(
        WHOLE_EPIC,
        CODEX_REVIEW,
        "  built_by: claude\n",
        "changes_requested",
    );
    refused_in_both_forms(
        "not approved",
        &landed,
        &[
            "work.records",
            "acceptance verdict is `changes_requested`, not approved",
        ],
    );
}

/// The task pull request route is judged as before: the task's own pull
/// request into its marked line, reviewed in the same lineage with no
/// `built_by`, still passes.
#[test]
fn a_task_pull_request_into_the_line_is_judged_as_before() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    write(root, &record_path("TSK-001"), &record("todo", "Pending.\n"));
    write(
        root,
        "project-management/epics/EPC-001.md",
        &EPIC.replacen(
            "status: planning\n",
            "status: planning\nmain_landing: whole-epic\n",
            1,
        ),
    );
    commit(root, "docs(records): target the line");
    git(root, &["branch", LINE, "main"]);
    git(root, &["switch", "-C", TASK_BRANCH, LINE]);
    write(root, "src/work.rs", "pub fn work() {}\n");
    let head = commit(root, "feat: build the work");
    write(
        root,
        &record_path("TSK-001"),
        &record(
            "complete",
            &completion(&head, CLAUDE_REVIEW, "", "approved"),
        ),
    );
    commit(root, "docs(records): complete TSK-001");
    let result = ci_on(root, LINE, TASK_BRANCH, "Task: TSK-001");
    assert_passes(&result, "the task pull request");
    assert!(!result.1.contains("with no pull request"), "{}", result.1);
}
