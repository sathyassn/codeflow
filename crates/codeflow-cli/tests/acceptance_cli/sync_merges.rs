//! A sync of `main` into an epic line that a task branch carries is judged
//! in the pull request form as the line push judges it (TSK-1066, issue
//! #394): a clean sync passes, and a sync that hand-edits a `main` task's
//! file or acceptance block blocks with a message that names the merge and
//! the edit.
use super::*;

const SYNC_LINE: &str = "integration/EPC-001-line-work";
const RECEIVER: &str = "task/TSK-002-extend-the-line";
const TOOL: &str = "src/tool.txt";
const OWNERS: &str = ".github/CODEOWNERS";

/// The issue's four fixture cases.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Case {
    Clean,
    /// The completion commit also edits the tool file.
    BadCompletion,
    /// The sync merge also edits TSK-001's acceptance block.
    TamperedAcceptance,
    /// The sync merge also edits the tool file.
    EditedMerge,
}

struct Fixture {
    dir: tempfile::TempDir,
    /// The line tip before the sync: the base of every form.
    base: String,
    /// `main`'s tip, after TSK-001 landed.
    main: String,
    /// The line's own sync merge.
    line_head: String,
    /// The task branch's sync merge.
    task_sync: String,
    /// The task branch's head: its sync, then one task commit.
    task_head: String,
}

/// Merge `main` into the checked-out branch, applying the case's edit
/// inside the merge; returns the merge.
fn sync(root: &Path, case: Case) -> String {
    git(root, &["merge", "--no-ff", "--no-commit", "main"]);
    match case {
        Case::TamperedAcceptance => {
            let path = root.join(record_path("TSK-001"));
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(text.contains("pr/1#review"), "{text}");
            std::fs::write(&path, text.replace("pr/1#review", "pr/1#review-edited")).unwrap();
        }
        Case::EditedMerge => write(root, TOOL, "tool line\nhand-resolved line\n"),
        Case::Clean | Case::BadCompletion => {}
    }
    commit(root, "chore(sync): merge main into the EPC-001 line")
}

/// The issue's history: TSK-001 reviewed and completed on its branch,
/// `main` changes the owners file (which the line syncs) and then lands
/// TSK-001; the line and TSK-002's branch each sync `main` once more.
fn fixture(case: Case) -> Fixture {
    let dir = repo(&[("TSK-001", OWN_JOURNEY), ("TSK-002", OWN_JOURNEY)], "");
    let root = dir.path();
    write(
        root,
        &record_path("TSK-002"),
        &task("TSK-002", "todo", OWN_JOURNEY, "Pending.\n").replace(
            "integration_target: main",
            &format!("integration_target: {SYNC_LINE}"),
        ),
    );
    write(root, OWNERS, "* @team-a\n");
    commit(root, "docs: target the line");
    git(root, &["branch", SYNC_LINE]);
    let remote = root.join(".git/destination.git");
    git(
        root,
        &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
    );
    git(root, &["remote", "add", "origin", remote.to_str().unwrap()]);

    git(root, &["switch", "-c", BRANCH]);
    write(root, TOOL, "tool line\n");
    let reviewed = commit(root, "feat: add the tool");
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "complete", OWN_JOURNEY, &valid_block(&reviewed)),
    );
    if case == Case::BadCompletion {
        write(root, TOOL, "tool line\nlate edit\n");
    }
    commit(root, "docs(records): complete TSK-001");

    git(root, &["switch", "main"]);
    write(root, OWNERS, "* @team-a @team-b\n");
    commit(root, "chore: add team-b");
    git(root, &["switch", SYNC_LINE]);
    git(
        root,
        &["merge", "--no-ff", "-m", "chore(sync): merge main", "main"],
    );
    git(root, &["switch", "main"]);
    git(
        root,
        &[
            "merge",
            "--no-ff",
            "-m",
            "Merge pull request #1 from task/TSK-001-work",
            BRANCH,
        ],
    );
    let main = head(root);
    git(root, &["switch", SYNC_LINE]);
    let base = head(root);
    git(root, &["push", "-q", "origin", "main", SYNC_LINE, BRANCH]);

    let line_head = sync(root, case);
    git(root, &["switch", "-c", RECEIVER, &base]);
    let task_sync = sync(root, case);
    write(root, "LINE.md", "line notes\n");
    let task_head = commit(root, "docs(line): add the line notes");
    let seed = codeflow()
        .args(["ids", "seed"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        seed.status.success(),
        "{}",
        String::from_utf8_lossy(&seed.stderr)
    );
    Fixture {
        dir,
        base,
        main,
        line_head,
        task_sync,
        task_head,
    }
}

fn run(root: &Path, args: &[&str]) -> (i32, String) {
    let out = codeflow().args(args).current_dir(root).output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

/// `codeflow ci` as a pull request from TSK-002's branch into the line,
/// with `head` as its head.
fn pull_request(fixture: &Fixture, head: &str) -> (i32, String) {
    let root = fixture.dir.path();
    let body = with_reviews(
        root,
        head,
        "## Summary\nThe line notes, on a branch that merged main first.\n\n- one change\n\nTask: TSK-002\n\n## Changes\n- Merge main into the task branch.\n\n## Testing\n- test\n",
    );
    run(
        root,
        &[
            "ci",
            "--base",
            &fixture.base,
            "--head",
            head,
            "--branch",
            RECEIVER,
            "--into",
            SYNC_LINE,
            "--pr-body",
            &body,
        ],
    )
}

/// `codeflow ci` as the pre-push hook runs it for the line's own push.
fn line_push(fixture: &Fixture) -> (i32, String) {
    let root = fixture.dir.path();
    let remote = root.join(".git/destination.git");
    run(
        root,
        &[
            "ci",
            "--base",
            &fixture.base,
            "--head",
            &fixture.line_head,
            "--branch",
            SYNC_LINE,
            "--baseline-from",
            &fixture.base,
            "--policy-from",
            &fixture.main,
            "--commits-from",
            &fixture.main,
            "--destination",
            remote.to_str().unwrap(),
        ],
    )
}

/// The three forms the issue runs: the sync alone and the sync with a task
/// commit as pull requests, and the line push. Each names the sync merge
/// its head carries.
fn three_forms(fixture: &Fixture) -> [(&'static str, (i32, String), String); 3] {
    [
        (
            "pr-sync",
            pull_request(fixture, &fixture.line_head),
            fixture.line_head.clone(),
        ),
        (
            "pr-task",
            pull_request(fixture, &fixture.task_head),
            fixture.task_sync.clone(),
        ),
        ("line-push", line_push(fixture), fixture.line_head.clone()),
    ]
}

#[test]
fn a_clean_sync_passes_in_every_form() {
    let fixture = fixture(Case::Clean);
    for (form, result, _) in three_forms(&fixture) {
        assert_passes(&result, form);
    }
}

#[test]
fn a_sync_that_edits_a_main_task_file_blocks_in_every_form() {
    let fixture = fixture(Case::EditedMerge);
    for (form, result, merge) in three_forms(&fixture) {
        assert_blocks(
            &result,
            form,
            &[
                "work.acceptance_binding",
                &format!("receiving line `{SYNC_LINE}` introduces {TOOL} through merge {merge}"),
            ],
        );
    }
}

#[test]
fn a_sync_that_edits_an_acceptance_block_names_the_edit_and_the_merge() {
    let fixture = fixture(Case::TamperedAcceptance);
    for (form, result, merge) in three_forms(&fixture) {
        assert_blocks(
            &result,
            form,
            &[&format!(
                "TSK-001: merge {merge} edits the acceptance block in {}",
                record_path("TSK-001")
            )],
        );
        for wrong in [
            OWNERS,
            "a parent other than its first",
            &format!("the landing merge {merge}"),
        ] {
            assert!(
                !result.1.contains(wrong),
                "{form}: names the wrong cause {wrong:?}: {}",
                result.1
            );
        }
    }
}

#[test]
fn a_completion_that_edits_product_code_still_blocks_in_every_form() {
    let fixture = fixture(Case::BadCompletion);
    for (form, result, _) in three_forms(&fixture) {
        assert_blocks(&result, form, &["TSK-001", TOOL]);
    }
}
