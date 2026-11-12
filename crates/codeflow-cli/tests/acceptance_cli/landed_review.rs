//! A task that lands on its epic line with no pull request is admitted only
//! when its record names an approving review, by another lineage than its
//! builder's, at the commit the landing carries (ADR-0102, TSK-1433). Each
//! landing is judged in both forms `codeflow ci` meets it: the line's push,
//! with no pull request body, and the epic's pull request into `main`. Both
//! give the same verdict. A task pull request into the line is judged as
//! before.
use super::*;

const LINE: &str = "integration/EPC-001-line";
const TASK_BRANCH: &str = "task/TSK-001-work";
const CLAUDE_BUILT: &str = "Claude Opus 5.5 at medium, a native subagent.";
const CODEX_REVIEW: &str = "Codex GPT-6.1 Sol at medium, whole unit, approved";

/// TSK-001 targeting [`LINE`] as `status`, built by `builder`, with
/// `closeout`.
fn record(status: &str, builder: &str, closeout: &str) -> String {
    let planned = task("TSK-001", status, OWN_JOURNEY, "").replace(
        "integration_target: main",
        &format!("integration_target: {LINE}"),
    );
    let planned = planned.trim_end_matches("## Closeout\n\n");
    format!("{planned}## Execution contract\n\n- Builder: {builder}\n\n## Closeout\n\n{closeout}")
}

/// An acceptance block reviewed at `reviewed` by `review` with `verdict`.
fn completion(reviewed: &str, review: &str, verdict: &str) -> String {
    format!(
        "```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: {review}\n  criteria:\n    AC-1: verified | cargo test | 3 passed\n    AC-2: verified | journey ran\n  journey: verified | tests/journey.rs\n  not_verified: none\n  follow_ups: none: nothing deferred\n  verdict: {verdict}\n```\n"
    )
}

/// One landing: the repository and the line's tip before it.
struct Landing {
    dir: tempfile::TempDir,
    before: String,
}

/// TSK-001 built on its branch in two commits, completed there with the
/// block `closeout` gives for (first commit, second commit), when it gives
/// one, then merged into [`LINE`] with no pull request.
fn landing(builder: &str, closeout: impl Fn(&str, &str) -> Option<String>) -> Landing {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    write(
        root,
        &record_path("TSK-001"),
        &record("todo", builder, "Pending.\n"),
    );
    commit(root, "docs(records): target the line");
    git(root, &["branch", LINE, "main"]);
    let before = git_out(root, &["rev-parse", LINE]);
    git(root, &["switch", "-C", TASK_BRANCH, LINE]);
    write(root, "src/work.rs", "pub fn work() {}\n");
    let first = commit(root, "feat: build the work");
    write(root, "src/more.rs", "pub fn more() {}\n");
    let head = commit(root, "feat: build more of the work");
    if let Some(done) = closeout(&first, &head) {
        write(
            root,
            &record_path("TSK-001"),
            &record("complete", builder, &done),
        );
        commit(root, "docs(records): complete TSK-001");
    }
    land(root, TASK_BRANCH);
    Landing { dir, before }
}

/// A landing completed with `review` and `verdict` at the head it carries.
fn reviewed_by(builder: &str, review: &'static str, verdict: &'static str) -> Landing {
    landing(builder, move |_, head| {
        Some(completion(head, review, verdict))
    })
}

/// The line's push: `codeflow ci` over the landing with no pull request
/// body, as the pre-push hook runs it.
fn push_form(landing: &Landing) -> (i32, String) {
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

/// Both forms refuse `landing`, each naming the cause by every needle.
fn refused_in_both_forms(what: &str, landing: &Landing, needles: &[&str]) {
    for (form, result) in [
        ("push", push_form(landing)),
        ("epic PR", epic_form(landing)),
    ] {
        assert_blocks(&result, &format!("{what} ({form})"), needles);
    }
}

/// AC-2, AC-4: a completion that names the commit the landing carries and
/// an approving reviewer of another lineage passes in both forms.
#[test]
fn a_landing_reviewed_by_another_lineage_passes_in_both_forms() {
    for (builder, review) in [
        (CLAUDE_BUILT, CODEX_REVIEW),
        ("Codex GPT-6 Astra at high.", "Grok 4.7 at high; whole unit"),
    ] {
        let landed = reviewed_by(builder, review, "approved");
        assert_passes(&push_form(&landed), &format!("{review} (push)"));
        assert_passes(&epic_form(&landed), &format!("{review} (epic PR)"));
    }
}

/// AC-3, AC-4: a task merged onto its line with no completion has no
/// review row; the product work it carries is refused in both forms.
#[test]
fn a_landing_with_no_review_row_is_refused_in_both_forms() {
    let landed = landing(CLAUDE_BUILT, |_, _| None);
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
    let landed = landing(CLAUDE_BUILT, |first, _| {
        Some(completion(first, CODEX_REVIEW, "approved"))
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

/// AC-3, AC-4: a reviewer of the lineage the record names for its builder
/// is refused in both forms, with the remedy for this landing.
#[test]
fn a_reviewer_of_the_builders_lineage_is_refused_in_both_forms() {
    for (builder, review) in [
        (CLAUDE_BUILT, "Claude Fable 5.1 as cf-reviewer, whole unit"),
        (
            "Codex GPT-6.1 Sol at medium, else a Claude Opus 5.5 subagent.",
            "Codex GPT-6 Astra at high; whole unit",
        ),
    ] {
        let landed = reviewed_by(builder, review, "approved");
        refused_in_both_forms(
            review,
            &landed,
            &[
                "work.acceptance_binding",
                "lands on its epic line with no pull request",
                "is of the lineage the record names for its builder",
                "have a reviewer of another lineage",
            ],
        );
    }
}

/// AC-3, AC-4: with the builder's lineage named, a review that names no
/// reviewer of a known lineage does not show another lineage, in both forms.
#[test]
fn a_review_naming_no_known_reviewer_is_refused_in_both_forms() {
    let landed = reviewed_by(CLAUDE_BUILT, "https://example.test/pr/1#review", "approved");
    refused_in_both_forms(
        "no known reviewer",
        &landed,
        &[
            "lands on its epic line with no pull request",
            "names no reviewer of a known lineage (claude, codex, grok)",
        ],
    );
}

/// AC-3, AC-4: a verdict that does not approve is refused in both forms.
#[test]
fn a_verdict_that_does_not_approve_is_refused_in_both_forms() {
    let landed = reviewed_by(CLAUDE_BUILT, CODEX_REVIEW, "changes_requested");
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
/// request into its line, reviewed in the same lineage, still passes.
#[test]
fn a_task_pull_request_into_the_line_is_judged_as_before() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    write(
        root,
        &record_path("TSK-001"),
        &record("todo", CLAUDE_BUILT, "Pending.\n"),
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
            CLAUDE_BUILT,
            &completion(
                &head,
                "Claude Fable 5.1 as cf-reviewer, whole unit",
                "approved",
            ),
        ),
    );
    commit(root, "docs(records): complete TSK-001");
    let result = ci_on(root, LINE, TASK_BRANCH, "Task: TSK-001");
    assert_passes(&result, "the task pull request");
    assert!(
        !result
            .1
            .contains("lands on its epic line with no pull request"),
        "{}",
        result.1
    );
}
