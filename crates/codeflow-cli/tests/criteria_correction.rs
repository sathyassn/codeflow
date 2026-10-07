//! A completed standalone task's criteria text can be corrected (TSK-253,
//! ADR-0080, SPC-013 R-52, sathyassn/codeflow#103). A pull request of
//! planning records only whose `Task:` line names the completed standalone
//! task, or a follow-up whose `follow_up_of` is that task, may change the
//! text of its existing criteria: `codeflow ci` passes and prints the delta
//! as a `work.criteria_frozen` note. The criteria set and every tag stay
//! frozen, and so does every other case: a non-planning path in the range,
//! an unrelated task, an epic task, the task's own branch and a reopen.
//! Each test runs the Cargo-built binary in a tempdir repository with
//! durable work tracking on.

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

const EPIC: &str = "---\nid: EPC-001\ntitle: \"outcome\"\nstatus: planning\nwork_type: feat\nspecs: []\ncreated: 2026-10-07\n---\n\n# EPC-001: outcome\n\n## Summary\n\nAn outcome.\n\n## Acceptance Criteria\n\n- AC-1 (journey) On a fresh project, an adopter shall finish the flow.\n";

/// The criteria every completed task starts with: AC-1 names a person.
const CRITERIA: &str = "- AC-1 When run, the system shall work as Jane Doe asked.\n- AC-2 (journey) On a fresh project, the command shall succeed.\n";
/// AC-1 with the person replaced by a role: the issue's correction.
const CORRECTED: &str = "- AC-1 When run, the system shall work as the release owner asked.\n- AC-2 (journey) On a fresh project, the command shall succeed.\n";

/// A task record: of `epic`, or standalone when `None`, following up
/// `follow_up_of` when set.
fn task(
    id: &str,
    epic: Option<&str>,
    follow_up_of: Option<&str>,
    status: &str,
    criteria: &str,
    closeout: &str,
) -> String {
    let parent = match epic {
        Some(epic) => format!("epic_id: {epic}\nstandalone_reason: null"),
        None => "epic_id: null\nstandalone_reason: \"one outcome\"".to_string(),
    };
    let follows = follow_up_of
        .map(|source| format!("follow_up_of: {source}\n"))
        .unwrap_or_default();
    format!(
        "---\nid: {id}\n{parent}\nintegration_target: main\ntitle: \"work {id}\"\nstatus: {status}\nwork_type: feat\nspecs: []\ndepends_on: []\ncreated: 2026-10-07\n{follows}---\n\n# {id}: work\n\n## Description\n\nWork.\n\n## Acceptance Criteria\n\n{criteria}\n## Closeout\n\n{closeout}"
    )
}

fn record_path(id: &str) -> String {
    format!("project-management/tasks/{id}.md")
}

fn write(root: &Path, path: &str, content: &str) {
    let full = root.join(path);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, content).unwrap();
}

fn commit(root: &Path, message: &str) -> String {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-m", message]);
    git_out(root, &["rev-parse", "HEAD"])
}

/// An acceptance block for [`CRITERIA`] reviewed at `reviewed`.
fn block(reviewed: &str) -> String {
    format!(
        "```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: https://example.test/pr/1#review\n  criteria:\n    AC-1: verified | cargo test | 3 passed\n    AC-2: verified | journey ran\n  journey: verified | tests/journey.rs\n  not_verified: none\n  follow_ups: none: nothing deferred\n  verdict: approved\n```\n"
    )
}

/// Who the tasks of the fixture are: (id, epic, follows up).
const TASKS: [(&str, Option<&str>, Option<&str>); 4] = [
    // The completed standalone task the issue corrects.
    ("TSK-003", None, None),
    // A completed task of an epic.
    ("TSK-001", Some("EPC-001"), None),
    // A follow-up of TSK-003, still open.
    ("TSK-005", None, Some("TSK-003")),
    // An unrelated standalone task, still open.
    ("TSK-006", None, None),
];

/// `main` holds the epic and [`TASKS`]; TSK-003 and TSK-001 are each built
/// on their own task branch, completed with a block reviewed at their code
/// commit and landed by a merge, as a task pull request lands.
fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "user.name", "t"]);
    write(root, "project-management/epics/EPC-001.md", EPIC);
    for (id, epic, follows) in TASKS {
        write(
            root,
            &record_path(id),
            &task(id, epic, follows, "todo", CRITERIA, "Pending.\n"),
        );
    }
    write(
        root,
        ".codeflow/policy.json",
        "{\n  \"schema_version\": 1,\n  \"git\": {\"product_paths\": [\"src/**\"]}\n}\n",
    );
    write(root, "src/lib.rs", "pub fn base() {}\n");
    commit(root, "chore: plan the work");
    for (id, epic) in [("TSK-003", None), ("TSK-001", Some("EPC-001"))] {
        let branch = format!("task/{id}-work");
        git(root, &["switch", "-C", &branch, "main"]);
        write(
            root,
            &format!("src/{}.rs", id.to_lowercase()),
            "pub fn work() {}\n",
        );
        let reviewed = commit(root, "feat: do the work");
        write(
            root,
            &record_path(id),
            &task(id, epic, None, "complete", CRITERIA, &block(&reviewed)),
        );
        commit(root, "docs(records): record the acceptance");
        git(root, &["switch", "main"]);
        git(
            root,
            &[
                "merge",
                "--no-ff",
                "-m",
                &format!("Merge {branch}"),
                &branch,
            ],
        );
    }
    dir
}

/// Rewrite the criteria of the completed task `id` on the current branch,
/// keeping its status and acceptance block.
fn rewrite(root: &Path, id: &str, criteria: &str) {
    let path = root.join(record_path(id));
    let current = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, current.replace(CRITERIA, criteria)).unwrap();
}

/// On a branch cut from `main`, rewrite `id`'s criteria and commit.
fn correct(root: &Path, branch: &str, id: &str, criteria: &str) {
    git(root, &["switch", "-C", branch, "main"]);
    rewrite(root, id, criteria);
    commit(root, "docs(records): correct a criterion");
}

/// `codeflow ci` from `main` to `HEAD` as `branch`, with `task_line` in the
/// pull request body, or with no body (as the pre-push hook runs it) when
/// `task_line` is `None`.
fn ci_on(root: &Path, branch: &str, task_line: Option<&str>) -> (i32, String) {
    let mut args = vec![
        "ci".to_string(),
        "--base".into(),
        "main".into(),
        "--head".into(),
        "HEAD".into(),
        "--branch".into(),
        branch.into(),
    ];
    if let Some(line) = task_line {
        args.push("--pr-body".into());
        args.push(format!(
            "## Summary\nA correction.\n\n- one change\n\n{line}\n\n## Changes\n- one\n\n## Testing\n- test\n"
        ));
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
}

fn ci(root: &Path, branch: &str, task_line: &str) -> (i32, String) {
    ci_on(root, branch, Some(task_line))
}

fn assert_passes(result: &(i32, String), what: &str, needles: &[&str]) {
    assert_eq!(result.0, 0, "{what}: {}", result.1);
    for needle in needles {
        assert!(
            result.1.contains(needle),
            "{what}: missing {needle:?} in {}",
            result.1
        );
    }
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

const PLAN: &str = "plan/correct-tsk-003";
const DELTA: &str = "work.criteria_frozen: TSK-003 criteria correction (a completed standalone task, text only), delta: AC-1 changed: When run, the system shall work as Jane Doe asked. -> When run, the system shall work as the release owner asked.";
const FROZEN: &str = "TSK-003 changes its criteria on this branch";

/// AC-1, AC-6: the issue's reproduction. A `plan/` pull request naming the
/// completed standalone task replaces a name with a role; CI passes, prints
/// the delta with its reviewer remedy, keeps the task complete and finds
/// nothing to bind. Before ADR-0080 this range was refused as frozen.
#[test]
fn a_named_task_corrects_its_criteria_text() {
    let dir = repo();
    let root = dir.path();
    correct(root, PLAN, "TSK-003", CORRECTED);
    let result = ci(root, PLAN, "Task: TSK-003");
    assert_passes(
        &result,
        "a text correction naming the task",
        &[
            DELTA,
            "a human reviewer confirms that the substance of each changed criterion is unchanged",
            "TSK-003 is complete at the target; this range of planning records only corrects its records and starts no work",
        ],
    );
    assert!(!result.1.contains(FROZEN), "{}", result.1);
    assert!(
        !result.1.contains("work.acceptance_binding"),
        "{}",
        result.1
    );
    let record = std::fs::read_to_string(root.join(record_path("TSK-003"))).unwrap();
    assert!(record.contains("status: complete"), "{record}");
    // The pre-push run, with no body, reads the range as a planning
    // amendment and lists the standalone task.
    assert_passes(
        &ci_on(root, PLAN, None),
        "the same correction pushed",
        &["no epic: TSK-003 criteria delta: AC-1 changed"],
    );
}

/// AC-2: a follow-up whose `follow_up_of` is the completed task may carry
/// the correction, on a planning branch and on its own task branch.
#[test]
fn a_follow_up_corrects_the_task_it_follows() {
    let dir = repo();
    let root = dir.path();
    correct(root, PLAN, "TSK-003", CORRECTED);
    assert_passes(
        &ci(root, PLAN, "Task: TSK-005"),
        "a correction naming the follow-up",
        &[DELTA],
    );
    let own = "task/TSK-005-correct";
    correct(root, own, "TSK-003", CORRECTED);
    let result = ci(root, own, "Task: TSK-005");
    assert!(result.1.contains(DELTA), "{}", result.1);
    assert!(!result.1.contains(FROZEN), "{}", result.1);
    let pushed = ci_on(root, own, None);
    assert!(pushed.1.contains(DELTA), "{}", pushed.1);
    assert!(!pushed.1.contains(FROZEN), "{}", pushed.1);
}

/// AC-3: the criteria set and every tag stay frozen. An added, removed,
/// renumbered or reordered criterion, a changed `(journey)`,
/// `(after release)` or `(serves ...)` tag, or a list item the parser does
/// not read as a criterion, is refused with the frozen wording and the
/// reason.
#[test]
fn the_criteria_set_stays_frozen() {
    let dir = repo();
    let root = dir.path();
    let set = "this changes the criteria set itself";
    let tags = "this changes the tags of";
    let cases: [(&str, String, &str); 8] = [
        (
            "an added criterion",
            format!("{CORRECTED}- AC-3 When asked, the system shall explain.\n"),
            set,
        ),
        (
            "a removed criterion",
            "- AC-1 When run, the system shall work as the release owner asked.\n".to_string(),
            set,
        ),
        (
            "a renumbered criterion",
            CORRECTED.replace("- AC-2 ", "- AC-3 "),
            set,
        ),
        (
            "reordered criteria",
            "- AC-2 (journey) On a fresh project, the command shall succeed.\n- AC-1 When run, the system shall work as the release owner asked.\n".to_string(),
            set,
        ),
        (
            "a dropped journey tag",
            CORRECTED.replace("(journey) ", ""),
            tags,
        ),
        (
            "an added after-release tag",
            CORRECTED.replace("owner asked.", "owner asked (after release)."),
            tags,
        ),
        (
            "an added serves tag",
            CORRECTED.replace("owner asked.", "owner asked (serves EPC-001 AC-1)."),
            tags,
        ),
        (
            "a list item that is no criterion",
            format!("{CORRECTED}- Reviewed by the release owner.\n"),
            "this changes the structure of its criteria section",
        ),
    ];
    for (what, criteria, reason) in cases {
        correct(root, PLAN, "TSK-003", &criteria);
        assert_blocks(
            &ci(root, PLAN, "Task: TSK-003"),
            what,
            &["work.criteria_frozen", FROZEN, reason],
        );
    }
}

/// AC-4: the route needs a range of planning records only, a `Task:` line
/// naming the task or its follow-up, and a standalone task; an epic task
/// keeps the planning amendment that names its epic (ADR-0078).
#[test]
fn the_route_needs_records_only_a_named_task_and_no_epic() {
    let dir = repo();
    let root = dir.path();

    // A non-planning path in the range.
    correct(root, PLAN, "TSK-003", CORRECTED);
    write(root, "docs/notes.md", "A note.\n");
    commit(root, "docs: add a note");
    assert_blocks(
        &ci(root, PLAN, "Task: TSK-003"),
        "a doc outside the planning records",
        &["work.criteria_frozen", FROZEN],
    );

    // An unrelated task named.
    correct(root, PLAN, "TSK-003", CORRECTED);
    assert_blocks(
        &ci(root, PLAN, "Task: TSK-006"),
        "an unrelated task",
        &["work.criteria_frozen", FROZEN],
    );

    // An epic task, named directly.
    correct(root, PLAN, "TSK-001", CORRECTED);
    assert_blocks(
        &ci(root, PLAN, "Task: TSK-001"),
        "an epic task",
        &[
            "work.criteria_frozen",
            "TSK-001 changes its criteria on this branch",
        ],
    );
    // Its route is the planning amendment that names its epic.
    assert_passes(
        &ci(root, PLAN, "Task: EPC-001"),
        "the epic's planning amendment",
        &["EPC-001: TSK-001 criteria delta: AC-1 changed"],
    );
}

/// AC-5: on the corrected task's own `task/` branch the range is its reopen
/// route and keeps the criteria frozen, as before; a range that reopens the
/// task keeps them frozen too (R-119).
#[test]
fn the_own_branch_and_a_reopen_stay_frozen() {
    let dir = repo();
    let root = dir.path();
    let own = "task/TSK-003-correct";
    correct(root, own, "TSK-003", CORRECTED);
    assert_blocks(
        &ci(root, own, "Task: TSK-003"),
        "the task's own branch",
        &["work.criteria_frozen", FROZEN],
    );
    assert_blocks(
        &ci_on(root, own, None),
        "the task's own branch on push",
        &["work.criteria_frozen", FROZEN],
    );

    git(root, &["switch", "-C", PLAN, "main"]);
    let path = root.join(record_path("TSK-003"));
    let current = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        current
            .replace(CRITERIA, CORRECTED)
            .replace("status: complete", "status: todo"),
    )
    .unwrap();
    commit(root, "docs(records): reopen and correct");
    assert_blocks(
        &ci(root, PLAN, "Task: TSK-003"),
        "a reopen",
        &["work.criteria_frozen", "a reopened task keeps its criteria"],
    );
}
