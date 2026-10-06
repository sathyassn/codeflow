//! Records and `codeflow ci` ranges: each finding raised in a real
//! repository, and the printed step taken there.
//!
//! A landing on the target is fixture input: the destination is a bare
//! repository standing in for the hosting platform, and a reviewed merge
//! lands there by a fetch into it, which runs no client hook.

use super::*;

/// The commit `revision` names in `root`.
fn rev(root: &Path, revision: &str) -> String {
    String::from_utf8(run("git", root, &["rev-parse", revision]).stdout)
        .unwrap()
        .trim()
        .to_string()
}

/// A standard project whose target lives in a bare destination `origin`.
struct Hosted {
    dir: tempfile::TempDir,
    root: PathBuf,
    dest: PathBuf,
    /// The target branch on the destination and in the checkout.
    target: String,
}

impl Hosted {
    fn new() -> Self {
        let dir = scaffolded("--standard");
        let root = project(&dir);
        let dest = with_destination(&root);
        let target =
            String::from_utf8(run("git", &dest, &["symbolic-ref", "--short", "HEAD"]).stdout)
                .unwrap()
                .trim()
                .to_string();
        Self {
            dir,
            root,
            dest,
            target,
        }
    }

    /// The remote-tracking name of the target.
    fn upstream(&self) -> String {
        format!("origin/{}", self.target)
    }

    /// Start `branch` from the destination's target.
    fn branch(&self, branch: &str) {
        git(
            &self.root,
            &["switch", "-q", "-c", branch, &self.upstream()],
        );
    }

    /// Plan the standalone task TSK-001 with one criterion on `branch`,
    /// committed.
    fn plan_task(&self, branch: &str) {
        self.branch(branch);
        let out = codeflow(
            &self.root,
            &[
                "task",
                "new",
                "--standalone-reason",
                "a fixture",
                "the work",
            ],
        );
        assert!(out.contains("TSK-001"), "{out}");
        let task = read(&self.root, TASK).replacen("- AC-1\n", CRITERION, 1);
        write(&self.root, TASK, &task);
        commit_all(&self.root, "docs: plan the work");
    }

    /// Land `branch` on the destination's target, as a reviewed merge on
    /// the platform does, then fast-forward the local target to it.
    fn land(&self, branch: &str) {
        git(
            &self.dest,
            &[
                "fetch",
                "-q",
                self.root.to_str().unwrap(),
                &format!("{branch}:{}", self.target),
            ],
        );
        git(&self.root, &["fetch", "-q", "origin"]);
        git(
            &self.root,
            &[
                "fetch",
                "-q",
                ".",
                &format!("{}:{}", self.upstream(), self.target),
            ],
        );
    }

    /// Seed the id registry on the destination, the registry authority, as
    /// a maintainer does once.
    fn seed(&self) {
        let out = codeflow(&self.root, &["ids", "seed"]);
        assert!(out.contains("reserved on the authority"), "{out}");
    }

    /// `codeflow ci` for the pull request of `branch` into the target.
    fn ci(&self, branch: &str, extra: &[&str]) -> String {
        let upstream = self.upstream();
        let mut args = vec![
            "ci", "--base", &upstream, "--head", "HEAD", "--branch", branch,
        ];
        args.extend(extra);
        codeflow(&self.root, &args)
    }
}

/// An acceptance block for TSK-001 that reviews the checked-out head,
/// written beside the project; returns its path.
fn review_head(root: &Path) -> String {
    let path = root.parent().unwrap().join("acceptance.yaml");
    let block = format!(
        "acceptance:\n  reviewed: {}\n  review: session:proof@sha256:00\n  criteria:\n    \
         AC-1: verified | the fixture's files hold their content\n  journey: none | no journey \
         criterion\n  not_verified: none\n  follow_ups: none: a fixture\n  verdict: approved\n",
        rev(root, "HEAD")
    );
    std::fs::write(&path, block).unwrap();
    path.to_str().unwrap().to_string()
}

/// Record the checked-out commit as the work-records migration baseline,
/// in a commit of its own.
fn record_baseline(root: &Path) {
    let baseline = rev(root, "HEAD");
    let state = read(root, ".codeflow/project.toml");
    write(
        root,
        ".codeflow/project.toml",
        &format!("{state}work_records_baseline = \"{baseline}\"\n"),
    );
    commit_all(root, "chore: record the records baseline");
}

const CRITERION: &str = "- AC-1 When run, the system shall work.\n";

// ---------------------------------------------------------------------------
// History a clone does not have: `git fetch --unshallow`, as printed.
// ---------------------------------------------------------------------------

#[test]
fn clears_baseline_history() {
    let dir = planned();
    let root = project(&dir);
    record_baseline(&root);
    // A depth-1 clone holds the commit that lists the baseline, not the
    // baseline commit itself.
    let clone = dir.path().join("clone");
    codeflow_fixture::clone(
        dir.path(),
        format!("file://{}", root.display()),
        clone.to_str().unwrap(),
    )
    .depth(1)
    .branch("plan/x")
    .env("GIT_CONFIG_GLOBAL", "/dev/null")
    .env("GIT_CONFIG_SYSTEM", "/dev/null")
    .run();
    prove(
        "BASELINE_HISTORY",
        "is not in this clone's history",
        || validate(&clone),
        |printed| {
            let step = printed_command(printed, "BASELINE_HISTORY", None);
            assert_eq!(step, "git fetch --unshallow");
            run_printed(&clone, &step, &[], &["-q"]);
        },
    );
}

#[test]
fn clears_ci_range_unreadable() {
    let dir = ci_repo(DEFAULTS);
    let root = dir.path();
    commit(root, "a.txt", "feat: add a");
    commit(root, "b.txt", "feat: add b");
    git(root, &["switch", "-q", "main"]);
    commit(root, "m.txt", "feat: add m");
    // A shallow CI checkout: each tip without the history that joins them.
    let clones = tempfile::tempdir().unwrap();
    let clone = clones.path().join("clone");
    codeflow_fixture::clone(
        clones.path(),
        format!("file://{}", root.display()),
        clone.to_str().unwrap(),
    )
    .depth(1)
    .branch("feat/x")
    .env("GIT_CONFIG_GLOBAL", "/dev/null")
    .env("GIT_CONFIG_SYSTEM", "/dev/null")
    .run();
    git(
        &clone,
        &[
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/origin/*",
        ],
    );
    git(&clone, &["fetch", "-q", "--depth", "1", "origin"]);
    let check = || {
        codeflow(
            &clone,
            &[
                "ci",
                "--base",
                "origin/main",
                "--head",
                "HEAD",
                "--branch",
                "feat/x",
            ],
        )
    };
    prove(
        "CI_RANGE_UNREADABLE",
        "could not diff the range",
        check,
        |printed| {
            let step = printed_command(printed, "CI_RANGE_UNREADABLE", None);
            assert_eq!(step, "git fetch --unshallow");
            run_printed(&clone, &step, &[], &["-q"]);
        },
    );
}

// ---------------------------------------------------------------------------
// Record edits named by `codeflow validate --docs`.
// ---------------------------------------------------------------------------

/// A standard project whose standalone task TSK-001 is planned and
/// committed, checked out on the line `feat/line` it lands on.
fn standalone_on_line() -> tempfile::TempDir {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    git(&root, &["switch", "-q", "-c", "plan/x"]);
    for args in [
        vec!["epic", "new", "an outcome"],
        vec![
            "task",
            "new",
            "--standalone-reason",
            "a fixture",
            "the work",
        ],
    ] {
        let out = codeflow(&root, &args);
        assert!(!out.contains("error"), "{args:?}: {out}");
    }
    let task = read(&root, TASK).replacen("- AC-1\n", CRITERION, 1);
    write(&root, TASK, &task);
    commit_all(&root, "docs: plan the work");
    git(&root, &["switch", "-q", "-c", "feat/line"]);
    dir
}

/// Mark TSK-001 complete with an approved acceptance block.
fn complete_task(root: &Path) {
    let acceptance = review_head(root);
    let out = codeflow(
        root,
        &[
            "task",
            "status",
            "TSK-001",
            "complete",
            "--acceptance",
            &acceptance,
        ],
    );
    assert!(out.contains("todo -> complete"), "{out}");
}

/// Advance `feat/line` with a commit, then merge it into `branch` with
/// `message` (git's own subject when `None`), as a catch-up merge of the
/// target into a task branch does.
fn catch_up(root: &Path, branch: &str, file: &str, message: Option<&str>) {
    git(root, &["switch", "-q", "feat/line"]);
    commit(root, file, &format!("feat: add {file}"));
    git(root, &["switch", "-q", branch]);
    match message {
        Some(message) => git(
            root,
            &["merge", "-q", "--no-ff", "-m", message, "feat/line"],
        ),
        None => git(root, &["merge", "-q", "--no-ff", "--no-edit", "feat/line"]),
    }
}

/// Land `branch` on `feat/line` with a merge commit titled `message`.
fn land(root: &Path, branch: &str, message: &str) {
    git(root, &["switch", "-q", "feat/line"]);
    git(root, &["merge", "-q", "--no-ff", "-m", message, branch]);
}

#[test]
fn clears_standalone_split() {
    let dir = standalone_on_line();
    let root = project(&dir);
    // Two pull requests deliver the standalone task onto one line.
    for part in ["a", "b"] {
        let branch = format!("task/TSK-001-{part}");
        git(&root, &["switch", "-q", "-c", &branch, "feat/line"]);
        commit(&root, &format!("{part}.txt"), &format!("feat: add {part}"));
        git(&root, &["switch", "-q", "feat/line"]);
        git(&root, &["merge", "-q", "--no-ff", "--no-edit", &branch]);
    }
    complete_task(&root);
    prove(
        "STANDALONE_SPLIT",
        "was completed by 2 pull requests",
        || validate(&root),
        |printed| {
            assert!(
                printed.contains(&format!("give {TASK} an `epic_id`")),
                "{printed}"
            );
            set_field(&root, TASK, "epic_id", "EPC-001");
            set_field(&root, TASK, "standalone_reason", "null");
        },
    );
}

/// The catch-up merges the rules require name the task branch as their
/// destination; they are not landings, so a task branch that took two of
/// them and landed once is one pull request, read from the line and from
/// the task branch itself.
#[test]
fn catch_up_merges_are_not_landings() {
    let dir = standalone_on_line();
    let root = project(&dir);
    let branch = "task/TSK-001-work";
    git(&root, &["switch", "-q", "-c", branch, "feat/line"]);
    commit(&root, "work.txt", "feat: add the work");
    catch_up(&root, branch, "one.txt", None);
    catch_up(
        &root,
        branch,
        "two.txt",
        Some("Merge remote-tracking branch 'origin/main' into task/TSK-001-work"),
    );
    land(
        &root,
        branch,
        "Merge pull request #7 from task/TSK-001-work",
    );
    complete_task(&root);
    let on_line = validate(&root);
    assert!(!on_line.contains("was completed by"), "{on_line}");
    // The completed record carries over unchanged to the task branch, whose
    // first-parent line holds both catch-up merges.
    git(&root, &["switch", "-q", branch]);
    let on_branch = validate(&root);
    assert!(!on_branch.contains("was completed by"), "{on_branch}");
}

/// Two real landings still warn, counted once each, with catch-up merges on
/// the second branch and GitHub's owner-prefixed merge subject.
#[test]
fn two_landings_warn_beside_catch_up_merges() {
    let dir = standalone_on_line();
    let root = project(&dir);
    git(
        &root,
        &["switch", "-q", "-c", "task/TSK-001-a", "feat/line"],
    );
    commit(&root, "a.txt", "feat: add a");
    land(
        &root,
        "task/TSK-001-a",
        "Merge pull request #7 from task/TSK-001-a",
    );
    git(&root, &["switch", "-q", "-c", "fix/TSK-001-b", "feat/line"]);
    commit(&root, "b.txt", "fix: add b");
    catch_up(&root, "fix/TSK-001-b", "one.txt", None);
    catch_up(
        &root,
        "fix/TSK-001-b",
        "two.txt",
        Some("Merge main into fix/TSK-001-b"),
    );
    land(
        &root,
        "fix/TSK-001-b",
        "Merge pull request #8 from owner/fix/TSK-001-b",
    );
    complete_task(&root);
    let warning = "standalone task TSK-001 was completed by 2 pull requests";
    let on_line = validate(&root);
    assert!(on_line.contains(warning), "{on_line}");
    // The line then lands on a release line in one merge; the two landings
    // stay visible from there.
    git(&root, &["switch", "-q", "-c", "feat/release", "plan/x"]);
    git(
        &root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "Merge pull request #9 from owner/feat/line",
            "feat/line",
        ],
    );
    let on_release = validate(&root);
    assert!(on_release.contains(warning), "{on_release}");
}

#[test]
fn clears_record_baseline_exempt() {
    let dir = planned();
    let root = project(&dir);
    // Before the migration, a task was blocked without a Blocker.
    set_field(&root, TASK, "status", "blocked");
    commit_all(&root, "docs: block the task");
    record_baseline(&root);
    // An edit that changes no status or criteria keeps the exemption.
    set_field(&root, TASK, "title", "\"the renamed task\"");
    prove(
        "RECORD_BASELINE_EXEMPT",
        "a blocked task needs a `## Blocker`",
        || validate(&root),
        |printed| {
            assert!(printed.contains(&format!("fix it in {TASK}")), "{printed}");
            let task = read(&root, TASK);
            write(
                &root,
                TASK,
                &format!(
                    "{task}\n## Blocker\n\n- reason: waits on a fixture\n- owner: proof\n\
                     - revisit: the fixture lands\n"
                ),
            );
        },
    );
}

// ---------------------------------------------------------------------------
// Task work in `codeflow ci`: planning landed on the target first.
// ---------------------------------------------------------------------------

#[test]
fn clears_work_start_reconcile() {
    let hosted = Hosted::new();
    let (root, target) = (&hosted.root, &hosted.target);
    git(
        root,
        &[
            "branch",
            "-q",
            "--set-upstream-to",
            &hosted.upstream(),
            target,
        ],
    );
    hosted.plan_task("plan/work");
    hosted.land("plan/work");
    // The destination's target is then rewritten with the same plan, so
    // the local target and its upstream have diverged.
    git(root, &["switch", "-q", "-c", "plan/again", "plan/work~1"]);
    git(root, &["checkout", "plan/work", "--", "project-management"]);
    write(root, "docs/plan/notes.md", "Planned again.\n");
    commit_all(root, "docs: plan the work again");
    git(
        &hosted.dest,
        &[
            "fetch",
            "-q",
            root.to_str().unwrap(),
            &format!("+plan/again:{target}"),
        ],
    );
    git(root, &["fetch", "-q", "origin"]);
    hosted.branch("task/TSK-001-work");
    commit(root, "x.txt", "feat: add x");
    prove(
        "WORK_START_RECONCILE",
        "have diverged",
        || hosted.ci("task/TSK-001-work", &[]),
        |printed| {
            // Reconcile as the finding says: reset the target to its upstream.
            git(root, &["branch", "-f", target, &hosted.upstream()]);
            let step = printed_command(printed, "WORK_START_RECONCILE", None);
            run_printed(root, &step, &[], &[]);
        },
    );
}

#[test]
fn clears_work_start_merge_planning() {
    let hosted = Hosted::new();
    let root = &hosted.root;
    hosted.plan_task("plan/work");
    // An epic task: its record lands by the epic's planning change, never
    // on the task branch (TSK-184 admits only a standalone record there).
    hosted.seed();
    let out = codeflow(root, &["epic", "new", "an outcome"]);
    assert!(out.contains("EPC-001"), "{out}");
    set_field(root, TASK, "epic_id", "EPC-001");
    set_field(root, TASK, "standalone_reason", "null");
    commit_all(root, "docs: plan the epic");
    git(root, &["switch", "-q", "-c", "task/TSK-001-work"]);
    commit(root, "x.txt", "feat: add x");
    prove(
        "WORK_START_MERGE_PLANNING",
        "is not present at the merge-base",
        || hosted.ci("task/TSK-001-work", &[]),
        |printed| {
            hosted.land("plan/work");
            let step = printed_command(printed, "WORK_START_MERGE_PLANNING", None);
            run_printed(root, &step, &[], &[]);
        },
    );
}

#[test]
fn clears_task_record_missing() {
    // A task on the target turns durable-work tracking on, and a
    // maintainer seeds the registry new ids are issued from.
    let hosted = registered();
    let root = &hosted.root;
    // Work begins on a branch for a task nobody planned.
    hosted.branch("task/TSK-002-more");
    commit(root, "x.txt", "feat: add x");
    prove(
        "TASK_RECORD_MISSING",
        "does not identify a visible task record",
        || hosted.ci("task/TSK-002-more", &[]),
        |printed| {
            hosted.branch("plan/more");
            let step = printed_command(printed, "TASK_RECORD_MISSING", None);
            let out = run_printed(
                root,
                &step,
                &[],
                &["--standalone-reason", "a fixture", "more work"],
            );
            assert!(out.contains("TSK-002"), "{out}");
            commit_all(root, "docs: plan more work");
            hosted.land("plan/more");
            git(root, &["switch", "-q", "task/TSK-002-more"]);
            git(root, &["merge", "-q", "--no-edit", &hosted.upstream()]);
        },
    );
}

#[test]
fn clears_id_registry() {
    let hosted = registered();
    let root = &hosted.root;
    // A second task written by hand, outside the registry.
    hosted.branch("plan/more");
    let hand = "project-management/tasks/TSK-002.md";
    let copied: String = read(root, TASK)
        .lines()
        .map(|line| {
            if line.starts_with("uid:") {
                "uid: 3f0c5a52-2d7e-4b8e-9d57-5d0b8f0f1a11".to_string()
            } else {
                line.replace("TSK-001", "TSK-002")
                    .replace("the work", "more work")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    write(root, hand, &format!("{copied}\n"));
    commit_all(root, "docs: plan more work");
    prove(
        "ID_REGISTRY",
        "TSK-002: not reserved in the registry",
        || hosted.ci("plan/more", &[]),
        |printed| {
            let step = printed_command(printed, "ID_REGISTRY", None);
            assert_eq!(step, format!("codeflow ids admit {hand}"));
            run_printed(root, &step, &[], &[]);
        },
    );
}

#[test]
fn clears_acceptance_binding() {
    let hosted = Hosted::new();
    let root = &hosted.root;
    hosted.plan_task("plan/work");
    hosted.land("plan/work");
    hosted.branch("task/TSK-001-work");
    commit(root, "x.txt", "feat: add x");
    let acceptance = review_head(root);
    let out = codeflow(
        root,
        &[
            "task",
            "status",
            "TSK-001",
            "complete",
            "--acceptance",
            &acceptance,
        ],
    );
    assert!(out.contains("todo -> complete"), "{out}");
    commit_all(root, "chore: complete the work");
    // Work pushed after the review: the reviewed commit is not the head.
    commit(root, "y.txt", "feat: add y");
    prove(
        "ACCEPTANCE_BINDING",
        "changed after the reviewed commit",
        || hosted.ci("task/TSK-001-work", &[]),
        |printed| {
            // Review the pull request head, then take the printed steps in
            // order: reopen the task, and complete it with the new review.
            let acceptance = review_head(root);
            let steps: Vec<&str> = printed
                .split('`')
                .skip(1)
                .step_by(2)
                .filter(|span| span.starts_with("codeflow task status"))
                .collect();
            assert_eq!(steps.len(), 2, "{printed}");
            assert_eq!(
                steps[0],
                printed_command(printed, "ACCEPTANCE_BINDING", None)
            );
            for step in steps {
                run_printed(
                    root,
                    step,
                    &[("<id>", "TSK-001"), ("<file>", &acceptance)],
                    &[],
                );
            }
            commit_all(root, "chore: record the review of the head");
        },
    );
}

/// An epic acceptance block for AC-1 reviewing `reviewed`, written beside
/// the project; returns its path.
fn epic_block(root: &Path, reviewed: &str, ac1: &str) -> String {
    let path = root.parent().unwrap().join("epic-acceptance.yaml");
    std::fs::write(&path, epic_yaml(reviewed, ac1)).unwrap();
    path.to_str().unwrap().to_string()
}

fn epic_yaml(reviewed: &str, ac1: &str) -> String {
    format!(
        "acceptance:\n  reviewed: {reviewed}\n  review: session:proof@sha256:00\n  criteria:\n    \
         AC-1: {ac1}\n  journey: none | no journey criterion\n  not_verified: none\n  \
         follow_ups: none: a fixture\n  verdict: approved\n"
    )
}

/// Plan epic `id` with one criterion no task serves, committed; returns
/// its record path.
fn plan_epic(root: &Path, title: &str) -> String {
    let out = codeflow(root, &["epic", "new", title]);
    // The printed path is native: Windows separates it with `\`.
    let path = out
        .split_whitespace()
        .map(|word| word.replace('\\', "/"))
        .find(|word| word.contains("project-management/epics/"))
        .unwrap_or_else(|| panic!("no epic path in:\n{out}"));
    let rel = path[path.find("project-management/").unwrap()..].to_string();
    let epic =
        read(root, &rel).replacen("- AC-1\n", "- AC-1 When used, the system shall work.\n", 1);
    write(root, &rel, &epic);
    commit_all(root, &format!("docs: plan {title}"));
    rel
}

/// TSK-214: an epic's own block that does not bind prints the epic's
/// route, never a task's reopen. A completed epic is repaired by replacing
/// its block in the Closeout, as printed; an open epic is closed again by
/// the printed `codeflow epic status` command.
#[test]
fn clears_epic_acceptance_binding() {
    let hosted = Hosted::new();
    let root = &hosted.root;
    // A task record turns durable tracking on in a standard project.
    hosted.plan_task("plan/outcome");
    hosted.seed();
    let epic = plan_epic(root, "the outcome");
    let planned = rev(root, "HEAD");

    // An epic completed by hand with a waiver that names no commit.
    let closed = read(root, &epic).replacen("status: draft", "status: complete", 1);
    let closed = closed.replacen("status: planning", "status: complete", 1);
    write(
        root,
        &epic,
        &format!(
            "{closed}\n## Closeout\n\n```yaml\n{}```\n",
            epic_yaml(&planned, "waived | deadbee")
        ),
    );
    commit_all(root, "docs: close the epic by hand");
    prove(
        "EPIC_ACCEPTANCE_BINDING",
        "AC-1 waiver names deadbee",
        || hosted.ci("plan/outcome", &[]),
        |printed| {
            assert!(!printed.contains("codeflow task status"), "{printed}");
            assert!(
                printed.contains(&format!("Closeout of {epic}")),
                "{printed}"
            );
            // The printed route for a completed epic: replace the block in
            // its Closeout with a corrected one, reviewed at the plan.
            let text = read(root, &epic);
            let start = text.find("```yaml\n").unwrap();
            let fixed = format!(
                "{}```yaml\n{}```\n",
                &text[..start],
                epic_yaml(&planned, "verified | the plan's criterion holds")
            );
            write(root, &epic, &fixed);
            commit_all(root, "docs: correct the epic's acceptance block");
        },
    );
    let after = hosted.ci("plan/outcome", &[]);
    // The corrected completed epic is clean under every records check.
    assert!(after.contains("codeflow ci: clean"), "{after}");

    // An open epic: the verb refuses the block and prints the route; the
    // printed command closes it with the corrected block.
    let open = plan_epic(root, "the second outcome");
    let head = rev(root, "HEAD");
    let bad = epic_block(root, &head, "waived | deadbee");
    let refused = codeflow(
        root,
        &[
            "epic",
            "status",
            "EPC-002",
            "complete",
            "--acceptance",
            &bad,
        ],
    );
    assert!(refused.contains("AC-1 waiver names deadbee"), "{refused}");
    assert!(!refused.contains("codeflow task status"), "{refused}");
    let step = printed_command(
        block(&refused, "clear it:"),
        "EPIC_ACCEPTANCE_BINDING",
        Some("codeflow epic status"),
    );
    let good = epic_block(root, &head, "verified | the plan's criterion holds");
    let said = run_printed(root, &step, &[("<id>", "EPC-002"), ("<file>", &good)], &[]);
    assert!(said.contains("-> complete"), "{said}");
    assert!(read(root, &open).contains("status: complete"));
}

#[test]
fn clears_journey_criterion() {
    let hosted = Hosted::new();
    let root = &hosted.root;
    hosted.plan_task("plan/work");
    hosted.land("plan/work");
    let body = hosted.dir.path().join("body.md");
    std::fs::write(&body, BODY).unwrap();
    let body = body.to_str().unwrap().to_string();
    hosted.branch("task/TSK-001-work");
    // Managed instructions are adopter-facing.
    let agents = read(root, "AGENTS.md");
    write(
        root,
        "AGENTS.md",
        &format!("{agents}\nNotes for adopters.\n"),
    );
    commit_all(root, "docs: add notes for adopters");
    prove(
        "JOURNEY_CRITERION",
        "has no `(journey)` criterion",
        || hosted.ci("task/TSK-001-work", &["--pr-body-file", &body]),
        |printed| {
            assert!(
                printed.contains(&format!("criterion to {TASK}")),
                "{printed}"
            );
            // The criterion lands by a planning pull request; the task
            // branch then takes the target.
            hosted.branch("plan/journey");
            let task = read(root, TASK).replacen(
                CRITERION,
                "- AC-1 When an adopter reads AGENTS.md, the notes are there (journey)\n",
                1,
            );
            write(root, TASK, &task);
            commit_all(root, "docs: add a journey criterion");
            hosted.land("plan/journey");
            git(root, &["switch", "-q", "task/TSK-001-work"]);
            git(root, &["merge", "-q", "--no-edit", &hosted.upstream()]);
        },
    );
}

#[test]
fn clears_baseline_review() {
    let hosted = Hosted::new();
    let root = &hosted.root;
    hosted.plan_task("plan/work");
    record_baseline(root);
    prove(
        "BASELINE_REVIEW",
        "introduces work_records_baseline",
        || hosted.ci("plan/work", &[]),
        |printed| {
            assert!(printed.contains(".codeflow/project.toml"), "{printed}");
            // The reviewer approves the list, and the change lands.
            hosted.land("plan/work");
        },
    );
}

// ---------------------------------------------------------------------------
// The id registry's merge rule, each message with its own step.
// ---------------------------------------------------------------------------

/// A hosted project whose registry is seeded with TSK-001 on the target.
fn registered() -> Hosted {
    let hosted = Hosted::new();
    hosted.plan_task("plan/work");
    hosted.land("plan/work");
    hosted.seed();
    hosted
}

/// Issue the next task id from the registry on a new `branch`, committed;
/// returns the command's output.
fn issue_task(hosted: &Hosted, branch: &str, title: &str) -> String {
    hosted.branch(branch);
    let out = codeflow(
        &hosted.root,
        &["task", "new", "--standalone-reason", "a fixture", title],
    );
    commit_all(&hosted.root, &format!("docs: plan {title}"));
    out
}

#[test]
fn clears_id_registry_unfetched() {
    let hosted = registered();
    let out = issue_task(&hosted, "plan/more", "more work");
    assert!(out.contains("TSK-002"), "{out}");
    // The pull request branch reaches the platform. Model a CI checkout
    // whose fetch rule omits the registry tracking ref.
    git(
        &hosted.dest,
        &[
            "fetch",
            "-q",
            hosted.root.to_str().unwrap(),
            "plan/more:plan/more",
        ],
    );
    let clone = hosted.dir.path().join("checkout");
    codeflow_fixture::clone(
        hosted.dir.path(),
        hosted.dest.to_str().unwrap(),
        clone.to_str().unwrap(),
    )
    .branch("plan/more")
    .env("GIT_CONFIG_GLOBAL", "/dev/null")
    .env("GIT_CONFIG_SYSTEM", "/dev/null")
    .run();
    // Fetch only the selected branch by default and remove the registry ref.
    git(
        &clone,
        &[
            "config",
            "remote.origin.fetch",
            "+refs/heads/plan/more:refs/remotes/origin/plan/more",
        ],
    );
    git(
        &clone,
        &["update-ref", "-d", "refs/remotes/origin/codeflow/registry"],
    );
    let upstream = hosted.upstream();
    git(
        &clone,
        &[
            "fetch",
            "-q",
            "origin",
            &format!("{}:refs/remotes/{upstream}", hosted.target),
        ],
    );
    let check = || {
        codeflow(
            &clone,
            &[
                "ci",
                "--base",
                &upstream,
                "--head",
                "HEAD",
                "--branch",
                "plan/more",
            ],
        )
    };
    prove(
        "ID_REGISTRY_UNFETCHED",
        "no `codeflow/registry` was fetched",
        check,
        |printed| {
            let step = printed_command(printed, "ID_REGISTRY_UNFETCHED", None);
            assert_eq!(
                step,
                "git fetch origin codeflow/registry:refs/remotes/origin/codeflow/registry"
            );
            run_printed(&clone, &step, &[], &["-q"]);
        },
    );
}

#[test]
fn clears_id_registry_retarget() {
    let hosted = registered();
    let root = &hosted.root;
    // One branch is issued TSK-002; another writes its own TSK-002.
    let out = issue_task(&hosted, "plan/theirs", "their work");
    assert!(out.contains("TSK-002"), "{out}");
    hosted.branch("plan/ours");
    let ours = "project-management/tasks/TSK-002.md";
    let forked: String = read(root, TASK)
        .lines()
        .map(|line| {
            if line.starts_with("uid:") {
                "uid: 6d1f0a3e-8c2b-4f5a-9e7d-2b4c6a8e0f13".to_string()
            } else {
                line.replace("TSK-001", "TSK-002")
                    .replace("the work", "our work")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    write(root, ours, &format!("{forked}\n"));
    commit_all(root, "docs: plan our work");
    prove(
        "ID_REGISTRY_RETARGET",
        "TSK-002 also exists on plan/theirs as a different record",
        || hosted.ci("plan/ours", &[]),
        |printed| {
            let step = printed_command(printed, "ID_REGISTRY_RETARGET", None);
            let out = run_printed(root, &step, &[("<id>", "TSK-002")], &[]);
            assert!(out.contains("TSK-003"), "{out}");
            commit_all(root, "docs: renumber our work");
        },
    );
}

#[test]
fn clears_id_registry_uid() {
    let hosted = registered();
    let root = &hosted.root;
    hosted.branch("plan/edit");
    set_field(root, TASK, "uid", "0b7e4c2a-5d3f-4a1b-8c9e-7f6a5b4c3d21");
    commit_all(root, "docs: edit the task");
    prove(
        "ID_REGISTRY_UID",
        "the range edits the uid of an existing record",
        || hosted.ci("plan/edit", &[]),
        |printed| {
            let step = printed_command(printed, "ID_REGISTRY_UID", None);
            run_printed(
                root,
                &step,
                &[("<target>", &hosted.upstream()), ("<path>", TASK)],
                &[],
            );
            commit_all(root, "docs: restore the task's uid");
        },
    );
}
