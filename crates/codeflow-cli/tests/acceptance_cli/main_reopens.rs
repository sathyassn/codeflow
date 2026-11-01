//! A sync of `main` into an epic line that carries `main`'s record of a
//! task unchanged, after `main` reopened the task with one more criterion
//! and completed it again (TSK-1124, issues #456 and #464).
use super::*;

const MAIN_LINE: &str = "integration/EPC-001-main-reopens";
const RECEIVER: &str = "task/TSK-002-on-the-line";

/// What the line held of TSK-001 before the sync under judgment.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Shape {
    /// The line never held TSK-001 (issue #456's table).
    NeverHeld,
    /// The line synced `main`'s first completion before the reopen.
    SyncedFirst,
    /// The line was cut after `main` planned TSK-001, so it holds the
    /// planned record at its base and never changed it (issue #464).
    HeldAtCut,
    /// As [`Shape::HeldAtCut`], but `main` completes TSK-001, relabels a
    /// criterion and completes it again inside the task's own pull request,
    /// as issue #464's history did.
    RelabeledInPr,
    /// As [`Shape::HeldAtCut`], but the line also completed TSK-001 itself
    /// before the sync, so both sides changed the record.
    BothCompleted,
}

struct Fixture {
    dir: tempfile::TempDir,
    /// The line tip before the sync: the base of every form.
    base: String,
    /// `main`'s tip, after the re-completion landed.
    main: String,
    /// The line's sync merge.
    sync: String,
    /// The task branch's own sync merge.
    task_sync: String,
    /// The task branch's head: the same sync, then one task commit.
    task_head: String,
}

/// Merge `main` into the checked-out branch; `edit` changes the merge's
/// tree before it is committed. Returns the merge. A conflicting hunk takes
/// `main`'s side, so a shape where both sides changed the record still
/// merges; only [`Shape::BothCompleted`] has one.
fn sync_main(root: &Path, edit: &dyn Fn(&Path)) -> String {
    git(
        root,
        &["merge", "--no-ff", "--no-commit", "-X", "theirs", "main"],
    );
    edit(root);
    commit(root, "chore(sync): merge main into the EPC-001 line")
}

/// Whether the line was cut after `main` planned TSK-001.
fn held_at_cut(shape: Shape) -> bool {
    matches!(
        shape,
        Shape::HeldAtCut | Shape::RelabeledInPr | Shape::BothCompleted
    )
}

fn plan_tsk_001(root: &Path) {
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", OWN_JOURNEY, "Pending.\n"),
    );
    commit(root, "docs(plan): plan TSK-001");
}

/// The line completes TSK-001 itself, from `main`, which stays checked out.
fn complete_on_the_line(root: &Path) {
    git(root, &["switch", MAIN_LINE]);
    write(root, "src/line.txt", "line tool\n");
    let line_reviewed = commit(root, "feat: add the line's tool");
    write(
        root,
        &record_path("TSK-001"),
        &task(
            "TSK-001",
            "complete",
            OWN_JOURNEY,
            &valid_block(&line_reviewed),
        ),
    );
    commit(root, "docs(records): complete TSK-001 on the line");
    git(root, &["switch", "main"]);
}

/// TSK-001's pull request lands its first completion on `main` (with the
/// relabel and re-completion inside it for [`Shape::RelabeledInPr`]).
/// Returns the first acceptance block.
fn first_completion(root: &Path, shape: Shape) -> String {
    git(root, &["switch", "-c", BRANCH]);
    write(root, "src/tool.txt", "tool line\n");
    let reviewed = commit(root, "feat: add the tool");
    let first = valid_block(&reviewed);
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "complete", OWN_JOURNEY, &first),
    );
    commit(root, "docs(records): complete TSK-001");
    if shape == Shape::RelabeledInPr {
        relabel_in_pr(root, &first);
        assert_passes(
            &ci_on(root, "main", BRANCH, "Task: TSK-001"),
            "the relabel pull request",
        );
    }
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
    first
}

/// One pull request into `main`: its branch and the commit it makes.
type PullRequest<'a> = (&'a str, &'a dyn Fn(&Path) -> String);

/// `main` reopens TSK-001, amends it with a third criterion, lands the fix
/// and completes it again, each through its own pull request that passes
/// `codeflow ci` on `main`.
fn reopen_on_main(root: &Path, first: &str) {
    let three = format!("{OWN_JOURNEY}- AC-3 When rerun, the system shall still work.\n");
    let archived = first.replace(
        "acceptance:\n",
        "acceptance_superseded:\n  reason: one more criterion\n",
    );
    let steps: [PullRequest; 4] = [
        ("plan/reopen-tsk-001", &|root| {
            write(
                root,
                &record_path("TSK-001"),
                &task("TSK-001", "todo", OWN_JOURNEY, &archived),
            );
            commit(root, "docs: reopen TSK-001")
        }),
        ("plan/amend-tsk-001", &|root| {
            write(
                root,
                &record_path("TSK-001"),
                &task("TSK-001", "todo", &three, &archived),
            );
            commit(root, "docs: add a third criterion to TSK-001")
        }),
        ("fix/tsk-001-rerun", &|root| {
            write(root, "src/tool.txt", "tool line\nrerun line\n");
            commit(root, "fix: keep working when rerun")
        }),
        ("plan/recomplete-tsk-001", &|root| {
            let fixed = head(root);
            write(
                root,
                &record_path("TSK-001"),
                &task(
                    "TSK-001",
                    "complete",
                    &three,
                    &format!("{archived}{}", three_criteria_block(&fixed)),
                ),
            );
            commit(root, "docs(records): complete TSK-001 again")
        }),
    ];
    for (number, (branch, step)) in steps.iter().enumerate() {
        git(root, &["switch", "-c", branch, "main"]);
        step(root);
        if !branch.starts_with("fix/") {
            assert_passes(&ci_on(root, "main", branch, ""), branch);
        }
        git(root, &["switch", "main"]);
        git(
            root,
            &[
                "merge",
                "--no-ff",
                "-m",
                &format!("Merge pull request #{} from {branch}", number + 2),
                branch,
            ],
        );
    }
}

/// TSK-001 is planned on `main` (after the line was cut unless the shape
/// holds it at the cut), completed, then
/// reopened with a third criterion and completed again, each through a pull
/// request merge; the line then syncs `main` with `edit` applied.
fn fixture(shape: Shape, edit: &dyn Fn(&Path)) -> Fixture {
    let dir = repo(&[("TSK-002", OWN_JOURNEY)], "");
    let root = dir.path();
    write(
        root,
        &record_path("TSK-002"),
        &task("TSK-002", "todo", OWN_JOURNEY, "Pending.\n").replace(
            "integration_target: main",
            &format!("integration_target: {MAIN_LINE}"),
        ),
    );
    commit(root, "docs: target the line");
    if held_at_cut(shape) {
        plan_tsk_001(root);
    }
    git(root, &["branch", MAIN_LINE]);
    if shape == Shape::BothCompleted {
        complete_on_the_line(root);
    }
    let remote = root.join(".git/destination.git");
    git(
        root,
        &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
    );
    git(root, &["remote", "add", "origin", remote.to_str().unwrap()]);
    if !held_at_cut(shape) {
        plan_tsk_001(root);
    }
    let first = first_completion(root, shape);
    if shape == Shape::SyncedFirst {
        git(root, &["switch", MAIN_LINE]);
        git(
            root,
            &["merge", "--no-ff", "-m", "chore(sync): merge main", "main"],
        );
        git(root, &["switch", "main"]);
    }
    if shape != Shape::RelabeledInPr {
        reopen_on_main(root, &first);
    }
    let main = head(root);
    git(root, &["switch", MAIN_LINE]);
    let base = head(root);
    git(root, &["push", "-q", "origin", "main", MAIN_LINE]);

    let sync = sync_main(root, edit);
    git(root, &["switch", "-c", RECEIVER, &base]);
    let task_sync = sync_main(root, edit);
    write(root, "LINE.md", "line notes\n");
    let task_head = commit(root, "docs(line): add the line notes");
    git(root, &["switch", MAIN_LINE]);
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
        sync,
        task_sync,
        task_head,
    }
}

/// On the task branch after its first completion: reopen TSK-001, relabel
/// AC-1 in a record-only commit and complete it again against that commit,
/// keeping the first block as superseded.
fn relabel_in_pr(root: &Path, first: &str) {
    let relabeled = OWN_JOURNEY.replace("AC-1 When run,", "AC-1 When run from a clean checkout,");
    let archived = first.replace(
        "acceptance:\n",
        "acceptance_superseded:\n  reason: AC-1 relabeled\n",
    );
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", &relabeled, &archived),
    );
    let relabel = commit(root, "docs(records): relabel AC-1 of TSK-001");
    write(
        root,
        &record_path("TSK-001"),
        &task(
            "TSK-001",
            "complete",
            &relabeled,
            &format!("{archived}{}", valid_block(&relabel)),
        ),
    );
    commit(root, "docs(records): complete TSK-001 again");
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

/// The issue's form: `codeflow ci --branch <line>` over the sync.
fn line_ci(fixture: &Fixture) -> (i32, String) {
    run(
        fixture.dir.path(),
        &[
            "ci",
            "--branch",
            MAIN_LINE,
            "--base",
            &fixture.base,
            "--head",
            &fixture.sync,
        ],
    )
}

/// `codeflow ci` as the pre-push hook runs it for the line's push.
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
            &fixture.sync,
            "--branch",
            MAIN_LINE,
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

/// `codeflow ci` as a pull request from TSK-002's branch, which carries the
/// same sync, into the line.
fn pull_request(fixture: &Fixture) -> (i32, String) {
    let root = fixture.dir.path();
    let body = with_reviews(
        root,
        &fixture.task_head,
        "## Summary\nThe line notes, on a branch that merged main first.\n\n- one change\n\nTask: TSK-002\n\n## Changes\n- Merge main into the task branch.\n\n## Testing\n- test\n- Not tested: nothing else\n",
    );
    run(
        root,
        &[
            "ci",
            "--base",
            &fixture.base,
            "--head",
            &fixture.task_head,
            "--branch",
            RECEIVER,
            "--into",
            MAIN_LINE,
            "--pr-body",
            &body,
        ],
    )
}

/// The three forms, each with the sync merge its head carries.
fn three_forms(fixture: &Fixture) -> [(&'static str, (i32, String), &str); 3] {
    [
        ("line-ci", line_ci(fixture), &fixture.sync),
        ("line-push", line_push(fixture), &fixture.sync),
        ("pull-request", pull_request(fixture), &fixture.task_sync),
    ]
}

fn unchanged(_: &Path) {}

/// Change TSK-001's record inside the sync merge with `edit`.
fn edit_record(root: &Path, edit: impl Fn(String) -> String) {
    let path = root.join(record_path("TSK-001"));
    let text = std::fs::read_to_string(&path).unwrap();
    let changed = edit(text.clone());
    assert_ne!(changed, text, "the edit changes nothing");
    std::fs::write(&path, changed).unwrap();
}

#[test]
fn a_sync_of_mains_reopened_record_passes_in_every_form() {
    for shape in [
        Shape::NeverHeld,
        Shape::SyncedFirst,
        Shape::HeldAtCut,
        Shape::RelabeledInPr,
    ] {
        let synced = fixture(shape, &unchanged);
        for (form, result, _) in three_forms(&synced) {
            assert_passes(&result, &format!("{shape:?} {form}"));
        }
    }
}

/// A sync that edits the imported record, by one byte of its acceptance
/// block or by an added criterion, is the line's own change and still
/// blocks, naming the task, and the merge where the block was edited.
#[test]
fn a_sync_that_edits_mains_reopened_record_still_blocks() {
    let one_byte = |root: &Path| {
        edit_record(root, |text| {
            text.replace("AC-3: verified | unit\n", "AC-3: verified | units\n")
        });
    };
    let criterion = |root: &Path| {
        edit_record(root, |text| {
            text.replace(
                "shall still work.\n",
                "shall still work.\n- AC-4 When asked, the system shall explain.\n",
            )
        });
    };
    for shape in [Shape::NeverHeld, Shape::SyncedFirst, Shape::HeldAtCut] {
        let edited = fixture(shape, &one_byte);
        for (form, result, merge) in three_forms(&edited) {
            assert_blocks(
                &result,
                &format!("{shape:?} one byte {form}"),
                &[&format!(
                    "TSK-001: merge {merge} edits the acceptance block in {}",
                    record_path("TSK-001")
                )],
            );
        }
        let added = fixture(shape, &criterion);
        for (form, result, _) in three_forms(&added) {
            assert_blocks(
                &result,
                &format!("{shape:?} criterion {form}"),
                &["TSK-001: a reopened task keeps its criteria"],
            );
        }
    }
}

/// A line that reopens a task it holds and adds a criterion, outside a
/// planning pull request on the target, is still refused: an imported
/// record is the only one whose reopened criteria are not compared.
#[test]
fn a_line_that_reopens_a_task_it_holds_is_still_refused() {
    let synced = fixture(Shape::SyncedFirst, &unchanged);
    let root = synced.dir.path();
    git(root, &["switch", "-c", "line-reopen", &synced.base]);
    edit_record(root, |text| {
        text.replace("status: complete", "status: todo")
            .replace(
                "acceptance:\n",
                "acceptance_superseded:\n  reason: reopened on the line\n",
            )
            .replace(
                "the command shall succeed.\n",
                "the command shall succeed.\n- AC-3 When asked, the system shall explain.\n",
            )
    });
    let reopen = commit(root, "docs(records): reopen TSK-001 on the line");
    let result = run(
        root,
        &[
            "ci",
            "--branch",
            MAIN_LINE,
            "--base",
            &synced.base,
            "--head",
            &reopen,
        ],
    );
    assert_blocks(
        &result,
        "line-side reopen",
        &["TSK-001: a reopened task keeps its criteria"],
    );
}

/// When the import of TSK-001's record cannot be proved, the sync is
/// judged as the line's change and refused with the cause.
#[test]
fn an_unproved_import_is_refused_with_its_cause() {
    for shape in [Shape::NeverHeld, Shape::SyncedFirst] {
        // No default target resolves: `main` is gone, locally and as
        // tracked, and nothing names another. The line cannot be proved,
        // so the run is refused before any record is judged.
        let gone = fixture(shape, &unchanged);
        let root = gone.dir.path();
        git(root, &["update-ref", "-d", "refs/heads/main"]);
        git(root, &["update-ref", "-d", "refs/remotes/origin/main"]);
        let (code, out) = line_ci(&gone);
        assert_eq!(code, 2, "{shape:?} no default target: {out}");
        assert!(
            out.contains("no main or master branch to land the line on"),
            "{shape:?} no default target: {out}"
        );

        // `origin/HEAD` names a default target that does not resolve, so
        // the import is not proved and the records check compares the
        // reopened criteria with the line's base.
        let dangling = fixture(shape, &unchanged);
        let root = dangling.dir.path();
        git(
            root,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/trunk",
            ],
        );
        let mut needles = vec!["published home `trunk` is unavailable"];
        let records = format!(
            "{}: a reopened task keeps its criteria",
            record_path("TSK-001")
        );
        if shape == Shape::SyncedFirst {
            needles.push(&records);
        }
        assert_blocks(
            &line_ci(&dangling),
            &format!("{shape:?} dangling default target"),
            &needles,
        );

        // A line commit that writes `main`'s record without merging `main`
        // imports nothing, so the completion's review is not in the head.
        let copied = fixture(shape, &unchanged);
        let root = copied.dir.path();
        git(root, &["switch", "-c", "plan/align-tsk-001", &copied.base]);
        let blob = git_out(
            root,
            &[
                "show",
                &format!("{}:{}", copied.main, record_path("TSK-001")),
            ],
        );
        write(root, &record_path("TSK-001"), &blob);
        let align = commit(root, "docs(plan): align TSK-001 with main");
        let result = run(
            root,
            &[
                "ci",
                "--base",
                &copied.base,
                "--head",
                &align,
                "--branch",
                "plan/align-tsk-001",
                "--policy-from",
                &copied.main,
            ],
        );
        assert_blocks(
            &result,
            &format!("{shape:?} record-only copy"),
            &["is not the head or an ancestor of it; review the result that lands"],
        );
    }
}

/// When both sides completed the task and the sync resolves the record to
/// `main`'s bytes, the resolution is the line's decision, so the record is
/// not imported and the reopened criteria are still compared.
#[test]
fn a_sync_that_resolves_both_completions_to_mains_is_refused() {
    let take_mains = |root: &Path| {
        git(root, &["checkout", "main", "--", &record_path("TSK-001")]);
    };
    let resolved = fixture(Shape::BothCompleted, &take_mains);
    let records = format!(
        "{}: a reopened task keeps its criteria",
        record_path("TSK-001")
    );
    for (form, result, _) in three_forms(&resolved) {
        assert_blocks(&result, &format!("resolved to main's {form}"), &[&records]);
    }
}
