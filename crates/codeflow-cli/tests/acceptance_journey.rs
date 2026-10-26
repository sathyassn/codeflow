//! Journey (TSK-105 AC-7): on a fresh `codeflow init --full` project, a task
//! completed with a valid acceptance block passes `task status complete` and
//! `codeflow ci`, and each fault fails for its stated reason: a stale block
//! after a later code change, a waiver without its planning amendment,
//! another task's criteria changed on the task branch, and an adopter-facing range whose
//! task has no journey. The binary under test is the one Cargo built; the
//! installed `codeflow` on `PATH` is never used.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[path = "support/process_folders.rs"]
mod process_folders;
use process_folders::isolated_home;

fn with_env(command: &mut Command) -> &mut Command {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(exe.parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("joinable PATH");
    command
        .env("CODEFLOW_HOME", isolated_home())
        .env("PATH", path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Journey")
        .env("GIT_AUTHOR_EMAIL", "journey@example.test")
        .env("GIT_COMMITTER_NAME", "Journey")
        .env("GIT_COMMITTER_EMAIL", "journey@example.test")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE")
        .env_remove("CODEFLOW_PR_BODY")
        .env_remove("GITHUB_BASE_REF")
        .env_remove("GITHUB_HEAD_REF")
        .env_remove("GITHUB_EVENT_NAME")
        // A caller's agent session must not judge the fixture's own
        // commits (`git.root_checkout_commits` reads these markers).
        .env_remove("CLAUDECODE")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CODEX_THREAD_ID")
        .env_remove("CODEX_CI")
        .env_remove("GROK_SESSION_ID")
}

fn codeflow(root: &Path, args: &[&str]) -> Output {
    with_env(&mut Command::new(env!("CARGO_BIN_EXE_codeflow")))
        .args(args)
        .current_dir(root)
        .output()
        .expect("codeflow runs")
}

fn git(root: &Path, args: &[&str]) -> String {
    let out = with_env(&mut Command::new("git"))
        .args(args)
        .current_dir(root)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn ok(out: &Output, what: &str) -> String {
    assert!(out.status.success(), "{what} failed:\n{}", text(out));
    text(out)
}

fn fails(out: &Output, what: &str, reason: &str) {
    assert_eq!(out.status.code(), Some(1), "{what}:\n{}", text(out));
    assert!(
        text(out).contains(reason),
        "{what}: missing {reason:?} in\n{}",
        text(out)
    );
}

fn commit(root: &Path, message: &str) -> String {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", message]);
    git(root, &["rev-parse", "HEAD"])
}

fn edit(root: &Path, relative: &str, from: &str, to: &str) {
    let path = root.join(relative);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(from), "{relative} lacks {from:?}");
    std::fs::write(path, text.replacen(from, to, 1)).unwrap();
}

const TASK: &str = "project-management/tasks/TSK-001.md";
const SECOND: &str = "project-management/tasks/TSK-002.md";
const EPIC: &str = "project-management/epics/EPC-001.md";
const LINE: &str = "integration/EPC-001-journey";
const SCOPE: &str = "the acceptance check proves structure and binding only";

/// An acceptance block for TSK-001 reviewed at `reviewed`, with AC-1's
/// outcome given.
fn acceptance(dir: &Path, reviewed: &str, ac1: &str) -> String {
    let body = format!(
        "acceptance:\n  reviewed: {reviewed}\n  review: session:journey@sha256:00\n  criteria:\n    AC-1: {ac1}\n    AC-2: verified | the journey ran\n  journey: verified | crates/codeflow-cli/tests/acceptance_journey.rs\n  not_verified: none\n  follow_ups: none: journey fixture\n  verdict: approved\n"
    );
    let path = dir.join("acceptance.yaml");
    std::fs::write(&path, body).unwrap();
    path.to_string_lossy().to_string()
}

/// The project's installed pull request template, filled for `task`.
fn body(root: &Path, task: &str) -> String {
    std::fs::read_to_string(root.join(".github/pull_request_template.md"))
        .unwrap()
        .replace("Task: `TSK-NNN | none: <reason>`", &format!("Task: {task}"))
        .replace(
            "Task: `TSK-NNN | EPC-NNN | EPC-001, EPC-002 | <unit name>`",
            &format!("Task: {task}"),
        )
        .replace(
            "## Summary\n",
            "## Summary\n\nBuild the task.\n\n- build the task\n",
        )
        .replace("\n-\n", "\n- Build the task.\n")
        .replace(
            "- Revision and command:",
            "- Revision and command: HEAD, cargo test",
        )
        .replace(
            "(paste the real test summary output here)",
            "Ran 1 test\nOK",
        )
        .replace("- Coverage:", "- Coverage: not measured for this journey")
        .replace("- New tests:", "- New tests: none")
        .replace("- Not tested:", "- Not tested: Windows")
        .replace(
            "|  |  |  |",
            &review_rows(root, "HEAD", "| Reviewer | HEAD | approved |"),
        )
        .replace(
            "- Impact: `none | patch | minor | major`",
            "- Impact: minor",
        )
        .replace("- Breaking: `yes | no`", "- Breaking: no")
        .replace("- Rationale:", "- Rationale: a new command.")
        .replace(
            "- Migration: `none`, steps, or \"see Breaking change\"",
            "- Migration: none",
        )
}

/// `codeflow ci` on the range from the line, as the pull request job runs
/// it, with the installed template filled for `task`.
fn ci(root: &Path, branch: &str, task: &str) -> Output {
    let body = body(root, task);
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
            &body,
        ],
    )
}

#[test]
#[allow(clippy::too_many_lines)] // One journey keeps the faults in the order a project meets them.
fn a_completion_is_bound_to_the_reviewed_commit_on_a_fresh_project() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    ok(
        &codeflow(&root, &["init", "--yes", "--full"]),
        "init --full",
    );
    git(&root, &["switch", "-q", "-c", LINE]);

    // Plan: an epic and two tasks; the first has a journey criterion, the
    // second has none. `src/**` is product code.
    git(&root, &["switch", "-q", "-c", "plan/acceptance"]);
    ok(&codeflow(&root, &["epic", "new", "outcome"]), "epic new");
    for title in ["first", "second"] {
        let args = ["task", "new", "--epic", "EPC-001", "--into", LINE, title];
        ok(&codeflow(&root, &args), "task new");
    }
    edit(
        &root,
        EPIC,
        "- AC-1\n",
        "- AC-1 When used, the system shall work.\n",
    );
    edit(
        &root,
        TASK,
        "- AC-1\n",
        "- AC-1 When run, the system shall work.\n- AC-2 (journey) On a fresh project, the command shall succeed.\n",
    );
    edit(
        &root,
        SECOND,
        "- AC-1\n",
        "- AC-1 When run, the system shall work.\n",
    );
    let policy_path = root.join(".codeflow/policy.json");
    let mut policy: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&policy_path).unwrap()).unwrap();
    policy["git"]["product_paths"] = serde_json::json!(["src/**"]);
    std::fs::write(
        &policy_path,
        serde_json::to_string_pretty(&policy).unwrap() + "\n",
    )
    .unwrap();
    commit(&root, "chore: plan the acceptance journey");
    git(&root, &["switch", "-q", LINE]);
    git(
        &root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "plan/acceptance",
            "-m",
            "chore: land the plan",
        ],
    );
    let planned = git(&root, &["rev-parse", "HEAD"]);

    // A planning amendment on the line narrows TSK-001 AC-1.
    git(&root, &["switch", "-q", "-c", "plan/narrow"]);
    edit(&root, TASK, "shall work.\n", "shall work on Linux.\n");
    let amendment = commit(&root, "chore: narrow TSK-001 AC-1");
    git(&root, &["switch", "-q", LINE]);
    git(
        &root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "plan/narrow",
            "-m",
            "chore: land the amendment",
        ],
    );

    // The task: build, review, complete with a valid block. CI passes and
    // states what it proves.
    let branch = "task/TSK-001-first";
    git(&root, &["switch", "-q", "-c", branch]);
    ok(
        &codeflow(&root, &["work", "start", "TSK-001"]),
        "work start",
    );
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn first() {}\n").unwrap();
    let reviewed = commit(&root, "feat: add the first function");
    let valid = acceptance(dir.path(), &reviewed, &format!("waived | {amendment}"));
    ok(
        &codeflow(
            &root,
            &[
                "task",
                "status",
                "TSK-001",
                "complete",
                "--acceptance",
                &valid,
            ],
        ),
        "task status complete with a valid block",
    );
    let completed = commit(&root, "chore: complete TSK-001");
    let passed = ok(&ci(&root, branch, "TSK-001"), "ci on the valid completion");
    assert!(passed.contains(SCOPE), "{passed}");

    // Fault: a later code change makes the block stale.
    std::fs::write(root.join("src/lib.rs"), "pub fn changed() {}\n").unwrap();
    commit(&root, "feat: change after review");
    fails(
        &ci(&root, branch, "TSK-001"),
        "a stale block after a later code change",
        "src/lib.rs changed after the reviewed commit",
    );
    git(&root, &["reset", "-q", "--hard", &completed]);

    // Fault: a waiver naming a commit that is not the amendment on the
    // target; the verb refuses before it writes.
    git(&root, &["reset", "-q", "--hard", &reviewed]);
    // The commit that landed the plan is on the target, but it also changed
    // the policy, so it is not a planning amendment (a target commit that
    // changes records only but not AC-1 is covered in `acceptance_cli`).
    let unamended = acceptance(dir.path(), &reviewed, &format!("waived | {planned}"));
    let refused = codeflow(
        &root,
        &[
            "task",
            "status",
            "TSK-001",
            "complete",
            "--acceptance",
            &unamended,
        ],
    );
    fails(
        &refused,
        "a waiver without its amendment",
        "which also changes .codeflow/policy.json; a planning amendment changes planning records only",
    );
    assert!(text(&refused).contains(SCOPE), "{}", text(&refused));
    let branch_only = acceptance(dir.path(), &reviewed, &format!("waived | {reviewed}"));
    fails(
        &codeflow(
            &root,
            &[
                "task",
                "status",
                "TSK-001",
                "complete",
                "--acceptance",
                &branch_only,
            ],
        ),
        "a waiver naming a branch commit",
        "strictly before the reviewed revision",
    );
    let record = std::fs::read_to_string(root.join(TASK)).unwrap();
    assert!(!record.contains("\nstatus: complete"), "{record}");

    // The task branch may change its own criteria before completion.
    edit(
        &root,
        TASK,
        "the command shall succeed",
        "the command may succeed",
    );
    commit(&root, "chore: loosen AC-2");
    let amended = ok(&ci(&root, branch, "TSK-001"), "own task criteria amendment");
    assert!(amended.contains("AC-2"), "{amended}");

    // Fault: an adopter-facing range whose task has no journey.
    git(&root, &["switch", "-q", "-c", "task/TSK-002-second", LINE]);
    ok(
        &codeflow(&root, &["work", "start", "TSK-002"]),
        "work start",
    );
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/second.rs"), "pub fn second() {}\n").unwrap();
    commit(&root, "feat: add the second function");
    fails(
        &ci(&root, "task/TSK-002-second", "TSK-002"),
        "an adopter-facing range without a journey",
        "TSK-002 changes the adopter-facing path set but has no `(journey)` criterion",
    );
}

/// TSK-152 AC-4 as its record lists it: the tag closes the sentence, so a
/// period follows it.
const TSK152_AC4: &str = "- AC-4 When a fresh repository is scaffolded with the built binary, a
  minimal-tier `AGENTS.md` names the `.codex/` starter as for interactive
  Codex and `.grok/hooks/` as for Grok Build, and a standard-tier install
  carries the restored present method byte for byte; evidence: the scaffold
  run's output files compared with their sources, or the `init_e2e` case
  that renders them (journey).
";

/// Journey (TSK-155 AC-5): on a fresh `codeflow init --full` project, a
/// task whose journey criterion is TSK-152's real AC-4 text, tag then
/// period, completes and passes `codeflow ci` on an adopter-facing range; a
/// tag followed by a comma passes too, and a tag inside the text is still
/// refused as no journey.
#[test]
fn a_journey_tag_reads_with_sentence_punctuation_on_a_fresh_project() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    ok(
        &codeflow(&root, &["init", "--yes", "--full"]),
        "init --full",
    );
    git(&root, &["switch", "-q", "-c", LINE]);
    git(&root, &["switch", "-q", "-c", "plan/tags"]);
    ok(&codeflow(&root, &["epic", "new", "outcome"]), "epic new");
    for title in ["period", "comma", "inside"] {
        let args = ["task", "new", "--epic", "EPC-001", "--into", LINE, title];
        ok(&codeflow(&root, &args), "task new");
    }
    edit(
        &root,
        EPIC,
        "- AC-1\n",
        "- AC-1 When used, the system shall work.\n",
    );
    let first = format!("- AC-1 When run, the system shall work.\n{TSK152_AC4}");
    edit(&root, TASK, "- AC-1\n", &first);
    edit(
        &root,
        SECOND,
        "- AC-1\n",
        "- AC-1 On a fresh project, the command shall succeed (journey),\n",
    );
    edit(
        &root,
        "project-management/tasks/TSK-003.md",
        "- AC-1\n",
        "- AC-1 When run, the (journey) path shall pass.\n",
    );
    let policy_path = root.join(".codeflow/policy.json");
    let mut policy: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&policy_path).unwrap()).unwrap();
    policy["git"]["product_paths"] = serde_json::json!(["src/**"]);
    std::fs::write(
        &policy_path,
        serde_json::to_string_pretty(&policy).unwrap() + "\n",
    )
    .unwrap();
    commit(&root, "chore: plan the tag journey");
    git(&root, &["switch", "-q", LINE]);
    git(
        &root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "plan/tags",
            "-m",
            "chore: land the plan",
        ],
    );

    // TSK-152's AC-4 text: built, completed with its journey verified, and
    // judged on an adopter-facing range.
    let branch = "task/TSK-001-period";
    git(&root, &["switch", "-q", "-c", branch]);
    ok(
        &codeflow(&root, &["work", "start", "TSK-001"]),
        "work start",
    );
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn first() {}\n").unwrap();
    let reviewed = commit(&root, "feat: add the first function");
    let block = format!(
        "acceptance:\n  reviewed: {reviewed}\n  review: session:journey@sha256:00\n  criteria:\n    AC-1: verified | the journey ran\n    AC-4: verified | the journey ran\n  journey: verified | crates/codeflow-cli/tests/acceptance_journey.rs\n  not_verified: none\n  follow_ups: none: journey fixture\n  verdict: approved\n"
    );
    let block_path = dir.path().join("acceptance.yaml");
    std::fs::write(&block_path, block).unwrap();
    ok(
        &codeflow(
            &root,
            &[
                "task",
                "status",
                "TSK-001",
                "complete",
                "--acceptance",
                &block_path.to_string_lossy(),
            ],
        ),
        "task status complete for TSK-152's AC-4 text",
    );
    commit(&root, "chore: complete TSK-001");
    let passed = ok(&ci(&root, branch, "TSK-001"), "ci with `(journey).`");
    assert!(!passed.contains("work.journey_criterion"), "{passed}");

    // A comma after the tag reads the same.
    let branch = "task/TSK-002-comma";
    git(&root, &["switch", "-q", "-c", branch, LINE]);
    ok(
        &codeflow(&root, &["work", "start", "TSK-002"]),
        "work start",
    );
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/second.rs"), "pub fn second() {}\n").unwrap();
    commit(&root, "feat: add the second function");
    let passed = ok(&ci(&root, branch, "TSK-002"), "ci with `(journey),`");
    assert!(!passed.contains("work.journey_criterion"), "{passed}");

    // A tag inside the text is not a tag.
    let branch = "task/TSK-003-inside";
    git(&root, &["switch", "-q", "-c", branch, LINE]);
    ok(
        &codeflow(&root, &["work", "start", "TSK-003"]),
        "work start",
    );
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/third.rs"), "pub fn third() {}\n").unwrap();
    commit(&root, "feat: add the third function");
    let inside = ci(&root, branch, "TSK-003");
    fails(
        &inside,
        "a tag inside the criterion text",
        "TSK-003 changes the adopter-facing path set but has no `(journey)` criterion",
    );
    // The refusal names the criterion and the rule it missed (TSK-223 AC-2).
    assert!(
        text(&inside).contains(
            "AC-1 carries `(journey)` inside its text, and the tag counts only where it opens or closes the criterion (R-50)"
        ),
        "{}",
        text(&inside)
    );
}

/// Journey (TSK-382 AC-4, issue #199): on a fresh `codeflow init --full`
/// project whose epic journey criterion carries amendment notes, a task
/// serving it passes `codeflow ci` on an adopter-facing range, and so does
/// a task whose own journey criterion carries one; a note that is no
/// amendment still hides the tag, and the refusal names it.
#[test]
#[allow(clippy::too_many_lines)] // One journey keeps the three tasks in the order a project meets them.
fn an_amended_journey_criterion_still_counts_on_a_fresh_project() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    ok(
        &codeflow(&root, &["init", "--yes", "--full"]),
        "init --full",
    );
    git(&root, &["switch", "-q", "-c", LINE]);
    git(&root, &["switch", "-q", "-c", "plan/amended"]);
    ok(&codeflow(&root, &["epic", "new", "outcome"]), "epic new");
    for title in ["serving", "own", "hidden"] {
        let args = ["task", "new", "--epic", "EPC-001", "--into", LINE, title];
        ok(&codeflow(&root, &args), "task new");
    }
    edit(
        &root,
        EPIC,
        "- AC-1\n",
        "- AC-1 On a fresh project, the release shall install (journey)\n  - Amended 2026-10-12 (plan version 4): reworded for the installer.\n  - Moved 2026-10-13 from AC-4.\n",
    );
    edit(
        &root,
        TASK,
        "- AC-1\n",
        "- AC-1 The installer shall run (serves EPC-001 AC-1)\n",
    );
    edit(
        &root,
        SECOND,
        "- AC-1\n",
        "- AC-1 On a fresh project, the command shall succeed (journey)\n  - Amended 2026-10-14 (plan version 5): narrowed to Linux.\n",
    );
    edit(
        &root,
        "project-management/tasks/TSK-003.md",
        "- AC-1\n",
        "- AC-1 On a fresh project, the command shall succeed (journey)\n  - Note: reworded after review.\n",
    );
    let policy_path = root.join(".codeflow/policy.json");
    let mut policy: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&policy_path).unwrap()).unwrap();
    policy["git"]["product_paths"] = serde_json::json!(["src/**"]);
    std::fs::write(
        &policy_path,
        serde_json::to_string_pretty(&policy).unwrap() + "\n",
    )
    .unwrap();
    commit(&root, "chore: plan the amended journey");
    git(&root, &["switch", "-q", LINE]);
    git(
        &root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "plan/amended",
            "-m",
            "chore: land the plan",
        ],
    );

    // A task serving the amended epic journey criterion, and a task whose
    // own journey criterion carries an amendment note.
    for (task, branch, file) in [
        ("TSK-001", "task/TSK-001-serving", "src/first.rs"),
        ("TSK-002", "task/TSK-002-own", "src/second.rs"),
    ] {
        git(&root, &["switch", "-q", "-c", branch, LINE]);
        ok(&codeflow(&root, &["work", "start", task]), "work start");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join(file), "pub fn added() {}\n").unwrap();
        commit(&root, "feat: add a function");
        let passed = ok(&ci(&root, branch, task), "ci with an amended journey");
        assert!(!passed.contains("work.journey_criterion"), "{passed}");
    }

    // A note that is no amendment still hides the closing tag.
    let branch = "task/TSK-003-hidden";
    git(&root, &["switch", "-q", "-c", branch, LINE]);
    ok(
        &codeflow(&root, &["work", "start", "TSK-003"]),
        "work start",
    );
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/third.rs"), "pub fn third() {}\n").unwrap();
    commit(&root, "feat: add the third function");
    let hidden = ci(&root, branch, "TSK-003");
    fails(
        &hidden,
        "a closing tag hidden by a note",
        "the note \"- Note: reworded after review.\" under AC-1 hides its `(journey)` tag; move the tag to the start of the criterion (R-50)",
    );
}

/// An acceptance block for a two-criterion record whose AC-2 is its
/// journey, with AC-1's outcome given and `TSK-008` as the follow-up.
fn round_acceptance(dir: &Path, reviewed: &str, ac1: &str) -> String {
    let body = format!(
        "acceptance:\n  reviewed: {reviewed}\n  review: session:journey@sha256:00\n  criteria:\n    AC-1: {ac1}\n    AC-2: verified | the journey ran\n  journey: verified | crates/codeflow-cli/tests/acceptance_journey.rs\n  not_verified: none\n  follow_ups: TSK-008\n  verdict: approved\n"
    );
    let path = dir.join("round.yaml");
    std::fs::write(&path, body).unwrap();
    path.to_string_lossy().to_string()
}

/// `codeflow task status <task> complete` with AC-1's outcome given.
fn complete(root: &Path, dir: &Path, task: &str, reviewed: &str, ac1: &str) -> Output {
    let block = round_acceptance(dir, reviewed, ac1);
    codeflow(
        root,
        &["task", "status", task, "complete", "--acceptance", &block],
    )
}

const DEFERRED: &str = "deferred | owner: ops; window: a month; follow-up: TSK-008";

/// TSK-382 review round 1, on a fresh `codeflow init --full` project: a
/// note is a dash sub-bullet whose first word is exactly `Amended` or
/// `Moved`, followed by a date; any other sub-bullet stays criterion text
/// and is named when it hides a tag; a reflow that moves a tag past a note
/// is a criteria delta for the task's own record and frozen for another's;
/// and `task status complete` reads the tags the same way.
#[test]
#[allow(clippy::too_many_lines)] // One journey keeps the review's inputs in the order it gave them.
fn the_review_inputs_are_judged_as_stated_on_a_fresh_project() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    ok(
        &codeflow(&root, &["init", "--yes", "--full"]),
        "init --full",
    );
    git(&root, &["switch", "-q", "-c", LINE]);
    git(&root, &["switch", "-q", "-c", "plan/round"]);
    ok(&codeflow(&root, &["epic", "new", "outcome"]), "epic new");
    let journey = "- AC-2 (journey) On a fresh project, the command shall succeed.\n";
    let after_release = format!(
        "- AC-1 Prove the metric now (after release) - Amended 2026-10-14: the rest of the sentence.\n{journey}"
    );
    let tasks = [
        (
            "reflow-journey",
            "- AC-1 The command shall pass (journey) - Amended 2026-10-14: keep the log.\n"
                .to_string(),
        ),
        ("reflow-after", after_release.clone()),
        ("other", after_release.clone()),
        (
            "moved-words",
            "- AC-1 On a fresh project, the command shall succeed (journey)\n  - Moved files shall stay listed in the report.\n".to_string(),
        ),
        (
            "bold",
            "- AC-1 On a fresh project, the command shall succeed (journey)\n  - **Amended** 2026-10-14: bold word.\n".to_string(),
        ),
        (
            "split",
            format!("- AC-1 Look fine (after release)\n  - Amended 2026-10-18: split the proof.\n  - The command shall pass the suite before release (journey).\n{}", journey.replace("(journey) ", "")),
        ),
        (
            "serves-note",
            "- AC-1 The installer shall run.\n  - Amended 2026-10-14: now (serves EPC-001 AC-1).\n".to_string(),
        ),
        ("follow", "- AC-1 The metric shall be read.\n".to_string()),
        (
            "serves-later",
            "- AC-1 The installer shall run (serves EPC-001 AC-1)\n  - Amended 2026-10-14: dated note.\n  - The command shall print the plan.\n".to_string(),
        ),
        (
            "serves-inside",
            "- AC-1 The installer (serves EPC-001 AC-1) shall run.\n".to_string(),
        ),
        (
            "serves-noted",
            "- AC-1 The installer shall run (serves EPC-001 AC-1)\n  - Amended 2026-10-14: dated note.\n".to_string(),
        ),
    ];
    for (index, (title, criteria)) in tasks.iter().enumerate() {
        let args = ["task", "new", "--epic", "EPC-001", "--into", LINE, title];
        ok(&codeflow(&root, &args), "task new");
        edit(
            &root,
            &format!("project-management/tasks/TSK-{:03}.md", index + 1),
            "- AC-1\n",
            criteria,
        );
    }
    edit(
        &root,
        EPIC,
        "- AC-1\n",
        "- AC-1 (journey) On a fresh project, the release shall install.\n",
    );
    let policy_path = root.join(".codeflow/policy.json");
    let mut policy: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&policy_path).unwrap()).unwrap();
    policy["git"]["product_paths"] = serde_json::json!(["src/**"]);
    std::fs::write(
        &policy_path,
        serde_json::to_string_pretty(&policy).unwrap() + "\n",
    )
    .unwrap();
    commit(&root, "chore: plan the review inputs");
    git(&root, &["switch", "-q", LINE]);
    git(
        &root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "plan/round",
            "-m",
            "chore: land the plan",
        ],
    );
    let start = |task: &str, branch: &str, file: &str| -> String {
        git(&root, &["switch", "-q", "-c", branch, LINE]);
        ok(&codeflow(&root, &["work", "start", task]), "work start");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join(file), "pub fn added() {}\n").unwrap();
        commit(&root, "feat: add a function")
    };

    // TSK-011 of the verdict: one line keeps the tag inside the text; the
    // reflow onto a note gains it, and CI prints that as the delta.
    let branch = "task/TSK-001-reflow-journey";
    start("TSK-001", branch, "src/first.rs");
    fails(
        &ci(&root, branch, "TSK-001"),
        "a tag inside one line",
        "AC-1 carries `(journey)` inside its text",
    );
    edit(&root, TASK, " - Amended", "\n  - Amended");
    commit(&root, "chore: reflow AC-1");
    let reflowed = ok(&ci(&root, branch, "TSK-001"), "ci after the reflow");
    assert!(
        reflowed.contains(
            "TSK-001 criteria delta: AC-1 changed its tags with the same text: no tag -> (journey)"
        ),
        "{reflowed}"
    );

    // TSK-012 of the verdict: completion reads the same tags before and
    // after the reflow, and CI prints the change.
    let branch = "task/TSK-002-reflow-after";
    let reviewed = start("TSK-002", branch, "src/second.rs");
    fails(
        &complete(&root, dir.path(), "TSK-002", &reviewed, DEFERRED),
        "deferred before the reflow",
        "AC-1 has outcome `deferred`; use verified, waived or failed",
    );
    ok(
        &complete(&root, dir.path(), "TSK-002", &reviewed, "verified | ran"),
        "verified before the reflow",
    );
    git(&root, &["reset", "-q", "--hard", &reviewed]);
    edit(&root, SECOND, " - Amended", "\n  - Amended");
    let reflow = commit(&root, "chore: reflow AC-1");
    let reflowed = ok(&ci(&root, branch, "TSK-002"), "ci after the reflow");
    assert!(
        reflowed.contains(
            "TSK-002 criteria delta: AC-1 changed its tags with the same text: no tag -> (after release)"
        ),
        "{reflowed}"
    );
    fails(
        &complete(&root, dir.path(), "TSK-002", &reflow, "verified | ran"),
        "verified after the reflow",
        "AC-1 is observable only after release and is never `verified` at build time",
    );
    ok(
        &complete(&root, dir.path(), "TSK-002", &reflow, DEFERRED),
        "deferred after the reflow",
    );
    commit(&root, "chore: complete TSK-002");

    // TSK-009 of the verdict: the requirement after the note counts, so
    // the journey passes and AC-1 is no longer deferrable; the refusal
    // names the sub-bullet that hides `(after release)`.
    let branch = "task/TSK-006-split";
    let reviewed = start("TSK-006", branch, "src/sixth.rs");
    let passed = ok(
        &ci(&root, branch, "TSK-006"),
        "ci with a requirement after a note",
    );
    assert!(!passed.contains("work.journey_criterion"), "{passed}");
    fails(
        &complete(&root, dir.path(), "TSK-006", &reviewed, DEFERRED),
        "deferred with the tag hidden",
        "AC-1 has outcome `deferred`; use verified, waived or failed; the note \"- The command shall pass the suite before release (journey).\" under AC-1 hides its `(after release)` tag; move the tag to the start of the criterion (R-50)",
    );

    // TSK-026 of the verdict: the same reflow of another task's record is
    // frozen.
    edit(
        &root,
        "project-management/tasks/TSK-003.md",
        " - Amended",
        "\n  - Amended",
    );
    commit(&root, "chore: reflow another task's AC-1");
    fails(
        &ci(&root, branch, "TSK-006"),
        "another task's tag-only reflow",
        "TSK-003 changes its criteria on this branch",
    );

    // TSK-010, TSK-023 and TSK-004 of the verdict: a sub-bullet that is no
    // dated dash note stays criterion text and is named, and so is a serve
    // only in a note.
    for (task, branch, file, named) in [
        (
            "TSK-004",
            "task/TSK-004-moved-words",
            "src/fourth.rs",
            "the note \"- Moved files shall stay listed in the report.\" under AC-1 hides its `(journey)` tag; move the tag to the start of the criterion (R-50)",
        ),
        (
            "TSK-005",
            "task/TSK-005-bold",
            "src/fifth.rs",
            "the note \"- **Amended** 2026-10-14: bold word.\" under AC-1 hides its `(journey)` tag",
        ),
        (
            "TSK-007",
            "task/TSK-007-serves-note",
            "src/seventh.rs",
            "the note \"- Amended 2026-10-14: now (serves EPC-001 AC-1).\" under AC-1 hides its `(serves ...)` tag; move the tag to the start of the criterion (R-50)",
        ),
    ] {
        start(task, branch, file);
        fails(&ci(&root, branch, task), task, named);
    }

    // Review round 2 (TSK-038, TSK-039, TSK-040 of the verdict): a serves
    // tag reads like the other tags, where it opens or closes the
    // criterion's own text.
    start("TSK-009", "task/TSK-009-serves-later", "src/ninth.rs");
    fails(
        &ci(&root, "task/TSK-009-serves-later", "TSK-009"),
        "a serve before a later sub-bullet",
        "the note \"- The command shall print the plan.\" under AC-1 hides its `(serves ...)` tag; move the tag to the start of the criterion (R-50)",
    );
    start("TSK-010", "task/TSK-010-serves-inside", "src/tenth.rs");
    fails(
        &ci(&root, "task/TSK-010-serves-inside", "TSK-010"),
        "a serve inside the text",
        "TSK-010 changes the adopter-facing path set but has no `(journey)` criterion and serves no epic journey criterion",
    );
    start("TSK-011", "task/TSK-011-serves-noted", "src/eleventh.rs");
    let passed = ok(
        &ci(&root, "task/TSK-011-serves-noted", "TSK-011"),
        "a closing serve before a dated note",
    );
    assert!(!passed.contains("work.journey_criterion"), "{passed}");
}

/// An acceptance block for a one-criterion record (`AC-1` only).
fn single_acceptance(dir: &Path, name: &str, reviewed: &str) -> String {
    let body = format!(
        "acceptance:\n  reviewed: {reviewed}\n  review: session:journey@sha256:00\n  criteria:\n    AC-1: verified | the release journey ran\n  journey: verified | crates/codeflow-cli/tests/acceptance_journey.rs\n  not_verified: none\n  follow_ups: none: journey fixture\n  verdict: approved\n"
    );
    let path = dir.join(format!("{name}.yaml"));
    std::fs::write(&path, body).unwrap();
    path.to_string_lossy().to_string()
}

/// `git push` through the installed hooks; returns (success, output).
fn push(root: &Path, args: &[&str]) -> (bool, String) {
    let out = with_env(&mut Command::new("git"))
        .arg("push")
        .args(args)
        .current_dir(root)
        .output()
        .expect("git runs");
    (out.status.success(), text(&out))
}

/// Journey (TSK-145 AC-6, SPC-013 R-120): on a fresh `codeflow init --full`
/// project two epic lines each land a task completed in its own pull
/// request; a release branch named under the built-in pattern merges both,
/// carries one direct fix and the release-integration task's completion at
/// its head. A real `git push` passes the installed pre-push hook, and the
/// release pull request into main is judged under R-120 and clean. Code
/// after the completion is refused at push and in CI; with no task carrying
/// the role the fix has no owner.
#[test]
#[allow(clippy::too_many_lines)] // One journey keeps the release in the order a project meets it.
fn a_release_branch_from_two_lines_passes_on_a_fresh_project() {
    const LINE_A: &str = "integration/EPC-001-first";
    const LINE_B: &str = "integration/EPC-002-second";
    const RELEASE: &str = "integration/release-1";
    const HOLDER: &str = "project-management/tasks/TSK-003.md";
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    ok(
        &codeflow(&root, &["init", "--yes", "--full"]),
        "init --full",
    );
    // The scaffold's default branch is the default target.
    let start = git(&root, &["branch", "--show-current"]);
    let main = start.as_str();
    // The policy lands on the default branch, through its own reviewed
    // branch, before the lines branch: a planning merge carries records and
    // plans only (TSK-1004 AC-4).
    git(&root, &["switch", "-q", "-c", "chore/product-paths"]);
    let policy_path = root.join(".codeflow/policy.json");
    let mut policy: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&policy_path).unwrap()).unwrap();
    policy["git"]["product_paths"] = serde_json::json!(["src/**"]);
    std::fs::write(
        &policy_path,
        serde_json::to_string_pretty(&policy).unwrap() + "\n",
    )
    .unwrap();
    commit(&root, "chore: name the product paths");
    git(&root, &["switch", "-q", main]);
    ok(
        &codeflow(&root, &["integrate", "chore/product-paths", "--into", main]),
        "integrate the policy",
    );
    git(&root, &["branch", LINE_A]);
    git(&root, &["branch", LINE_B]);

    // Plan: two epics, a task on each line, and the release-integration
    // task, landed on both lines by planning merges.
    git(&root, &["switch", "-q", "-c", "plan/release"]);
    ok(&codeflow(&root, &["epic", "new", "first"]), "epic new");
    ok(&codeflow(&root, &["epic", "new", "second"]), "epic new");
    for (epic, line, title) in [("EPC-001", LINE_A, "first"), ("EPC-002", LINE_B, "second")] {
        let args = ["task", "new", "--epic", epic, "--into", line, title];
        ok(&codeflow(&root, &args), "task new");
    }
    let args = [
        "task",
        "new",
        "--standalone-reason",
        "owns release integration",
        "--into",
        main,
        "release integration",
    ];
    ok(&codeflow(&root, &args), "task new (holder)");
    for record in [EPIC, "project-management/epics/EPC-002.md"] {
        edit(
            &root,
            record,
            "- AC-1\n",
            "- AC-1 When used, the system shall work.\n",
        );
    }
    for record in [TASK, SECOND, HOLDER] {
        edit(
            &root,
            record,
            "- AC-1\n",
            "- AC-1 When run, the system shall work.\n",
        );
    }
    edit(
        &root,
        HOLDER,
        "created:",
        "role: release-integration\ncreated:",
    );
    commit(&root, "chore: plan the release journey");
    // Both lines take the plan with one message at one pinned second, so
    // the two merges are one commit P that both lines hold (TSK-1004; hosted
    // run 40158545922). Each line then advances past P on its own, and P's
    // planning records are shared provenance, never an import from the
    // other, still unpublished, line.
    for line in [LINE_A, LINE_B] {
        git(&root, &["switch", "-q", line]);
        let out = with_env(&mut Command::new("git"))
            .env("GIT_AUTHOR_DATE", "2026-10-25T00:00:00Z")
            .env("GIT_COMMITTER_DATE", "2026-10-25T00:00:00Z")
            .args([
                "merge",
                "-q",
                "--no-ff",
                "plan/release",
                "-m",
                "chore: land the plan",
            ])
            .current_dir(&root)
            .output()
            .expect("git runs");
        assert!(out.status.success(), "plan merge: {}", text(&out));
    }
    assert_eq!(
        git(&root, &["rev-parse", LINE_A]),
        git(&root, &["rev-parse", LINE_B]),
        "both lines hold one planning merge"
    );

    // Each line lands a task completed in its own pull request.
    for (line, id, file) in [
        (LINE_A, "TSK-001", "src/first.rs"),
        (LINE_B, "TSK-002", "src/second.rs"),
    ] {
        let branch = format!("task/{id}-work");
        git(&root, &["switch", "-q", "-c", &branch, line]);
        ok(&codeflow(&root, &["work", "start", id]), "work start");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join(file), "pub fn work() {}\n").unwrap();
        let reviewed = commit(&root, "feat: build the work");
        let block = single_acceptance(dir.path(), id, &reviewed);
        ok(
            &codeflow(
                &root,
                &["task", "status", id, "complete", "--acceptance", &block],
            ),
            "task status complete",
        );
        commit(&root, "chore: complete the task");
        git(&root, &["switch", "-q", line]);
        git(
            &root,
            &[
                "merge",
                "-q",
                "--no-ff",
                &branch,
                "-m",
                "chore: land the task",
            ],
        );
    }

    // The destination: a bare repository holding main (a push to a
    // protected branch is refused), then both lines through the hooks.
    let origin = dir.path().join("origin.git");
    git(
        dir.path(),
        &["init", "-q", "--bare", "-b", main, "origin.git"],
    );
    git(
        &origin,
        &[
            "fetch",
            "-q",
            root.to_str().unwrap(),
            &format!("{main}:{main}"),
        ],
    );
    git(
        &root,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&root, &["fetch", "-q", "origin"]);
    // Each new tree must be checked from its own complete checkout.
    for line in [LINE_A, LINE_B] {
        git(&root, &["switch", "-q", line]);
        let (pushed, said) = push(&root, &["origin", line]);
        assert!(pushed, "the line {line} pushes:\n{said}");
    }
    git(&root, &["fetch", "-q", "origin"]);

    // The release branch: both imports, one direct fix, and the
    // release-integration task's completion at its head.
    git(&root, &["switch", "-q", "-c", RELEASE, main]);
    for line in [LINE_A, LINE_B] {
        let from = format!("origin/{line}");
        git(
            &root,
            &[
                "merge",
                "-q",
                "--no-ff",
                &from,
                "-m",
                "chore: import a line",
            ],
        );
    }
    std::fs::write(root.join("src/release.rs"), "pub fn release() {}\n").unwrap();
    let fix = commit(&root, "fix: integrate the lines");
    let block = single_acceptance(dir.path(), "TSK-003", &fix);
    ok(
        &codeflow(
            &root,
            &[
                "task",
                "status",
                "TSK-003",
                "complete",
                "--acceptance",
                &block,
            ],
        ),
        "the release-integration task completes at the head",
    );
    let completed = commit(&root, "chore: complete the release integration");
    let (pushed, said) = push(&root, &["origin", RELEASE]);
    assert!(pushed, "the release branch pushes:\n{said}");
    assert!(
        said.contains(
            "'integration/release-1' is a release branch: `codeflow ci` judges everything it adds to"
        ),
        "{said}"
    );

    // The release pull request into main, judged under R-120: clean.
    let into_main = codeflow(
        &root,
        &[
            "ci", "--base", main, "--head", &completed, "--branch", RELEASE, "--into", main,
        ],
    );
    let judged = ok(&into_main, "the release pull request into main");
    assert!(
        judged.contains(&format!(
            "release range ('integration/release-1' into '{main}')"
        )),
        "{judged}"
    );

    // Fault: code after the completion, refused at push and in CI.
    git(
        &root,
        &["switch", "-q", "-c", "integration/release-2", &completed],
    );
    std::fs::write(root.join("src/late.rs"), "pub fn late() {}\n").unwrap();
    let late = commit(&root, "fix: a late change");
    let (pushed, said) = push(&root, &["origin", "integration/release-2"]);
    assert!(!pushed, "code after the completion pushed:\n{said}");
    // The push is judged on everything the release branch adds to main, as
    // its pull request is: the late code follows the owner's completion.
    assert!(
        said.contains(&format!(
            "TSK-003 (completed directly on the release line at {}): src/late.rs changed after the reviewed commit",
            &completed[..9]
        )),
        "{said}"
    );
    fails(
        &codeflow(
            &root,
            &[
                "ci",
                "--base",
                main,
                "--head",
                &late,
                "--branch",
                "integration/release-2",
                "--into",
                main,
            ],
        ),
        "code after the completion",
        &format!(
            "TSK-003 (completed directly on the release line at {}): src/late.rs changed after the reviewed commit",
            &completed[..9]
        ),
    );

    // Fault: no task carries the role, so the fix has no owner.
    git(
        &root,
        &["switch", "-q", "-c", "integration/release-3", &fix],
    );
    edit(&root, HOLDER, "role: release-integration\n", "");
    let unowned = commit(&root, "chore: drop the role");
    git(&root, &["switch", "-q", main]);
    fails(
        &codeflow(
            &root,
            &[
                "ci",
                "--base",
                main,
                "--head",
                &unowned,
                "--branch",
                "integration/release-3",
                "--into",
                main,
            ],
        ),
        "no release-integration task",
        "no release-integration task to own it",
    );
}

/// TSK-140: use the scaffolded command and CI surfaces for two complete PR
/// ranges, then reject a copied review and a missing reopen reason.
#[test]
#[allow(clippy::too_many_lines)] // The ordered journey includes its two negative branches.
fn a_completed_task_is_fixed_in_one_pull_request_on_a_fresh_project() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    std::fs::create_dir(&root).unwrap();
    ok(&codeflow(&root, &["init", "--yes", "--full"]), "init full");
    git(&root, &["switch", "-qc", LINE]);
    git(&root, &["switch", "-qc", "plan/fix-journey"]);
    ok(
        &codeflow(&root, &["epic", "new", "fix journey"]),
        "epic new",
    );
    ok(
        &codeflow(
            &root,
            &["task", "new", "--epic", "EPC-001", "--into", LINE, "first"],
        ),
        "task new",
    );
    edit(
        &root,
        EPIC,
        "- AC-1\n",
        "- AC-1 When run, the system shall work.\n",
    );
    edit(&root, TASK, "- AC-1\n", "- AC-1 When run, the system shall work.\n- AC-2 (journey) On a fresh project, the fix shall complete.\n");
    commit(&root, "chore: plan fix journey");
    git(&root, &["switch", "-q", LINE]);
    git(
        &root,
        &[
            "merge",
            "--no-ff",
            "-m",
            "chore: land plan",
            "plan/fix-journey",
        ],
    );
    git(&root, &["switch", "-qc", "task/TSK-001-first"]);
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn first() {}\n").unwrap();
    let reviewed = commit(&root, "feat: first work");
    let old_file = acceptance(dir.path(), &reviewed, "verified | first run");
    let old = std::fs::read_to_string(&old_file).unwrap();
    ok(
        &codeflow(
            &root,
            &[
                "task",
                "status",
                "TSK-001",
                "complete",
                "--acceptance",
                &old_file,
            ],
        ),
        "complete first PR",
    );
    commit(&root, "chore: complete first work");
    ok(&ci(&root, "task/TSK-001-first", "TSK-001"), "first PR CI");
    git(&root, &["switch", "-q", LINE]);
    git(
        &root,
        &[
            "merge",
            "--no-ff",
            "-m",
            "chore: land first work",
            "task/TSK-001-first",
        ],
    );
    git(&root, &["switch", "-qc", "task/TSK-001-fix"]);
    ok(
        &codeflow(
            &root,
            &[
                "task",
                "status",
                "TSK-001",
                "todo",
                "--reason",
                "regression",
            ],
        ),
        "reopen in fix PR",
    );
    commit(&root, "chore: reopen task");
    ok(
        &codeflow(&root, &["work", "start", "TSK-001"]),
        "start reopened task",
    );
    std::fs::write(root.join("src/lib.rs"), "pub fn fixed() {}\n").unwrap();
    let fixed = commit(&root, "fix: repair work");
    let reopened = std::fs::read_to_string(root.join(TASK)).unwrap();
    let new_file = acceptance(dir.path(), &fixed, "verified | fixed run");
    let new = std::fs::read_to_string(&new_file).unwrap();
    ok(
        &codeflow(
            &root,
            &[
                "task",
                "status",
                "TSK-001",
                "complete",
                "--acceptance",
                &new_file,
            ],
        ),
        "complete fix PR",
    );
    commit(&root, "chore: complete fixed work");
    ok(
        &ci(&root, "task/TSK-001-fix", "TSK-001"),
        "single fix PR CI",
    );

    // Each fault forks from the reviewed fix, not from the valid completion.
    for (fault, active, archive, reason) in [
        (
            "copied",
            old.as_str(),
            reopened.clone(),
            "inside the fix range",
        ),
        (
            "reason",
            new.as_str(),
            reopened.replace("  reason: regression\n", ""),
            "reason",
        ),
    ] {
        git(
            &root,
            &["switch", "-qc", &format!("task/TSK-001-{fault}"), &fixed],
        );
        let record = format!(
            "{}\n```yaml\n{active}```\n",
            archive.replace("status: todo", "status: complete")
        );
        std::fs::write(root.join(TASK), record).unwrap();
        commit(&root, "chore: record invalid completion");
        fails(
            &ci(&root, &format!("task/TSK-001-{fault}"), "TSK-001"),
            fault,
            reason,
        );
    }
}

/// The `work.journey_criterion` findings in a check's output, one line
/// each, without the reporter's prefix, so a push and a pull request run
/// can be compared.
fn journey_findings(output: &str) -> Vec<String> {
    let mut found: Vec<String> = output
        .lines()
        .filter(|line| line.contains("changes the adopter-facing path set"))
        .map(|line| {
            let at = line.find("TSK-").unwrap_or(0);
            line[at..].trim().to_string()
        })
        .collect();
    found.sort();
    found.dedup();
    found
}

/// `codeflow ci` over `branch` as its pull request into `main` runs it,
/// with the installed template filled for TSK-001.
fn pull_request_ci(root: &Path, main: &str, branch: &str) -> Output {
    codeflow(
        root,
        &[
            "ci",
            "--base",
            main,
            "--head",
            "HEAD",
            "--branch",
            branch,
            "--into",
            main,
            "--pr-body",
            &body(root, "TSK-001"),
        ],
    )
}

/// Journey (TSK-223 AC-1, sathyassn/codeflow#50): on a fresh
/// `codeflow init --full` project, a standalone task branch that changes
/// an adopter-facing path while its task has no journey criterion is
/// refused by the installed pre-push hook with the same journey finding
/// the pull request check reports for that range. Once the task carries a
/// journey criterion, both paths pass that check.
#[test]
fn a_push_and_its_pull_request_reach_the_same_journey_verdict() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    ok(
        &codeflow(&root, &["init", "--yes", "--full"]),
        "init --full",
    );
    let start = git(&root, &["branch", "--show-current"]);
    let main = start.as_str();
    let origin = dir.path().join("origin.git");
    git(
        dir.path(),
        &["init", "-q", "--bare", "-b", main, "origin.git"],
    );
    git(
        &origin,
        &[
            "fetch",
            "-q",
            root.to_str().unwrap(),
            &format!("{main}:{main}"),
        ],
    );

    // A standalone task on its own branch, with no journey criterion,
    // that changes a managed instruction file (an adopter-facing path).
    // The record is written before the remote exists, as a project
    // without a shared id registry does.
    let branch = "task/TSK-001-parity";
    git(&root, &["switch", "-q", "-c", branch]);
    let args = [
        "task",
        "new",
        "--standalone-reason",
        "parity fixture",
        "--into",
        main,
        "parity",
    ];
    ok(&codeflow(&root, &args), "task new");
    edit(
        &root,
        TASK,
        "- AC-1\n",
        "- AC-1 When run, the system shall work.\n",
    );
    commit(&root, "docs(tasks): plan the parity task");
    ok(
        &codeflow(&root, &["work", "start", "TSK-001"]),
        "work start",
    );
    let agents = std::fs::read_to_string(root.join("AGENTS.md")).unwrap();
    std::fs::write(
        root.join("AGENTS.md"),
        format!("{agents}\n- Parity fixture note.\n"),
    )
    .unwrap();
    commit(&root, "docs: add a project note");
    git(
        &root,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&root, &["fetch", "-q", "origin"]);

    // The pull request check over the range blocks on the journey.
    let reason = "TSK-001 changes the adopter-facing path set but has no `(journey)` criterion";
    let pull = pull_request_ci(&root, main, branch);
    fails(&pull, "the pull request check without a journey", reason);

    // The push of the same range is refused with the same finding.
    let (pushed, said) = push(&root, &["origin", branch]);
    assert!(!pushed, "the push without a journey went through:\n{said}");
    assert!(said.contains("work.journey_criterion"), "{said}");
    let from_pull = journey_findings(&text(&pull));
    assert_eq!(from_pull.len(), 1, "{}", text(&pull));
    assert_eq!(journey_findings(&said), from_pull, "push:\n{said}");

    // From a checkout without the task record, the task is read at the
    // pushed head: a bodyless run and a push of the branch find the same.
    git(&root, &["switch", "-q", main]);
    let bodyless = codeflow(
        &root,
        &["ci", "--base", main, "--head", branch, "--branch", branch],
    );
    fails(&bodyless, "a bodyless run from the main checkout", reason);
    assert_eq!(journey_findings(&text(&bodyless)), from_pull);
    let (pushed, said) = push(&root, &["origin", branch]);
    assert!(!pushed, "the push from main went through:\n{said}");
    assert_eq!(
        journey_findings(&said),
        from_pull,
        "push from main:\n{said}"
    );
    git(&root, &["switch", "-q", branch]);

    // With a journey criterion, neither path reports the journey.
    edit(
        &root,
        TASK,
        "shall work.\n",
        "shall work.\n- AC-2 On a fresh project, the note shall read (journey).\n",
    );
    commit(&root, "docs(tasks): add the parity journey");
    let pull = pull_request_ci(&root, main, branch);
    assert!(
        !text(&pull).contains("work.journey_criterion"),
        "{}",
        text(&pull)
    );
    let (pushed, said) = push(&root, &["origin", branch]);
    assert!(pushed, "the push with a journey was refused:\n{said}");
    assert!(!said.contains("work.journey_criterion"), "{said}");
}

/// One approving Reviews row for the whole unit at each commit an acceptance
/// block at `rev` binds (issue 121), or `fallback` when none is bound.
fn review_rows(root: &Path, rev: &str, fallback: &str) -> String {
    let out = std::process::Command::new("git")
        .args([
            "grep",
            "-h",
            "-o",
            "-E",
            "reviewed: [0-9a-f]{40}",
            rev,
            "--",
            "project-management",
        ])
        .current_dir(root)
        .output()
        .unwrap();
    let rows: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.split_whitespace().last())
        .map(|sha| format!("| Reviewer | whole unit at {sha} | approved |"))
        .collect();
    if rows.is_empty() {
        fallback.to_string()
    } else {
        rows.join("\n")
    }
}

/// TSK-293 AC-4: a completed task remains bound to its reviewed commit when
/// merge mode lands it on a line that advanced after the review.
#[test]
fn integrate_merge_preserves_a_completed_tasks_acceptance_binding() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    plan_merge_journey(&root);
    let branch = "task/TSK-001-merge-journey";
    git(&root, &["switch", "-qc", branch]);
    ok(
        &codeflow(&root, &["work", "start", "TSK-001"]),
        "work start",
    );
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/first.rs"), "pub fn first() {}\n").unwrap();
    let reviewed = commit(&root, "feat: build reviewed work");
    let block = acceptance(dir.path(), &reviewed, "verified | fixture built");
    ok(
        &codeflow(
            &root,
            &[
                "task",
                "status",
                "TSK-001",
                "complete",
                "--acceptance",
                &block,
            ],
        ),
        "complete the task",
    );
    let completed = commit(&root, "chore: complete the task");
    ok(&ci(&root, branch, "TSK-001"), "ci before landing");
    git(&root, &["switch", "-q", LINE]);
    std::fs::write(root.join("line.txt"), "line advanced\n").unwrap();
    let line = commit(&root, "docs: advance the line");
    let policy: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join(".codeflow/policy.json")).unwrap())
            .unwrap();
    assert_eq!(policy["git"]["root_checkout_commits"], "block");
    let linked = root.join(".worktrees/reviewed");
    git(
        &root,
        &["worktree", "add", linked.to_str().unwrap(), branch],
    );
    // Cargo blanks inherited harness markers. Set the actor explicitly so
    // real installed hooks enforce the block policy on the merge candidate.
    let landed = with_env(&mut Command::new(env!("CARGO_BIN_EXE_codeflow")))
        .args(["integrate", branch, "--into", LINE, "--merge"])
        .env("CLAUDECODE", "1")
        .env("CODEFLOW_HOOK_BINARY", env!("CARGO_BIN_EXE_codeflow"))
        .current_dir(&linked)
        .output()
        .unwrap();
    ok(&landed, "land with merge mode under agent hooks");
    assert_eq!(git(&root, &["rev-parse", "HEAD^1"]), line);
    assert_eq!(git(&root, &["rev-parse", "HEAD^2"]), completed);
    assert_eq!(git(&root, &["rev-parse", branch]), completed);
    let record = std::fs::read_to_string(root.join(TASK)).unwrap();
    assert!(record.contains(&format!("reviewed: {reviewed}")));
    let passed = ok(
        &codeflow(
            &root,
            &["ci", "--base", &line, "--head", "HEAD", "--branch", LINE],
        ),
        "ci over the landing",
    );
    assert!(passed.contains(SCOPE), "{passed}");
}

fn plan_merge_journey(root: &Path) {
    ok(&codeflow(root, &["init", "--yes", "--full"]), "init");
    git(root, &["switch", "-qc", LINE]);
    git(root, &["switch", "-qc", "plan/merge"]);
    ok(&codeflow(root, &["epic", "new", "merge journey"]), "epic");
    ok(
        &codeflow(
            root,
            &[
                "task",
                "new",
                "--epic",
                "EPC-001",
                "--into",
                LINE,
                "merge journey",
            ],
        ),
        "task",
    );
    edit(
        root,
        EPIC,
        "- AC-1\n",
        "- AC-1 When used, the system shall work.\n",
    );
    edit(
        root,
        TASK,
        "- AC-1\n",
        "- AC-1 When used, the system shall work.\n- AC-2 (journey) When landed, the binding shall remain valid.\n",
    );
    // The fixture has a real, small gate so integrate exercises its normal
    // full-mode stage without running the consuming project's full suite.
    std::fs::write(
        root.join(".codeflow/test-config.json"),
        r#"{"schema_version":"1.0","targets":[{"name":"merged-feature","runner":"custom","modes":{"full":{"command":"git ls-files --error-unmatch src/first.rs line.txt"}}}]}"#,
    ).unwrap();
    commit(root, "chore: plan the merge journey");
    git(root, &["switch", "-q", LINE]);
    git(root, &["merge", "--ff-only", "plan/merge"]);
}
