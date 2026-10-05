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
    let task_line = if task_line.is_empty() {
        "Task: EPC-001"
    } else {
        task_line
    };
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
                "## Summary\nA change.\n\n- one change\n\n{task_line}\n\n## Changes\n- one\n\n## Testing\n- test\n"
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

/// `codeflow ci` from `main` to `HEAD` as `branch` with no pull request
/// body, as the pre-push hook runs it.
fn ci_push(root: &Path, branch: &str) -> (i32, String) {
    let out = codeflow()
        .args(["ci", "--base", "main", "--head", "HEAD", "--branch", branch])
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
        &["AC-1 waiver", "strictly before the reviewed revision"],
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

/// Own-task criteria may change; other records remain frozen even at warn.
#[test]
fn own_task_criteria_delta_is_printed_and_other_records_stay_frozen() {
    let dir = repo(
        &[("TSK-001", OWN_JOURNEY), ("TSK-002", OWN_JOURNEY)],
        ", \"work_records\": \"warn\"",
    );
    let root = dir.path();
    code_change(root, BRANCH, "pub fn work() {}\n");
    let loosened = OWN_JOURNEY.replace("shall work.", "shall mostly work.");
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", &loosened, "Pending.\n"),
    );
    commit(root, "docs(records): loosen AC-1");
    let result = ci(root, BRANCH, "TSK-001");
    assert_passes(&result, "own criteria amendment");
    assert!(result.1.contains("AC-1 changed"), "{}", result.1);
    assert!(result.1.contains("shall mostly work"), "{}", result.1);
    assert!(
        result.1.contains("changes its own criteria"),
        "{}",
        result.1
    );
    assert!(!result.1.contains("completion is bound"), "{}", result.1);
    // A push has no body yet; the branch's own task still decides.
    let pushed = ci_push(root, BRANCH);
    assert_passes(&pushed, "own criteria amendment on push");
    assert!(
        pushed.1.contains("changes its own criteria"),
        "{}",
        pushed.1
    );

    write(
        root,
        &record_path("TSK-002"),
        &task("TSK-002", "todo", &loosened, "Pending.\n"),
    );
    commit(root, "docs: change another task criterion");
    assert_blocks(
        &ci(root, BRANCH, "TSK-001"),
        "other criteria remain frozen",
        &["work.criteria_frozen", "TSK-002 changes its criteria"],
    );
    assert_blocks(
        &ci_push(root, BRANCH),
        "other criteria remain frozen on push",
        &["work.criteria_frozen", "TSK-002 changes its criteria"],
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
    assert_passes(
        &ci(root, "plan/code-with-criteria", "TSK-001"),
        "own task amendment regardless of prefix",
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
        (
            "the task branch's head",
            "strictly before the reviewed revision",
        ),
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
        if what == "only on a side branch" {
            assert_eq!(verb.0, 0, "record-only amendment in own range: {}", verb.1);
            continue;
        }
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

#[test]
fn planning_reopen_criteria_freeze_precedes_amendment_exemption() {
    for recomplete in [false, true] {
        for changed in [false, true] {
            let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
            let root = dir.path();
            let reviewed = head(root);
            let old = block(
                &reviewed,
                &["AC-1: verified | test", "AC-2: verified | journey"],
                "verified | test",
                "none: done",
            );
            write(
                root,
                &record_path("TSK-001"),
                &task("TSK-001", "complete", OWN_JOURNEY, &old),
            );
            commit(root, "docs: complete task");
            git(root, &["switch", "-c", "plan/reopen"]);
            let retired = old.replace(
                "acceptance:",
                "acceptance_superseded:\n  reason: regression",
            );
            let criteria = if changed {
                OWN_JOURNEY.replace("shall work.", "shall mostly work.")
            } else {
                OWN_JOURNEY.to_string()
            };
            write(
                root,
                &record_path("TSK-001"),
                &task("TSK-001", "todo", &criteria, &retired),
            );
            let revision = commit(root, "docs: reopen task");
            if recomplete {
                let active = block(
                    &revision,
                    &["AC-1: verified | test", "AC-2: verified | journey"],
                    "verified | test",
                    "none: done",
                );
                write(
                    root,
                    &record_path("TSK-001"),
                    &task(
                        "TSK-001",
                        "complete",
                        &criteria,
                        &format!("{retired}\n{active}"),
                    ),
                );
                commit(root, "docs: complete task again");
            }
            let result = ci_with(root, "plan/reopen", "Task: EPC-001");
            if changed {
                assert_blocks(
                    &result,
                    "reopen criteria frozen",
                    &["work.criteria_frozen", "reopened task keeps its criteria"],
                );
            } else {
                assert_passes(&result, "equal criteria reopen");
            }
        }
    }
}

#[test]
fn clean_line_merge_after_review_preserves_binding_but_product_resolution_does_not() {
    for resolved in [false, true] {
        let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
        let root = dir.path();
        let reviewed = code_change(root, BRANCH, "pub fn work() {}\n");
        git(root, &["switch", "main"]);
        write(root, "src/line.rs", "pub fn line() {}\n");
        commit(root, "feat: advance line");
        git(root, &["switch", BRANCH]);
        git(root, &["merge", "--no-ff", "--no-commit", "main"]);
        if resolved {
            write(root, "src/lib.rs", "pub fn unreviewed_resolution() {}\n");
        }
        commit(root, "chore: merge line");
        complete(root, "TSK-001", OWN_JOURNEY, &valid_block(&reviewed));
        let result = ci(root, BRANCH, "TSK-001");
        if resolved {
            assert_blocks(&result, "product resolution", &["work.acceptance_binding"]);
        } else {
            assert_passes(&result, "clean line merge after review");
        }
    }
}

#[test]
fn own_range_waiver_requires_a_record_only_amendment_before_review() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    code_change(root, BRANCH, "pub fn work() {}\n");
    let revised = OWN_JOURNEY.replace("shall work.", "shall mostly work.");
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", &revised, "Pending.\n"),
    );
    let amendment = commit(root, "docs: amend own criterion");
    write(root, "src/lib.rs", "pub fn reviewed_work() {}\n");
    let reviewed = commit(root, "feat: implement amended criterion");
    let waiver = block(
        &reviewed,
        &[
            &format!("AC-1: waived | {amendment}"),
            "AC-2: verified | journey",
        ],
        "verified | test",
        "none: done",
    );
    complete(root, "TSK-001", &revised, &waiver);
    assert_passes(
        &ci(root, BRANCH, "TSK-001"),
        "record-only pre-review waiver",
    );
    git(root, &["checkout", "--detach"]);
    assert_passes(&ci(root, BRANCH, "TSK-001"), "detached CI own-range waiver");
    git(root, &["switch", BRANCH]);
    let release = codeflow_core::workgraph::acceptance::pull_request_findings(
        root,
        "main",
        "HEAD",
        &codeflow_core::workgraph::acceptance::Criteria::Amendable,
    )
    .unwrap();
    assert!(
        release.iter().any(|finding| !finding.note
            && finding.message.contains("waiver")
            && finding.message.contains("which is not on the target")),
        "a release range cannot borrow the checkout's task context: {release:?}"
    );
}

#[test]
fn planning_branch_older_than_completion_is_not_a_reopen() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    git(root, &["switch", "-c", "plan/amend"]);
    write(root, "docs/plan/extra.md", "planning work\n");
    commit(root, "docs: plan more");
    git(root, &["switch", "main"]);
    let reviewed = head(root);
    complete(root, "TSK-001", OWN_JOURNEY, &valid_block(&reviewed));
    git(root, &["switch", "plan/amend"]);
    git(
        root,
        &["merge", "--no-ff", "-m", "chore: update plan base", "main"],
    );
    let file = root.join(record_path("TSK-001"));
    let text = std::fs::read_to_string(&file)
        .unwrap()
        .replace("shall work.", "shall mostly work.");
    std::fs::write(file, text).unwrap();
    commit(root, "docs: amend completed task");
    assert_passes(
        &ci_with(root, "plan/amend", "Task: EPC-001"),
        "ordinary planning amendment after merging completion",
    );
}

#[test]
fn a_batch_binds_two_heads_and_only_refuses_the_hand_resolved_task() {
    for hand_resolved in [false, true] {
        let dir = line_repo(&["TSK-001", "TSK-002"]);
        let root = dir.path();
        let first = build(root, "task/TSK-001-first", "src/first.rs");
        write_done(root, "TSK-001", "complete", &first);
        let first_completion = commit(root, "docs: complete first");
        let second = build(root, "task/TSK-002-second", "src/second.rs");
        land(root, "task/TSK-001-first");
        git(root, &["switch", "task/TSK-002-second"]);
        git(root, &["merge", "--no-ff", "--no-commit", LINE]);
        if hand_resolved {
            write(root, "src/second.rs", "// unreviewed resolution\n");
        }
        commit(root, "chore: merge line after review");
        write_done(root, "TSK-002", "complete", &second);
        let second_completion = commit(root, "docs: complete second");
        land(root, "task/TSK-002-second");
        let result = ci_on(root, "main", LINE, "Task: EPC-001");
        assert!(result.1.contains(&first_completion), "{}", result.1);
        assert!(result.1.contains(&second_completion), "{}", result.1);
        if hand_resolved {
            assert_blocks(
                &result,
                "only second binding fails",
                &["TSK-002", "work.acceptance_binding"],
            );
            assert!(
                !binding_lines(&result)
                    .iter()
                    .any(|line| line.contains("TSK-001")),
                "{}",
                result.1
            );
        } else {
            assert_passes(&result, "two reviewed heads in one release range");
        }
    }
}

#[test]
fn a_landed_task_cannot_disguise_an_unreviewed_side_merge_as_a_line_merge() {
    let dir = line_repo(&["TSK-001"]);
    let root = dir.path();
    let reviewed = build(root, "task/TSK-001-work", "src/task.rs");
    build(root, "feat/unreviewed", "src/unreviewed.rs");
    git(root, &["switch", "task/TSK-001-work"]);
    git(
        root,
        &[
            "merge",
            "--no-ff",
            "-m",
            "chore: merge unreviewed side",
            "feat/unreviewed",
        ],
    );
    write_done(root, "TSK-001", "complete", &reviewed);
    commit(root, "docs: complete task");
    land(root, "task/TSK-001-work");
    let result = ci_on(root, "main", LINE, "Task: EPC-001");
    assert_blocks(
        &result,
        "unreviewed side is not line history",
        &["work.acceptance_binding", "TSK-001"],
    );
}

#[test]
fn prospective_recompletion_freezes_reopened_criteria_even_at_warn() {
    for changed in [false, true] {
        let dir = repo(&[("TSK-001", OWN_JOURNEY)], ", \"work_records\": \"warn\"");
        let root = dir.path();
        let reviewed = head(root);
        let old = valid_block(&reviewed);
        complete(root, "TSK-001", OWN_JOURNEY, &old);
        git(root, &["switch", "-c", BRANCH]);
        let retired = old.replace(
            "acceptance:",
            "acceptance_superseded:\n  reason: regression",
        );
        let criteria = if changed {
            OWN_JOURNEY.replace("shall work.", "shall mostly work.")
        } else {
            OWN_JOURNEY.into()
        };
        write(
            root,
            &record_path("TSK-001"),
            &task("TSK-001", "todo", &criteria, &retired),
        );
        let revision = commit(root, "docs: reopen task");
        write(
            root,
            &record_path("TSK-001"),
            &task(
                "TSK-001",
                "todo",
                &criteria,
                &format!("{retired}\n{}", valid_block(&revision)),
            ),
        );
        let before = std::fs::read(root.join(record_path("TSK-001"))).unwrap();
        let result = status_complete(root, "TSK-001");
        if changed {
            assert_blocks(
                &result,
                "prospective reopen freeze",
                &["reopened task keeps its criteria"],
            );
            assert_eq!(
                std::fs::read(root.join(record_path("TSK-001"))).unwrap(),
                before
            );
        } else {
            assert_passes(&result, "equal-criteria recompletion");
        }
    }
}

#[test]
fn mentioning_a_superseded_block_is_not_itself_a_reopen() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    let reviewed = head(root);
    complete(root, "TSK-001", OWN_JOURNEY, &valid_block(&reviewed));
    git(root, &["switch", "-c", "plan/amend"]);
    let path = root.join(record_path("TSK-001"));
    let text = std::fs::read_to_string(&path)
        .unwrap()
        .replace(
            "Work.\n",
            "Work. An example token is `acceptance_superseded:`.\n",
        )
        .replace("shall work.", "shall mostly work.");
    std::fs::write(path, text).unwrap();
    commit(root, "docs: amend without reopening");
    assert_passes(
        &ci_with(root, "plan/amend", "Task: EPC-001"),
        "a quoted field is not a reopen transition",
    );
}

fn fix_block(reviewed: &str) -> String {
    block(
        reviewed,
        &["AC-1: verified | unit", "AC-2: verified | journey"],
        "verified | journey",
        "none: nothing deferred",
    )
}

/// A completed task on main, then a fix branch carrying its archived review.
fn one_pr_fix() -> (tempfile::TempDir, String, String) {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    let reviewed = code_change(root, BRANCH, "pub fn initial() {}\n");
    let old = fix_block(&reviewed);
    complete(root, "TSK-001", OWN_JOURNEY, &old);
    git(root, &["switch", "main"]);
    git(root, &["merge", "--ff-only", BRANCH]);
    git(root, &["switch", "-c", "task/TSK-001-fix"]);
    let archived = old.replace(
        "acceptance:\n",
        "acceptance_superseded:\n  reason: regression\n",
    );
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", OWN_JOURNEY, &archived),
    );
    commit(root, "docs: reopen the task");
    (dir, old, archived)
}

#[test]
fn one_pr_fix_accepts_reopen_fix_and_recompletion() {
    let (dir, _, archived) = one_pr_fix();
    let root = dir.path();
    let start = codeflow()
        .args(["work", "start", "TSK-001"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        start.status.success(),
        "{}{}",
        String::from_utf8_lossy(&start.stdout),
        String::from_utf8_lossy(&start.stderr)
    );
    write(root, "src/lib.rs", "pub fn fixed() {}\n");
    let reviewed = commit(root, "fix: repair the regression");
    write(
        root,
        &record_path("TSK-001"),
        &task(
            "TSK-001",
            "todo",
            OWN_JOURNEY,
            &format!("{archived}{}", fix_block(&reviewed)),
        ),
    );
    let done = codeflow()
        .args(["task", "status", "TSK-001", "complete"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        done.status.success(),
        "{}{}",
        String::from_utf8_lossy(&done.stdout),
        String::from_utf8_lossy(&done.stderr)
    );
    commit(root, "docs: complete the fixed task");
    assert_passes(
        &ci(root, "task/TSK-001-fix", "TSK-001"),
        "one pull request fix",
    );
}

#[test]
fn one_pr_fix_copied_review_is_bound_even_when_unchanged() {
    let (dir, old, archived) = one_pr_fix();
    let root = dir.path();
    write(root, "src/lib.rs", "pub fn unreviewed_fix() {}\n");
    commit(root, "fix: repair the regression");
    complete(root, "TSK-001", OWN_JOURNEY, &format!("{archived}{old}"));
    assert_blocks(
        &ci(root, "task/TSK-001-fix", "TSK-001"),
        "copied review",
        &["work.acceptance_binding", "reviewed commit"],
    );
}

#[test]
fn one_pr_fix_must_preserve_the_anchored_review() {
    for fault in ["dropped", "edited", "reason", "criteria"] {
        let (dir, _, archived) = one_pr_fix();
        let root = dir.path();
        write(root, "src/lib.rs", "pub fn fixed() {}\n");
        let reviewed = commit(root, "fix: repair the regression");
        let archive = match fault {
            "dropped" => String::new(),
            "edited" => archived.replace("verified | unit", "verified | invented"),
            "reason" => archived.replace("  reason: regression\n", ""),
            _ => archived,
        };
        let criteria = if fault == "criteria" {
            OWN_JOURNEY.replace("shall work", "may work")
        } else {
            OWN_JOURNEY.to_string()
        };
        complete(
            root,
            "TSK-001",
            &criteria,
            &format!("{archive}{}", fix_block(&reviewed)),
        );
        let reason = if fault == "criteria" {
            "criteria"
        } else {
            "reopen"
        };
        assert_blocks(&ci(root, "task/TSK-001-fix", "TSK-001"), fault, &[reason]);
    }
}

fn assert_invalid_fix(fault: &str) {
    let (dir, old, archived) = one_pr_fix();
    let root = dir.path();
    let criteria = if fault.contains("criteria") {
        OWN_JOURNEY.replace("shall work", "shall work sometimes")
    } else {
        OWN_JOURNEY.to_string()
    };
    if fault != "records-only-criteria" {
        write(root, "src/lib.rs", "pub fn fixed() {}\n");
    }
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", &criteria, &archived),
    );
    let reviewed = commit(root, "fix: repair the regression");
    if fault == "later-code" {
        write(root, "src/lib.rs", "pub fn later() {}\n");
        commit(root, "fix: change after review");
    }
    let archive = match fault {
        "dropped" => String::new(),
        "edited" => archived.replace("verified | unit", "verified | invented"),
        "reason" => archived.replace("  reason: regression\n", ""),
        _ => archived,
    };
    let active = match fault {
        "copied" => old,
        "missing" => fix_block(&reviewed).replace("    AC-1: verified | unit\n", ""),
        "unverified" => fix_block(&reviewed).replace("AC-1: verified", "AC-1: deferred"),
        "waiver" => {
            fix_block(&reviewed).replace("verified | unit", &format!("waived | {reviewed}"))
        }
        _ => fix_block(&reviewed),
    };
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", &criteria, &format!("{archive}{active}")),
    );
    let out = codeflow()
        .args(["task", "status", "TSK-001", "complete"])
        .current_dir(root)
        .output()
        .unwrap();
    let message = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let reason = match fault {
        "copied" => "inside the fix range",
        "later-code" => "after the reviewed commit",
        "dropped" | "edited" => "old acceptance block",
        "reason" => "reason",
        "missing" | "unverified" => "AC-1",
        "criteria" | "records-only-criteria" => "reopened task keeps its criteria",
        "waiver" => "waiver",
        _ => unreachable!(),
    };
    assert!(
        !out.status.success() && message.contains(reason),
        "{fault}: {message}"
    );
    complete(root, "TSK-001", &criteria, &format!("{archive}{active}"));
    assert_blocks(&ci(root, "task/TSK-001-fix", "TSK-001"), fault, &[reason]);
}

macro_rules! fix_faults {
    ($($test:ident: $fault:literal),* $(,)?) => {$(
        #[test]
        fn $test() { assert_invalid_fix($fault); }
    )*};
}

fix_faults! {
    one_pr_fix_copied_block: "copied",
    one_pr_fix_code_after_review: "later-code",
    one_pr_fix_dropped_old_block: "dropped",
    one_pr_fix_edited_old_block: "edited",
    one_pr_fix_missing_reason: "reason",
    one_pr_fix_missing_criterion: "missing",
    one_pr_fix_unverified_criterion: "unverified",
    one_pr_fix_changed_criterion: "criteria",
    one_pr_fix_records_only_criterion: "records-only-criteria",
    one_pr_fix_waiver_without_amendment: "waiver",
}

#[test]
fn one_pr_fix_after_late_completion_cannot_reuse_the_landed_review() {
    let dir = line_repo(&["TSK-001"]);
    let root = dir.path();
    let reviewed = build(root, "task/TSK-001-work", "src/work.rs");
    land(root, "task/TSK-001-work");
    git(root, &["switch", "-c", "plan/complete", LINE]);
    write_done(root, "TSK-001", "complete", &reviewed);
    commit(root, "docs: complete the task late");
    land(root, "plan/complete");
    git(root, &["switch", "-c", "task/TSK-001-fix", LINE]);
    let reopen = codeflow()
        .args([
            "task",
            "status",
            "TSK-001",
            "todo",
            "--reason",
            "regression",
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(reopen.status.success());
    commit(root, "docs: reopen the task");
    git(root, &["switch", "-c", "fix/implementation"]);
    write(root, "src/work.rs", "// fixed\n");
    commit(root, "fix: repair the regression");
    git(root, &["switch", "task/TSK-001-fix"]);
    git(
        root,
        &[
            "merge",
            "--no-ff",
            "-m",
            "chore: merge the fix",
            "fix/implementation",
        ],
    );
    let mut record = std::fs::read_to_string(root.join(record_path("TSK-001"))).unwrap();
    record.push_str(&valid_block(&reviewed));
    write(root, &record_path("TSK-001"), &record);
    let out = codeflow()
        .args(["task", "status", "TSK-001", "complete"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "old review accepted after merged fix"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("inside the fix range"));
    write(
        root,
        &record_path("TSK-001"),
        &record.replace("status: todo", "status: complete"),
    );
    commit(root, "docs: re-complete the task");
    assert_blocks(
        &ci_on(root, LINE, "task/TSK-001-fix", "Task: TSK-001"),
        "late merged fix",
        &["inside the fix range"],
    );
}

#[test]
fn a_late_completion_carries_an_ancestor_review_past_unrelated_line_work() {
    let dir = line_repo(&["TSK-001"]);
    let root = dir.path();
    let reviewed = build(root, "task/TSK-001-work", "src/work.rs");
    write(
        root,
        &record_path("TSK-001"),
        &line_task("TSK-001", "todo", "Reviewed; completion follows landing.\n"),
    );
    commit(root, "docs: record the reviewed state");
    git(root, &["switch", LINE]);
    write(root, "src/unrelated.rs", "// independent line work\n");
    commit(root, "feat: add unrelated line work");
    land(root, "task/TSK-001-work");
    git(root, &["switch", "-c", "plan/complete", LINE]);
    write_done(root, "TSK-001", "todo", &reviewed);
    let out = codeflow()
        .args(["task", "status", "TSK-001", "complete"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    commit(root, "docs: complete the landed task");
    assert_passes(&ci_on(root, LINE, "plan/complete", ""), "R C P M D landing");
}

/// Complete a task after its first reviewed work has landed on its line.
fn late_completed_line() -> (tempfile::TempDir, String, String) {
    let dir = line_repo(&["TSK-001"]);
    let root = dir.path();
    let reviewed = build(root, "task/TSK-001-work", "src/work.rs");
    land(root, "task/TSK-001-work");
    git(root, &["switch", "-c", "plan/complete-first", LINE]);
    write_done(root, "TSK-001", "complete", &reviewed);
    commit(root, "docs: complete task after landing");
    land(root, "plan/complete-first");
    let archived = valid_block(&reviewed).replace(
        "acceptance:\n",
        "acceptance_superseded:\n  reason: regression\n",
    );
    (dir, reviewed, archived)
}

/// A todo range base can hide the completed record whose review a reopen
/// must supersede. Every record transition below lands through a merge.
#[test]
fn a_fully_merged_recompletion_cannot_reuse_review_across_a_reopen() {
    let (dir, reviewed, archived) = late_completed_line();
    let root = dir.path();
    git(root, &["switch", "-c", "plan/reopen", LINE]);
    write(
        root,
        &record_path("TSK-001"),
        &line_task("TSK-001", "todo", &archived),
    );
    commit(root, "docs: reopen task");
    land(root, "plan/reopen");

    git(root, &["switch", "-c", "fix/implementation", LINE]);
    write(root, "src/work.rs", "// fixed after review\n");
    commit(root, "fix: change reviewed work");
    land(root, "fix/implementation");

    git(root, &["switch", "-c", "plan/recomplete", LINE]);
    write(
        root,
        &record_path("TSK-001"),
        &line_task(
            "TSK-001",
            "complete",
            &format!("{archived}{}", valid_block(&reviewed)),
        ),
    );
    commit(root, "docs: reuse the earlier review");
    land(root, "plan/recomplete");

    // No task declaration selects the epic-line class, whose criteria are
    // amendable; a declared task selects the frozen tracked-task class.
    for task_line in ["", "Task: TSK-001"] {
        assert_blocks(
            &ci_on(root, "main", LINE, task_line),
            &format!("fully merged stale review with {task_line:?}"),
            &["must lie inside the fix range"],
        );
    }
}

/// A separate reopen pull request, then a fix branched after it that lands
/// by a clean task landing while other work lands on the line, then a late
/// completion reviewed at the fix: the landing carries that review, as for
/// any late completion (R-119 keeps a separate reopen pull request valid).
/// A direct code commit after the landing is still refused. This is
/// TSK-102's history on EPC-020 (reopen #612, fix #615, completion
/// `66c995192`).
#[test]
fn a_separate_reopen_then_a_landed_fix_binds_a_late_recompletion() {
    for direct in [false, true] {
        let (dir, _, archived) = late_completed_line();
        let root = dir.path();
        git(root, &["switch", "-c", "plan/reopen", LINE]);
        write(
            root,
            &record_path("TSK-001"),
            &line_task("TSK-001", "todo", &archived),
        );
        commit(root, "docs: reopen task");
        land(root, "plan/reopen");

        git(root, &["switch", "-c", "task/TSK-001-fix", LINE]);
        write(root, "src/work.rs", "// fixed after the reopen\n");
        let fixed = commit(root, "fix: repair the reopened work");
        git(root, &["switch", "-c", "other/change", LINE]);
        write(root, "src/other.rs", "pub fn other() {}\n");
        commit(root, "feat: other line work");
        land(root, "other/change");
        land(root, "task/TSK-001-fix");
        if direct {
            write(root, "src/direct.rs", "pub fn direct() {}\n");
            commit(root, "feat: straight onto the line");
        }

        git(root, &["switch", "-c", "plan/recomplete", LINE]);
        let closeout = format!("{archived}{}", valid_block(&fixed));
        write(
            root,
            &record_path("TSK-001"),
            &line_task("TSK-001", "todo", &closeout),
        );
        let verb = status_complete(root, "TSK-001");
        write(
            root,
            &record_path("TSK-001"),
            &line_task("TSK-001", "complete", &closeout),
        );
        commit(root, "docs: complete the fixed task late");
        let result = ci_on(root, LINE, "plan/recomplete", "");
        if direct {
            let needle = "changes src/direct.rs after the landing merge";
            assert_ne!(verb.0, 0, "the verb after a direct commit: {}", verb.1);
            assert!(verb.1.contains(needle), "{}", verb.1);
            assert_blocks(&result, "a direct commit after the fix", &[needle]);
        } else {
            assert_passes(&verb, "the verb after a separate reopen");
            assert_passes(&result, "a late completion after a separate reopen");
            land(root, "plan/recomplete");
            assert_passes(
                &ci_on(root, "main", LINE, ""),
                "the line carrying the reopen, fix and completion",
            );
        }
    }
}

/// The twin of the separate reopen: a pull request that reopens the task
/// itself owns its range (R-119), so a fix merged into it does not carry
/// its review past other code merged after it.
#[test]
fn a_one_pr_reopen_does_not_carry_a_merged_review_past_later_code() {
    let (dir, _, archived) = late_completed_line();
    let root = dir.path();
    git(root, &["switch", "-c", "feat/side", LINE]);
    write(root, "src/side.rs", "pub fn side() {}\n");
    commit(root, "feat: side work");
    git(root, &["switch", "-c", "task/TSK-001-fix", LINE]);
    write(
        root,
        &record_path("TSK-001"),
        &line_task("TSK-001", "todo", &archived),
    );
    commit(root, "docs: reopen the task");
    git(root, &["switch", "-c", "fix/implementation"]);
    write(root, "src/work.rs", "// fixed in the reopening range\n");
    let fixed = commit(root, "fix: repair the regression");
    git(root, &["switch", "task/TSK-001-fix"]);
    for branch in ["fix/implementation", "feat/side"] {
        git(
            root,
            &[
                "merge",
                "--no-ff",
                "-m",
                &format!("chore: merge {branch}"),
                branch,
            ],
        );
    }
    let closeout = format!("{archived}{}", valid_block(&fixed));
    write(
        root,
        &record_path("TSK-001"),
        &line_task("TSK-001", "complete", &closeout),
    );
    commit(root, "docs: re-complete the task");
    assert_blocks(
        &ci_on(root, LINE, "task/TSK-001-fix", "Task: TSK-001"),
        "a merged review carried past later code in a reopening range",
        &["src/side.rs changed after the reviewed commit"],
    );
}

/// TSK-170's history on EPC-020 (pull request 761): a task completed on its
/// branch, which then takes the line by a merge, is reopened and completed
/// again reviewed at that merge. The range base holds no completion, and
/// the review names the range's last change outside the record, so it binds
/// (AC-2). Naming the first review instead still refuses: the merge came
/// after it.
#[test]
fn a_completion_reopened_inside_its_own_range_binds_at_the_later_merge() {
    for stale in [false, true] {
        let dir = line_repo(&["TSK-001"]);
        let root = dir.path();
        let first = build(root, "task/TSK-001-work", "src/work.rs");
        write_done(root, "TSK-001", "complete", &first);
        commit(root, "docs: complete the task");
        git(root, &["switch", "-c", "other/change", LINE]);
        write(root, "src/other.rs", "pub fn other() {}\n");
        commit(root, "feat: other line work");
        land(root, "other/change");
        git(root, &["switch", "task/TSK-001-work"]);
        git(
            root,
            &["merge", "--no-ff", "-m", "chore: take the line", LINE],
        );
        let merged = head(root);
        let archived = valid_block(&first).replace(
            "acceptance:\n",
            "acceptance_superseded:\n  reason: review the line merge\n",
        );
        write(
            root,
            &record_path("TSK-001"),
            &line_task("TSK-001", "todo", &archived),
        );
        commit(root, "docs: reopen for the line merge");
        let reviewed = if stale { &first } else { &merged };
        write(
            root,
            &record_path("TSK-001"),
            &line_task(
                "TSK-001",
                "complete",
                &format!("{archived}{}", valid_block(reviewed)),
            ),
        );
        commit(root, "docs: complete at the line merge");
        let result = ci_on(root, LINE, "task/TSK-001-work", "Task: TSK-001");
        if stale {
            assert_blocks(
                &result,
                "the first review reused after the merge",
                &["must lie inside the fix range"],
            );
        } else {
            assert_passes(&result, "a review of the line merge");
        }
    }
}

/// The target checkout has no range base to reveal the completion that the
/// task record reopened before accepting a fresh status transition.
#[test]
fn target_checkout_status_cannot_reuse_review_after_a_merged_fix() {
    let (dir, reviewed, archived) = late_completed_line();
    let root = dir.path();
    git(root, &["switch", "-c", "plan/reopen", LINE]);
    write(
        root,
        &record_path("TSK-001"),
        &line_task("TSK-001", "todo", &archived),
    );
    commit(root, "docs: reopen task");
    land(root, "plan/reopen");

    git(root, &["switch", "-c", "fix/implementation", LINE]);
    write(root, "src/work.rs", "// fixed after review\n");
    commit(root, "fix: change reviewed work");
    land(root, "fix/implementation");

    write(
        root,
        &record_path("TSK-001"),
        &line_task(
            "TSK-001",
            "todo",
            &format!("{archived}{}", valid_block(&reviewed)),
        ),
    );
    let result = status_complete(root, "TSK-001");
    assert_ne!(result.0, 0, "stale review accepted on target: {}", result.1);
    assert!(
        result.1.contains("must lie inside the fix range"),
        "{}",
        result.1
    );
}

/// A criterion amended by its own planning pull request after the reopen
/// is the criterion a later completion keeps (R-52), while a completion
/// that changes it again is still refused.
#[test]
fn a_recompletion_keeps_a_criterion_amended_on_the_target() {
    let (dir, _, archived) = late_completed_line();
    let root = dir.path();
    git(root, &["switch", "-c", "plan/reopen", LINE]);
    write(
        root,
        &record_path("TSK-001"),
        &line_task("TSK-001", "todo", &archived),
    );
    commit(root, "docs: reopen task");
    land(root, "plan/reopen");

    let amended = |status: &str, closeout: &str, criterion: &str| {
        line_task("TSK-001", status, closeout).replace("shall work.", criterion)
    };
    git(root, &["switch", "-c", "plan/amend", LINE]);
    write(
        root,
        &record_path("TSK-001"),
        &amended("todo", &archived, "shall work on Linux."),
    );
    commit(root, "docs: amend the criterion");
    assert_passes(&ci_on(root, LINE, "plan/amend", ""), "planning amendment");
    land(root, "plan/amend");

    git(root, &["switch", "-c", "fix/implementation", LINE]);
    write(root, "src/work.rs", "// fixed after review\n");
    commit(root, "fix: change reviewed work");
    land(root, "fix/implementation");

    git(root, &["switch", "-c", "plan/loosen", LINE]);
    let fresh = head(root);
    let closeout = format!("{archived}{}", valid_block(&fresh));
    write(
        root,
        &record_path("TSK-001"),
        &amended("todo", &closeout, "shall mostly work on Linux."),
    );
    let loosened = status_complete(root, "TSK-001");
    assert_ne!(loosened.0, 0, "loosened criterion accepted: {}", loosened.1);
    assert!(
        loosened.1.contains("reopened task keeps its criteria"),
        "{}",
        loosened.1
    );
    write(
        root,
        &record_path("TSK-001"),
        &amended("complete", &closeout, "shall mostly work on Linux."),
    );
    commit(root, "docs: loosen and complete");
    assert_blocks(
        &ci_on(root, LINE, "plan/loosen", ""),
        "records-only loosening after an amendment",
        &["reopened task keeps its criteria"],
    );

    git(root, &["switch", "-c", "plan/recomplete", LINE]);
    write(
        root,
        &record_path("TSK-001"),
        &amended("todo", &closeout, "shall work on Linux."),
    );
    assert_passes(&status_complete(root, "TSK-001"), "amended criterion");
    commit(root, "docs: re-complete task");
    assert_passes(
        &ci_on(root, LINE, "plan/recomplete", ""),
        "re-completion with the amended criterion",
    );
    land(root, "plan/recomplete");
    assert_passes(
        &ci_on(root, "main", LINE, ""),
        "line carrying the amendment and re-completion",
    );
}

/// TSK-140 and TSK-184 composed: a task pull request may merge its target
/// after the review (TSK-184), but a one-PR fix reviews its own range and
/// never stacks such a merge on that review (TSK-140, R-119).
#[test]
fn a_one_pr_fix_never_stacks_a_line_merge_on_its_review() {
    for stacked in [false, true] {
        let (dir, _, archived) = one_pr_fix();
        let root = dir.path();
        write(root, "src/lib.rs", "pub fn fixed() {}\n");
        let reviewed = commit(root, "fix: repair the regression");
        if stacked {
            git(root, &["switch", "main"]);
            write(root, "src/line.rs", "pub fn line() {}\n");
            commit(root, "feat: advance the line");
            git(root, &["switch", "task/TSK-001-fix"]);
            git(
                root,
                &["merge", "--no-ff", "-m", "chore: merge line", "main"],
            );
        }
        complete(
            root,
            "TSK-001",
            OWN_JOURNEY,
            &format!("{archived}{}", fix_block(&reviewed)),
        );
        let result = ci(root, "task/TSK-001-fix", "TSK-001");
        if stacked {
            assert_blocks(
                &result,
                "a fix range does not stack a line merge",
                &["work.acceptance_binding", "TSK-001", "inside the fix range"],
            );
        } else {
            assert_passes(&result, "one pull request fix");
        }
    }
}

/// TSK-140 and TSK-184 composed: the record-only amendment a task pull
/// request names as its own-range waiver (TSK-184) is on the target once
/// the task lands, so the line's judge, which binds without the task's
/// own range as the release judge does, accepts the same waiver.
#[test]
fn an_own_range_waiver_is_the_target_amendment_once_the_task_lands() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    code_change(root, BRANCH, "pub fn work() {}\n");
    let revised = OWN_JOURNEY.replace("shall work.", "shall mostly work.");
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", &revised, "Pending.\n"),
    );
    let amendment = commit(root, "docs: amend own criterion");
    write(root, "src/lib.rs", "pub fn reviewed_work() {}\n");
    let reviewed = commit(root, "feat: implement amended criterion");
    let waiver = block(
        &reviewed,
        &[
            &format!("AC-1: waived | {amendment}"),
            "AC-2: verified | journey",
        ],
        "verified | test",
        "none: done",
    );
    complete(root, "TSK-001", &revised, &waiver);
    assert_passes(
        &ci(root, BRANCH, "TSK-001"),
        "own-range waiver in the task PR",
    );
    let before = git_out(root, &["rev-parse", "main"]);
    git(root, &["switch", "main"]);
    git(
        root,
        &[
            "merge",
            "--no-ff",
            "-m",
            "Merge the task pull request",
            BRANCH,
        ],
    );
    let landed = codeflow_core::workgraph::acceptance::pull_request_findings(
        root,
        &before,
        "main",
        &codeflow_core::workgraph::acceptance::Criteria::Amendable,
    )
    .unwrap();
    assert!(
        !landed
            .iter()
            .any(|finding| !finding.note && finding.rule == "work.acceptance_binding"),
        "the landed waiver binds by the target route: {landed:?}"
    );
}

/// A task still `todo` on main, completed and then reopened on its own fix
/// branch; returns the archived block.
fn reopened_in_its_own_range() -> (tempfile::TempDir, String) {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    let first = code_change(root, "task/TSK-001-fix", "pub fn initial() {}\n");
    let old = fix_block(&first);
    complete(root, "TSK-001", OWN_JOURNEY, &old);
    let archived = old.replace(
        "acceptance:\n",
        "acceptance_superseded:\n  reason: regression\n",
    );
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", OWN_JOURNEY, &archived),
    );
    commit(root, "docs: reopen in the task range");
    (dir, archived)
}

/// A completion made and reopened inside its own pull request is a
/// reopening range too (R-119): a clean merge of the target after the fix
/// review is not stacked on that review, by the verb or by CI.
#[test]
fn a_completion_reopened_in_its_own_range_never_stacks_a_line_merge() {
    for stacked in [false, true] {
        let (dir, archived) = reopened_in_its_own_range();
        let root = dir.path();
        write(root, "src/lib.rs", "pub fn fixed() {}\n");
        let reviewed = commit(root, "fix: repair the regression");
        if stacked {
            git(root, &["switch", "main"]);
            write(root, "src/line.rs", "pub fn line() {}\n");
            commit(root, "feat: advance the line");
            git(root, &["switch", "task/TSK-001-fix"]);
            git(
                root,
                &["merge", "--no-ff", "-m", "chore: merge line", "main"],
            );
        }
        let closeout = format!("{archived}{}", fix_block(&reviewed));
        write(
            root,
            &record_path("TSK-001"),
            &task("TSK-001", "todo", OWN_JOURNEY, &closeout),
        );
        let verb = status_complete(root, "TSK-001");
        complete(root, "TSK-001", OWN_JOURNEY, &closeout);
        let result = ci(root, "task/TSK-001-fix", "TSK-001");
        if stacked {
            let needle = "src/line.rs changed after the reviewed commit";
            assert_ne!(verb.0, 0, "the verb stacked a line merge: {}", verb.1);
            assert!(verb.1.contains(needle), "{}", verb.1);
            assert_blocks(&result, "an in-range reopen stacks nothing", &[needle]);
        } else {
            assert_passes(&verb, "the verb on an in-range reopen and fix");
            assert_passes(&result, "an in-range reopen and fix");
        }
    }
}

/// Amend AC-1 on the fix branch, restore it, review a fix and complete with
/// AC-1 waived by that own-branch amendment; returns the verb and CI.
fn waive_by_own_amendment(root: &Path, archived: &str) -> [(i32, String); 2] {
    let revised = OWN_JOURNEY.replace("shall work.", "shall mostly work.");
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", &revised, archived),
    );
    let amendment = commit(root, "docs: amend the reopened criterion");
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", OWN_JOURNEY, archived),
    );
    commit(root, "docs: restore the anchored criterion");
    write(root, "src/lib.rs", "pub fn fixed() {}\n");
    let reviewed = commit(root, "fix: repair the regression");
    let waiver = block(
        &reviewed,
        &[
            &format!("AC-1: waived | {amendment}"),
            "AC-2: verified | journey",
        ],
        "verified | journey",
        "none: done",
    );
    let closeout = format!("{archived}{waiver}");
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", OWN_JOURNEY, &closeout),
    );
    let verb = status_complete(root, "TSK-001");
    complete(root, "TSK-001", OWN_JOURNEY, &closeout);
    [verb, ci(root, "task/TSK-001-fix", "TSK-001")]
}

/// A reopening range has no own-range waiver route (R-60): a one-PR fix of
/// a task complete on its target is refused by the verb as by CI.
#[test]
fn a_one_pr_fix_has_no_own_range_waiver() {
    let (dir, _, archived) = one_pr_fix();
    for result in waive_by_own_amendment(dir.path(), &archived) {
        assert_blocks(
            &result,
            "an own-range waiver in a reopening range",
            &["AC-1 waiver", "which is not on the target"],
        );
    }
}

/// The same refusal when the range itself completed and reopened the task,
/// which is still `todo` on its target.
#[test]
fn a_completion_reopened_in_its_own_range_has_no_own_range_waiver() {
    let (dir, archived) = reopened_in_its_own_range();
    for result in waive_by_own_amendment(dir.path(), &archived) {
        assert_blocks(
            &result,
            "an own-range waiver after an in-range reopen",
            &["AC-1 waiver", "which is not on the target"],
        );
    }
}

// TSK-220: a reviewed task pull request merges its moved target so the
// release check sees the current base. The target is read as `work start`
// reads it, so a local branch strictly behind its upstream never stands in
// for the target the merge brought, and anything that is not a clean merge
// of that target still blocks, naming the commit.

/// A standalone task record (no epic) with [`OWN_JOURNEY`] criteria,
/// targeting `target`.
fn standalone(id: &str, status: &str, target: &str, closeout: &str) -> String {
    task(id, status, OWN_JOURNEY, closeout)
        .replace(
            "epic_id: EPC-001\nstandalone_reason: null",
            "epic_id: null\nstandalone_reason: \"one fix\"",
        )
        .replace(
            "integration_target: main",
            &format!("integration_target: {target}"),
        )
}

/// `git` in `dir`, whatever its exit status: a conflicted merge fails.
fn git_try(dir: &Path, args: &[&str]) {
    let _ = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("git runs");
}

/// A bare `origin` whose default branch is `main`, holding `main` and
/// `line`, which the local `line` tracks.
fn with_origin(root: &Path, line: &str) -> tempfile::TempDir {
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "-q", "--bare"]);
    git(remote.path(), &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git(
        root,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(root, &["push", "-q", "origin", "main:main"]);
    git(root, &["push", "-q", "origin", &format!("{line}:{line}")]);
    git(root, &["fetch", "-q", "origin"]);
    git(
        root,
        &[
            "branch",
            "--set-upstream-to",
            &format!("origin/{line}"),
            line,
        ],
    );
    remote
}

/// Move `origin/<line>` one commit ahead, writing `path`, and leave the
/// local `line` strictly behind it, as a root checkout that never pulls is.
fn advance_origin(root: &Path, line: &str, path: &str, content: &str) -> String {
    let back = git_out(root, &["branch", "--show-current"]);
    git(
        root,
        &["switch", "-q", "-c", "advance", &format!("origin/{line}")],
    );
    write(root, path, content);
    let tip = commit(root, "feat: advance the target");
    git(root, &["push", "-q", "origin", &format!("advance:{line}")]);
    git(root, &["switch", "-q", &back]);
    git(root, &["branch", "-q", "-D", "advance"]);
    git(root, &["fetch", "-q", "origin"]);
    assert_ne!(
        git_out(root, &["rev-parse", line]),
        git_out(root, &["rev-parse", &format!("origin/{line}")]),
        "the local {line} stays behind"
    );
    tip
}

/// A standalone task targeting `main`, reviewed on its branch and completed
/// there, with `origin/main` one commit ahead of the local `main`. Returns
/// the repository, its remote and the reviewed commit.
fn reviewed_standalone() -> (tempfile::TempDir, tempfile::TempDir, String) {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    write(
        root,
        &record_path("TSK-001"),
        &standalone("TSK-001", "todo", "main", "Pending.\n"),
    );
    commit(root, "docs(records): a standalone task");
    let remote = with_origin(root, "main");
    let reviewed = code_change(root, BRANCH, "pub fn work() {}\n");
    write(
        root,
        &record_path("TSK-001"),
        &standalone("TSK-001", "complete", "main", &valid_block(&reviewed)),
    );
    commit(root, "docs(records): complete the task");
    (dir, remote, reviewed)
}

/// AC-1: a completed standalone task that merges its moved target cleanly
/// keeps its binding, though the local `main` is behind `origin/main`;
/// before TSK-220 the stale local branch made the merge look foreign. The
/// verb binds the same way when the completion follows the merge.
#[test]
fn a_clean_merge_of_the_moved_target_keeps_the_binding() {
    let (dir, _remote, _) = reviewed_standalone();
    let root = dir.path();
    advance_origin(root, "main", "src/line.rs", "pub fn line() {}\n");
    git(
        root,
        &[
            "merge",
            "--no-ff",
            "-m",
            "chore: merge the target",
            "origin/main",
        ],
    );
    assert_passes(
        &ci_on(root, "origin/main", BRANCH, "Task: TSK-001"),
        "a clean merge of origin/main after the completion",
    );

    // The completion made after the merge, by the verb and then by CI.
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    write(
        root,
        &record_path("TSK-001"),
        &standalone("TSK-001", "todo", "main", "Pending.\n"),
    );
    commit(root, "docs(records): a standalone task");
    let _remote = with_origin(root, "main");
    let reviewed_here = code_change(root, BRANCH, "pub fn work() {}\n");
    advance_origin(root, "main", "src/line.rs", "pub fn line() {}\n");
    git(
        root,
        &[
            "merge",
            "--no-ff",
            "-m",
            "chore: merge the target",
            "origin/main",
        ],
    );
    write(
        root,
        &record_path("TSK-001"),
        &standalone("TSK-001", "todo", "main", &valid_block(&reviewed_here)),
    );
    assert_passes(
        &status_complete(root, "TSK-001"),
        "the verb after a clean merge of origin/main",
    );
    commit(root, "docs(records): complete the task");
    assert_passes(
        &ci_on(root, "origin/main", BRANCH, "Task: TSK-001"),
        "a completion after a clean merge of origin/main",
    );
}

/// AC-2: after the review, a merge of the moved target with a hand edit, a
/// conflicted merge resolved by hand, a regular commit and a merge of an
/// unrelated branch each still block, naming the commit.
#[test]
fn anything_but_a_clean_target_merge_still_blocks_and_names_the_commit() {
    for case in ["edited", "conflicted", "commit", "unrelated"] {
        let (dir, _remote, reviewed) = reviewed_standalone();
        let root = dir.path();
        let conflicting = if case == "conflicted" {
            "src/lib.rs"
        } else {
            "src/line.rs"
        };
        advance_origin(root, "main", conflicting, "pub fn line() {}\n");
        let culprit = match case {
            "edited" => {
                git(root, &["merge", "--no-ff", "--no-commit", "origin/main"]);
                write(root, "src/extra.rs", "pub fn unreviewed() {}\n");
                commit(root, "chore: merge the target")
            }
            "conflicted" => {
                git_try(root, &["merge", "--no-ff", "origin/main"]);
                write(root, "src/lib.rs", "pub fn work() {}\npub fn line() {}\n");
                commit(root, "chore: merge the target")
            }
            "commit" => {
                git(
                    root,
                    &[
                        "merge",
                        "--no-ff",
                        "-m",
                        "chore: merge the target",
                        "origin/main",
                    ],
                );
                write(root, "src/extra.rs", "pub fn unreviewed() {}\n");
                commit(root, "feat: change after the merge")
            }
            _ => {
                git(root, &["switch", "-q", "-c", "feat/side", "main"]);
                write(root, "src/side.rs", "pub fn side() {}\n");
                commit(root, "feat: side work");
                git(root, &["switch", "-q", BRANCH]);
                git(
                    root,
                    &["merge", "--no-ff", "-m", "chore: merge side", "feat/side"],
                );
                head(root)
            }
        };
        let result = ci_on(root, "origin/main", BRANCH, "Task: TSK-001");
        let needles: Vec<String> = match case {
            "edited" | "conflicted" => vec![format!(
                "merge {culprit} is not a clean re-merge from the task's integration target"
            )],
            "commit" => vec![
                format!("after the reviewed commit {reviewed}"),
                format!("(commit {culprit}: src/extra.rs changed)"),
            ],
            _ => vec![
                format!("after the reviewed commit {reviewed}"),
                format!("merge {culprit} brings"),
                "which is not on the first-parent line of the target tip this run is judged against".to_string(),
            ],
        };
        let mut all = vec!["work.acceptance_binding", "TSK-001"];
        all.extend(needles.iter().map(String::as_str));
        assert_blocks(&result, case, &all);
    }
}

/// AC-3: a task landing into an integration line keeps working when the
/// local line is behind `origin`'s: the task merges the moved line cleanly
/// after its review, and the line is resolved as `work start` resolves it.
#[test]
fn a_task_into_a_moved_integration_line_keeps_the_binding() {
    let dir = line_repo(&["TSK-001"]);
    let root = dir.path();
    let _remote = with_origin(root, LINE);
    let reviewed = build(root, BRANCH, "src/one.rs");
    write_done(root, "TSK-001", "complete", &reviewed);
    commit(root, "docs(records): complete the task");
    advance_origin(root, LINE, "src/line.rs", "pub fn line() {}\n");
    git(
        root,
        &[
            "merge",
            "--no-ff",
            "-m",
            "chore: merge the line",
            &format!("origin/{LINE}"),
        ],
    );
    assert_passes(
        &ci_on(root, &format!("origin/{LINE}"), BRANCH, "Task: TSK-001"),
        "a clean merge of the moved line",
    );
}

/// Round 1 of the TSK-220 review: locally writable refs and configuration
/// are never target authority. With the run's base held at the published
/// target, a merge of an unpublished commit refuses whether the local
/// `main` tracks another remote that holds it or `origin/main` is forged to
/// it; a graft that puts it on the target's first-parent line refuses as an
/// overlay.
#[test]
fn local_refs_and_overlays_never_make_a_merge_the_target() {
    for case in ["alternate-remote", "forged-origin", "graft"] {
        let (dir, _remote, reviewed) = reviewed_standalone();
        let root = dir.path();
        let published = if case == "graft" {
            advance_origin(root, "main", "src/published.rs", "pub fn published() {}\n")
        } else {
            git_out(root, &["rev-parse", "origin/main"])
        };
        git(root, &["switch", "-q", "-c", "feat/unreviewed", "main"]);
        write(root, "src/unreviewed.rs", "pub fn unreviewed() {}\n");
        let foreign = commit(root, "feat: unpublished work");
        git(root, &["switch", "-q", BRANCH]);
        match case {
            "alternate-remote" => {
                git(
                    root,
                    &[
                        "remote",
                        "add",
                        "alternate",
                        "https://example.test/alternate",
                    ],
                );
                git(
                    root,
                    &["update-ref", "refs/remotes/alternate/main", &foreign],
                );
                git(root, &["config", "branch.main.remote", "alternate"]);
            }
            "forged-origin" => git(root, &["update-ref", "refs/remotes/origin/main", &foreign]),
            _ => write(
                root,
                ".git/info/grafts",
                &format!("{published} {foreign}\n"),
            ),
        }
        git(
            root,
            &[
                "merge",
                "--no-ff",
                "-m",
                "chore: merge the target",
                &foreign,
            ],
        );
        let merge = head(root);
        let needle = if case == "graft" {
            "overlays its recorded history with the graft file".to_string()
        } else {
            format!("merge {merge} brings {foreign}, which is not on the first-parent line of the target tip this run is judged against ({published})")
        };
        assert_blocks(
            &ci_on(root, &published, BRANCH, "Task: TSK-001"),
            case,
            &[
                "work.acceptance_binding",
                &format!("src/unreviewed.rs changed after the reviewed commit {reviewed}"),
                &needle,
            ],
        );
    }
}

/// The criteria a reopen may add to: `OWN_JOURNEY` plus AC-3.
const REOPEN_ADDS: &str = "- AC-1 When run, the system shall work.\n- AC-2 (journey) On a fresh project, the command shall succeed.\n- AC-3 When rerun, the system shall still work.\n";

/// An approved block for `reviewed` that verifies AC-1 to AC-3.
fn three_criteria_block(reviewed: &str) -> String {
    block(
        reviewed,
        &[
            "AC-1: verified | unit",
            "AC-2: verified | journey",
            "AC-3: verified | unit",
        ],
        "verified | journey",
        "none: nothing deferred",
    )
}

/// `record` as a standalone task with a fixed uid, as `task new` writes a
/// record that exists only on its own branch.
fn new_standalone(record: &str) -> String {
    record.replace(
        "id: TSK-002\nepic_id: EPC-001\nstandalone_reason: null",
        "id: TSK-002\nuid: 6f1c2b8e-3d4a-4f5b-9c6d-7e8f9a0b1c2d\nepic_id: null\nstandalone_reason: \"a fixture\"",
    )
}

/// Reopen the task completed by `old` on the current branch with
/// `criteria`, review a fix, and complete it; returns the verb and CI.
fn reopen_with_criteria(root: &Path, id: &str, old: &str, criteria: &str) -> [(i32, String); 2] {
    let branch = git_out(root, &["branch", "--show-current"]);
    let archived = old.replace(
        "acceptance:\n",
        "acceptance_superseded:\n  reason: one more criterion\n",
    );
    let standalone = |text: String| {
        if id == "TSK-001" {
            text
        } else {
            new_standalone(&text)
        }
    };
    write(
        root,
        &record_path(id),
        &standalone(task(id, "todo", criteria, &archived)),
    );
    commit(root, "docs: reopen with one more criterion");
    write(root, "src/lib.rs", "pub fn rerun() {}\n");
    let reviewed = commit(root, "fix: keep working when rerun");
    let closeout = format!("{archived}{}", three_criteria_block(&reviewed));
    write(
        root,
        &record_path(id),
        &standalone(task(id, "todo", criteria, &closeout)),
    );
    let verb = status_complete(root, id);
    write(
        root,
        &record_path(id),
        &standalone(task(id, "complete", criteria, &closeout)),
    );
    commit(root, "docs(records): record the acceptance");
    [verb, ci(root, &branch, id)]
}

/// A standalone task whose record exists only on its own branch may add a
/// criterion when the branch reopens it (TSK-217): the target holds no
/// criteria to keep, so the verb and CI accept the same change.
#[test]
fn a_task_new_in_its_range_may_change_criteria_on_reopen() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    let reviewed = code_change(root, "task/TSK-002-new", "pub fn initial() {}\n");
    let old = fix_block(&reviewed);
    let record = new_standalone(&task("TSK-002", "complete", OWN_JOURNEY, &old));
    write(root, &record_path("TSK-002"), &record);
    commit(root, "docs(records): complete the new task");
    // The range adds the record, so its id is bound in the local registry.
    let admitted = codeflow()
        .args(["ids", "admit", &record_path("TSK-002")])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        admitted.status.success(),
        "{}{}",
        String::from_utf8_lossy(&admitted.stdout),
        String::from_utf8_lossy(&admitted.stderr)
    );
    for result in reopen_with_criteria(root, "TSK-002", &old, REOPEN_ADDS) {
        assert_passes(&result, "a reopen of a task new in its range");
    }
}

/// A task whose completion landed on the target keeps its criteria across
/// a reopen in its own range, by the verb as by CI, whether the fix branch
/// reopens it or a separate reopen landed first (issue #67 keeps the freeze
/// for every landed task). A later target change that clears the landed
/// Closeout hides nothing: the target's history still shows the landing.
#[test]
fn a_landed_task_keeps_its_criteria_on_reopen() {
    for (separate_reopen, cleared) in [(false, false), (true, false), (true, true)] {
        let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
        let root = dir.path();
        let reviewed = code_change(root, BRANCH, "pub fn initial() {}\n");
        let old = fix_block(&reviewed);
        complete(root, "TSK-001", OWN_JOURNEY, &old);
        git(root, &["switch", "main"]);
        git(
            root,
            &["merge", "--no-ff", "-m", "chore: land the task", BRANCH],
        );
        if separate_reopen {
            let archived = old.replace(
                "acceptance:\n",
                "acceptance_superseded:\n  reason: one more criterion\n",
            );
            write(
                root,
                &record_path("TSK-001"),
                &task("TSK-001", "todo", OWN_JOURNEY, &archived),
            );
            commit(root, "docs: reopen the landed task");
        }
        if cleared {
            write(
                root,
                &record_path("TSK-001"),
                &task("TSK-001", "todo", OWN_JOURNEY, "Pending.\n"),
            );
            commit(root, "docs: clear the reopened closeout");
        }
        git(root, &["switch", "-c", "task/TSK-001-fix"]);
        for result in reopen_with_criteria(root, "TSK-001", &old, REOPEN_ADDS) {
            assert_blocks(
                &result,
                &format!("a landed task (separate reopen: {separate_reopen}, cleared: {cleared})"),
                &["reopened task keeps its criteria"],
            );
        }
    }
}

/// A landed completion stays landed when a later target change clears it
/// from the current record (TSK-234 review): the task landed, a separate
/// reopen landed, and an ordinary pull request then cleared the archived
/// Closeout. A fix branch that completes, reopens and changes the criteria
/// is still refused by the verb and by CI, because the target's history
/// shows the landing.
#[test]
fn a_cleared_closeout_does_not_hide_a_landed_completion() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    let first = code_change(root, BRANCH, "pub fn landed() {}\n");
    let old = fix_block(&first);
    complete(root, "TSK-001", OWN_JOURNEY, &old);
    git(root, &["switch", "main"]);
    git(
        root,
        &["merge", "--no-ff", "-m", "chore: land the task", BRANCH],
    );
    let archived = old.replace(
        "acceptance:\n",
        "acceptance_superseded:\n  reason: fix the task\n",
    );
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", OWN_JOURNEY, &archived),
    );
    commit(root, "docs: reopen the landed task");
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", OWN_JOURNEY, "Pending.\n"),
    );
    commit(root, "docs: clear the reopened closeout");
    let target = head(root);
    git(root, &["switch", "-c", "task/TSK-001-fix"]);
    for result in
        complete_reopen_complete(root, &record_path("TSK-001"), &|text| text, &target, false)
    {
        assert_blocks(
            &result,
            "a landed task whose target record was cleared",
            &["reopened task keeps its criteria"],
        );
    }
}

/// A target that deleted a landed record does not make the task new
/// (TSK-234 review round 2, design D2): the task landed, the target then
/// removed its record, and the task branch reopens it with another
/// criterion, merges the target keeping the reopened record, and completes
/// again. The verb and CI refuse, because the target's history shows the
/// landing and its newest judged version holds the criteria to keep.
#[test]
fn a_deleted_landed_record_keeps_its_newest_judged_criteria() {
    let dir = repo(&[], "");
    let root = dir.path();
    let path = record_path("TSK-001");
    write(
        root,
        &path,
        &standalone_one(&task("TSK-001", "todo", OWN_JOURNEY, "Pending.\n")),
    );
    commit(root, "docs: plan the standalone task");
    let reviewed = code_change(root, BRANCH, "pub fn original() {}\n");
    let old = fix_block(&reviewed);
    write(
        root,
        &path,
        &standalone_one(&task("TSK-001", "complete", OWN_JOURNEY, &old)),
    );
    commit(root, "docs: complete the task");
    git(root, &["switch", "main"]);
    git(
        root,
        &["merge", "--no-ff", "-m", "chore: land the task", BRANCH],
    );
    git(root, &["rm", "-q", &path]);
    commit(root, "docs: remove the task record");
    let target = head(root);
    git(root, &["switch", BRANCH]);
    let archived = old.replace(
        "acceptance:\n",
        "acceptance_superseded:\n  reason: fix the task\n",
    );
    let reopened = standalone_one(&task("TSK-001", "todo", REOPEN_ADDS, &archived));
    write(root, &path, &reopened);
    commit(root, "docs: reopen with one more criterion");
    // The target deleted the record this branch changed, so the merge
    // stops on that conflict; the branch keeps its reopened record.
    Command::new("git")
        .args(["merge", "--no-ff", "--no-commit", "main"])
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap();
    write(root, &path, &reopened);
    let fixed = commit(root, "chore: merge the target and keep the reopened task");
    let closeout = format!("{archived}{}", three_criteria_block(&fixed));
    write(
        root,
        &path,
        &standalone_one(&task("TSK-001", "todo", REOPEN_ADDS, &closeout)),
    );
    let verb = status_complete(root, "TSK-001");
    write(
        root,
        &path,
        &standalone_one(&task("TSK-001", "complete", REOPEN_ADDS, &closeout)),
    );
    commit(root, "docs(records): complete the fix");
    let ci = ci_on(root, &target, BRANCH, "Task: TSK-001");
    for (who, result) in [("verb", verb), ("ci", ci)] {
        assert_blocks(
            &result,
            &format!("{who}: a landed task whose record the target deleted"),
            &["reopened task keeps its criteria"],
        );
    }
}

/// Issue #67: a task the target records, never landed, adds a criterion on
/// its own branch and completes; CI prints the delta as a note. The line
/// then moves, so the task is reopened to take a line merge, the merge is
/// reviewed, and the task completes again. The verb and CI accept the same
/// criterion they accepted one completion earlier, still printing the
/// delta for the reviewer.
#[test]
fn a_reopen_to_take_a_line_merge_keeps_an_unlanded_criterion() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    git(root, &["switch", "-c", BRANCH]);
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", REOPEN_ADDS, "Pending.\n"),
    );
    write(root, "src/lib.rs", "pub fn rerun() {}\n");
    let first = commit(root, "feat: work with one more criterion");
    let old = three_criteria_block(&first);
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", REOPEN_ADDS, &old),
    );
    assert_passes(&status_complete(root, "TSK-001"), "the first completion");
    commit(root, "docs(records): complete the task");
    let first_ci = ci(root, BRANCH, "TSK-001");
    assert_passes(&first_ci, "the first completion in CI");
    assert!(
        first_ci.1.contains("TSK-001 criteria delta"),
        "{}",
        first_ci.1
    );

    git(root, &["switch", "main"]);
    write(root, "src/line.rs", "pub fn line() {}\n");
    commit(root, "feat: advance the line");
    git(root, &["switch", BRANCH]);
    let archived = old.replace(
        "acceptance:\n",
        "acceptance_superseded:\n  reason: take the line\n",
    );
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", REOPEN_ADDS, &archived),
    );
    commit(root, "docs(records): reopen to take the line");
    git(
        root,
        &["merge", "--no-ff", "-m", "chore: merge the line", "main"],
    );
    let reviewed = head(root);
    let closeout = format!("{archived}{}", three_criteria_block(&reviewed));
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", REOPEN_ADDS, &closeout),
    );
    let verb = status_complete(root, "TSK-001");
    assert_passes(&verb, "the verb after a reopen to take the line");
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "complete", REOPEN_ADDS, &closeout),
    );
    commit(root, "docs(records): complete the task again");
    let result = ci(root, BRANCH, "TSK-001");
    assert_passes(&result, "CI after a reopen to take the line");
    assert!(result.1.contains("TSK-001 criteria delta"), "{}", result.1);
    assert!(
        !result.1.contains("reopened task keeps its criteria"),
        "{}",
        result.1
    );
}

const FIXED_UID: &str = "6f1c2b8e-3d4a-4f5b-9c6d-7e8f9a0b1c2d";

/// The Closeout of a task whose earlier completion landed and was reopened
/// before acceptance blocks existed: evidence that the task landed (issue
/// #67), so a reopen keeps its criteria.
const LANDED: &str = "- reopened: an earlier completion landed\n";

/// `record` for TSK-001 as a standalone task with a fixed uid.
fn standalone_one(record: &str) -> String {
    record
        .replace("id: TSK-001\n", &format!("id: TSK-001\nuid: {FIXED_UID}\n"))
        .replace(
            "epic_id: EPC-001\nstandalone_reason: null",
            "epic_id: null\nstandalone_reason: \"a fixture\"",
        )
}

/// On the current branch, write TSK-001 at `path` through `shape`, complete
/// it, reopen it with AC-3 added, review a fix and complete it again; then
/// run the verb and `codeflow ci` from `base`, binding the record's id in
/// the local registry first when `admit`. Returns both results.
fn complete_reopen_complete(
    root: &Path,
    path: &str,
    shape: &dyn Fn(String) -> String,
    base: &str,
    admit: bool,
) -> [(i32, String); 2] {
    complete_reopen_complete_with(root, path, shape, base, admit, &|_| {})
}

/// [`complete_reopen_complete`], calling `before_verb` with the reviewed
/// fix committed and the branch checked out, just before the verb runs.
fn complete_reopen_complete_with(
    root: &Path,
    path: &str,
    shape: &dyn Fn(String) -> String,
    base: &str,
    admit: bool,
    before_verb: &dyn Fn(&str),
) -> [(i32, String); 2] {
    let branch = git_out(root, &["branch", "--show-current"]);
    write(
        root,
        path,
        &shape(task("TSK-001", "todo", OWN_JOURNEY, "Pending.\n")),
    );
    write(root, "src/lib.rs", "pub fn initial() {}\n");
    let first = commit(root, "feat: initial work");
    let old = fix_block(&first);
    write(
        root,
        path,
        &shape(task("TSK-001", "complete", OWN_JOURNEY, &old)),
    );
    commit(root, "docs(records): complete the task");
    let archived = old.replace(
        "acceptance:\n",
        "acceptance_superseded:\n  reason: one more criterion\n",
    );
    write(
        root,
        path,
        &shape(task("TSK-001", "todo", REOPEN_ADDS, &archived)),
    );
    commit(root, "docs: reopen with one more criterion");
    write(root, "src/lib.rs", "pub fn fixed() {}\n");
    let reviewed = commit(root, "fix: keep working when rerun");
    before_verb(&branch);
    let closeout = format!("{archived}{}", three_criteria_block(&reviewed));
    write(
        root,
        path,
        &shape(task("TSK-001", "todo", REOPEN_ADDS, &closeout)),
    );
    let verb = status_complete(root, "TSK-001");
    write(
        root,
        path,
        &shape(task("TSK-001", "complete", REOPEN_ADDS, &closeout)),
    );
    commit(root, "docs(records): record the acceptance");
    if admit {
        let admitted = codeflow()
            .args(["ids", "admit", path])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            admitted.status.success(),
            "{}{}",
            String::from_utf8_lossy(&admitted.stdout),
            String::from_utf8_lossy(&admitted.stderr)
        );
    }
    [verb, ci_on(root, base, &branch, "Task: TSK-001")]
}

/// A record moved to another supported layout is the same task: the
/// target's copy of a landed task keeps its criteria, by the verb as by CI
/// (TSK-217, issue #67).
#[test]
fn a_moved_record_keeps_the_target_criteria_on_reopen() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    let original = record_path("TSK-001");
    let planned = standalone_one(&std::fs::read_to_string(root.join(&original)).unwrap())
        .replace("Pending.\n", LANDED);
    write(root, &original, &planned);
    commit(root, "docs: plan the standalone task");
    git(root, &["switch", "-c", "task/TSK-001-fix"]);
    std::fs::remove_file(root.join(&original)).unwrap();
    let moved = "project-management/epics/EPC-001/tasks/TSK-001.md";
    let shape = |record: String| standalone_one(&record);
    for result in complete_reopen_complete(root, moved, &shape, "main", false) {
        assert_blocks(
            &result,
            "a reopen of a moved record the target holds",
            &["reopened task keeps its criteria"],
        );
    }
}

/// Round 1 of the TSK-220 review: a stopped stack refuses even when the
/// change from the review nets out (SPC-013 R-60), so an octopus merge
/// that keeps the reviewed tree and an unreviewed change followed by its
/// removal each refuse, naming the commit.
#[test]
fn a_stopped_stack_refuses_even_when_the_change_nets_out() {
    for case in ["octopus", "transient"] {
        let (dir, _remote, reviewed) = reviewed_standalone();
        let root = dir.path();
        let base = git_out(root, &["rev-parse", "origin/main"]);
        let culprit = if case == "octopus" {
            git(root, &["switch", "-q", "-c", "feat/side", "main"]);
            write(root, "src/side.rs", "pub fn side() {}\n");
            let side = commit(root, "feat: side work");
            git(root, &["switch", "-q", BRANCH]);
            let current = head(root);
            let tree = git_out(root, &["rev-parse", "HEAD^{tree}"]);
            let octopus = git_out(
                root,
                &[
                    "commit-tree",
                    &tree,
                    "-p",
                    &current,
                    "-p",
                    &side,
                    "-p",
                    &base,
                    "-m",
                    "chore: octopus",
                ],
            );
            git(root, &["reset", "-q", "--hard", &octopus]);
            octopus
        } else {
            write(root, "src/temp.rs", "pub fn temporary() {}\n");
            let added = commit(root, "feat: unreviewed transient");
            std::fs::remove_file(root.join("src/temp.rs")).unwrap();
            commit(root, "fix: remove the transient");
            added
        };
        let needle = if case == "octopus" {
            format!("merge {culprit} has more than two parents")
        } else {
            // The walk meets the removal first; it names that commit.
            "src/temp.rs changed".to_string()
        };
        assert_blocks(
            &ci_on(root, &base, BRANCH, "Task: TSK-001"),
            case,
            &[
                "work.acceptance_binding",
                &format!("the history after the reviewed commit {reviewed} does not stack on it"),
                &needle,
            ],
        );
    }
}

/// TSK-220 (TSK-213's refusal): a task completed, reopened and given a new
/// criterion inside its own unmerged branch completes again, since no
/// landed criteria exist to protect. Issue #67: that holds also when the
/// target records the task, so long as its completion never landed there;
/// `a_landed_task_keeps_its_criteria_on_reopen` keeps the landed refusal.
#[test]
fn an_unlanded_task_may_change_its_criteria_when_reopened_in_its_range() {
    const EXTENDED: &str = "- AC-1 When run, the system shall work.\n- AC-2 (journey) On a fresh project, the command shall succeed.\n- AC-3 When reopened, the system shall still work.\n";
    for on_target in [false, true] {
        let dir = repo(&[], "");
        let root = dir.path();
        if on_target {
            write(
                root,
                &record_path("TSK-001"),
                &standalone("TSK-001", "todo", "main", "Pending.\n"),
            );
            commit(root, "docs(records): the task on the target");
        }
        git(root, &["switch", "-q", "-c", BRANCH, "main"]);
        write(
            root,
            &record_path("TSK-001"),
            &standalone("TSK-001", "todo", "main", "Pending.\n"),
        );
        write(root, "src/lib.rs", "pub fn first() {}\n");
        let first = commit(root, "feat: first attempt");
        let old = fix_block(&first);
        write(
            root,
            &record_path("TSK-001"),
            &standalone("TSK-001", "complete", "main", &old),
        );
        commit(root, "docs(records): complete the task");
        let archived = old.replace(
            "acceptance:\n",
            "acceptance_superseded:\n  reason: a criterion was missing\n",
        );
        write(
            root,
            &record_path("TSK-001"),
            &standalone("TSK-001", "todo", "main", &archived).replace(OWN_JOURNEY, EXTENDED),
        );
        commit(root, "docs(records): reopen and add AC-3");
        write(root, "src/lib.rs", "pub fn second() {}\n");
        let reviewed = commit(root, "feat: meet AC-3");
        let closeout = format!(
            "{archived}{}",
            block(
                &reviewed,
                &[
                    "AC-1: verified | unit",
                    "AC-2: verified | journey",
                    "AC-3: verified | unit",
                ],
                "verified | journey",
                "none: nothing deferred",
            )
        );
        write(
            root,
            &record_path("TSK-001"),
            &standalone("TSK-001", "todo", "main", &closeout).replace(OWN_JOURNEY, EXTENDED),
        );
        let verb = status_complete(root, "TSK-001");
        assert_passes(&verb, "the verb on an unlanded in-range reopen");
        commit(root, "docs(records): complete the task again");
        // The fixture has no id registry, so CI refuses a record the range
        // adds under `work.id_registry`; the binding and the freeze accept it.
        let result = ci(root, BRANCH, "TSK-001");
        if on_target {
            assert_passes(&result, "an unlanded task the target records");
        }
        for rule in ["work.acceptance_binding", "work.criteria_frozen (block)"] {
            assert!(!result.1.contains(rule), "{rule}: {}", result.1);
        }
        assert!(result.1.contains("TSK-001 criteria delta"), "{}", result.1);
    }
}

/// TSK-220: the pre-push hook can judge a range from an old fork point,
/// whose first-parent line does not reach the target commit a refresh
/// merged; the hook's candidate authority, the destination default
/// branch's tip passed as `--policy-from`, is the target tip that carries
/// it.
#[test]
fn the_pre_push_candidate_authority_carries_a_target_merge() {
    let (dir, _remote, _reviewed) = reviewed_standalone();
    let root = dir.path();
    let pushed = git_out(root, &["rev-parse", "main"]);
    let tip = advance_origin(root, "main", "src/line.rs", "pub fn line() {}\n");
    git(
        root,
        &[
            "merge",
            "--no-ff",
            "-m",
            "chore: merge the target",
            "origin/main",
        ],
    );
    let run = |authority: Option<&str>| {
        let mut args = vec![
            "ci", "--base", &pushed, "--head", "HEAD", "--branch", BRANCH,
        ];
        if let Some(tip) = authority {
            args.extend(["--policy-from", tip]);
        }
        let out = codeflow().args(&args).current_dir(root).output().unwrap();
        (
            out.status.code().unwrap_or(-1),
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        )
    };
    assert_blocks(
        &run(None),
        "the branch's own last push alone",
        &["work.acceptance_binding", "not on the first-parent line"],
    );
    assert_passes(&run(Some(&tip)), "with the candidate authority");
}

/// Round 2 of the TSK-220 review: a line range's own line carries a merge
/// after the review only for a task whose target is that line. A task
/// bound for `main` that merges the line after its review still refuses
/// when the line lands on `main`; a task bound for the line passes.
#[test]
fn a_line_carries_merges_only_for_the_tasks_that_target_it() {
    for target_main in [true, false] {
        // TSK-002 keeps the line an epic integration line, so the range
        // binds each completion where it was introduced.
        let dir = line_repo(&["TSK-001", "TSK-002"]);
        let root = dir.path();
        if target_main {
            write(
                root,
                &record_path("TSK-001"),
                &standalone("TSK-001", "todo", "main", "Pending.\n"),
            );
            commit(root, "docs(records): the task targets main");
            git(root, &["branch", "-f", LINE, "main"]);
        }
        let reviewed = build(root, BRANCH, "src/reviewed.rs");
        git(root, &["switch", "-q", "-c", "feat/side", LINE]);
        write(root, "src/side.rs", "pub fn side() {}\n");
        commit(root, "feat: work on the line");
        let line_tip = land(root, "feat/side");
        git(root, &["switch", "-q", BRANCH]);
        git(
            root,
            &["merge", "--no-ff", "-m", "chore: merge the line", &line_tip],
        );
        let record = if target_main {
            standalone("TSK-001", "complete", "main", &valid_block(&reviewed))
        } else {
            line_task("TSK-001", "complete", &valid_block(&reviewed))
        };
        write(root, &record_path("TSK-001"), &record);
        commit(root, "docs(records): complete the task");
        land(root, BRANCH);
        let result = ci_on(root, "main", LINE, "Task: EPC-001");
        if target_main {
            assert_blocks(
                &result,
                "a main-bound task merging the line",
                &[
                    "work.acceptance_binding",
                    "TSK-001",
                    "src/side.rs changed after the reviewed commit",
                ],
            );
        } else {
            assert_passes(&result, "a line-bound task merging its line");
        }
    }
}

/// Round 2 of the TSK-220 review: a landed record moved to another
/// supported layout is still found on the target by its identity, so a
/// reopen in the fix range keeps the landed criteria.
#[test]
fn a_moved_landed_record_keeps_its_criteria() {
    let (dir, _, archived) = one_pr_fix();
    let root = dir.path();
    git(root, &["switch", "-q", "main"]);
    git(root, &["merge", "-q", "--ff-only", "task/TSK-001-fix"]);
    git(root, &["switch", "-q", "task/TSK-001-fix"]);
    let moved = "project-management/epics/EPC-001/tasks/TSK-001.md";
    std::fs::remove_file(root.join(record_path("TSK-001"))).unwrap();
    write(
        root,
        moved,
        &task("TSK-001", "todo", OWN_JOURNEY, &archived),
    );
    write(root, "src/lib.rs", "pub fn second() {}\n");
    let first = commit(root, "feat: first repair and move the record");
    let active = fix_block(&first);
    write(
        root,
        moved,
        &task(
            "TSK-001",
            "complete",
            OWN_JOURNEY,
            &format!("{archived}{active}"),
        ),
    );
    commit(root, "docs(records): complete the first repair");
    let archived = format!(
        "{archived}{}",
        active.replace(
            "acceptance:\n",
            "acceptance_superseded:\n  reason: revise again\n"
        )
    );
    let changed = OWN_JOURNEY.replace("shall work.", "shall work differently.");
    write(root, moved, &task("TSK-001", "todo", &changed, &archived));
    commit(
        root,
        "docs(records): reopen and change the landed criterion",
    );
    write(root, "src/lib.rs", "pub fn third() {}\n");
    let second = commit(root, "feat: second repair");
    let closeout = format!("{archived}{}", fix_block(&second));
    write(root, moved, &task("TSK-001", "todo", &changed, &closeout));
    let verb = status_complete(root, "TSK-001");
    assert_ne!(verb.0, 0, "the verb kept the landed criteria: {}", verb.1);
    assert!(
        verb.1.contains("a reopened task keeps its criteria"),
        "{}",
        verb.1
    );
    write(
        root,
        moved,
        &task("TSK-001", "complete", &changed, &closeout),
    );
    commit(root, "docs(records): complete the second repair");
    assert_blocks(
        &ci(root, "task/TSK-001-fix", "TSK-001"),
        "a moved record's landed criteria",
        &["TSK-001", "a reopened task keeps its criteria"],
    );
}

/// TSK-220 round 3: the work-record reader accepts a `.md` extension in any
/// case, so a landed legacy record at `tasks/TSK-001.MD` with no `uid` is on
/// the target. Reopened with a changed criterion, kept at that path or moved
/// into its epic's folder, it keeps its landed criteria and CI blocks.
#[test]
fn a_landed_record_with_an_uppercase_extension_keeps_its_criteria() {
    for to in [
        "project-management/tasks/TSK-001.MD",
        "project-management/epics/EPC-001/tasks/TSK-001.md",
    ] {
        let (dir, _, archived) = one_pr_fix();
        let root = dir.path();
        let landed = "project-management/tasks/TSK-001.MD";
        git(root, &["switch", "-q", "main"]);
        git(root, &["merge", "-q", "--ff-only", "task/TSK-001-fix"]);
        git(root, &["mv", &record_path("TSK-001"), landed]);
        commit(root, "docs(records): uppercase the record's extension");
        git(root, &["switch", "-q", "task/TSK-001-fix"]);
        git(root, &["merge", "-q", "--ff-only", "main"]);
        std::fs::remove_file(root.join(landed)).unwrap();
        write(root, to, &task("TSK-001", "todo", OWN_JOURNEY, &archived));
        write(root, "src/lib.rs", "pub fn second() {}\n");
        let first = commit(root, "feat: first repair");
        let active = fix_block(&first);
        let completed = format!("{archived}{active}");
        write(
            root,
            to,
            &task("TSK-001", "complete", OWN_JOURNEY, &completed),
        );
        commit(root, "docs(records): complete the first repair");
        let archived = format!(
            "{archived}{}",
            active.replace(
                "acceptance:\n",
                "acceptance_superseded:\n  reason: revise again\n"
            )
        );
        let changed = OWN_JOURNEY.replace("shall work.", "shall work differently.");
        write(root, to, &task("TSK-001", "todo", &changed, &archived));
        commit(
            root,
            "docs(records): reopen and change the landed criterion",
        );
        write(root, "src/lib.rs", "pub fn third() {}\n");
        let second = commit(root, "feat: second repair");
        let closeout = format!("{archived}{}", fix_block(&second));
        write(root, to, &task("TSK-001", "complete", &changed, &closeout));
        commit(root, "docs(records): complete the second repair");
        assert_blocks(
            &ci(root, "task/TSK-001-fix", "TSK-001"),
            &format!("a landed .MD record reopened at {to}"),
            &["TSK-001", "a reopened task keeps its criteria"],
        );
    }
}

/// Neither a stale local target, nor an older comparison base, nor a record
/// retargeted to a branch that predates it makes a task that landed on the
/// target look unlanded: the verb and CI both refuse the changed criteria
/// (TSK-217, issue #67).
#[test]
fn every_anchor_sees_the_target_record_on_reopen() {
    let mut accepted = Vec::new();
    for case in ["stale-local", "old-base", "retarget"] {
        let dir = repo(&[], "");
        let root = dir.path();
        let early = head(root);
        let path = record_path("TSK-001");
        write(
            root,
            &path,
            &standalone_one(&task("TSK-001", "todo", OWN_JOURNEY, LANDED)),
        );
        let planned = commit(root, "docs: plan the standalone task");
        git(root, &["branch", "alternate", &early]);
        git(root, &["switch", "-c", "task/TSK-001-fix"]);
        if case == "stale-local" {
            git(root, &["update-ref", "refs/remotes/origin/main", &planned]);
            git(root, &["branch", "-f", "main", &early]);
        }
        let shape = |record: String| {
            let record = standalone_one(&record);
            if case == "retarget" {
                record.replace("integration_target: main", "integration_target: alternate")
            } else {
                record
            }
        };
        let base = match case {
            "stale-local" => "origin/main".to_string(),
            "old-base" => early.clone(),
            _ => "main".to_string(),
        };
        let [verb, check] = complete_reopen_complete(root, &path, &shape, &base, false);
        for (who, result) in [("verb", verb), ("ci", check)] {
            if result.0 == 0 || !result.1.contains("reopened task keeps its criteria") {
                accepted.push(format!("{case} {who}: {}", result.1));
            }
        }
    }
    assert!(accepted.is_empty(), "{}", accepted.join("\n---\n"));
}

/// A new task's reopen still binds its review: a review taken before the
/// criteria changed is refused by the verb and by CI.
#[test]
fn a_new_task_review_before_its_criteria_change_is_refused() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    git(root, &["switch", "-c", "task/TSK-002-new"]);
    let path = record_path("TSK-002");
    write(
        root,
        &path,
        &new_standalone(&task("TSK-002", "todo", OWN_JOURNEY, "Pending.\n")),
    );
    let first = commit(root, "feat: initial work");
    let old = fix_block(&first);
    write(
        root,
        &path,
        &new_standalone(&task("TSK-002", "complete", OWN_JOURNEY, &old)),
    );
    commit(root, "docs(records): complete the task");
    let admitted = codeflow()
        .args(["ids", "admit", &path])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(admitted.status.success());
    let archived = old.replace(
        "acceptance:\n",
        "acceptance_superseded:\n  reason: reopen\n",
    );
    write(
        root,
        &path,
        &new_standalone(&task("TSK-002", "todo", OWN_JOURNEY, &archived)),
    );
    let reviewed = commit(root, "docs: reopen before the criteria change");
    write(
        root,
        &path,
        &new_standalone(&task("TSK-002", "todo", REOPEN_ADDS, &archived)),
    );
    commit(root, "docs: change the criteria after the review");
    let closeout = format!("{archived}{}", three_criteria_block(&reviewed));
    write(
        root,
        &path,
        &new_standalone(&task("TSK-002", "todo", REOPEN_ADDS, &closeout)),
    );
    let verb = status_complete(root, "TSK-002");
    write(
        root,
        &path,
        &new_standalone(&task("TSK-002", "complete", REOPEN_ADDS, &closeout)),
    );
    commit(root, "docs(records): record the acceptance");
    for result in [verb, ci(root, "task/TSK-002-new", "TSK-002")] {
        assert_ne!(
            result.0, 0,
            "a review before the criteria change: {}",
            result.1
        );
    }
}

/// Each path in `results` (the verb, then CI) that accepted the changed
/// criteria or refused them without `needle`, with its output.
fn wrongly_accepted(case: &str, results: [(i32, String); 2], needle: &str) -> Vec<String> {
    let mut wrong = Vec::new();
    for (who, result) in ["verb", "ci"].into_iter().zip(results) {
        if result.0 == 0 || !result.1.contains(needle) {
            wrong.push(format!("{case} {who}: {}", result.1));
        }
    }
    wrong
}

/// The target's tip on a configured upstream of another remote holds the
/// landed task, while the local target branch is stale: the verb reads that
/// upstream as CI does (TSK-217, issue #67).
#[test]
fn a_configured_upstream_on_another_remote_holds_the_task() {
    let dir = repo(&[], "");
    let root = dir.path();
    let early = head(root);
    let path = record_path("TSK-001");
    write(
        root,
        &path,
        &standalone_one(&task("TSK-001", "todo", OWN_JOURNEY, LANDED)),
    );
    let planned = commit(root, "docs: plan the standalone task");
    git(root, &["switch", "-c", "task/TSK-001-fix"]);
    git(
        root,
        &["remote", "add", "upstream", "https://example.test/repo"],
    );
    git(
        root,
        &["update-ref", "refs/remotes/upstream/main", &planned],
    );
    git(root, &["config", "branch.main.remote", "upstream"]);
    git(root, &["config", "branch.main.merge", "refs/heads/main"]);
    git(root, &["branch", "-f", "main", &early]);
    let shape = |record: String| standalone_one(&record);
    let results =
        complete_reopen_complete(root, &path, &shape, "refs/remotes/upstream/main", false);
    let wrong = wrongly_accepted("upstream", results, "reopened task keeps its criteria");
    assert!(wrong.is_empty(), "{}", wrong.join("\n---\n"));
}

/// A clone that lacks the default target cannot prove a retargeted task
/// new: both paths refuse the changed criteria and name the ref to fetch.
#[test]
fn a_clone_without_the_default_target_fails_closed() {
    let dir = repo(&[], "");
    let root = dir.path();
    let early = head(root);
    let path = record_path("TSK-001");
    write(
        root,
        &path,
        &standalone_one(&task("TSK-001", "todo", OWN_JOURNEY, "Pending.\n")),
    );
    commit(root, "docs: plan the standalone task");
    git(root, &["branch", "alternate", &early]);
    git(root, &["switch", "-c", "task/TSK-001-fix"]);
    git(root, &["branch", "-D", "main"]);
    let shape = |record: String| {
        standalone_one(&record).replace("integration_target: main", "integration_target: alternate")
    };
    let results = complete_reopen_complete(root, &path, &shape, "alternate", true);
    let wrong = wrongly_accepted("limited refs", results, "does not resolve here; fetch it");
    assert!(wrong.is_empty(), "{}", wrong.join("\n---\n"));
}

/// The target's landed TSK-002, renumbered TSK-001 with its uid kept, is the
/// same record in every YAML form of that uid (TSK-217, issue #67).
#[test]
fn a_kept_uid_matches_in_every_yaml_form() {
    let mut wrong = Vec::new();
    for form in ["plain", "single", "double"] {
        let dir = repo(&[], "");
        let root = dir.path();
        let old_path = record_path("TSK-002");
        write(
            root,
            &old_path,
            &new_standalone(&task("TSK-002", "todo", OWN_JOURNEY, LANDED)),
        );
        commit(root, "docs: plan the standalone task");
        git(root, &["switch", "-c", "task/TSK-001-fix"]);
        std::fs::remove_file(root.join(&old_path)).unwrap();
        let shape = |record: String| {
            let record = standalone_one(&record);
            let quoted = match form {
                "single" => format!("uid: '{FIXED_UID}'"),
                "double" => format!("uid: \"{FIXED_UID}\""),
                _ => format!("uid: {FIXED_UID}"),
            };
            record.replace(&format!("uid: {FIXED_UID}"), &quoted)
        };
        let path = record_path("TSK-001");
        let results = complete_reopen_complete(root, &path, &shape, "main", true);
        wrong.extend(wrongly_accepted(
            form,
            results,
            "reopened task keeps its criteria",
        ));
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n---\n"));
}

/// A uid YAML reads as another type is the same uid in any spelling of
/// that value: the target's landed TSK-002 with `uid: TRUE`, renumbered
/// TSK-001 with `uid: true`, keeps its criteria as when both spell it
/// alike. A file is skipped unread only when its text cannot spell the
/// task (TSK-234 review round 13).
#[test]
fn a_uid_yaml_reads_as_another_type_keeps_its_history() {
    let mut wrong = Vec::new();
    for landed in ["TRUE", "true"] {
        let dir = repo(&[], "");
        let root = dir.path();
        let old_path = record_path("TSK-002");
        write(
            root,
            &old_path,
            &new_standalone(&task("TSK-002", "todo", OWN_JOURNEY, LANDED))
                .replace(FIXED_UID, landed),
        );
        commit(root, "docs: plan the standalone task");
        git(root, &["switch", "-c", "task/TSK-001-fix"]);
        std::fs::remove_file(root.join(&old_path)).unwrap();
        let shape = |record: String| standalone_one(&record).replace(FIXED_UID, "true");
        let path = record_path("TSK-001");
        let results = complete_reopen_complete(root, &path, &shape, "main", false);
        let verb = results[0].clone();
        if verb.0 == 0 || !verb.1.contains("reopened task keeps its criteria") {
            wrong.push(format!("{landed} verb: {}", verb.1));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n---\n"));
}

/// A task landed on an integration line that `main` predates keeps the
/// line's criteria however its record is later retargeted (issue #67): kept on the
/// line, retargeted to `main` from the first commit of its branch, or
/// retargeted after its first completion. The record's own history in the
/// range names the line, so the verb and CI both read it (TSK-217).
#[test]
fn a_task_planned_on_an_integration_line_keeps_its_criteria_on_retarget() {
    let line = "integration/EPC-001-source";
    let mut wrong = Vec::new();
    for case in ["kept", "retarget", "later"] {
        let dir = repo(&[], "");
        let root = dir.path();
        let path = record_path("TSK-001");
        git(root, &["switch", "-c", line]);
        let on_line = |record: String| {
            standalone_one(&record).replace(
                "integration_target: main",
                &format!("integration_target: {line}"),
            )
        };
        write(
            root,
            &path,
            &on_line(task("TSK-001", "todo", OWN_JOURNEY, LANDED)),
        );
        commit(root, "docs: plan the task on the integration line");
        let admitted = codeflow()
            .args(["ids", "admit", &path])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(admitted.status.success());
        git(root, &["switch", "-c", "task/TSK-001-fix"]);
        let writes = std::cell::Cell::new(0);
        let shape = |record: String| {
            writes.set(writes.get() + 1);
            let keep_line = case == "kept" || (case == "later" && writes.get() <= 2);
            if keep_line {
                on_line(record)
            } else {
                standalone_one(&record)
            }
        };
        let base = if case == "kept" { line } else { "main" };
        let results = complete_reopen_complete(root, &path, &shape, base, false);
        wrong.extend(wrongly_accepted(
            case,
            results,
            "reopened task keeps its criteria",
        ));
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n---\n"));
}

/// A default branch other than `main` or `master` is found through
/// `origin/HEAD`: a task new in its range may then change its criteria on
/// reopen. Without `origin/HEAD` the default target is unknown and both
/// paths refuse with the discovery rule; with `origin/HEAD` naming a branch
/// this clone lacks, both name that branch to fetch (TSK-217).
#[test]
fn a_custom_default_branch_is_found_through_origin_head() {
    let mut wrong = Vec::new();
    for case in ["origin-head", "no-origin-head", "unfetched"] {
        let dir = repo(&[], "");
        let root = dir.path();
        git(root, &["branch", "-m", "develop"]);
        let at = head(root);
        git(root, &["update-ref", "refs/remotes/origin/develop", &at]);
        let named = match case {
            "origin-head" => Some("refs/remotes/origin/develop"),
            "unfetched" => Some("refs/remotes/origin/trunk"),
            _ => None,
        };
        if let Some(named) = named {
            git(root, &["symbolic-ref", "refs/remotes/origin/HEAD", named]);
        }
        git(root, &["switch", "-c", "task/TSK-001-fix"]);
        let shape = |record: String| {
            standalone_one(&record)
                .replace("integration_target: main", "integration_target: develop")
        };
        let results =
            complete_reopen_complete(root, &record_path("TSK-001"), &shape, "develop", true);
        match case {
            "origin-head" => {
                for (who, result) in ["verb", "ci"].into_iter().zip(results) {
                    if result.0 != 0 {
                        wrong.push(format!("{case} {who} refused: {}", result.1));
                    }
                }
            }
            "no-origin-head" => wrong.extend(wrongly_accepted(
                case,
                results,
                "discovery reads `origin/HEAD`, then `main` and `master`",
            )),
            _ => wrong.extend(wrongly_accepted(
                case,
                results,
                "the default target `trunk`, which `origin/HEAD` names, does not resolve here",
            )),
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n---\n"));
}

/// The task's planning commit rewritten on the branch with its target
/// changed to `main` cannot hide a landing on its integration line: another
/// branch adds a completed record outside this range, so the verb and CI
/// both refuse the changed criteria and name that branch (TSK-217, issue
/// #67).
#[test]
fn a_rewritten_planning_commit_cannot_hide_the_line_record() {
    let line = "integration/EPC-001-source";
    let mut wrong = Vec::new();
    for case in ["kept", "rewritten"] {
        let dir = repo(&[], "");
        let root = dir.path();
        let path = record_path("TSK-001");
        git(root, &["switch", "-c", line]);
        write(
            root,
            &path,
            &standalone_one(&task("TSK-001", "todo", OWN_JOURNEY, LANDED)).replace(
                "integration_target: main",
                &format!("integration_target: {line}"),
            ),
        );
        commit(root, "docs: plan the task on the integration line");
        let admitted = codeflow()
            .args(["ids", "admit", &path])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(admitted.status.success());
        if case == "kept" {
            git(root, &["switch", "-c", "task/TSK-001-fix"]);
        } else {
            git(root, &["switch", "-c", "task/TSK-001-fix", "main"]);
            write(
                root,
                &path,
                &standalone_one(&task("TSK-001", "todo", OWN_JOURNEY, "Pending.\n")),
            );
            commit(root, "docs: plan the task on the integration line");
        }
        let shape = |record: String| standalone_one(&record);
        let results = complete_reopen_complete(root, &path, &shape, "main", false);
        let needle = if case == "kept" {
            "reopened task keeps its criteria"
        } else {
            "`integration/EPC-001-source` records a completion of this task outside this range"
        };
        wrong.extend(wrongly_accepted(case, results, needle));
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n---\n"));
}

/// A stale branch that adds a completed copy of the task's record outside
/// the range keeps a new task from changing its criteria on reopen; both
/// paths name it, so a human can delete it if it is abandoned (TSK-217). A
/// branch holding only a planned copy shows no landing, so it no longer
/// stops the change (issue #67).
#[test]
fn a_stale_branch_holding_the_record_is_named() {
    for completed in [true, false] {
        let dir = repo(&[], "");
        let root = dir.path();
        let path = record_path("TSK-001");
        git(root, &["switch", "-c", "planning-draft"]);
        write(
            root,
            &path,
            &standalone_one(&task(
                "TSK-001",
                "todo",
                OWN_JOURNEY,
                if completed { LANDED } else { "A draft.\n" },
            )),
        );
        commit(root, "docs: draft the task");
        let admitted = codeflow()
            .args(["ids", "admit", &path])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(admitted.status.success());
        git(root, &["switch", "-c", "task/TSK-001-fix", "main"]);
        let shape = |record: String| standalone_one(&record);
        let results = complete_reopen_complete(root, &path, &shape, "main", false);
        if !completed {
            for (who, result) in ["verb", "ci"].into_iter().zip(results) {
                assert_eq!(result.0, 0, "a planned copy, {who}: {}", result.1);
            }
            continue;
        }
        let wrong = wrongly_accepted(
            "stale branch",
            results,
            "`planning-draft` records a completion of this task outside this range",
        );
        assert!(wrong.is_empty(), "{}", wrong.join("\n---\n"));
    }
}

/// Copies of the task's record that came from this range's own commits do
/// not count as recorded elsewhere: a branch stacked on the reviewed fix
/// and a remote copy of the task branch with an unrelated commit on top
/// leave a new task free to change its criteria on reopen (TSK-217).
#[test]
fn copies_from_the_range_leave_a_new_task_new() {
    let dir = repo(&[], "");
    let root = dir.path();
    git(root, &["switch", "-c", "task/TSK-001-fix"]);
    let stack = |branch: &str| {
        git(root, &["switch", "-c", "task/TSK-009-next"]);
        write(root, "src/next.rs", "pub fn next() {}\n");
        commit(root, "feat: start the next task");
        git(root, &["switch", "-c", "pushed-copy", branch]);
        write(root, ".github/notes.txt", "a human edit\n");
        let copy = commit(root, "ci: a human edit on the pushed branch");
        git(
            root,
            &[
                "update-ref",
                &format!("refs/remotes/origin/{branch}"),
                &copy,
            ],
        );
        git(root, &["switch", branch]);
        git(root, &["branch", "-D", "pushed-copy"]);
    };
    let shape = |record: String| standalone_one(&record);
    let results =
        complete_reopen_complete_with(root, &record_path("TSK-001"), &shape, "main", true, &stack);
    for (who, result) in ["verb", "ci"].into_iter().zip(results) {
        assert_eq!(result.0, 0, "{who}: {}", result.1);
    }
}

/// TSK-220, merged-head review: only the target the run is judged against
/// supplies a reopened task's criteria. The landed task is on `main`;
/// its branch, cut before that, completes it, reopens it and adds a
/// criterion. With local `main` stale, pointing `origin/main` or `main`'s
/// configured upstream on another remote at the branch's own commit never
/// makes that commit the target's record: CI against the published base
/// refuses the changed criteria, as it does with ordinary refs.
#[test]
fn only_the_judged_target_supplies_a_reopened_tasks_criteria() {
    let mut passed = Vec::new();
    for case in [
        "control",
        "stale-only",
        "forged-origin",
        "alternate-upstream",
    ] {
        let dir = repo(&[], "");
        let root = dir.path();
        let early = head(root);
        let path = record_path("TSK-001");
        write(
            root,
            &path,
            &standalone_one(&task("TSK-001", "todo", OWN_JOURNEY, LANDED)),
        );
        let published = commit(root, "docs(records): publish the task on main");
        git(root, &["switch", "-q", "-c", "task/TSK-001-fix", &early]);
        let spoof = |_branch: &str| {
            let tip = head(root);
            if case != "control" {
                git(root, &["branch", "-f", "main", &early]);
            }
            if case == "forged-origin" {
                git(root, &["update-ref", "refs/remotes/origin/main", &tip]);
            }
            if case == "alternate-upstream" {
                git(
                    root,
                    &[
                        "remote",
                        "add",
                        "alternate",
                        "https://example.test/alternate",
                    ],
                );
                git(root, &["update-ref", "refs/remotes/alternate/main", &tip]);
                git(root, &["config", "branch.main.remote", "alternate"]);
                git(root, &["config", "branch.main.merge", "refs/heads/main"]);
            }
        };
        let shape = |record: String| standalone_one(&record);
        // The landed record on `main` and the branch's own copy differ, so
        // the id registry cannot seed them as one record; the binding is
        // what this judges.
        let [_, ci] = complete_reopen_complete_with(root, &path, &shape, &published, false, &spoof);
        if ci.0 == 0 {
            passed.push(case);
        } else {
            assert_blocks(
                &ci,
                &format!("{case}: CI against the published base"),
                &["work.acceptance_binding", "TSK-001", "keeps its criteria"],
            );
        }
    }
    assert!(
        passed.is_empty(),
        "CI took the changed criteria in: {passed:?}"
    );
}

/// TSK-220, re-review of the judged-target fix: a line range's own head may
/// carry merges for the tasks that target the line, but it never supplies
/// a reopened task's criteria. The task has landed on the line; its branch,
/// cut before that planning, completes it, reopens it and adds a
/// criterion, which the verb and CI refuse there. Carrying the same
/// completion onto the line, with no criteria amendment, still refuses on
/// the line's pull request into `main`.
#[test]
fn a_lines_own_head_never_supplies_a_reopened_tasks_criteria() {
    let dir = repo(&[("TSK-002", OWN_JOURNEY)], "");
    let root = dir.path();
    write(
        root,
        &record_path("TSK-002"),
        &line_task("TSK-002", "todo", "Pending.\n"),
    );
    commit(root, "docs(records): target the line");
    let early = head(root);
    let path = record_path("TSK-001");
    git(root, &["switch", "-q", "-c", LINE]);
    git(root, &["switch", "-q", "-c", "plan/add-task", LINE]);
    write(
        root,
        &path,
        &standalone_one(&line_task("TSK-001", "todo", LANDED)),
    );
    commit(root, "docs(records): plan the task on the line");
    land(root, "plan/add-task");
    git(root, &["switch", "-q", "-c", "task/TSK-001-fix", &early]);
    let shape = |record: String| {
        standalone_one(&record).replace(
            "integration_target: main",
            &format!("integration_target: {LINE}"),
        )
    };
    let [verb, ci] = complete_reopen_complete(root, &path, &shape, LINE, false);
    assert_ne!(verb.0, 0, "the verb on the task branch: {}", verb.1);
    assert_ne!(ci.0, 0, "CI on the task branch: {}", ci.1);
    let completed = std::fs::read_to_string(root.join(&path)).unwrap();
    git(root, &["switch", "-q", LINE]);
    git_try(
        root,
        &["merge", "--no-ff", "--no-commit", "task/TSK-001-fix"],
    );
    write(root, &path, &completed);
    commit(root, "merge: task/TSK-001-fix");
    assert_blocks(
        &ci_on(root, "main", LINE, ""),
        "the carried completion on the line",
        &["work.acceptance_binding", "TSK-001", "keeps its criteria"],
    );
}

/// Issue #69: `follow_ups: none: <reason>` is not valid YAML, so a block a
/// reviewer loads with a YAML parser may quote it. The quoted form completes
/// by the verb and passes CI, as the plain form still does.
#[test]
fn a_quoted_follow_ups_completes_by_the_verb_and_in_ci() {
    for follow_ups in ["\"none: nothing deferred\"", "none: nothing deferred"] {
        let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
        let root = dir.path();
        let reviewed = code_change(root, BRANCH, "pub fn quoted() {}\n");
        let closeout = block(
            &reviewed,
            &["AC-1: verified | unit", "AC-2: verified | journey"],
            "verified | journey",
            follow_ups,
        );
        write(
            root,
            &record_path("TSK-001"),
            &task("TSK-001", "todo", OWN_JOURNEY, &closeout),
        );
        assert_passes(&status_complete(root, "TSK-001"), follow_ups);
        complete(root, "TSK-001", OWN_JOURNEY, &closeout);
        assert_passes(&ci(root, BRANCH, "TSK-001"), follow_ups);
    }
}

/// TSK-234 review round 5: `src/lib.rs` and a file literally named
/// `src\lib.rs` are two paths. A merge that changes the first beyond the
/// automatic remerge is the range's own work, which the review never saw,
/// however the two paths read once converted.
#[cfg(unix)]
#[test]
fn a_merge_edit_is_not_hidden_by_a_path_that_reads_alike() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    write(root, r"src\lib.rs", "shadow\n");
    commit(root, "docs: add a path that reads alike");
    let reviewed = code_change(root, BRANCH, "pub fn work() {}\n");
    git(root, &["switch", "main"]);
    write(root, "src/line.rs", "pub fn line() {}\n");
    commit(root, "feat: advance the target");
    git(root, &["switch", BRANCH]);
    git(root, &["merge", "--no-ff", "--no-commit", "main"]);
    write(root, "src/lib.rs", "pub fn unreviewed() {}\n");
    commit(root, "chore: merge the target with an extra edit");
    complete(root, "TSK-001", OWN_JOURNEY, &valid_block(&reviewed));
    assert_blocks(
        &ci(root, BRANCH, "TSK-001"),
        "an unreviewed merge edit beside a path that reads alike",
        &["work.acceptance_binding"],
    );
}

/// TSK-234 review round 5: a landed completion whose task directory object
/// cannot be read is no proof that the task never landed, so the task's
/// own pull request cannot change its criteria.
#[test]
fn an_unreadable_historical_record_tree_never_proves_a_planned_task() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    let reviewed = head(root);
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "complete", OWN_JOURNEY, &valid_block(&reviewed)),
    );
    let landed = commit(root, "docs: land a completion");
    let tree = git_out(
        root,
        &["rev-parse", &format!("{landed}:project-management/tasks")],
    );
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", OWN_JOURNEY, "Pending.\n"),
    );
    commit(root, "docs: reopen and clear the evidence");
    git(root, &["switch", "-c", BRANCH]);
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", REOPEN_ADDS, "Pending.\n"),
    );
    commit(root, "docs: loosen its criteria");
    std::fs::remove_file(root.join(".git/objects").join(&tree[..2]).join(&tree[2..])).unwrap();
    assert_blocks(
        &ci(root, BRANCH, "TSK-001"),
        "an unreadable historical record tree",
        &["work.criteria_frozen", "cannot be read"],
    );
}

/// TSK-234 review round 5: an id the target's history carried with two
/// uids is ambiguous even after its newest version drops its uid, so it
/// proves no planned task.
#[test]
fn a_dropped_uid_does_not_hide_an_ambiguous_history() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    for uid in [
        "6f1c2b8e-3d4a-4f5b-9c6d-7e8f9a0b1c2d",
        "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d",
    ] {
        write(
            root,
            &record_path("TSK-001"),
            &task("TSK-001", "todo", OWN_JOURNEY, "Pending.\n")
                .replace("id: TSK-001\n", &format!("id: TSK-001\nuid: {uid}\n")),
        );
        commit(root, "docs: set its identity");
    }
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", OWN_JOURNEY, "Pending.\n"),
    );
    commit(root, "docs: drop the uid on the target");
    git(root, &["switch", "-c", BRANCH]);
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", REOPEN_ADDS, "Pending.\n"),
    );
    commit(root, "docs: loosen its criteria");
    assert_blocks(
        &ci(root, BRANCH, "TSK-001"),
        "one id with two uids in the history",
        &["work.criteria_frozen", "another uid"],
    );
}

/// TSK-234 review round 5: two identical records of one task in one
/// historical commit are two records, so the history proves no planned
/// task.
#[test]
fn identical_duplicate_records_are_ambiguous() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    let copy = "project-management/epics/EPC-001/tasks/TSK-001.md";
    write(
        root,
        copy,
        &task("TSK-001", "todo", OWN_JOURNEY, "Pending.\n"),
    );
    commit(root, "docs: copy the task record");
    git(root, &["rm", "-q", copy]);
    commit(root, "docs: remove the copy");
    git(root, &["switch", "-c", BRANCH]);
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", REOPEN_ADDS, "Pending.\n"),
    );
    commit(root, "docs: loosen its criteria");
    assert_blocks(
        &ci(root, BRANCH, "TSK-001"),
        "two records of one task in one commit",
        &["work.criteria_frozen", "holds 2 records"],
    );
}

/// TSK-234 review round 5: two judged lines each completed the task with
/// different criteria and then deleted it. Neither line contains the
/// other, so neither version is the target's authority, and recreating the
/// task with either set of criteria is refused.
#[test]
fn deleted_histories_that_disagree_give_no_authority() {
    use codeflow_core::workgraph::acceptance::{pull_request_findings_judged, Criteria};
    let dir = repo(&[], "");
    let root = dir.path();
    let origin = head(root);
    for (branch, criteria) in [("base-a", OWN_JOURNEY), ("base-b", REOPEN_ADDS)] {
        git(root, &["switch", "-c", branch, &origin]);
        write(
            root,
            &record_path("TSK-001"),
            &task("TSK-001", "complete", criteria, "Pending.\n"),
        );
        commit(root, "docs: complete the task on a target");
        git(root, &["rm", "-q", &record_path("TSK-001")]);
        commit(root, "docs: delete the task on a target");
    }
    git(root, &["switch", "-c", BRANCH, &origin]);
    for criteria in [OWN_JOURNEY, REOPEN_ADDS] {
        write(
            root,
            &record_path("TSK-001"),
            &task("TSK-001", "todo", criteria, "Pending.\n"),
        );
        commit(root, "docs: recreate the task");
        let findings = pull_request_findings_judged(
            root,
            "base-a",
            "HEAD",
            &Criteria::OwnTask("TSK-001".into()),
            Some("base-b"),
            Some(BRANCH),
            &[],
        )
        .unwrap();
        assert!(
            findings.iter().any(|found| !found.note),
            "recreated with {criteria:?}: {findings:?}"
        );
    }
}

/// TSK-234 review round 6: two judged lines each held two versions of a
/// reopened task, in opposite orders, and then deleted it. Each newest
/// version also appears in the other line's history, but neither line
/// inherited it from the other, so neither is the target's authority.
#[test]
fn crossed_versions_on_two_lines_give_no_authority() {
    use codeflow_core::workgraph::acceptance::{pull_request_findings_judged, Criteria};
    let dir = repo(&[], "");
    let root = dir.path();
    let origin = head(root);
    for (branch, first, last) in [
        ("base-a", OWN_JOURNEY, REOPEN_ADDS),
        ("base-b", REOPEN_ADDS, OWN_JOURNEY),
    ] {
        git(root, &["switch", "-c", branch, &origin]);
        for criteria in [first, last] {
            write(
                root,
                &record_path("TSK-001"),
                &task("TSK-001", "todo", criteria, LANDED),
            );
            commit(root, "docs: a version on a target");
        }
        git(root, &["rm", "-q", &record_path("TSK-001")]);
        commit(root, "docs: delete the task on a target");
    }
    git(root, &["switch", "-c", BRANCH, &origin]);
    for criteria in [OWN_JOURNEY, REOPEN_ADDS] {
        write(
            root,
            &record_path("TSK-001"),
            &task("TSK-001", "todo", criteria, "Pending.\n"),
        );
        commit(root, "docs: add the record again");
        let findings = pull_request_findings_judged(
            root,
            "base-a",
            "HEAD",
            &Criteria::OwnTask("TSK-001".into()),
            Some("base-b"),
            Some(BRANCH),
            &[],
        )
        .unwrap();
        assert!(
            findings.iter().any(|found| !found.note),
            "added again with {criteria:?}: {findings:?}"
        );
    }
}

/// TSK-234 review round 6: a historical task file that does not parse but
/// spells the task's id by a YAML escape may be the task, so the history
/// proves no planned task.
#[test]
fn an_escaped_record_that_does_not_parse_gives_no_planned_answer() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    let broken = "---\nid: \"\\x54SK-001\"\nstatus: complete\nbroken: [\n---\n";
    write(root, "project-management/tasks/TSK-999.md", broken);
    commit(root, "docs: a task file that does not parse");
    git(root, &["rm", "-q", "project-management/tasks/TSK-999.md"]);
    commit(root, "docs: drop it");
    git(root, &["switch", "-c", BRANCH]);
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", REOPEN_ADDS, "Pending.\n"),
    );
    commit(root, "docs: loosen its criteria");
    assert_blocks(
        &ci(root, BRANCH, "TSK-001"),
        "a task file that does not parse and may be the task",
        &["work.criteria_frozen", "does not parse"],
    );
}

/// TSK-234 review round 6: the criteria a landed task keeps come from the
/// newest judged point. The target holds the reopened task with two
/// criteria; a newer run base adds a third through planning. The task's
/// branch keeps the target's two, which still changes the authority's
/// criteria, so it is refused by the core check and by `codeflow ci`.
#[test]
fn a_newer_run_base_holds_the_criteria_to_keep() {
    use codeflow_core::workgraph::acceptance::{pull_request_findings_judged, Criteria};
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", OWN_JOURNEY, LANDED),
    );
    commit(root, "docs: the target holds a reopened task");
    let origin = head(root);
    git(root, &["switch", "-c", "candidate"]);
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", REOPEN_ADDS, LANDED),
    );
    commit(root, "docs: a planning amendment adds a criterion");
    git(root, &["switch", "-c", BRANCH, &origin]);
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", OWN_JOURNEY, LANDED).replace("Work.", "Work described."),
    );
    commit(root, "docs: the task keeps the older criteria");
    let findings = pull_request_findings_judged(
        root,
        "main",
        "HEAD",
        &Criteria::OwnTask("TSK-001".into()),
        Some("candidate"),
        Some(BRANCH),
        &[],
    )
    .unwrap();
    assert!(
        findings.iter().any(|found| !found.note),
        "the newer base's criteria: {findings:?}"
    );
    let out = codeflow()
        .args([
            "ci",
            "--base",
            "main",
            "--head",
            "HEAD",
            "--branch",
            BRANCH,
            "--policy-from",
            "candidate",
            "--pr-body",
            "## Summary\nA change.\n\n- one change\n\nTask: TSK-001\n\n## Changes\n- one\n\n## Testing\n- test\n",
        ])
        .current_dir(root)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!out.status.success(), "CI accepted it:\n{text}");
    assert!(text.contains("work.criteria_frozen"), "{text}");
}

/// TSK-234 review round 7: the frontmatter of a task file that does not
/// parse is found by the parser's own delimiter rules, so a line that only
/// starts with `---` does not end it early and hide an escaped id.
#[test]
fn a_broken_file_is_read_to_the_parsers_own_delimiter() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    let broken =
        "---\n---note: an ordinary key\nid: \"\\x54SK-001\"\nstatus: complete\nbroken: [\n---\n";
    let parsed = codeflow_core::workgraph::lifecycle::RecordView::parse(
        codeflow_core::workgraph::work_start::RecordKind::Task,
        "project-management/tasks/TSK-999.md",
        &broken.replace("broken: [\n", ""),
    )
    .unwrap();
    assert_eq!(parsed.id, "TSK-001", "the parser reads the escaped id");
    write(root, "project-management/tasks/TSK-999.md", broken);
    commit(root, "docs: a task file that does not parse");
    git(root, &["rm", "-q", "project-management/tasks/TSK-999.md"]);
    commit(root, "docs: drop it");
    git(root, &["switch", "-c", BRANCH]);
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", REOPEN_ADDS, "Pending.\n"),
    );
    commit(root, "docs: loosen its criteria");
    assert_blocks(
        &ci(root, BRANCH, "TSK-001"),
        "an escaped id after a line that starts with ---",
        &["work.criteria_frozen", "does not parse"],
    );
}

/// TSK-234 review round 7: an uncommitted file literally named
/// `project-management\tasks\TSK-001.md` is not the record, so completing
/// the task with it in the working tree changes more than the record's
/// status and Closeout, and the verb refuses.
#[cfg(unix)]
#[test]
fn the_verb_does_not_take_a_path_that_reads_like_the_record_for_it() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    let reviewed = code_change(root, BRANCH, "pub fn reviewed() {}\n");
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", OWN_JOURNEY, &valid_block(&reviewed)),
    );
    write(
        root,
        r"project-management\tasks\TSK-001.md",
        "an unreviewed file\n",
    );
    let out = status_complete(root, "TSK-001");
    assert_ne!(
        out.0, 0,
        "an unreviewed file is not status and Closeout:\n{}",
        out.1
    );
}

/// TSK-234 review round 7: a merge that resolves a record to an older
/// version against the automatic remerge chose that version, so it is that
/// line's own newest version, not one inherited unchanged. Two lines whose
/// newest versions then disagree give no authority.
#[test]
fn a_merge_resolution_is_a_lines_own_version() {
    use codeflow_core::workgraph::acceptance::{
        owned_paths, pull_request_findings_judged, Criteria,
    };
    let dir = repo(&[], "");
    let root = dir.path();
    let empty = head(root);
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", OWN_JOURNEY, LANDED),
    );
    commit(root, "docs: a landed and reopened task");
    let origin = head(root);
    git(root, &["switch", "-c", "stale"]);
    write(root, "docs/note.md", "older side work\n");
    commit(root, "docs: work on an older line");
    git(root, &["switch", "-c", "base-a", &origin]);
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", REOPEN_ADDS, LANDED),
    );
    commit(root, "docs: amend its criteria to B");
    git(root, &["rm", "-q", &record_path("TSK-001")]);
    commit(root, "docs: delete the record");
    git(root, &["switch", "-c", "base-b", &origin]);
    write(
        root,
        &record_path("TSK-001"),
        &task(
            "TSK-001",
            "todo",
            &format!("{REOPEN_ADDS}- AC-4 When tested, it shall work.\n"),
            LANDED,
        ),
    );
    commit(root, "docs: amend its criteria to C");
    git(root, &["merge", "--no-ff", "--no-commit", "stale"]);
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", OWN_JOURNEY, LANDED),
    );
    commit(root, "docs: resolve the merge by restoring A");
    let owned = owned_paths(root, "HEAD", &["HEAD^1", "HEAD^2"]).unwrap();
    assert!(
        owned.contains(&record_path("TSK-001")),
        "the merge reading counts the resolution: {owned:?}"
    );
    git(root, &["rm", "-q", &record_path("TSK-001")]);
    commit(root, "docs: delete the record");
    git(root, &["switch", "-c", BRANCH, &empty]);
    for criteria in [OWN_JOURNEY, REOPEN_ADDS] {
        write(
            root,
            &record_path("TSK-001"),
            &task("TSK-001", "todo", criteria, "Pending.\n"),
        );
        commit(root, "docs: add the record again");
        let findings = pull_request_findings_judged(
            root,
            "base-a",
            "HEAD",
            &Criteria::OwnTask("TSK-001".into()),
            Some("base-b"),
            Some(BRANCH),
            &[],
        )
        .unwrap();
        assert!(
            findings.iter().any(|found| !found.note),
            "added again with {criteria:?}: {findings:?}"
        );
    }
}

/// TSK-234 review round 8: three lines keep A, change A to B, and change A
/// to C and back to A written in other bytes; all delete the record and
/// merge. The restored A is that line's own newest version, so it competes
/// with B whatever order the lines merge in, and recreating the task with
/// B's criteria is refused in every order.
#[test]
fn equal_versions_in_other_bytes_keep_their_own_history() {
    use codeflow_core::workgraph::acceptance::{pull_request_findings_judged, Criteria};
    let orders = [
        ["a", "c", "b"],
        ["a", "b", "c"],
        ["b", "a", "c"],
        ["b", "c", "a"],
        ["c", "a", "b"],
        ["c", "b", "a"],
    ];
    let restored = format!("{REOPEN_ADDS}- AC-4 When tested, it shall work.\n");
    let mut accepted = Vec::new();
    for order in orders {
        let dir = repo(&[], "");
        let root = dir.path();
        let empty = head(root);
        write(
            root,
            &record_path("TSK-001"),
            &task("TSK-001", "todo", OWN_JOURNEY, LANDED),
        );
        commit(root, "docs: the original criteria A");
        let origin = head(root);
        for line in ["a", "b", "c"] {
            git(root, &["switch", "-c", line, &origin]);
            match line {
                "a" => {
                    write(root, "docs/a.md", "older unrelated work\n");
                    commit(root, "docs: an older line");
                }
                "b" => {
                    write(
                        root,
                        &record_path("TSK-001"),
                        &task("TSK-001", "todo", REOPEN_ADDS, LANDED),
                    );
                    commit(root, "docs: choose criteria B");
                }
                _ => {
                    write(
                        root,
                        &record_path("TSK-001"),
                        &task("TSK-001", "todo", &restored, LANDED),
                    );
                    commit(root, "docs: choose criteria C");
                    write(
                        root,
                        &record_path("TSK-001"),
                        &task("TSK-001", "todo", OWN_JOURNEY, LANDED)
                            .replace("Work.", "Work described."),
                    );
                    commit(root, "docs: restore criteria A on its own");
                }
            }
            git(root, &["rm", "-q", &record_path("TSK-001")]);
            commit(root, "docs: delete the record");
        }
        git(root, &["switch", "-c", "target", order[0]]);
        for line in &order[1..] {
            git(root, &["merge", "-q", "--no-ff", "--no-edit", line]);
        }
        git(root, &["switch", "-c", BRANCH, &empty]);
        write(
            root,
            &record_path("TSK-001"),
            &task("TSK-001", "todo", REOPEN_ADDS, "Pending.\n"),
        );
        commit(root, "docs: recreate it with B's criteria");
        let findings = pull_request_findings_judged(
            root,
            "target",
            "HEAD",
            &Criteria::OwnTask("TSK-001".into()),
            None,
            Some(BRANCH),
            &[],
        )
        .unwrap();
        if !findings.iter().any(|found| !found.note) {
            accepted.push(order);
        }
    }
    assert!(
        accepted.is_empty(),
        "the restored A must compete with B: {accepted:?}"
    );
}

/// TSK-234 review round 8: any epic directory may hold task records, so
/// one whose name is not UTF-8 hides them; the history then proves no
/// planned task, and the task's own pull request cannot change its
/// criteria.
#[cfg(unix)]
#[test]
fn an_epic_directory_named_in_other_bytes_is_no_proof_of_absence() {
    use std::io::Write as _;
    use std::os::unix::ffi::OsStringExt;
    use std::process::{Command, Stdio};
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    let content = task("TSK-001", "todo", OWN_JOURNEY, LANDED);
    let mut hash = Command::new("git")
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    hash.stdin
        .take()
        .unwrap()
        .write_all(content.as_bytes())
        .unwrap();
    let blob = String::from_utf8(hash.wait_with_output().unwrap().stdout).unwrap();
    git(root, &["rm", "-q", &record_path("TSK-001")]);
    let path = std::ffi::OsString::from_vec(
        b"project-management/epics/EPC-\xff/tasks/TSK-001.md".to_vec(),
    );
    let out = Command::new("git")
        .args([
            "update-index",
            "--add",
            "--cacheinfo",
            "100644",
            blob.trim(),
        ])
        .arg(&path)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    git(
        root,
        &[
            "commit",
            "-qm",
            "docs: a completion under an epic directory",
        ],
    );
    let out = Command::new("git")
        .args(["update-index", "--force-remove"])
        .arg(&path)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(out.status.success());
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", OWN_JOURNEY, "Pending.\n"),
    );
    git(root, &["add", &record_path("TSK-001")]);
    git(root, &["commit", "-qm", "docs: move the record back"]);
    git(root, &["switch", "-c", BRANCH]);
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", REOPEN_ADDS, "Pending.\n"),
    );
    commit(root, "docs: change its criteria");
    assert_blocks(
        &ci(root, BRANCH, "TSK-001"),
        "a completion under an epic directory named in other bytes",
        &["work.criteria_frozen", "not UTF-8"],
    );
}

/// Write `content` as a blob and stage it at the raw name `name`, which
/// need not be UTF-8.
#[cfg(unix)]
fn stage_raw(root: &Path, name: &[u8], content: &[u8]) -> std::ffi::OsString {
    use std::io::Write as _;
    use std::os::unix::ffi::OsStringExt;
    use std::process::{Command, Stdio};
    let mut hash = Command::new("git")
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    hash.stdin.take().unwrap().write_all(content).unwrap();
    let blob = String::from_utf8(hash.wait_with_output().unwrap().stdout).unwrap();
    let raw = std::ffi::OsString::from_vec(name.to_vec());
    let out = Command::new("git")
        .args([
            "update-index",
            "--add",
            "--cacheinfo",
            "100644",
            blob.trim(),
        ])
        .arg(&raw)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    raw
}

/// TSK-234 review round 9: the task lives under an epic directory named
/// with a literal U+FFFD; a staged file under a directory named with the
/// byte FF reads alike once decoded lossily, but it is another file, so
/// the verb refuses to complete the task with it staged.
#[cfg(unix)]
#[test]
fn the_verb_keeps_every_byte_of_a_staged_name() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    let real = "project-management/epics/EPC-\u{fffd}/tasks/TSK-001.md";
    write(
        root,
        real,
        &task("TSK-001", "todo", OWN_JOURNEY, "Pending.\n"),
    );
    git(root, &["rm", "-q", &record_path("TSK-001")]);
    commit(root, "docs: keep the task under its epic");
    let reviewed = code_change(root, BRANCH, "pub fn reviewed() {}\n");
    write(
        root,
        real,
        &task("TSK-001", "todo", OWN_JOURNEY, &valid_block(&reviewed)),
    );
    stage_raw(
        root,
        b"project-management/epics/EPC-\xff/tasks/TSK-001.md",
        b"an unreviewed file\n",
    );
    let out = status_complete(root, "TSK-001");
    assert_ne!(out.0, 0, "a staged file is not the record:\n{}", out.1);
}

/// TSK-234 review round 9: only a directory under `epics/` can hold task
/// records, so a plain file there named in other bytes is no unreadable
/// epic directory, and a planned task's own pull request may still change
/// its criteria.
#[cfg(unix)]
#[test]
fn a_plain_file_under_epics_named_in_other_bytes_is_not_a_directory() {
    use std::process::Command;
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    let raw = stage_raw(
        root,
        b"project-management/epics/note-\xff.txt",
        b"an unrelated note\n",
    );
    git(root, &["commit", "-qm", "docs: a note that is no record"]);
    let out = Command::new("git")
        .args(["update-index", "--force-remove"])
        .arg(&raw)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(out.status.success());
    git(root, &["commit", "-qm", "docs: remove the note"]);
    git(root, &["switch", "-c", BRANCH]);
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", "todo", REOPEN_ADDS, "Pending.\n"),
    );
    commit(root, "docs: change its planned criteria");
    assert_passes(
        &ci(root, BRANCH, "TSK-001"),
        "a plain file under epics named in other bytes",
    );
}

/// TSK-234 review round 10: a product change that cancels out before the
/// head is still the range's own work. With the product pattern
/// `src/*[!0-9].rs`, adding and removing `src/q.rs` needs a journey; a name
/// that is not UTF-8 cannot be matched by any rendering, so CI refuses it
/// instead of reading it as outside the product paths.
#[cfg(unix)]
#[test]
fn a_cancelled_change_to_a_name_in_other_bytes_still_refuses() {
    use std::process::Command;
    for (path, needle) in [
        (b"src/q.rs".as_slice(), "work.journey"),
        (b"src/\xff.rs".as_slice(), "not UTF-8"),
    ] {
        let dir = repo(&[("TSK-001", NO_JOURNEY)], "");
        let root = dir.path();
        write(
            root,
            ".codeflow/policy.json",
            r#"{"schema_version":1,"git":{"product_paths":["src/*[!0-9].rs"]}}"#,
        );
        commit(root, "docs: name the product paths");
        git(root, &["switch", "-c", BRANCH]);
        let raw = stage_raw(root, path, b"pub fn own() {}\n");
        git(root, &["commit", "-qm", "feat: add own code"]);
        let out = Command::new("git")
            .args(["update-index", "--force-remove"])
            .arg(&raw)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(out.status.success());
        git(root, &["commit", "-qm", "fix: remove own code"]);
        assert_blocks(
            &ci(root, BRANCH, "TSK-001"),
            "own product work that cancels out",
            &[needle],
        );
    }
}

/// TSK-234 review round 11: with two merge bases, the net change can name
/// a path no commit in the walked range touched. Commits X and Y add a
/// product file and a note; the target and the task each merge X and Y
/// cleanly. The net change still lists the product file, so `src/q.rs`
/// needs a journey, and a name that is not UTF-8 refuses instead of being
/// decoded out of the product pattern `src/*[!\u{fffd}].rs`.
#[cfg(unix)]
#[test]
fn a_net_change_name_in_other_bytes_refuses_with_two_merge_bases() {
    use std::process::Command;
    for (path, needle) in [
        (b"src/q.rs".as_slice(), "work.journey"),
        (b"src/\xff.rs".as_slice(), "not UTF-8"),
    ] {
        let dir = repo(&[("TSK-001", NO_JOURNEY)], "");
        let root = dir.path();
        write(
            root,
            ".codeflow/policy.json",
            "{\"schema_version\":1,\"git\":{\"product_paths\":[\"src/*[!\u{fffd}].rs\"]}}",
        );
        let start = commit(root, "docs: name the product paths");
        let commit_tree = |tree: &str, parents: &[&str], message: &str, date: &str| {
            let mut args = vec!["commit-tree", tree];
            for parent in parents {
                args.extend(["-p", parent]);
            }
            args.extend(["-m", message]);
            let out = Command::new("git")
                .args(&args)
                .env("GIT_AUTHOR_DATE", date)
                .env("GIT_COMMITTER_DATE", date)
                .current_dir(root)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8(out.stdout).unwrap().trim().to_string()
        };
        let raw = stage_raw(root, path, b"pub fn raw() {}\n");
        let x_tree = git_out(root, &["write-tree"]);
        let x = commit_tree(&x_tree, &[&start], "feat: raw code", "2000000010 +0000");
        let out = Command::new("git")
            .args(["update-index", "--force-remove"])
            .arg(&raw)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(out.status.success());
        write(root, "docs/note.md", "the other side\n");
        git(root, &["add", "docs/note.md"]);
        let y_tree = git_out(root, &["write-tree"]);
        let y = commit_tree(&y_tree, &[&start], "docs: a note", "2000000020 +0000");
        stage_raw(root, path, b"pub fn raw() {}\n");
        let both = git_out(root, &["write-tree"]);
        let target = commit_tree(&both, &[&x, &y], "chore: target merge", "2000000030 +0000");
        let own = commit_tree(&both, &[&y, &x], "chore: own merge", "2000000040 +0000");
        git(root, &["update-ref", "refs/heads/main", &target]);
        git(root, &["update-ref", &format!("refs/heads/{BRANCH}"), &own]);
        git(
            root,
            &["symbolic-ref", "HEAD", &format!("refs/heads/{BRANCH}")],
        );
        assert_blocks(
            &ci(root, BRANCH, "TSK-001"),
            "a net change name with two merge bases",
            &[needle],
        );
    }
}
