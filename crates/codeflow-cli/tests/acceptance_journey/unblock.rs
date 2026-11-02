//! Journey (TSK-1036, issues #365, #400 and #424): on a fresh
//! `codeflow init --full` project, a blocked task is unblocked and completed
//! in its own pull request, for an epic task and for a standalone one; a
//! range is judged one status step per commit, so a sync merge that brings
//! legal steps passes and a step hidden inside the range is refused.

use std::path::Path;
use std::process::Output;

use super::sequence::{host_merge, merge, planned_project, status, task_block, write};
use super::{body, codeflow, commit, edit, fails, git, ok, push, text, LINE, SECOND, TASK};

/// The `work.records` refusal header.
const REFUSED: &str = "lifecycle invariant work.records";
const STANDALONE: &str = "project-management/tasks/TSK-003.md";
const BLOCK: [&str; 7] = [
    "blocked",
    "--reason",
    "waits on the host",
    "--owner",
    "ops",
    "--revisit",
    "the host is ready",
];

/// `codeflow ci` on the pull request of `branch` into `base` naming `task`.
fn ci_into(root: &Path, base: &str, branch: &str, task: &str) -> Output {
    codeflow(
        root,
        &[
            "ci",
            "--base",
            base,
            "--head",
            "HEAD",
            "--branch",
            branch,
            "--pr-body",
            &body(root, task),
        ],
    )
}

/// The planned project with TSK-001 (depending on TSK-002) and TSK-002
/// blocked on the line, and the standalone TSK-003 blocked on the default
/// branch. Returns the default branch's name.
fn blocked_project(root: &Path) -> String {
    let main = planned_project(root, true);
    git(root, &["switch", "-q", "-c", "plan/block", LINE]);
    edit(root, TASK, "depends_on: []", "depends_on: [TSK-002]");
    for id in ["TSK-001", "TSK-002"] {
        ok(&status(root, "task", id, &BLOCK), "task status blocked");
    }
    commit(root, "chore: block the epic tasks");
    merge(root, LINE, "plan/block", "chore: land the blocks");

    git(root, &["switch", "-q", &main]);
    git(root, &["switch", "-q", "-c", "plan/standalone"]);
    let args = [
        "task",
        "new",
        "--standalone-reason",
        "unblock fixture",
        "--into",
        &main,
        "standalone",
    ];
    ok(&codeflow(root, &args), "task new --standalone-reason");
    assert!(
        root.join(STANDALONE).exists(),
        "the standalone task is TSK-003"
    );
    edit(
        root,
        STANDALONE,
        "- AC-1\n",
        "- AC-1 When run, the system shall work.\n- AC-2 (journey) On a fresh project, the command shall succeed.\n",
    );
    ok(
        &status(root, "task", "TSK-003", &BLOCK),
        "task status blocked",
    );
    commit(root, "chore: plan and block the standalone task");
    host_merge(root, &main, "plan/standalone");
    main
}

/// Unblock `id` (record `path`) in its own pull request from `target`,
/// build it and complete it: the unblock commit and the completed head
/// both pass `codeflow ci` and `work start`, and the evidence names the
/// removed Blocker. Returns the completed head.
fn unblock_and_complete(root: &Path, dir: &Path, target: &str, id: &str, path: &str) -> String {
    let branch = format!("task/{id}-unblock");
    git(root, &["switch", "-q", "-c", &branch, target]);
    ok(&status(root, "task", id, &["todo"]), "unblock");
    let record = std::fs::read_to_string(root.join(path)).unwrap();
    assert!(!record.contains("- reason: waits on the host"), "{record}");
    commit(root, &format!("chore: unblock {id}"));
    let unblocked = ok(&ci_into(root, target, &branch, id), "ci after the unblock");
    assert!(
        unblocked.contains(&format!("{id} is unblocked in this range"))
            && unblocked.contains("waits on the host"),
        "the evidence shows the removed Blocker:\n{unblocked}"
    );
    ok(&codeflow(root, &["work", "start", id]), "work start");
    write(
        root,
        &format!("src/{}.rs", id.to_lowercase()),
        "pub fn work() {}\n",
    );
    let reviewed = commit(root, &format!("feat: build {id}"));
    let block = task_block(dir, id, &reviewed, "verified | the unblock run");
    ok(
        &status(root, "task", id, &["complete", "--acceptance", &block]),
        "complete after the unblock",
    );
    let completed = commit(root, &format!("chore: complete {id}"));
    ok(
        &ci_into(root, target, &branch, id),
        "ci after blocked -> todo -> complete",
    );
    completed
}

/// AC-2, AC-3 and AC-4 (issue #365): a blocked epic task and a blocked
/// standalone task are each unblocked and completed in their own pull
/// request. A direct `blocked -> complete` in one commit, an unblock
/// while a `depends_on` predecessor is open, and an unblock that also
/// changes another task's status are still refused.
#[test]
#[allow(clippy::too_many_lines)] // One journey keeps both kinds and their faults together.
fn a_blocked_task_is_unblocked_and_completed_in_its_own_pull_request() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    let main = blocked_project(&root);

    // The epic task on the line, and the standalone task on the default
    // branch.
    unblock_and_complete(&root, dir.path(), LINE, "TSK-002", SECOND);
    unblock_and_complete(&root, dir.path(), &main, "TSK-003", STANDALONE);

    // A direct `blocked -> complete` in one commit stays refused.
    let direct = "task/TSK-002-direct";
    git(&root, &["switch", "-q", "-c", direct, LINE]);
    write(&root, "src/direct.rs", "pub fn direct() {}\n");
    let reviewed = commit(&root, "feat: build without unblocking");
    let record = std::fs::read_to_string(root.join(SECOND)).unwrap();
    let start = record.find("\n## Blocker\n").unwrap() + 1;
    let end = record[start..].find("## Closeout").unwrap() + start;
    let inner = std::fs::read_to_string(task_block(
        dir.path(),
        "direct",
        &reviewed,
        "verified | direct",
    ))
    .unwrap();
    let completed = format!(
        "{}{}\n```yaml\n{inner}```\n",
        &record[..start],
        record[end..].trim_end_matches('\n')
    )
    .replacen("status: blocked", "status: complete", 1);
    std::fs::write(root.join(SECOND), completed).unwrap();
    commit(&root, "chore: complete TSK-002 straight from blocked");
    fails(
        &ci_into(&root, LINE, direct, "TSK-002"),
        "a direct blocked -> complete",
        "status blocked -> complete is refused",
    );

    // AC-4: the unblock never admits work the graph does not; TSK-001
    // depends on the open TSK-002.
    let waiting = "task/TSK-001-waiting";
    git(&root, &["switch", "-q", "-c", waiting, LINE]);
    ok(&status(&root, "task", "TSK-001", &["todo"]), "unblock");
    commit(&root, "chore: unblock TSK-001");
    fails(
        &ci_into(&root, LINE, waiting, "TSK-001"),
        "an unblock with an open predecessor",
        "TSK-002",
    );

    // AC-4: a task pull request still never changes another task's status.
    let other = "task/TSK-002-and-another";
    git(&root, &["switch", "-q", "-c", other, LINE]);
    ok(&status(&root, "task", "TSK-002", &["todo"]), "unblock");
    ok(
        &status(&root, "task", "TSK-001", &["todo"]),
        "unblock another",
    );
    commit(&root, "chore: unblock TSK-002 and TSK-001");
    fails(
        &ci_into(&root, LINE, other, "TSK-002"),
        "an unblock that changes another record",
        "TSK-001",
    );
}

/// A bare remote holding `main` and `LINE`, as the push path needs.
fn publish(root: &Path, dir: &Path, main: &str) {
    let origin = dir.join("origin.git");
    git(dir, &["init", "-q", "--bare", "-b", main, "origin.git"]);
    let source = root.to_str().unwrap();
    git(
        &origin,
        &[
            "fetch",
            "-q",
            source,
            &format!("{main}:{main}"),
            &format!("{LINE}:{LINE}"),
        ],
    );
    git(root, &["remote", "add", "origin", origin.to_str().unwrap()]);
    git(root, &["fetch", "-q", "origin"]);
    git(root, &["remote", "set-head", "origin", "-a"]);
}

/// The line's pull request check over the sync range `base..HEAD`.
fn sync_ci(root: &Path, base: &str) -> Output {
    codeflow(
        root,
        &["ci", "--base", base, "--head", "HEAD", "--branch", LINE],
    )
}

/// Sync the line with the default branch once (`sync/one`) and in two
/// stages, first at `unblocked`, then at the tip (`sync/two`): both end at
/// the same tree, and neither `codeflow ci` over `before_sync..HEAD` nor
/// the pre-push hook refuses a status step.
fn syncs_pass(root: &Path, main: &str, before_sync: &str, unblocked: &str) {
    git(root, &["switch", "-q", "-c", "sync/one", LINE]);
    git(
        root,
        &[
            "merge",
            "-q",
            "--no-ff",
            main,
            "-m",
            "chore: sync in one merge",
        ],
    );
    let one = sync_ci(root, before_sync);
    assert!(!text(&one).contains(REFUSED), "one merge:\n{}", text(&one));
    let one_tree = git(root, &["rev-parse", "HEAD^{tree}"]);

    git(root, &["switch", "-q", "-c", "sync/two", LINE]);
    git(
        root,
        &[
            "merge",
            "-q",
            "--no-ff",
            unblocked,
            "-m",
            "chore: sync the unblock",
        ],
    );
    git(
        root,
        &[
            "merge",
            "-q",
            "--no-ff",
            main,
            "-m",
            "chore: sync the completion",
        ],
    );
    let two = sync_ci(root, before_sync);
    assert!(!text(&two).contains(REFUSED), "two merges:\n{}", text(&two));
    assert_eq!(git(root, &["rev-parse", "HEAD^{tree}"]), one_tree);

    for sync in ["sync/one", "sync/two"] {
        git(root, &["switch", "-q", sync]);
        let (_, said) = push(
            root,
            &["origin", &format!("HEAD:refs/heads/{LINE}"), "--dry-run"],
        );
        assert!(!said.contains(REFUSED), "{sync} push:\n{said}");
        assert!(
            said.contains("pre-push"),
            "the hook ran for {sync}:\n{said}"
        );
    }
}

/// The project with the line holding the blocked TSK-003; returns the
/// default branch and the line's commit before the sync under test.
fn line_at_blocked(root: &Path) -> (String, String) {
    let main = blocked_project(root);
    git(root, &["switch", "-q", LINE]);
    git(
        root,
        &[
            "merge",
            "-q",
            "--no-ff",
            &main,
            "-m",
            "chore: sync the default branch",
        ],
    );
    let before_sync = git(root, &["rev-parse", "HEAD"]);
    (main, before_sync)
}

/// AC-7 (a) (issue #400): the line holds TSK-003 at `blocked`; the default
/// branch moves it `blocked -> todo -> complete` in its own pull request,
/// landed by a merge. The line's sync passes in one merge and in two.
#[test]
fn a_sync_merge_that_brings_legal_steps_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    let (main, before_sync) = line_at_blocked(&root);
    unblock_and_complete(&root, dir.path(), &main, "TSK-003", STANDALONE);
    let unblocked = git(&root, &["rev-parse", "HEAD~2"]);
    host_merge(&root, &main, "task/TSK-003-unblock");
    publish(&root, dir.path(), &main);
    syncs_pass(&root, &main, &before_sync, &unblocked);
}

/// A commit on the default branch as its host makes one: no client hook.
fn host_commit(root: &Path, message: &str) -> String {
    let hooks = root.join(".git/no-hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let path = format!("core.hooksPath={}", hooks.display());
    git(root, &["add", "-A"]);
    git(root, &["-c", &path, "commit", "-q", "-m", message]);
    git(root, &["rev-parse", "HEAD"])
}

/// AC-8 (issue #424), the reporter's shape: the default branch moves
/// TSK-003 `blocked -> todo` and then `todo -> complete` in two commits of
/// its own first-parent history. The line's one-merge and two-merge syncs
/// end at the same tree and pass alike; a direct `blocked -> complete`
/// edit on the line stays refused.
#[test]
fn the_reporters_two_commit_sync_gets_one_verdict() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    let (main, before_sync) = line_at_blocked(&root);
    git(&root, &["switch", "-q", &main]);
    ok(
        &status(&root, "task", "TSK-003", &["todo"]),
        "unblock on main",
    );
    write(&root, "src/tsk-003.rs", "pub fn work() {}\n");
    let unblocked = host_commit(&root, "chore: unblock TSK-003 with its work");
    let block = task_block(
        dir.path(),
        "reporter",
        &unblocked,
        "verified | the reporter run",
    );
    ok(
        &status(
            &root,
            "task",
            "TSK-003",
            &["complete", "--acceptance", &block],
        ),
        "complete on main",
    );
    host_commit(&root, "chore: complete TSK-003");
    publish(&root, dir.path(), &main);
    syncs_pass(&root, &main, &before_sync, &unblocked);

    // A direct edit on the line from `blocked` to `complete` is refused.
    git(&root, &["switch", "-q", "-c", "line/direct", LINE]);
    let done = git(&root, &["show", &format!("{main}:{STANDALONE}")]) + "\n";
    std::fs::write(root.join(STANDALONE), done).unwrap();
    host_commit(&root, "chore: complete TSK-003 by hand");
    let direct = sync_ci(&root, &before_sync);
    fails(
        &direct,
        "a direct edit on the line",
        "status blocked -> complete is refused",
    );
}
