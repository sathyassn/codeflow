//! Acceptance bound to the reviewed commit in `codeflow ci` and
//! `task status complete` (TSK-105, SPC-013 R-52, R-53, R-54, R-60, R-62,
//! R-80, R-81): a valid completion passes, and a stale block, a waiver
//! without its amendment, a later code change, criteria changed on a task
//! branch, a missing journey criterion and a silent leaf each fail for their
//! stated reason. Each test runs the Cargo-built binary in a tempdir
//! repository with durable work tracking on.

use std::path::Path;
use std::process::Command;

fn isolated_home() -> &'static Path {
    static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| tempfile::tempdir().expect("home tempdir"))
        .path()
}

fn codeflow() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_codeflow"));
    cmd.env("CODEFLOW_HOME", isolated_home())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("CODEFLOW_PR_BODY")
        .env_remove("GITHUB_EVENT_NAME")
        .env_remove("GITHUB_HEAD_REF")
        .env_remove("GITHUB_BASE_REF")
        .env_remove("CI_PIPELINE_SOURCE")
        .env_remove("CI_MERGE_REQUEST_IID")
        .env_remove("BITBUCKET_PR_ID")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE");
    cmd
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn git(dir: &Path, args: &[&str]) {
    git_out(dir, args);
}

fn head(dir: &Path) -> String {
    git_out(dir, &["rev-parse", "HEAD"])
}

const EPIC: &str = "---\nid: EPC-001\ntitle: \"outcome\"\nstatus: planning\nwork_type: feat\nspecs: []\ncreated: 2026-09-26\n---\n\n# EPC-001: outcome\n\n## Summary\n\nAn outcome.\n\n## Acceptance Criteria\n\n- AC-1 (journey) On a fresh project, an adopter shall finish the flow.\n";

/// A task record under `project-management/tasks/` with `criteria` as its
/// acceptance criteria lines and `closeout` as its Closeout body.
fn task(id: &str, status: &str, criteria: &str, closeout: &str) -> String {
    format!(
        "---\nid: {id}\nepic_id: EPC-001\nstandalone_reason: null\nintegration_target: main\ntitle: \"work {id}\"\nstatus: {status}\nwork_type: feat\nspecs: []\ndepends_on: []\ncreated: 2026-09-26\n---\n\n# {id}: work\n\n## Description\n\nWork.\n\n## Acceptance Criteria\n\n{criteria}\n## Closeout\n\n{closeout}"
    )
}

const OWN_JOURNEY: &str = "- AC-1 When run, the system shall work.\n- AC-2 (journey) On a fresh project, the command shall succeed.\n";
const NO_JOURNEY: &str = "- AC-1 When run, the system shall work.\n";
const SERVES_EPIC: &str = "- AC-1 When run, the system shall work (serves EPC-001 AC-1)\n";

fn record_path(id: &str) -> String {
    format!("project-management/tasks/{id}.md")
}

/// `main` holds the epic and `tasks` (id, criteria), a policy naming
/// `src/**` as product code, and `extra_policy` inside `git`.
fn repo(tasks: &[(&str, &str)], extra_policy: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "user.name", "t"]);
    for sub in [
        "project-management/epics",
        "project-management/tasks",
        ".codeflow",
        "src",
    ] {
        std::fs::create_dir_all(root.join(sub)).unwrap();
    }
    std::fs::write(root.join("project-management/epics/EPC-001.md"), EPIC).unwrap();
    for (id, criteria) in tasks {
        std::fs::write(
            root.join(record_path(id)),
            task(id, "todo", criteria, "Pending.\n"),
        )
        .unwrap();
    }
    std::fs::write(
        root.join(".codeflow/policy.json"),
        format!(
            "{{\n  \"schema_version\": 1,\n  \"git\": {{\"product_paths\": [\"src/**\"]{extra_policy}}}\n}}\n"
        ),
    )
    .unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn base() {}\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "chore: plan the work"]);
    dir
}

fn write(root: &Path, path: &str, content: &str) {
    let full = root.join(path);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, content).unwrap();
}

fn commit(root: &Path, message: &str) -> String {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-m", message]);
    head(root)
}

/// A code change on `branch`, cut from `main`; returns its commit.
fn code_change(root: &Path, branch: &str, body: &str) -> String {
    git(root, &["switch", "-C", branch, "main"]);
    write(root, "src/lib.rs", body);
    commit(root, "feat: change the code")
}

/// An acceptance block with `criteria` lines (`AC-n: outcome | evidence`).
fn block(reviewed: &str, criteria: &[&str], journey: &str, follow_ups: &str) -> String {
    let mut lines = String::new();
    for line in criteria {
        lines.push_str("    ");
        lines.push_str(line);
        lines.push('\n');
    }
    format!(
        "```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: https://example.test/pr/1#review\n  criteria:\n{lines}  journey: {journey}\n  not_verified: none\n  follow_ups: {follow_ups}\n  verdict: approved\n```\n"
    )
}

/// Mark `id` complete with `closeout`, keeping its criteria as `main` has
/// them; returns the new head.
fn complete(root: &Path, id: &str, criteria: &str, closeout: &str) -> String {
    write(
        root,
        &record_path(id),
        &task(id, "complete", criteria, closeout),
    );
    commit(root, "docs(records): record the acceptance")
}

fn ci(root: &Path, branch: &str, id: &str) -> (i32, String) {
    ci_with(root, branch, &format!("Task: {id}"))
}

fn ci_with(root: &Path, branch: &str, task_line: &str) -> (i32, String) {
    ci_on(root, "main", branch, task_line)
}

/// `codeflow ci` from `base` to `HEAD` as `branch`, with `task_line` in the
/// pull request body.
fn ci_on(root: &Path, base: &str, branch: &str, task_line: &str) -> (i32, String) {
    let out = codeflow()
        .args([
            "ci",
            "--base",
            base,
            "--head",
            "HEAD",
            "--branch",
            branch,
            "--pr-body",
            &format!(
                "## Summary\nA change.\n\n{task_line}\n\n## Changes\n- one\n\n## Testing\n- test\n"
            ),
        ])
        .current_dir(root)
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

fn assert_passes(result: &(i32, String), what: &str) {
    assert_eq!(result.0, 0, "{what}: {}", result.1);
}

fn assert_blocks(result: &(i32, String), what: &str, needles: &[&str]) {
    assert_eq!(result.0, 1, "{what}: {}", result.1);
    for needle in needles {
        assert!(
            result.1.contains(needle),
            "{what}: missing {needle:?} in {}",
            result.1
        );
    }
}

const SCOPE: &str = "the acceptance check proves structure and binding only";
const BRANCH: &str = "task/TSK-001-work";

/// AC-1, AC-4: a block reviewed at the head's code commit passes, with the
/// scope note in the output; a later code change and a block reviewed on
/// another line each fail for their reason.
#[test]
fn a_completion_binds_to_the_reviewed_commit() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    let valid = |reviewed: &str| {
        block(
            reviewed,
            &[
                "AC-1: verified | cargo test | 3 passed",
                "AC-2: verified | journey ran",
            ],
            "verified | tests/journey.rs",
            "none: nothing deferred",
        )
    };

    let reviewed = code_change(root, BRANCH, "pub fn work() {}\n");
    complete(root, "TSK-001", OWN_JOURNEY, &valid(&reviewed));
    let pass = ci(root, BRANCH, "TSK-001");
    assert_passes(&pass, "reviewed code commit, record-only after it");
    assert!(pass.1.contains(SCOPE), "scope note: {}", pass.1);

    write(root, "src/lib.rs", "pub fn later() {}\n");
    commit(root, "feat: change after review");
    assert_blocks(
        &ci(root, BRANCH, "TSK-001"),
        "a later code change",
        &[
            "work.acceptance_binding",
            "src/lib.rs changed after the reviewed commit",
        ],
    );

    // A reviewed commit this repository does not have.
    code_change(root, BRANCH, "pub fn work() {}\n");
    complete(root, "TSK-001", OWN_JOURNEY, &valid(&"b".repeat(40)));
    assert_blocks(
        &ci(root, BRANCH, "TSK-001"),
        "an unknown reviewed commit",
        &["is not in this repository"],
    );

    // A block copied from another line: its reviewed commit is not an
    // ancestor of the head.
    git(root, &["switch", "-C", "other", "main"]);
    write(root, "src/other.rs", "pub fn other() {}\n");
    let elsewhere = commit(root, "feat: other work");
    code_change(root, BRANCH, "pub fn work() {}\n");
    complete(root, "TSK-001", OWN_JOURNEY, &valid(&elsewhere));
    assert_blocks(
        &ci(root, BRANCH, "TSK-001"),
        "a copied block",
        &["is not the head or an ancestor of it", SCOPE],
    );

    // The record itself changed outside its status and Closeout after the
    // reviewed commit: the title.
    let reviewed = code_change(root, BRANCH, "pub fn work() {}\n");
    let record = task("TSK-001", "complete", OWN_JOURNEY, &valid(&reviewed))
        .replace("title: \"work TSK-001\"", "title: \"renamed\"");
    write(root, &record_path("TSK-001"), &record);
    commit(root, "docs(records): rename and complete");
    assert_blocks(
        &ci(root, BRANCH, "TSK-001"),
        "a record change beyond status and Closeout",
        &["the record changed outside its status and Closeout"],
    );
}

/// AC-1: a waiver names the planning amendment on the target that changed
/// that criterion; a commit that is only on the branch, or that did not
/// amend it, fails.
#[test]
fn a_waiver_names_its_amendment_on_the_target() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    let amended = OWN_JOURNEY.replace("shall work.", "shall work on Linux.");
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", &amended, "Pending.\n"),
    );
    let amendment = commit(root, "docs(records): narrow TSK-001 AC-1");
    write(
        root,
        "project-management/epics/EPC-001.md",
        &EPIC.replace("An outcome.", "An outcome, restated."),
    );
    let unrelated = commit(root, "docs(records): restate the epic");

    let waived = |reviewed: &str, by: &str| {
        block(
            reviewed,
            &[
                &format!("AC-1: waived | {by}"),
                "AC-2: verified | journey ran",
            ],
            "verified | tests/journey.rs",
            "none: nothing deferred",
        )
    };

    let reviewed = code_change(root, BRANCH, "pub fn work() {}\n");
    complete(root, "TSK-001", &amended, &waived(&reviewed, &amendment));
    assert_passes(
        &ci(root, BRANCH, "TSK-001"),
        "a waiver naming its amendment",
    );

    let reviewed = code_change(root, BRANCH, "pub fn work() {}\n");
    complete(root, "TSK-001", &amended, &waived(&reviewed, &unrelated));
    assert_blocks(
        &ci(root, BRANCH, "TSK-001"),
        "a waiver naming an unrelated target commit",
        &["AC-1 waiver", "which does not amend AC-1 of TSK-001"],
    );

    let reviewed = code_change(root, BRANCH, "pub fn work() {}\n");
    complete(root, "TSK-001", &amended, &waived(&reviewed, &reviewed));
    assert_blocks(
        &ci(root, BRANCH, "TSK-001"),
        "a waiver naming a branch commit",
        &["AC-1 waiver", "which is not on the target"],
    );

    // A ref named like an abbreviated id never stands in for the amendment.
    git(root, &["branch", "0000000", &amendment]);
    let reviewed = code_change(root, BRANCH, "pub fn work() {}\n");
    complete(root, "TSK-001", &amended, &waived(&reviewed, "0000000"));
    assert_blocks(
        &ci(root, BRANCH, "TSK-001"),
        "a waiver naming a ref, not an object id",
        &["AC-1 waiver", "names 0000000, which is not a commit here"],
    );
}

/// AC-2, AC-6: a task branch that changes its record's criteria blocks, and
/// `git.work_records: warn` does not relax it.
#[test]
fn criteria_are_frozen_on_a_task_branch() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], ", \"work_records\": \"warn\"");
    let root = dir.path();
    code_change(root, BRANCH, "pub fn work() {}\n");
    let loosened = OWN_JOURNEY.replace("shall work.", "shall mostly work.");
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", &loosened, "Pending.\n"),
    );
    commit(root, "docs(records): loosen AC-1");
    assert_blocks(
        &ci(root, BRANCH, "TSK-001"),
        "criteria changed on the task branch",
        &[
            "work.criteria_frozen",
            "TSK-001 changes its criteria on this branch",
        ],
    );

    // A planning branch is where criteria change.
    git(root, &["switch", "-C", "plan/loosen", "main"]);
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", &loosened, "Pending.\n"),
    );
    commit(root, "docs(records): loosen AC-1");
    assert_passes(&ci_with(root, "plan/loosen", ""), "a planning amendment");
}

/// AC-3, AC-6: a range touching the adopter-facing set needs a task with a
/// journey criterion or one serving the epic's; `warn` reports it without
/// blocking; a leaf serving the epic journey says what ran.
#[test]
fn an_adopter_facing_range_needs_a_journey() {
    let tasks = [
        ("TSK-001", OWN_JOURNEY),
        ("TSK-002", NO_JOURNEY),
        ("TSK-003", SERVES_EPIC),
    ];
    let dir = repo(&tasks, "");
    let root = dir.path();

    code_change(root, "task/TSK-001-work", "pub fn one() {}\n");
    assert_passes(&ci(root, "task/TSK-001-work", "TSK-001"), "own journey");

    code_change(root, "task/TSK-002-work", "pub fn two() {}\n");
    assert_blocks(
        &ci(root, "task/TSK-002-work", "TSK-002"),
        "no journey",
        &["work.journey_criterion", "has no `(journey)` criterion"],
    );

    // A range outside the adopter-facing set needs no journey.
    git(root, &["switch", "-C", "task/TSK-002-docs", "main"]);
    write(root, "docs/guide.md", "# Guide\n");
    commit(root, "docs: add a guide");
    assert_passes(&ci(root, "task/TSK-002-docs", "TSK-002"), "docs only");

    code_change(root, "task/TSK-003-work", "pub fn three() {}\n");
    assert_passes(
        &ci(root, "task/TSK-003-work", "TSK-003"),
        "serves the epic journey",
    );
    let reviewed = head(root);
    let silent = block(
        &reviewed,
        &["AC-1: verified | cargo test | 1 passed"],
        "none: a leaf",
        "none: nothing deferred",
    );
    complete(root, "TSK-003", SERVES_EPIC, &silent);
    assert_blocks(
        &ci(root, "task/TSK-003-work", "TSK-003"),
        "a silent leaf",
        &["serves EPC-001 AC-1 (a journey)", "narrower | <path>"],
    );
    let narrower = block(
        &reviewed,
        &["AC-1: verified | cargo test | 1 passed"],
        "narrower | the unit path of src/lib.rs",
        "none: nothing deferred",
    );
    complete(root, "TSK-003", SERVES_EPIC, &narrower);
    assert_passes(
        &ci(root, "task/TSK-003-work", "TSK-003"),
        "a leaf naming its narrower path",
    );

    let dir = repo(&tasks, ", \"work_records\": \"warn\"");
    let root = dir.path();
    code_change(root, "task/TSK-002-work", "pub fn two() {}\n");
    let warned = ci(root, "task/TSK-002-work", "TSK-002");
    assert_passes(&warned, "warn level");
    assert!(warned.1.contains("work.journey_criterion"), "{}", warned.1);
}

fn status_complete(root: &Path, id: &str) -> (i32, String) {
    let out = codeflow()
        .args(["task", "status", id, "complete"])
        .current_dir(root)
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

/// AC-1, AC-6: `task status complete` applies the same binding before it
/// writes; `warn` completes with a warning.
#[test]
fn task_status_complete_binds_the_block() {
    for (policy, blocks) in [("", true), (", \"work_records\": \"warn\"", false)] {
        let dir = repo(&[("TSK-001", OWN_JOURNEY)], policy);
        let root = dir.path();
        let reviewed = code_change(root, BRANCH, "pub fn work() {}\n");
        write(root, "src/lib.rs", "pub fn later() {}\n");
        commit(root, "feat: change after review");
        let closeout = block(
            &reviewed,
            &[
                "AC-1: verified | cargo test | 3 passed",
                "AC-2: verified | journey ran",
            ],
            "verified | tests/journey.rs",
            "none: nothing deferred",
        );
        write(
            root,
            &record_path("TSK-001"),
            &task("TSK-001", "in_progress", OWN_JOURNEY, &closeout),
        );
        let result = status_complete(root, "TSK-001");
        assert!(
            result
                .1
                .contains("src/lib.rs changed after the reviewed commit"),
            "{policy}: {}",
            result.1
        );
        let record = std::fs::read_to_string(root.join(record_path("TSK-001"))).unwrap();
        if blocks {
            assert_ne!(result.0, 0, "block level refuses: {}", result.1);
            assert!(result.1.contains(SCOPE), "scope note: {}", result.1);
            assert!(record.contains("status: in_progress"), "{record}");
        } else {
            assert_eq!(result.0, 0, "warn level completes: {}", result.1);
            assert!(record.contains("status: complete"), "{record}");
        }
    }
}

/// AC-8: a criterion observable only after release is deferred with an
/// owner, a window and a listed follow-up; verified at build time fails.
#[test]
fn an_after_release_criterion_is_deferred_never_verified() {
    let criteria = "- AC-1 When run, the system shall work.\n- AC-2 (journey) On a fresh project, the command shall succeed.\n- AC-3 (after release) Adopters shall report fewer failed installs.\n";
    let dir = repo(&[("TSK-001", criteria)], "");
    let root = dir.path();
    let reviewed = code_change(root, BRANCH, "pub fn work() {}\n");
    let with = |third: &str, follow_ups: &str| {
        block(
            &reviewed,
            &[
                "AC-1: verified | cargo test | 3 passed",
                "AC-2: verified | journey ran",
                third,
            ],
            "verified | tests/journey.rs",
            follow_ups,
        )
    };
    let deferred = "AC-3: deferred | owner: release lead; window: 30 days; follow-up: TSK-009";
    for (closeout, ok, needle) in [
        (with(deferred, "TSK-009"), true, ""),
        (
            with("AC-3: verified | looks fine", "none: nothing"),
            false,
            "never `verified` at build time",
        ),
        (
            with(deferred, "none: nothing"),
            false,
            "follow-up TSK-009 is not listed in `follow_ups`",
        ),
    ] {
        write(
            root,
            &record_path("TSK-001"),
            &task("TSK-001", "in_progress", criteria, &closeout),
        );
        let result = status_complete(root, "TSK-001");
        if ok {
            assert_eq!(result.0, 0, "deferred with owner and window: {}", result.1);
            git(root, &["checkout", "--", "."]);
        } else {
            assert_ne!(result.0, 0, "{needle}: {}", result.1);
            assert!(result.1.contains(needle), "{needle}: {}", result.1);
        }
    }
}

/// A valid two-criterion block reviewed at `reviewed`.
fn valid_block(reviewed: &str) -> String {
    block(
        reviewed,
        &[
            "AC-1: verified | cargo test | 3 passed",
            "AC-2: verified | journey ran",
        ],
        "verified | tests/journey.rs",
        "none: nothing deferred",
    )
}

/// Review round 1, T105-1: the freeze follows the validated class, not the
/// branch prefix. Tracked code on a `plan/` branch keeps criteria frozen; a
/// records-only planning change on a `fix/` branch may amend them.
#[test]
fn the_freeze_follows_the_class_not_the_prefix() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    let loosened = OWN_JOURNEY.replace("shall work.", "shall mostly work.");

    code_change(root, "plan/code-with-criteria", "pub fn work() {}\n");
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", &loosened, "Pending.\n"),
    );
    commit(root, "docs(records): loosen AC-1");
    assert_blocks(
        &ci(root, "plan/code-with-criteria", "TSK-001"),
        "tracked code on a plan/ prefix",
        &["work.criteria_frozen"],
    );

    git(root, &["switch", "-C", "fix/criteria", "main"]);
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", &loosened, "Pending.\n"),
    );
    commit(root, "docs(records): loosen AC-1");
    let planning = ci_with(root, "fix/criteria", "");
    assert_passes(&planning, "a records-only planning change on fix/");
    assert!(
        !planning.1.contains("work.criteria_frozen"),
        "{}",
        planning.1
    );
}

/// Review round 1, T105-2: a reviewed commit that predates the record
/// never reviewed its scope, even when only the record changed since.
#[test]
fn a_reviewed_commit_without_the_record_fails() {
    let dir = repo(&[], "");
    let root = dir.path();
    let before_record = head(root);
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", OWN_JOURNEY, "Pending.\n"),
    );
    commit(root, "docs(records): plan TSK-001");
    git(root, &["switch", "-C", BRANCH, "main"]);
    complete(root, "TSK-001", OWN_JOURNEY, &valid_block(&before_record));
    assert_blocks(
        &ci(root, BRANCH, "TSK-001"),
        "a reviewed commit that predates the record",
        &[
            "work.acceptance_binding",
            "is not in the reviewed commit, so its scope was never reviewed",
        ],
    );
}

/// Review round 1, T105-3: a `## Closeout` line in a Description comment
/// does not hide the text after it from the binding.
#[test]
fn a_hidden_closeout_heading_hides_nothing() {
    let dir = repo(&[], "");
    let root = dir.path();
    let hidden = |text: &str, status: &str, closeout: &str| {
        task("TSK-001", status, OWN_JOURNEY, closeout).replace(
            "## Description\n\nWork.\n",
            &format!("## Description\n\n<!--\n## Closeout\n-->\n\n{text}\n"),
        )
    };
    write(
        root,
        &record_path("TSK-001"),
        &hidden("Visible scope.", "todo", "Pending.\n"),
    );
    commit(root, "docs(records): plan TSK-001");
    let reviewed = code_change(root, BRANCH, "pub fn work() {}\n");

    write(
        root,
        &record_path("TSK-001"),
        &hidden("Visible scope.", "complete", &valid_block(&reviewed)),
    );
    commit(root, "docs(records): record the acceptance");
    assert_passes(
        &ci(root, BRANCH, "TSK-001"),
        "status and the real Closeout only",
    );

    write(
        root,
        &record_path("TSK-001"),
        &hidden("Narrower scope.", "complete", &valid_block(&reviewed)),
    );
    commit(root, "docs(records): narrow the scope");
    assert_blocks(
        &ci(root, BRANCH, "TSK-001"),
        "a scope edit after a hidden heading",
        &["the record changed outside its status and Closeout"],
    );
}

/// Review round 1, T105-4: `task status complete` refuses staged, unstaged
/// and new files outside the record, which the reviewed commit never held;
/// uncommitted edits to the record itself are how it completes.
#[test]
fn task_status_complete_refuses_uncommitted_code() {
    for what in ["unstaged", "staged", "untracked"] {
        let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
        let root = dir.path();
        let reviewed = code_change(root, BRANCH, "pub fn work() {}\n");
        write(
            root,
            &record_path("TSK-001"),
            &task(
                "TSK-001",
                "in_progress",
                OWN_JOURNEY,
                &valid_block(&reviewed),
            ),
        );
        match what {
            "untracked" => write(root, "src/new.rs", "pub fn new() {}\n"),
            _ => write(root, "src/lib.rs", "pub fn dirty() {}\n"),
        }
        if what == "staged" {
            git(root, &["add", "src/lib.rs"]);
        }
        let result = status_complete(root, "TSK-001");
        assert_ne!(result.0, 0, "{what}: {}", result.1);
        let named = if what == "untracked" {
            "src/new.rs"
        } else {
            "src/lib.rs"
        };
        for needle in [
            "uncommitted changes outside the record were never reviewed",
            named,
            "commit, remove or ignore them",
        ] {
            assert!(result.1.contains(needle), "{what}: {needle}: {}", result.1);
        }
        let record = std::fs::read_to_string(root.join(record_path("TSK-001"))).unwrap();
        assert!(record.contains("status: in_progress"), "{what}: {record}");
    }

    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    let reviewed = code_change(root, BRANCH, "pub fn work() {}\n");
    write(
        root,
        &record_path("TSK-001"),
        &task(
            "TSK-001",
            "in_progress",
            OWN_JOURNEY,
            &valid_block(&reviewed),
        ),
    );
    let renamed = task(
        "TSK-001",
        "in_progress",
        OWN_JOURNEY,
        &valid_block(&reviewed),
    )
    .replace("title: \"work TSK-001\"", "title: \"renamed\"");
    write(root, &record_path("TSK-001"), &renamed);
    let result = status_complete(root, "TSK-001");
    assert_ne!(result.0, 0, "a scope edit in the record: {}", result.1);
    assert!(
        result
            .1
            .contains("the record changed outside its status and Closeout"),
        "{}",
        result.1
    );

    write(
        root,
        &record_path("TSK-001"),
        &task(
            "TSK-001",
            "in_progress",
            OWN_JOURNEY,
            &valid_block(&reviewed),
        ),
    );
    let result = status_complete(root, "TSK-001");
    assert_eq!(result.0, 0, "record-only dirt completes: {}", result.1);
}

/// Review round 1, T105-5: a waiver names a planning amendment. A merge or
/// a commit that also brings code onto the target is not one; a merge of a
/// records-only planning branch is.
#[test]
fn a_waiver_amendment_changes_planning_records_only() {
    let amended = OWN_JOURNEY.replace("shall work.", "shall work on Linux.");
    let waived = |reviewed: &str, by: &str| {
        block(
            reviewed,
            &[
                &format!("AC-1: waived | {by}"),
                "AC-2: verified | journey ran",
            ],
            "verified | tests/journey.rs",
            "none: nothing deferred",
        )
    };
    for (what, with_code, merged) in [
        ("a mixed merge", true, true),
        ("a mixed commit", true, false),
        ("a planning-only merge", false, true),
    ] {
        let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
        let root = dir.path();
        if merged {
            git(root, &["switch", "-C", "plan/narrow", "main"]);
        }
        write(
            root,
            &record_path("TSK-001"),
            &task("TSK-001", "todo", &amended, "Pending.\n"),
        );
        if with_code {
            write(root, "src/lib.rs", "pub fn own_work() {}\n");
        }
        let mut amendment = commit(root, "docs(records): narrow TSK-001 AC-1");
        if merged {
            git(root, &["switch", "main"]);
            git(
                root,
                &[
                    "merge",
                    "--no-ff",
                    "-m",
                    "merge: narrow AC-1",
                    "plan/narrow",
                ],
            );
            amendment = head(root);
        }
        let reviewed = code_change(root, BRANCH, "pub fn work() {}\n");
        complete(root, "TSK-001", &amended, &waived(&reviewed, &amendment));
        let result = ci(root, BRANCH, "TSK-001");
        if with_code {
            assert_blocks(
                &result,
                what,
                &[
                    "AC-1 waiver",
                    "which also changes src/lib.rs; a planning amendment changes planning records only",
                ],
            );
        } else {
            assert_passes(&result, what);
        }
    }
}

// The binding of a completion made after its task landed (the merge rule):
// R is the second parent of a clean landing merge M on the completion's
// first-parent chain, only merges and planning records follow M, and the
// completion changes planning records only. `task status complete` and
// `codeflow ci` apply the same judge.

const LINE: &str = "integration/EPC-001-line";

/// A task record targeting [`LINE`], with [`OWN_JOURNEY`] criteria.
fn line_task(id: &str, status: &str, closeout: &str) -> String {
    task(id, status, OWN_JOURNEY, closeout).replace(
        "integration_target: main",
        &format!("integration_target: {LINE}"),
    )
}

/// `main` holds `ids` targeting [`LINE`], which is cut from it.
fn line_repo(ids: &[&str]) -> tempfile::TempDir {
    let tasks: Vec<(&str, &str)> = ids.iter().map(|id| (*id, OWN_JOURNEY)).collect();
    let dir = repo(&tasks, "");
    let root = dir.path();
    for id in ids {
        write(root, &record_path(id), &line_task(id, "todo", "Pending.\n"));
    }
    commit(root, "docs(records): target the line");
    git(root, &["branch", LINE, "main"]);
    dir
}

/// A commit on `branch`, cut from [`LINE`], writing `path`; returns it.
fn build(root: &Path, branch: &str, path: &str) -> String {
    git(root, &["switch", "-C", branch, LINE]);
    write(root, path, &format!("// {branch}\n"));
    commit(root, "feat: build on the line")
}

/// Land `branch` on [`LINE`] with a merge commit; returns the merge.
fn land(root: &Path, branch: &str) -> String {
    git(root, &["switch", LINE]);
    git(
        root,
        &[
            "merge",
            "--no-ff",
            "-m",
            &format!("merge: {branch}"),
            branch,
        ],
    );
    head(root)
}

/// Write `id` as `status` with a valid block reviewed at `reviewed`.
fn write_done(root: &Path, id: &str, status: &str, reviewed: &str) {
    write(
        root,
        &record_path(id),
        &line_task(id, status, &valid_block(reviewed)),
    );
}

/// The binding findings of a result, as its needle list sees them.
fn binding_lines(result: &(i32, String)) -> Vec<&str> {
    result
        .1
        .lines()
        .filter(|line| line.contains("TSK-00") && line.contains("reviewed commit"))
        .collect()
}

/// The line lands on main as one pull request. Two tasks each completed in
/// their own pull request (rule 1) and landed in turn, so the second
/// landing merge also brings the first task's code; a third task landed
/// first and completed later by a planning pull request (rule 2), and a
/// planning amendment then narrows a completed record. Each completion is
/// judged where it was introduced, not at the line tip.
#[test]
fn the_line_into_main_binds_each_completion_where_it_was_made() {
    let dir = line_repo(&["TSK-001", "TSK-002", "TSK-003"]);
    let root = dir.path();
    let first = build(root, "task/TSK-001-one", "src/one.rs");
    write_done(root, "TSK-001", "complete", &first);
    commit(root, "docs(records): complete TSK-001");
    let second = build(root, "task/TSK-002-two", "src/two.rs");
    write_done(root, "TSK-002", "complete", &second);
    commit(root, "docs(records): complete TSK-002");
    let third = build(root, "task/TSK-003-three", "src/three.rs");
    land(root, "task/TSK-001-one");
    land(root, "task/TSK-002-two");
    land(root, "task/TSK-003-three");
    build(root, "feat/other", "src/other.rs");
    land(root, "feat/other");
    git(root, &["switch", "-C", "plan/complete-tsk003", LINE]);
    write_done(root, "TSK-003", "complete", &third);
    commit(root, "docs(records): complete TSK-003");
    land(root, "plan/complete-tsk003");
    // A later planning amendment of a completed record is not part of its
    // completion.
    git(root, &["switch", "-C", "plan/narrow-tsk001", LINE]);
    let record = std::fs::read_to_string(root.join(record_path("TSK-001"))).unwrap();
    write(
        root,
        &record_path("TSK-001"),
        &record.replace("Work.\n", "Narrower work.\n"),
    );
    commit(root, "docs(records): narrow TSK-001");
    land(root, "plan/narrow-tsk001");

    let result = ci_on(root, "main", LINE, "");
    assert_passes(&result, "the line into main");
    assert!(result.1.contains(SCOPE), "{}", result.1);
}

/// A task landed without its completion is completed by a planning pull
/// request with `reviewed` at the landed head, after a planning merge, a
/// planning amendment of the record itself, or an unrelated code merge on
/// the line; the verb and CI agree.
#[test]
fn a_late_completion_binds_to_the_landing_merge() {
    let restated = EPIC.replace("An outcome.", "An outcome, restated.");
    let narrowed =
        line_task("TSK-001", "todo", "Pending.\n").replace("Work.\n", "Narrower work.\n");
    let amended = record_path("TSK-001");
    for (what, other, content) in [
        (
            "after a planning merge",
            "project-management/epics/EPC-001.md",
            restated.as_str(),
        ),
        (
            "after an amendment of the record",
            amended.as_str(),
            narrowed.as_str(),
        ),
        (
            "after an unrelated code merge",
            "src/other.rs",
            "pub fn other() {}\n",
        ),
    ] {
        let dir = line_repo(&["TSK-001"]);
        let root = dir.path();
        let reviewed = build(root, "task/TSK-001-work", "src/work.rs");
        land(root, "task/TSK-001-work");
        git(root, &["switch", "-C", "other/change", LINE]);
        write(root, other, content);
        commit(root, "docs: another change");
        land(root, "other/change");

        git(root, &["switch", "-C", "plan/complete-tsk001", LINE]);
        let done = line_task("TSK-001", "in_progress", &valid_block(&reviewed));
        let done = if other == amended {
            done.replace("Work.\n", "Narrower work.\n")
        } else {
            done
        };
        write(root, &amended, &done);
        let verb = status_complete(root, "TSK-001");
        assert_eq!(verb.0, 0, "{what}: the verb: {}", verb.1);
        commit(root, "docs(records): complete TSK-001");
        let result = ci_on(root, LINE, "plan/complete-tsk001", "");
        assert_passes(&result, what);
        assert!(binding_lines(&result).is_empty(), "{what}: {}", result.1);
    }
}

/// A late completion fails when the landing merge is not the clean
/// re-merge of its parents (an evil merge), when code lands directly on
/// the line after it, when the task landed by a squash (no merge has the
/// reviewed commit as a parent), or when the completion itself changes
/// code. The verb refuses each before it writes, and CI blocks it.
#[test]
fn a_late_completion_fails_without_a_clean_landing() {
    for (what, needle) in [
        ("an evil merge", "is not the clean re-merge of its parents"),
        (
            "a direct code commit on the line",
            "changes src/direct.rs after the landing merge",
        ),
        ("a squash landing", "is not the head or an ancestor of it"),
        (
            "a completion that changes code",
            "the completion also changes src/extra.rs",
        ),
    ] {
        let dir = line_repo(&["TSK-001"]);
        let root = dir.path();
        let reviewed = build(root, "task/TSK-001-work", "src/work.rs");
        git(root, &["switch", LINE]);
        match what {
            "an evil merge" => {
                git(
                    root,
                    &["merge", "--no-ff", "--no-commit", "task/TSK-001-work"],
                );
                write(root, "src/evil.rs", "pub fn evil() {}\n");
                commit(root, "merge: task/TSK-001-work");
            }
            "a squash landing" => {
                git(root, &["merge", "--squash", "task/TSK-001-work"]);
                commit(root, "feat: squash TSK-001");
            }
            _ => {
                land(root, "task/TSK-001-work");
            }
        }
        if what == "a direct code commit on the line" {
            write(root, "src/direct.rs", "pub fn direct() {}\n");
            commit(root, "feat: straight onto the line");
        }

        git(root, &["switch", "-C", "plan/complete-tsk001", LINE]);
        if what == "a completion that changes code" {
            write(root, "src/extra.rs", "pub fn extra() {}\n");
        }

        write_done(root, "TSK-001", "in_progress", &reviewed);
        let verb = status_complete(root, "TSK-001");
        assert_ne!(verb.0, 0, "{what}: the verb: {}", verb.1);
        assert!(verb.1.contains(needle), "{what}: the verb: {}", verb.1);

        write_done(root, "TSK-001", "complete", &reviewed);
        commit(root, "docs(records): complete TSK-001");
        assert_blocks(
            &ci_on(root, LINE, "plan/complete-tsk001", ""),
            what,
            &["work.acceptance_binding", needle],
        );
    }
}

/// One planning commit completes two landed tasks; each binds to its own
/// landing merge.
#[test]
fn one_pull_request_completes_two_landed_tasks() {
    let dir = line_repo(&["TSK-001", "TSK-002"]);
    let root = dir.path();
    let first = build(root, "task/TSK-001-one", "src/one.rs");
    let second = build(root, "task/TSK-002-two", "src/two.rs");
    land(root, "task/TSK-001-one");
    land(root, "task/TSK-002-two");
    git(root, &["switch", "-C", "plan/complete-both", LINE]);
    write_done(root, "TSK-001", "complete", &first);
    write_done(root, "TSK-002", "complete", &second);
    commit(root, "docs(records): complete TSK-001 and TSK-002");
    let result = ci_on(root, LINE, "plan/complete-both", "");
    assert_passes(&result, "two completions in one commit");
    assert!(binding_lines(&result).is_empty(), "{}", result.1);
}

/// Known and accepted: a later fix pull request for the same task is a
/// merge after the landing merge, so a completion reviewed before the fix
/// still binds by the merge rule. cf-reviewer refuses a block whose
/// reviewed commit predates a later fix; the binding does not see it.
#[test]
fn known_hole_a_later_fix_for_the_same_task_still_binds() {
    let dir = line_repo(&["TSK-001"]);
    let root = dir.path();
    let reviewed = build(root, "task/TSK-001-work", "src/work.rs");
    land(root, "task/TSK-001-work");
    build(root, "fix/TSK-001-follow-up", "src/work.rs");
    land(root, "fix/TSK-001-follow-up");
    git(root, &["switch", "-C", "plan/complete-tsk001", LINE]);
    write_done(root, "TSK-001", "complete", &reviewed);
    commit(root, "docs(records): complete TSK-001");
    assert_passes(
        &ci_on(root, LINE, "plan/complete-tsk001", ""),
        "a completion reviewed before a later fix",
    );
}

/// [`line_task`] with AC-1 narrowed, as a planning amendment leaves it.
fn amended_task(id: &str, status: &str, closeout: &str) -> String {
    line_task(id, status, closeout).replace("shall work.", "shall work on Linux.")
}

/// A block reviewed at `reviewed` whose AC-1 is waived by `by`.
fn waived_block(reviewed: &str, by: &str) -> String {
    block(
        reviewed,
        &[
            &format!("AC-1: waived | {by}"),
            "AC-2: verified | journey ran",
        ],
        "verified | tests/journey.rs",
        "none: nothing deferred",
    )
}

/// Amend TSK-001 AC-1 on `branch`, cut from [`LINE`]; returns the commit.
fn amend(root: &Path, branch: &str) -> String {
    git(root, &["switch", "-C", branch, LINE]);
    write(
        root,
        &record_path("TSK-001"),
        &amended_task("TSK-001", "todo", "Pending.\n"),
    );
    commit(root, "docs(records): narrow TSK-001 AC-1")
}

/// A waiver is judged against the task's own integration target, not the
/// pull request's base: an amendment landed on the line binds in the task
/// pull request and again when the line lands on main.
#[test]
fn a_waiver_amended_on_the_line_binds_into_main() {
    let dir = line_repo(&["TSK-001"]);
    let root = dir.path();
    amend(root, "plan/narrow");
    let amendment = land(root, "plan/narrow");
    let reviewed = build(root, "task/TSK-001-work", "src/work.rs");
    write(
        root,
        &record_path("TSK-001"),
        &amended_task("TSK-001", "complete", &waived_block(&reviewed, &amendment)),
    );
    commit(root, "docs(records): complete TSK-001");
    assert_passes(
        &ci_on(root, LINE, "task/TSK-001-work", "Task: TSK-001"),
        "the task pull request into its line",
    );
    land(root, "task/TSK-001-work");
    let result = ci_on(root, "main", LINE, "");
    assert_passes(&result, "the line into main");
    assert!(binding_lines(&result).is_empty(), "{}", result.1);
}

/// A waiver amendment fails when it never landed on the task's line (it is
/// only on a side branch the task merged), when the completion does not
/// contain it, or when it names the task branch's own head, which is not on
/// the line.
#[test]
fn a_waiver_amendment_must_have_landed_on_the_task_line() {
    for (what, needle) in [
        ("only on a side branch", "which is not on the target"),
        (
            "landed after the task branched",
            "which the completion does not contain",
        ),
        ("the task branch's head", "which is not on the target"),
    ] {
        let dir = line_repo(&["TSK-001"]);
        let root = dir.path();
        let (reviewed, amendment) = match what {
            "only on a side branch" => {
                let amendment = amend(root, "side/narrow");
                git(root, &["switch", "-C", "task/TSK-001-work", LINE]);
                git(
                    root,
                    &["merge", "--no-ff", "-m", "merge: side", "side/narrow"],
                );
                write(root, "src/work.rs", "pub fn work() {}\n");
                (commit(root, "feat: build on the line"), amendment)
            }
            "landed after the task branched" => {
                let reviewed = build(root, "task/TSK-001-work", "src/work.rs");
                amend(root, "plan/narrow");
                let amendment = land(root, "plan/narrow");
                git(root, &["switch", "task/TSK-001-work"]);
                (reviewed, amendment)
            }
            _ => {
                amend(root, "plan/narrow");
                land(root, "plan/narrow");
                let reviewed = build(root, "task/TSK-001-work", "src/work.rs");
                (reviewed.clone(), reviewed)
            }
        };
        write(
            root,
            &record_path("TSK-001"),
            &amended_task(
                "TSK-001",
                "in_progress",
                &waived_block(&reviewed, &amendment),
            ),
        );
        let verb = status_complete(root, "TSK-001");
        assert_ne!(verb.0, 0, "{what}: the verb: {}", verb.1);
        assert!(verb.1.contains(needle), "{what}: the verb: {}", verb.1);
        if what == "the task branch's head" {
            continue;
        }
        write(
            root,
            &record_path("TSK-001"),
            &amended_task("TSK-001", "complete", &waived_block(&reviewed, &amendment)),
        );
        commit(root, "docs(records): complete TSK-001");
        assert_blocks(
            &ci_on(root, LINE, "task/TSK-001-work", "Task: TSK-001"),
            what,
            &["AC-1 waiver", needle],
        );
    }
}

/// A late completion cut exactly at the amendment merge completes: the
/// verb's `HEAD` is the completion's parent, so naming it as the waiver's
/// amendment is not naming the head.
#[test]
fn a_completion_cut_at_the_amendment_merge_binds() {
    let dir = line_repo(&["TSK-001"]);
    let root = dir.path();
    let reviewed = build(root, "task/TSK-001-work", "src/work.rs");
    land(root, "task/TSK-001-work");
    amend(root, "plan/narrow");
    let amendment = land(root, "plan/narrow");
    git(root, &["switch", "-C", "plan/complete-tsk001", LINE]);
    assert_eq!(head(root), amendment, "cut at the amendment merge");
    write(
        root,
        &record_path("TSK-001"),
        &amended_task(
            "TSK-001",
            "in_progress",
            &waived_block(&reviewed, &amendment),
        ),
    );
    let verb = status_complete(root, "TSK-001");
    assert_eq!(verb.0, 0, "the verb: {}", verb.1);
    commit(root, "docs(records): complete TSK-001");
    let result = ci_on(root, LINE, "plan/complete-tsk001", "");
    assert_passes(&result, "the completion pull request");
    assert!(binding_lines(&result).is_empty(), "{}", result.1);
}
