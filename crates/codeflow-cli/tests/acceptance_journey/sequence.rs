//! Journey (TSK-340, issues #198, #217 and #278): on a fresh
//! `codeflow init --full` project, a task becomes blocked, completes,
//! reopens and completes again, and its epic closes, with ordinary commands
//! that `task status`, the task pull request's `codeflow ci` and the
//! epic line's pull request into `main` all accept. Each symptom the issues
//! report is reproduced here, and tampered history or record text is still
//! refused.

use std::path::Path;
use std::process::Output;

use super::{body, codeflow, commit, edit, fails, git, ok, EPIC, LINE, SECOND, TASK};

/// Complete TSK-001 by hand with the block in `block_file`, commit, and
/// return its task PR's CI on `branch`.
fn hand_complete_ci(root: &Path, branch: &str, block_file: &str) -> Output {
    let record = std::fs::read_to_string(root.join(TASK)).unwrap();
    let inner = std::fs::read_to_string(block_file).unwrap();
    std::fs::write(
        root.join(TASK),
        format!(
            "{}\n```yaml\n{inner}```\n",
            record
                .replacen("status: todo", "status: complete", 1)
                .trim_end_matches('\n')
        ),
    )
    .unwrap();
    commit(root, "chore: complete TSK-001 by hand");
    task_ci(root, branch, "TSK-001")
}

/// TSK-001's acceptance block with both criteria verified at `reviewed`.
fn task_block(dir: &Path, name: &str, reviewed: &str, ac1: &str) -> String {
    let body = format!(
        "acceptance:\n  reviewed: {reviewed}\n  review: session:sequence@sha256:00\n  criteria:\n    AC-1: {ac1}\n    AC-2: verified | the journey ran\n  journey: verified | crates/codeflow-cli/tests/acceptance_journey/sequence.rs\n  not_verified: none\n  follow_ups: none: journey fixture\n  verdict: approved\n"
    );
    let path = dir.join(format!("{name}.yaml"));
    std::fs::write(&path, body).unwrap();
    path.to_string_lossy().to_string()
}

/// The epic's own acceptance block, reviewed at `reviewed`.
fn epic_block(dir: &Path, reviewed: &str) -> String {
    let body = format!(
        "acceptance:\n  reviewed: {reviewed}\n  review: session:sequence@sha256:01\n  criteria:\n    AC-1: verified | the line ran\n  journey: none | no journey criterion\n  not_verified: none\n  follow_ups: none: journey fixture\n  verdict: approved\n"
    );
    let path = dir.join("epic.yaml");
    std::fs::write(&path, body).unwrap();
    path.to_string_lossy().to_string()
}

fn status(root: &Path, kind: &str, id: &str, args: &[&str]) -> Output {
    let mut all = vec![kind, "status", id];
    all.extend_from_slice(args);
    codeflow(root, &all)
}

/// `codeflow ci` on a task pull request into the line.
fn task_ci(root: &Path, branch: &str, task: &str) -> Output {
    codeflow(
        root,
        &[
            "ci",
            "--base",
            LINE,
            "--head",
            "HEAD",
            "--branch",
            branch,
            "--pr-body",
            &body(root, task),
        ],
    )
}

/// `codeflow ci` on a planning pull request into the line.
fn planning_ci(root: &Path, branch: &str) -> Output {
    codeflow(
        root,
        &[
            "ci",
            "--base",
            LINE,
            "--head",
            "HEAD",
            "--branch",
            branch,
            "--pr-body",
            &body(root, "EPC-001"),
        ],
    )
}

/// `codeflow ci` on the epic line's pull request into `main`, with `HEAD`
/// as the line head.
fn line_ci(root: &Path, main: &str) -> Output {
    codeflow(
        root,
        &[
            "ci",
            "--base",
            main,
            "--head",
            "HEAD",
            "--branch",
            LINE,
            "--into",
            main,
            "--pr-body",
            &body(root, "EPC-001"),
        ],
    )
}

fn merge(root: &Path, into: &str, branch: &str, message: &str) {
    git(root, &["switch", "-q", into]);
    git(root, &["merge", "-q", "--no-ff", branch, "-m", message]);
}

/// The operator's merge of `branch` into the protected `main`, as the host
/// makes it: the local hooks, which refuse an agent's merge there, do not
/// run.
pub(super) fn host_merge(root: &Path, main: &str, branch: &str) {
    let hooks = root.join(".git/no-hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let path = format!("core.hooksPath={}", hooks.display());
    git(root, &["-c", &path, "switch", "-q", main]);
    git(
        root,
        &[
            "-c",
            &path,
            "merge",
            "-q",
            "--no-ff",
            branch,
            "-m",
            "Merge pull request",
        ],
    );
}

/// A fresh project with an epic line, its plan landed, and `src/**` as
/// product code. With `second`, the epic has two tasks. Returns the default branch's name.
fn planned_project(root: &Path, second: bool) -> String {
    ok(&codeflow(root, &["init", "--yes", "--full"]), "init --full");
    let main = git(root, &["branch", "--show-current"]);
    git(root, &["switch", "-q", "-c", LINE]);
    git(root, &["switch", "-q", "-c", "plan/sequence"]);
    ok(&codeflow(root, &["epic", "new", "sequence"]), "epic new");
    let titles: &[&str] = if second {
        &["first", "second"]
    } else {
        &["first"]
    };
    for title in titles {
        let args = ["task", "new", "--epic", "EPC-001", "--into", LINE, title];
        ok(&codeflow(root, &args), "task new");
    }
    edit(
        root,
        EPIC,
        "- AC-1\n",
        "- AC-1 When used, the line shall work.\n",
    );
    let criteria = "- AC-1 When run, the system shall work.\n- AC-2 (journey) On a fresh project, the command shall succeed.\n";
    edit(root, TASK, "- AC-1\n", criteria);
    if second {
        edit(root, SECOND, "- AC-1\n", criteria);
    }
    set_policy(root, |git| {
        git["product_paths"] = serde_json::json!(["src/**"]);
    });
    commit(root, "chore: plan the sequence journey");
    merge(root, LINE, "plan/sequence", "chore: land the plan");
    main
}

fn set_policy(root: &Path, change: impl FnOnce(&mut serde_json::Value)) {
    let path = root.join(".codeflow/policy.json");
    let mut policy: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    change(&mut policy["git"]);
    std::fs::write(path, serde_json::to_string_pretty(&policy).unwrap() + "\n").unwrap();
}

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// The documented sequence (AC-4): block, unblock and complete in the task
/// pull request (#198); land it; a checkpoint into `main`; reopen and
/// complete in a fix pull request; reopen and complete again on the line
/// with records only; close the epic (#217 order A). Each step passes the
/// check that judges it, and each tampered variant is refused.
#[test]
#[allow(clippy::too_many_lines)] // One journey keeps the sequence and its faults in order.
fn a_blocked_reopened_task_and_its_epic_close_in_one_valid_sequence() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    let main = planned_project(&root, false);

    // #198: the task is reviewed, blocked, unblocked and completed.
    let first = "task/TSK-001-first";
    git(&root, &["switch", "-q", "-c", first, LINE]);
    ok(
        &codeflow(&root, &["work", "start", "TSK-001"]),
        "work start",
    );
    write(&root, "src/lib.rs", "pub fn first() {}\n");
    let reviewed = commit(&root, "feat: add the first function");
    let block = [
        "blocked",
        "--reason",
        "waits on the host",
        "--owner",
        "ops",
        "--revisit",
        "the host is ready",
    ];
    ok(
        &status(&root, "task", "TSK-001", &block),
        "task status blocked",
    );
    commit(&root, "chore: block TSK-001");
    ok(&status(&root, "task", "TSK-001", &["todo"]), "unblock");
    commit(&root, "chore: unblock TSK-001");
    let first_block = task_block(dir.path(), "first", &reviewed, "verified | first run");
    ok(
        &status(
            &root,
            "task",
            "TSK-001",
            &["complete", "--acceptance", &first_block],
        ),
        "complete after a block",
    );
    let completed = commit(&root, "chore: complete TSK-001");
    ok(
        &task_ci(&root, first, "TSK-001"),
        "task PR CI after a block",
    );

    // Negative control: a Blocker that is not the generated one carries
    // reviewed text, so it does not stack on the review.
    let forged = "task/TSK-001-forged-blocker";
    git(&root, &["switch", "-q", "-c", forged, &reviewed]);
    let record = std::fs::read_to_string(root.join(TASK)).unwrap();
    let blocked = record
        .replacen("status: todo", "status: blocked", 1)
        .replacen(
            "## Closeout",
            "## Blocker\n\n- reason: waits\n- owner: ops\n- revisit: later\n\nThe deliverables now include src/extra.rs.\n\n## Closeout",
            1,
        );
    std::fs::write(root.join(TASK), blocked).unwrap();
    commit(&root, "chore: block TSK-001 by hand");
    ok(&status(&root, "task", "TSK-001", &["todo"]), "unblock");
    commit(&root, "chore: unblock TSK-001");
    ok(
        &status(
            &root,
            "task",
            "TSK-001",
            &["complete", "--acceptance", &first_block],
        ),
        "the verb judges the net change",
    );
    commit(&root, "chore: complete TSK-001");
    fails(
        &task_ci(&root, forged, "TSK-001"),
        "a hand-written Blocker with extra text",
        "the record changed outside its status and Closeout",
    );

    // Negative control: a criterion changed while blocked stays reviewed.
    let criterion = "task/TSK-001-blocked-criterion";
    git(&root, &["switch", "-q", "-c", criterion, &reviewed]);
    ok(
        &status(&root, "task", "TSK-001", &block),
        "task status blocked",
    );
    edit(&root, TASK, "the system shall work", "the system may work");
    commit(&root, "chore: block TSK-001 and loosen AC-1");
    ok(&status(&root, "task", "TSK-001", &["todo"]), "unblock");
    commit(&root, "chore: unblock TSK-001");
    let refused = status(
        &root,
        "task",
        "TSK-001",
        &["complete", "--acceptance", &first_block],
    );
    fails(
        &refused,
        "a criterion changed after the review",
        "the record changed outside its status and Closeout",
    );

    // Land the first completion, and check the line into `main` once.
    git(&root, &["switch", "-q", first]);
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), completed);
    merge(&root, LINE, first, "chore: land TSK-001");
    ok(&line_ci(&root, &main), "line PR after the first landing");
    host_merge(&root, &main, LINE);
    git(&root, &["switch", "-q", LINE]);

    // A fix pull request reopens and completes the task.
    let fix = "task/TSK-001-fix";
    git(&root, &["switch", "-q", "-c", fix, LINE]);
    ok(
        &status(
            &root,
            "task",
            "TSK-001",
            &["todo", "--reason", "regression"],
        ),
        "reopen in the fix PR",
    );
    commit(&root, "chore: reopen TSK-001");
    ok(
        &codeflow(&root, &["work", "start", "TSK-001"]),
        "work start",
    );
    write(&root, "src/lib.rs", "pub fn fixed() {}\n");
    let fixed = commit(&root, "fix: repair the first function");
    let fix_block = task_block(dir.path(), "fix", &fixed, "verified | fixed run");
    ok(
        &status(
            &root,
            "task",
            "TSK-001",
            &["complete", "--acceptance", &fix_block],
        ),
        "complete the fix",
    );
    commit(&root, "chore: complete TSK-001 again");
    ok(&task_ci(&root, fix, "TSK-001"), "fix PR CI");
    merge(&root, LINE, fix, "chore: land the TSK-001 fix");

    // #217: a second fix pull request reopens the task again. `main` holds
    // the first block active and the line the second, so the archive must
    // read the same against both. The line moves on after the review (a
    // planning amendment of the epic), and the fix branch merges it before
    // it lands, as an ordinary task pull request may.
    let again = "task/TSK-001-again";
    git(&root, &["switch", "-q", "-c", again, LINE]);
    ok(
        &status(
            &root,
            "task",
            "TSK-001",
            &["todo", "--reason", "second regression"],
        ),
        "reopen in the second fix PR",
    );
    commit(&root, "chore: reopen TSK-001 again");
    ok(
        &codeflow(&root, &["work", "start", "TSK-001"]),
        "work start",
    );
    write(&root, "src/lib.rs", "pub fn fixed_again() {}\n");
    let refixed = commit(&root, "fix: repair the first function again");
    let again_block = task_block(dir.path(), "again", &refixed, "verified | re-reviewed");
    ok(
        &status(
            &root,
            "task",
            "TSK-001",
            &["complete", "--acceptance", &again_block],
        ),
        "complete the second fix",
    );
    commit(&root, "chore: complete TSK-001 a third time");
    ok(&task_ci(&root, again, "TSK-001"), "second fix PR CI");
    let archive = std::fs::read_to_string(root.join(TASK)).unwrap();
    assert_eq!(
        archive.matches("acceptance_superseded:").count(),
        2,
        "{archive}"
    );
    let order = |marker: &str| archive.find(marker).unwrap();
    assert!(
        order(&format!("reviewed: {reviewed}")) < order(&format!("reviewed: {fixed}"))
            && order(&format!("reviewed: {fixed}")) < order(&format!("reviewed: {refixed}")),
        "the archive is chronological:\n{archive}"
    );

    // Negative controls (TSK-340 review round 1): an archived block that no
    // completion made, and an archived result rewritten by hand, are
    // refused by the verb and by CI.
    let todo_record = git(&root, &["show", &format!("{refixed}:{TASK}")]) + "\n";
    let invented = format!(
        "\n```yaml\nacceptance_superseded:\n  reason: invented\n  reviewed: {}\n  review: session:sequence@sha256:02\n  criteria:\n    AC-1: verified | invented\n    AC-2: verified | invented\n  journey: verified | invented\n  not_verified: none\n  follow_ups: none: invented\n  verdict: approved\n```\n",
        "f".repeat(40)
    );
    let again_inner = std::fs::read_to_string(&again_block).unwrap();
    for (fault, record, reason) in [
        (
            "invented",
            format!("{}{invented}", todo_record.trim_end_matches('\n')),
            "is not a completion this range made",
        ),
        (
            "rewritten",
            todo_record.replacen(
                "AC-1: verified | first run",
                "AC-1: waived | not-a-commit",
                1,
            ),
            "acceptance_superseded",
        ),
    ] {
        let branch = format!("task/TSK-001-{fault}-archive");
        git(&root, &["switch", "-q", "-c", &branch, &refixed]);
        std::fs::write(root.join(TASK), &record).unwrap();
        commit(&root, "docs(records): edit the archive by hand");
        fails(
            &status(
                &root,
                "task",
                "TSK-001",
                &["complete", "--acceptance", &again_block],
            ),
            &format!("the verb on an {fault} archive"),
            reason,
        );
        std::fs::write(
            root.join(TASK),
            format!(
                "{}\n```yaml\n{again_inner}```\n",
                record
                    .replacen("status: todo", "status: complete", 1)
                    .trim_end_matches('\n')
            ),
        )
        .unwrap();
        commit(&root, "chore: complete TSK-001 by hand");
        fails(
            &task_ci(&root, &branch, "TSK-001"),
            &format!("CI on an {fault} archive"),
            reason,
        );
    }
    git(&root, &["switch", "-q", again]);

    // Negative controls (TSK-340 review round 2): the archive is every
    // completion the range made, in order, each with real provenance.
    // A temporary completion with a forged review, archived by a reopen,
    // is refused; so is an archive that drops a completion the range made.
    git(
        &root,
        &["switch", "-q", "-c", "task/TSK-001-laundered", &refixed],
    );
    let forged = format!(
        "acceptance:\n  reviewed: {}\n  review: session:sequence@sha256:03\n  criteria:\n    AC-1: waived | not-a-commit\n    AC-2: verified | invented\n  journey: verified | invented\n  not_verified: none\n  follow_ups: none: invented\n  verdict: approved\n",
        "f".repeat(40)
    );
    std::fs::write(
        root.join(TASK),
        format!(
            "{}\n```yaml\n{forged}```\n",
            todo_record
                .replacen("status: todo", "status: complete", 1)
                .trim_end_matches('\n')
        ),
    )
    .unwrap();
    commit(&root, "chore: complete TSK-001 with a forged review");
    ok(
        &status(&root, "task", "TSK-001", &["todo", "--reason", "launder"]),
        "reopen the forged completion",
    );
    let laundered = commit(&root, "chore: reopen TSK-001");
    let after_launder = task_block(dir.path(), "launder", &laundered, "verified | re-reviewed");
    fails(
        &status(
            &root,
            "task",
            "TSK-001",
            &["complete", "--acceptance", &after_launder],
        ),
        "a laundered forged completion",
        "has no real provenance",
    );
    fails(
        &hand_complete_ci(&root, "task/TSK-001-laundered", &after_launder),
        "CI on a laundered forged completion",
        "has no real provenance",
    );

    git(
        &root,
        &["switch", "-q", "-c", "task/TSK-001-dropped", &refixed],
    );
    ok(
        &status(
            &root,
            "task",
            "TSK-001",
            &["complete", "--acceptance", &again_block],
        ),
        "an intermediate completion",
    );
    commit(&root, "chore: complete TSK-001");
    ok(
        &status(
            &root,
            "task",
            "TSK-001",
            &["todo", "--reason", "one more fix"],
        ),
        "reopen the intermediate completion",
    );
    commit(&root, "chore: reopen TSK-001");
    let reopened_record = std::fs::read_to_string(root.join(TASK)).unwrap();
    let last = reopened_record
        .rfind("```yaml\nacceptance_superseded:")
        .unwrap();
    let end = last + 3 + reopened_record[last + 3..].find("```\n").unwrap() + 4;
    std::fs::write(
        root.join(TASK),
        format!("{}{}", &reopened_record[..last], &reopened_record[end..]),
    )
    .unwrap();
    let dropped = commit(&root, "docs(records): drop the intermediate archive");
    let after_drop = task_block(dir.path(), "drop", &dropped, "verified | re-reviewed");
    fails(
        &status(
            &root,
            "task",
            "TSK-001",
            &["complete", "--acceptance", &after_drop],
        ),
        "a dropped completion",
        "is missing",
    );
    fails(
        &hand_complete_ci(&root, "task/TSK-001-dropped", &after_drop),
        "CI on a dropped completion",
        "is missing",
    );
    git(&root, &["switch", "-q", again]);

    // TSK-340 review round 3: a completion is an occurrence, not a block's
    // content. Completing, reopening and completing again with the same
    // block is two completions: the archive the reopen wrote is accepted,
    // and deleting it is refused, by the verb and by CI.
    git(
        &root,
        &["switch", "-q", "-c", "task/TSK-001-repeat", &refixed],
    );
    ok(
        &status(
            &root,
            "task",
            "TSK-001",
            &["complete", "--acceptance", &again_block],
        ),
        "a first completion with a block",
    );
    commit(&root, "chore: complete TSK-001");
    ok(
        &status(
            &root,
            "task",
            "TSK-001",
            &["todo", "--reason", "same review again"],
        ),
        "reopen it",
    );
    let repeat_reopen = commit(&root, "chore: reopen TSK-001");
    ok(
        &status(
            &root,
            "task",
            "TSK-001",
            &["complete", "--acceptance", &again_block],
        ),
        "the same block again, archive kept",
    );
    commit(&root, "chore: complete TSK-001 with the same block");
    ok(
        &task_ci(&root, "task/TSK-001-repeat", "TSK-001"),
        "CI on a repeated block with its archive",
    );
    git(
        &root,
        &[
            "switch",
            "-q",
            "-c",
            "task/TSK-001-repeat-dropped",
            &repeat_reopen,
        ],
    );
    let repeated = std::fs::read_to_string(root.join(TASK)).unwrap();
    let last = repeated.rfind("```yaml\nacceptance_superseded:").unwrap();
    let end = last + 3 + repeated[last + 3..].find("```\n").unwrap() + 4;
    std::fs::write(
        root.join(TASK),
        format!("{}{}", &repeated[..last], &repeated[end..]),
    )
    .unwrap();
    commit(&root, "docs(records): drop the repeated block's archive");
    fails(
        &status(
            &root,
            "task",
            "TSK-001",
            &["complete", "--acceptance", &again_block],
        ),
        "the same block again, archive deleted",
        "is missing",
    );
    fails(
        &hand_complete_ci(&root, "task/TSK-001-repeat-dropped", &again_block),
        "CI on a repeated block with its archive deleted",
        "is missing",
    );
    git(&root, &["switch", "-q", again]);

    let note = "plan/epic-note";
    git(&root, &["switch", "-q", "-c", note, LINE]);
    edit(
        &root,
        EPIC,
        "## Planning notes\n",
        "## Planning notes\n\n- The line also carries the sequence journey.\n",
    );
    commit(&root, "docs(records): note the sequence journey");
    ok(&planning_ci(&root, note), "the planning amendment's CI");
    merge(&root, LINE, note, "chore: land the epic note");
    git(&root, &["switch", "-q", again]);
    git(
        &root,
        &[
            "merge",
            "-q",
            "--no-ff",
            LINE,
            "-m",
            "chore: merge the line",
        ],
    );
    let merged = git(&root, &["rev-parse", "HEAD"]);
    ok(
        &task_ci(&root, again, "TSK-001"),
        "a reopening PR that merged the line after its review",
    );

    // Negative control: the task's own product edit after the review.
    git(&root, &["switch", "-q", "-c", "task/TSK-001-late", &merged]);
    write(&root, "src/lib.rs", "pub fn unreviewed() {}\n");
    commit(&root, "feat: change after the review");
    fails(
        &task_ci(&root, "task/TSK-001-late", "TSK-001"),
        "a product edit after the review",
        "src/lib.rs changed",
    );
    merge(&root, LINE, again, "chore: land the second TSK-001 fix");

    // The epic closes in a planning pull request, and the line's pull
    // request into `main` accepts the whole sequence.
    let close = "plan/close-EPC-001";
    git(&root, &["switch", "-q", "-c", close, LINE]);
    let line_head = git(&root, &["rev-parse", "HEAD"]);
    let closeout = epic_block(dir.path(), &line_head);
    ok(
        &status(
            &root,
            "epic",
            "EPC-001",
            &["complete", "--acceptance", &closeout],
        ),
        "epic closeout after the task",
    );
    commit(&root, "chore: close EPC-001");
    ok(&planning_ci(&root, close), "the closeout's CI");
    merge(&root, LINE, close, "chore: land the closeout");
    let closed = git(&root, &["rev-parse", "HEAD"]);
    ok(&line_ci(&root, &main), "line PR after the closeout");

    // Negative controls: a planning pull request that edits the archive by
    // hand is refused.
    let archive = std::fs::read_to_string(root.join(TASK)).unwrap();
    let mut spans = Vec::new();
    let mut from = 0;
    while let Some(open) = archive[from..].find("```yaml\nacceptance_superseded:") {
        let start = from + open;
        let end = start + 3 + archive[start + 3..].find("```\n").unwrap() + 4;
        spans.push((start, end));
        from = end;
    }
    assert_eq!(spans.len(), 2, "{archive}");
    let first_archive = &archive[spans[0].0..spans[0].1];
    let second_archive = &archive[spans[1].0..spans[1].1];
    let reordered = format!(
        "{}{second_archive}{}{first_archive}{}",
        &archive[..spans[0].0],
        &archive[spans[0].1..spans[1].0],
        &archive[spans[1].1..]
    );
    let dropped = format!("{}{}", &archive[..spans[0].0], &archive[spans[0].1..]);
    let renamed = archive.replacen("reason: regression", "reason: a different reason", 1);
    for (fault, content, reason) in [
        ("reordered", reordered.as_str(), "chronological"),
        ("dropped", dropped.as_str(), "acceptance_superseded"),
        ("renamed", renamed.as_str(), "acceptance_superseded"),
    ] {
        let branch = format!("plan/tamper-{fault}");
        git(&root, &["switch", "-q", "-c", &branch, &closed]);
        std::fs::write(root.join(TASK), content).unwrap();
        commit(&root, "docs(records): edit the archive by hand");
        fails(&planning_ci(&root, &branch), fault, reason);
    }
}

/// The #217 orderings, each made with ordinary commands. Records-only
/// commits made directly on the line (order A, and route 3 of the issue's
/// comment) are refused as unclassified line work; the same steps through
/// pull requests, two tasks re-completed in turn and then the closeout,
/// pass. Order B, a task and its epic completed in one commit, stays
/// refused: each completion's span changes only its own record, so the
/// closeout rides its own planning pull request. Order C, the reopen in a
/// pull request of its own before the fix, passes.
#[test]
#[allow(clippy::too_many_lines)] // The orderings share one planned line.
fn the_reported_orderings_are_judged_by_one_completion_span() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    let main = planned_project(&root, true);
    for (task, file) in [("TSK-001", "src/first.rs"), ("TSK-002", "src/second.rs")] {
        let branch = format!("task/{task}-work");
        git(&root, &["switch", "-q", "-c", &branch, LINE]);
        ok(&codeflow(&root, &["work", "start", task]), "work start");
        write(&root, file, "pub fn added() {}\n");
        let reviewed = commit(&root, "feat: add a function");
        let file_block = task_block(dir.path(), task, &reviewed, "verified | first run");
        ok(
            &status(
                &root,
                "task",
                task,
                &["complete", "--acceptance", &file_block],
            ),
            "complete",
        );
        commit(&root, "chore: complete the task");
        ok(&task_ci(&root, &branch, task), "task PR CI");
        merge(&root, LINE, &branch, "chore: land the task");
    }
    let landed = git(&root, &["rev-parse", "HEAD"]);
    let reopen_and_complete = |task: &str| -> String {
        ok(
            &status(&root, "task", task, &["todo", "--reason", "re-review"]),
            "reopen",
        );
        let reopen = commit(&root, "chore: reopen for a re-review");
        let again = task_block(dir.path(), task, &reopen, "verified | re-reviewed");
        ok(
            &status(&root, "task", task, &["complete", "--acceptance", &again]),
            "complete again",
        );
        commit(&root, "chore: complete after the re-review")
    };
    let close = |name: &str| {
        let branch = format!("plan/close-{name}");
        git(&root, &["switch", "-q", "-c", &branch, LINE]);
        let head = git(&root, &["rev-parse", "HEAD"]);
        let closeout = epic_block(dir.path(), &head);
        ok(
            &status(
                &root,
                "epic",
                "EPC-001",
                &["complete", "--acceptance", &closeout],
            ),
            "closeout",
        );
        commit(&root, "chore: close EPC-001");
        ok(&planning_ci(&root, &branch), "the closeout's CI");
        merge(&root, LINE, &branch, "chore: land the closeout");
    };

    // Order A and route 3, made directly on the line: refused as line work
    // outside a pull request.
    git(&root, &["switch", "-q", "-c", "direct/order-a", &landed]);
    reopen_and_complete("TSK-001");
    let last = reopen_and_complete("TSK-002");
    let closeout = epic_block(dir.path(), &last);
    ok(
        &status(
            &root,
            "epic",
            "EPC-001",
            &["complete", "--acceptance", &closeout],
        ),
        "closeout",
    );
    commit(&root, "chore: close EPC-001");
    fails(
        &line_ci(&root, &main),
        "records-only commits made directly on the line",
        "has commits made directly on the line",
    );

    // Order B: the task and the epic completed for one commit. The epic's
    // verb refuses the uncommitted task completion beside it, so each
    // completion is its own commit and the closeout its own pull request.
    git(
        &root,
        &["switch", "-q", "-c", "task/TSK-001-order-b", &landed],
    );
    ok(
        &status(&root, "task", "TSK-001", &["todo", "--reason", "re-review"]),
        "reopen",
    );
    let reopen = commit(&root, "chore: reopen for a re-review");
    let again = task_block(dir.path(), "TSK-001", &reopen, "verified | re-reviewed");
    ok(
        &status(
            &root,
            "task",
            "TSK-001",
            &["complete", "--acceptance", &again],
        ),
        "complete again",
    );
    let closeout = epic_block(dir.path(), &reopen);
    fails(
        &status(&root, "epic", "EPC-001", &["complete", "--acceptance", &closeout]),
        "order B",
        "uncommitted changes outside the record were never reviewed (project-management/tasks/TSK-001.md)",
    );
    git(&root, &["reset", "-q", "--hard"]);

    // Route 3 through pull requests: two tasks re-completed in turn, then
    // the closeout.
    git(&root, &["switch", "-q", LINE]);
    git(&root, &["switch", "-q", "-c", "line/route-3"]);
    let route_3 = git(&root, &["rev-parse", "HEAD"]);
    for task in ["TSK-001", "TSK-002"] {
        let branch = format!("task/{task}-route-3");
        git(&root, &["switch", "-q", "-c", &branch, LINE]);
        reopen_and_complete(task);
        ok(
            &task_ci(&root, &branch, task),
            "a records-only re-completion",
        );
        merge(&root, LINE, &branch, "chore: land the re-completion");
    }
    close("route-3");
    ok(
        &line_ci(&root, &main),
        "two tasks re-completed, then closed",
    );

    // Order C: the reopen lands in a pull request of its own, then the fix
    // completes, then the closeout. It starts again from the landed tasks.
    git(&root, &["switch", "-q", LINE]);
    git(&root, &["reset", "-q", "--hard", &landed]);
    let _ = route_3;
    for task in ["TSK-001", "TSK-002"] {
        let reopen = format!("task/{task}-reopen");
        git(&root, &["switch", "-q", "-c", &reopen, LINE]);
        ok(
            &status(&root, "task", task, &["todo", "--reason", "re-review"]),
            "reopen",
        );
        commit(&root, "chore: reopen for a re-review");
        ok(&task_ci(&root, &reopen, task), "a reopen pull request");
        merge(&root, LINE, &reopen, "chore: land the reopen");
        let fix = format!("task/{task}-order-c");
        git(&root, &["switch", "-q", "-c", &fix, LINE]);
        ok(&codeflow(&root, &["work", "start", task]), "work start");
        let reviewed = git(&root, &["rev-parse", "HEAD"]);
        let again = task_block(dir.path(), task, &reviewed, "verified | re-reviewed");
        ok(
            &status(&root, "task", task, &["complete", "--acceptance", &again]),
            "complete after the landed reopen",
        );
        commit(&root, "chore: complete after the landed reopen");
        ok(&task_ci(&root, &fix, task), "the fix after a landed reopen");
        merge(&root, LINE, &fix, "chore: land the fix");
    }
    close("order-c");
    ok(&line_ci(&root, &main), "order C");
}

/// TSK-001's acceptance block with AC-1 waived by `waiver`.
fn waived_block(dir: &Path, name: &str, reviewed: &str, waiver: &str) -> String {
    task_block(dir, name, reviewed, &format!("waived | {waiver}"))
}

/// Issue #278 (TSK-340 AC-5 and AC-6): a waiver that names a records-only
/// commit changing only its criterion's line, strictly before `reviewed:`,
/// follows the completion span. A task whose completion never landed is
/// still in its own pull request, whatever its branch is called, so the
/// verb and CI accept the waiver before and after a reopen in that range,
/// with or without a merge of the line. A reopen of a landed completion
/// keeps its criteria frozen, and the refusal names the planning commit on
/// the target that the waiver needs.
#[test]
#[allow(clippy::too_many_lines)] // The three #278 branches share one planned line.
fn a_waiver_in_the_tasks_own_range_follows_its_completion_span() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    planned_project(&root, true);

    // AC-6: a never-reopened task on a branch whose name carries no id.
    let unnamed = "fix/narrow-the-first-task";
    git(&root, &["switch", "-q", "-c", unnamed, LINE]);
    edit(
        &root,
        TASK,
        "the system shall work.",
        "the system shall work on Linux.",
    );
    let waiver = commit(&root, "docs(records): narrow TSK-001 AC-1");
    write(&root, "src/first.rs", "pub fn first() {}\n");
    let reviewed = commit(&root, "feat: add the first function");
    let block = waived_block(dir.path(), "unnamed", &reviewed, &waiver);
    ok(
        &status(
            &root,
            "task",
            "TSK-001",
            &["complete", "--acceptance", &block],
        ),
        "an in-branch waiver on a branch without the task id",
    );
    commit(&root, "chore: complete TSK-001");
    ok(&task_ci(&root, unnamed, "TSK-001"), "its pull request");

    // AC-5, the pre-reopen control: TSK-002 completes with an in-branch
    // waiver on its own branch.
    let own = "task/TSK-002-waiver";
    git(&root, &["switch", "-q", "-c", own, LINE]);
    ok(
        &codeflow(&root, &["work", "start", "TSK-002"]),
        "work start",
    );
    edit(
        &root,
        SECOND,
        "the system shall work.",
        "the system shall work on Linux.",
    );
    let first_waiver = commit(&root, "docs(records): narrow TSK-002 AC-1");
    write(&root, "src/second.rs", "pub fn second() {}\n");
    let first_review = commit(&root, "feat: add the second function");
    let block = waived_block(dir.path(), "control", &first_review, &first_waiver);
    ok(
        &status(
            &root,
            "task",
            "TSK-002",
            &["complete", "--acceptance", &block],
        ),
        "the pre-reopen control",
    );
    let control = commit(&root, "chore: complete TSK-002");
    ok(
        &task_ci(&root, own, "TSK-002"),
        "the pre-reopen control's CI",
    );

    // AC-5: reopened in the same unlanded range, a new records-only waiver
    // commit before the new review; then the same with a merge of the line.
    ok(
        &status(
            &root,
            "task",
            "TSK-002",
            &["todo", "--reason", "review found a gap"],
        ),
        "reopen in the range",
    );
    let reopened = commit(&root, "chore: reopen TSK-002");
    for merge_line in [false, true] {
        git(&root, &["switch", "-q", own]);
        git(&root, &["reset", "-q", "--hard", &reopened]);
        if merge_line {
            let note = "plan/epic-note";
            git(&root, &["switch", "-q", "-c", note, LINE]);
            edit(
                &root,
                EPIC,
                "## Planning notes\n",
                "## Planning notes\n\n- The waiver journey runs here.\n",
            );
            commit(&root, "docs(records): note the waiver journey");
            merge(&root, LINE, note, "chore: land the epic note");
            git(&root, &["switch", "-q", own]);
            git(
                &root,
                &[
                    "merge",
                    "-q",
                    "--no-ff",
                    LINE,
                    "-m",
                    "chore: merge the line",
                ],
            );
        }
        edit(&root, SECOND, "on Linux.", "on Linux and macOS.");
        let second_waiver = commit(&root, "docs(records): widen TSK-002 AC-1");
        write(&root, "src/second.rs", "pub fn second_fixed() {}\n");
        let second_review = commit(&root, "fix: repair the second function");
        for (name, named) in [("new", &second_waiver), ("original", &first_waiver)] {
            let block = waived_block(dir.path(), name, &second_review, named);
            // The original waiver from the first completion and a new one
            // after the reopen are both records-only amendments of AC-1 in
            // the task's own range, as the never-reopened path accepts.
            ok(
                &status(
                    &root,
                    "task",
                    "TSK-002",
                    &["complete", "--acceptance", &block],
                ),
                &format!("the {name} waiver inside a reopened, unlanded range"),
            );
            commit(&root, "chore: complete TSK-002 again");
            ok(&task_ci(&root, own, "TSK-002"), "its pull request");
            git(&root, &["reset", "-q", "--hard", "HEAD~1"]);
        }
    }

    // AC-5: a reopen of a landed completion keeps its criteria, so an
    // in-branch waiver is refused, naming the planning commit on the target.
    // The control lands; the unlanded reopen above was a draft of it.
    git(&root, &["switch", "-q", own]);
    git(&root, &["reset", "-q", "--hard", &control]);
    git(&root, &["switch", "-q", LINE]);
    git(
        &root,
        &[
            "merge",
            "-q",
            "--no-ff",
            &control,
            "-m",
            "chore: land TSK-002",
        ],
    );
    let landed = "task/TSK-002-landed-fix";
    git(&root, &["switch", "-q", "-c", landed, LINE]);
    ok(
        &status(
            &root,
            "task",
            "TSK-002",
            &["todo", "--reason", "regression"],
        ),
        "reopen a landed completion",
    );
    commit(&root, "chore: reopen TSK-002");
    edit(&root, SECOND, "on Linux.", "on Linux and macOS.");
    let late_waiver = commit(&root, "docs(records): widen TSK-002 AC-1");
    write(&root, "src/second.rs", "pub fn second_fixed() {}\n");
    let late_review = commit(&root, "fix: repair the second function");
    let block = waived_block(dir.path(), "landed", &late_review, &late_waiver);
    let named = "a reopened task's waiver must name a planning commit on the target that amends this criterion";
    fails(
        &status(
            &root,
            "task",
            "TSK-002",
            &["complete", "--acceptance", &block],
        ),
        "an in-branch waiver after a landed reopen",
        named,
    );
    let record = std::fs::read_to_string(root.join(SECOND)).unwrap();
    let inner = std::fs::read_to_string(&block).unwrap();
    std::fs::write(
        root.join(SECOND),
        format!(
            "{}\n```yaml\n{inner}```\n",
            record.replacen("status: todo", "status: complete", 1)
        ),
    )
    .unwrap();
    commit(&root, "chore: complete TSK-002 by hand");
    fails(
        &task_ci(&root, landed, "TSK-002"),
        "its pull request",
        named,
    );
}

/// Issue #217, comment of 2026-10-26: the two-base shape. Two tasks are
/// complete on `main` through a checkpoint; the line reopens and completes
/// each again through its own task pull request, with reviews that exist
/// only on the line; the second fix merges the line after its review, so
/// the other task's records-only completion arrives after that review;
/// then the line merges `main`. `task status`, each task pull request's
/// `codeflow ci` and the line's pull request into `main` accept both, and
/// a tampered archive or a product edit after the review stays refused.
#[test]
#[allow(clippy::too_many_lines)] // One fixture keeps the two-base sequence and its controls in order.
fn two_tasks_complete_on_main_are_completed_again_on_the_line() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    let main = planned_project(&root, true);
    for (task, file) in [("TSK-001", "src/first.rs"), ("TSK-002", "src/second.rs")] {
        let branch = format!("task/{task}-work");
        git(&root, &["switch", "-q", "-c", &branch, LINE]);
        ok(&codeflow(&root, &["work", "start", task]), "work start");
        write(&root, file, "pub fn added() {}\n");
        let reviewed = commit(&root, "feat: add a function");
        let first = task_block(dir.path(), task, &reviewed, "verified | first run");
        ok(
            &status(&root, "task", task, &["complete", "--acceptance", &first]),
            "complete",
        );
        commit(&root, "chore: complete the task");
        ok(&task_ci(&root, &branch, task), "task PR CI");
        merge(&root, LINE, &branch, "chore: land the task");
    }
    host_merge(&root, &main, LINE);

    // Each task reopens and completes again on the line; the second fix
    // merges the line, which now holds the first fix, after its review.
    let fixes = [
        ("TSK-001", "src/first.rs", "task/TSK-001-again"),
        ("TSK-002", "src/second.rs", "task/TSK-002-again"),
    ];
    for (task, file, branch) in fixes {
        git(&root, &["switch", "-q", "-c", branch, LINE]);
        ok(
            &status(
                &root,
                "task",
                task,
                &["todo", "--reason", "line regression"],
            ),
            "reopen on the line",
        );
        commit(&root, "chore: reopen on the line");
        ok(&codeflow(&root, &["work", "start", task]), "work start");
        write(&root, file, "pub fn fixed() {}\n");
        let reviewed = commit(&root, "fix: repair on the line");
        let again = task_block(
            dir.path(),
            &format!("{task}-again"),
            &reviewed,
            "verified | line run",
        );
        ok(
            &status(&root, "task", task, &["complete", "--acceptance", &again]),
            "complete on the line",
        );
        commit(&root, "chore: complete on the line");
        ok(
            &task_ci(&root, branch, task),
            "each fix PR before either lands",
        );
        git(&root, &["switch", "-q", LINE]);
    }
    merge(
        &root,
        LINE,
        "task/TSK-001-again",
        "chore: land the first fix",
    );
    git(&root, &["switch", "-q", "task/TSK-002-again"]);
    git(
        &root,
        &[
            "merge",
            "-q",
            "--no-ff",
            LINE,
            "-m",
            "chore: merge the line",
        ],
    );
    let merged = git(&root, &["rev-parse", "HEAD"]);
    assert_eq!(
        git(
            &root,
            &[
                "rev-list",
                "--count",
                "--parents",
                "-1",
                "--merges",
                &merged
            ]
        ),
        "1",
        "the line merge is a real merge"
    );
    ok(
        &task_ci(&root, "task/TSK-002-again", "TSK-002"),
        "the second fix PR after the other task's completion arrived",
    );

    // Negative control: a product edit after the review.
    git(&root, &["switch", "-q", "-c", "task/TSK-002-late", &merged]);
    write(&root, "src/second.rs", "pub fn unreviewed() {}\n");
    commit(&root, "feat: change after the review");
    fails(
        &task_ci(&root, "task/TSK-002-late", "TSK-002"),
        "a product edit after the review",
        "src/second.rs changed",
    );
    merge(
        &root,
        LINE,
        "task/TSK-002-again",
        "chore: land the second fix",
    );

    // `main` moves on, and the line merges it.
    git(&root, &["switch", "-q", "-c", "docs/main-note", &main]);
    write(&root, "docs/note.md", "# Note\n\nMain moved on.\n");
    commit(&root, "docs: add a note on main");
    host_merge(&root, &main, "docs/main-note");
    git(&root, &["switch", "-q", LINE]);
    git(
        &root,
        &[
            "merge",
            "-q",
            "--no-ff",
            &main,
            "-m",
            "chore: merge main into the line",
        ],
    );
    let synced = git(&root, &["rev-parse", "HEAD"]);
    ok(&line_ci(&root, &main), "the line into main after the sync");

    // Negative control: a planning pull request that rewrites an archived
    // result.
    git(
        &root,
        &["switch", "-q", "-c", "plan/tamper-two-base", &synced],
    );
    edit(
        &root,
        TASK,
        "AC-1: verified | first run",
        "AC-1: waived | not-a-commit",
    );
    commit(&root, "docs(records): edit the archive by hand");
    fails(
        &planning_ci(&root, "plan/tamper-two-base"),
        "a rewritten archive on the line",
        "acceptance_superseded",
    );
}
