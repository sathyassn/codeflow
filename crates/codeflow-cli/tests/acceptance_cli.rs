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
    let out = codeflow()
        .args([
            "ci",
            "--base",
            "main",
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
    write(root, "docs/note.md", "# Note\n");
    let unrelated = commit(root, "docs: add a note");

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
