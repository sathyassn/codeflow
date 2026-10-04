//! One planning amendment names several epics and carries its instruction
//! text (TSK-229, ADR-0078, SPC-013 R-52, R-70): `Task: EPC-001, EPC-002` on
//! a planning range classifies as one amendment of both epics, which may
//! also carry `docs/` and `AGENTS.md` outside its managed block. CI reports
//! each change per epic, refuses a change to an epic the line does not
//! name, and keeps refusing product, instruction and enforcement paths. The
//! pre-push run (no body) reaches the same verdicts, and a line takes the
//! amendment by merging the target. Each test runs the Cargo-built binary in
//! a tempdir repository with durable work tracking on.

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

fn epic(id: &str) -> String {
    format!(
        "---\nid: {id}\ntitle: \"outcome {id}\"\nstatus: planning\nwork_type: feat\nspecs: []\ncreated: 2026-10-03\n---\n\n# {id}: outcome\n\n## Summary\n\nAn outcome.\n\n## Acceptance Criteria\n\n- AC-1 (journey) On a fresh project, an adopter shall finish the flow.\n"
    )
}

const CRITERIA: &str = "- AC-1 When run, the system shall work.\n- AC-2 (journey) On a fresh project, the command shall succeed.\n";
const LOOSER: &str = "- AC-1 When run, the system shall mostly work.\n- AC-2 (journey) On a fresh project, the command shall succeed.\n";

/// A task record of `epic` (or standalone when `None`) bound for `target`.
fn task(id: &str, epic: Option<&str>, target: &str, status: &str, criteria: &str) -> String {
    let parent = match epic {
        Some(epic) => format!("epic_id: {epic}\nstandalone_reason: null"),
        None => "epic_id: null\nstandalone_reason: \"one outcome\"".to_string(),
    };
    format!(
        "---\nid: {id}\n{parent}\nintegration_target: {target}\ntitle: \"work {id}\"\nstatus: {status}\nwork_type: feat\nspecs: []\ndepends_on: []\ncreated: 2026-10-03\n---\n\n# {id}: work\n\n## Description\n\nWork.\n\n## Acceptance Criteria\n\n{criteria}\n## Closeout\n\nPending.\n"
    )
}

const MANAGED: &str =
    "<!-- codeflow:managed:begin scaffold=3.1.0 -->\nmanaged rules\n<!-- codeflow:managed:end -->";

fn agents(project: &str) -> String {
    format!("# p\n\n{MANAGED}\n\n## Project\n\n{project}\n")
}

fn record_path(id: &str) -> String {
    format!("project-management/tasks/{id}.md")
}

/// The three tasks every test starts from: one per epic and a standalone.
const TASKS: [(&str, Option<&str>); 3] = [
    ("TSK-001", Some("EPC-001")),
    ("TSK-002", Some("EPC-002")),
    ("TSK-003", None),
];

/// `main` holds EPC-001 and EPC-002, the three [`TASKS`] bound for `main`,
/// an `AGENTS.md` with a managed block, and a policy naming `src/**`.
fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "user.name", "t"]);
    for id in ["EPC-001", "EPC-002"] {
        write(
            root,
            &format!("project-management/epics/{id}.md"),
            &epic(id),
        );
    }
    for (id, parent) in TASKS {
        write(
            root,
            &record_path(id),
            &task(id, parent, "main", "todo", CRITERIA),
        );
    }
    write(
        root,
        ".codeflow/policy.json",
        "{\n  \"schema_version\": 1,\n  \"git\": {\"product_paths\": [\"src/**\"]}\n}\n",
    );
    write(root, "AGENTS.md", &agents("mine"));
    write(root, "src/lib.rs", "pub fn base() {}\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "chore: plan the work"]);
    dir
}

fn write(root: &Path, path: &str, content: &str) {
    let full = root.join(path);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, content).unwrap();
}

fn commit(root: &Path, message: &str) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-m", message]);
}

/// Loosen AC-1 of `id` on the current branch.
fn loosen(root: &Path, id: &str, epic: Option<&str>, target: &str) {
    write(
        root,
        &record_path(id),
        &task(id, epic, target, "todo", LOOSER),
    );
}

/// `codeflow ci` from `base` to `HEAD` as `branch`, with `task_line` in the
/// pull request body, or with no body (as the pre-push hook runs it) when
/// `task_line` is `None`.
fn ci_on(root: &Path, base: &str, branch: &str, task_line: Option<&str>) -> (i32, String) {
    let mut args = vec![
        "ci".to_string(),
        "--base".into(),
        base.into(),
        "--head".into(),
        "HEAD".into(),
        "--branch".into(),
        branch.into(),
    ];
    if let Some(line) = task_line {
        args.push("--pr-body".into());
        args.push(format!(
            "## Summary\nA plan change.\n\n- one change\n\n{line}\n\n## Changes\n- one\n\n## Testing\n- test\n"
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
    ci_on(root, "main", branch, Some(task_line))
}

fn ci_push(root: &Path, branch: &str) -> (i32, String) {
    ci_on(root, "main", branch, None)
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

const BOTH: &str = "Task: EPC-001, EPC-002";
const PLAN: &str = "plan/two-epics";

/// The amendment of AC-1: on a `plan/` branch, both epics' tasks loosen
/// AC-1.
fn two_epic_amendment(root: &Path) {
    git(root, &["switch", "-C", PLAN, "main"]);
    loosen(root, "TSK-001", Some("EPC-001"), "main");
    loosen(root, "TSK-002", Some("EPC-002"), "main");
    commit(root, "docs(records): loosen AC-1 in both epics");
}

/// AC-1, AC-3: one amendment changes criteria in two epics; CI names both
/// epics in the class line and prints a delta note per task, and the push
/// with no body passes too.
#[test]
fn one_amendment_changes_criteria_in_two_epics() {
    let dir = repo();
    let root = dir.path();
    two_epic_amendment(root);
    let result = ci(root, PLAN, BOTH);
    assert_passes(
        &result,
        "a two-epic amendment",
        &[
            "pull request class: planning-only amendment of EPC-001, EPC-002",
            "work.planning_amendment: EPC-001: TSK-001 criteria delta: AC-1 changed: When run, the system shall work. -> When run, the system shall mostly work.",
            "work.planning_amendment: EPC-002: TSK-002 criteria delta: AC-1 changed",
        ],
    );
    assert!(!result.1.contains("work.criteria_frozen"), "{}", result.1);
    assert_passes(
        &ci_push(root, PLAN),
        "the same amendment pushed",
        &[
            "EPC-001: TSK-001 criteria delta",
            "EPC-002: TSK-002 criteria delta",
        ],
    );
}

/// AC-2, AC-3: the amendment also carries `AGENTS.md` below its managed
/// block, a plan and a doc, in CI and on push; one byte inside the managed
/// block makes it unclassified, and the refusal names the block.
#[test]
fn an_amendment_carries_its_instruction_text_but_not_the_managed_block() {
    let dir = repo();
    let root = dir.path();
    two_epic_amendment(root);
    write(root, "AGENTS.md", &agents("mine, with the amended plan"));
    write(root, "docs/plan/plan.md", "The plan.\n");
    write(root, "docs/reading.md", "Read this.\n");
    commit(root, "docs: carry the instruction text");
    let carried = ci(root, PLAN, BOTH);
    assert_passes(
        &carried,
        "instruction text in the amendment",
        &[
            "planning-only amendment of EPC-001, EPC-002",
            "instruction and doc files: AGENTS.md (outside its managed block), docs/plan/plan.md, docs/reading.md",
        ],
    );
    assert_passes(&ci_push(root, PLAN), "instruction text on push", &[]);

    let inside = agents("mine").replace("managed rules", "managed rules!");
    write(root, "AGENTS.md", &inside);
    commit(root, "docs: edit the managed block");
    assert_blocks(
        &ci(root, PLAN, BOTH),
        "a byte inside the managed block",
        &[
            "work.classification",
            "managed block of AGENTS.md (codeflow:managed:begin to codeflow:managed:end)",
        ],
    );
    assert_blocks(
        &ci_push(root, PLAN),
        "a byte inside the managed block on push",
        &["work.criteria_frozen", "TSK-001 changes its criteria"],
    );
}

/// AC-4: product code, `CLAUDE.md`, harness settings and policy keep a
/// planning amendment out of the planning class, as before.
#[test]
fn a_planning_amendment_still_refuses_product_and_enforcement_paths() {
    let dir = repo();
    let root = dir.path();
    let settings = [".claude", "settings.json"].join("/");
    let policy = [".codeflow", "policy.json"].join("/");
    for (path, content) in [
        ("src/lib.rs", "pub fn planned() {}\n".to_string()),
        ("CLAUDE.md", "@AGENTS.md\n".to_string()),
        (settings.as_str(), "{}\n".to_string()),
        (
            policy.as_str(),
            "{\n  \"schema_version\": 1,\n  \"git\": {\"product_paths\": [\"src/**\"]}\n}\n\n"
                .to_string(),
        ),
    ] {
        two_epic_amendment(root);
        write(root, path, &content);
        commit(root, "docs: plan with a product path");
        assert_blocks(
            &ci(root, PLAN, BOTH),
            path,
            &[
                "work.classification",
                &format!("touches a product path: {path}"),
            ],
        );
    }
}

/// AC-5: a task PR still prints only its own delta and keeps every other
/// task's criteria frozen, now naming the planning amendment as the route;
/// a reopened task keeps its criteria inside an amendment too.
#[test]
fn a_task_pull_request_keeps_other_criteria_frozen() {
    let dir = repo();
    let root = dir.path();
    let branch = "task/TSK-001-work";
    git(root, &["switch", "-C", branch, "main"]);
    write(root, "src/lib.rs", "pub fn work() {}\n");
    loosen(root, "TSK-001", Some("EPC-001"), "main");
    commit(root, "feat: work and loosen AC-1");
    assert_passes(
        &ci(root, branch, "Task: TSK-001"),
        "own criteria",
        &["TSK-001 criteria delta: AC-1 changed"],
    );
    loosen(root, "TSK-002", Some("EPC-002"), "main");
    commit(root, "docs: loosen another task");
    assert_blocks(
        &ci(root, branch, "Task: TSK-001"),
        "another task's criteria",
        &[
            "work.criteria_frozen",
            "TSK-002 changes its criteria on this branch; another task's criteria change by its own PR or by a planning amendment that names its epic",
        ],
    );

    // A complete task reopened in an amendment keeps its criteria.
    git(root, &["switch", "-C", "main", "main"]);
    let reviewed = git_out(root, &["rev-parse", "HEAD"]);
    write(
        root,
        &record_path("TSK-002"),
        &completed("TSK-002", CRITERIA, &reviewed),
    );
    commit(root, "docs: complete TSK-002");
    git(root, &["switch", "-C", PLAN, "main"]);
    loosen(root, "TSK-002", Some("EPC-002"), "main");
    commit(root, "docs(records): reopen and loosen TSK-002");
    assert_blocks(
        &ci(root, PLAN, BOTH),
        "a reopened task in an amendment",
        &["work.criteria_frozen", "reopened task keeps its criteria"],
    );
    // Kept complete, with its valid block, a criteria change is admitted
    // and flagged for the reviewer; a description change is not flagged.
    git(root, &["switch", "-C", PLAN, "main"]);
    let described =
        completed("TSK-002", CRITERIA, &reviewed).replace("Work.\n", "Work, described.\n");
    write(root, &record_path("TSK-002"), &described);
    commit(root, "docs(records): describe TSK-002");
    let described_run = ci(root, PLAN, BOTH);
    assert_passes(&described_run, "a complete task's description", &[]);
    assert!(
        !described_run
            .1
            .contains("reviewed against the earlier criteria"),
        "{}",
        described_run.1
    );
    write(
        root,
        &record_path("TSK-002"),
        &completed("TSK-002", LOOSER, &reviewed).replace("Work.\n", "Work, described.\n"),
    );
    commit(root, "docs(records): loosen complete TSK-002");
    assert_passes(
        &ci(root, PLAN, BOTH),
        "a complete task's criteria",
        &[
            "EPC-002: TSK-002 criteria delta: AC-1 changed",
            "EPC-002: TSK-002 is complete, and its acceptance block was reviewed against the earlier criteria",
        ],
    );
}

/// TSK-002 complete with `criteria` and a valid acceptance block reviewed
/// at `reviewed`.
fn completed(id: &str, criteria: &str, reviewed: &str) -> String {
    task(id, Some("EPC-002"), "main", "complete", criteria).replace(
        "Pending.\n",
        &format!(
            "```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: https://example.test/pr/1#review\n  criteria:\n    AC-1: verified | test ran\n    AC-2: verified | journey ran\n  journey: verified | fixture\n  not_verified: none\n  follow_ups: none: done\n  verdict: approved\n```\n"
        ),
    )
}

/// AC-7: an amendment that changes a record of an epic its `Task:` line does
/// not name is refused and names the record and its epic; a standalone
/// task's record is listed, never refused.
#[test]
fn an_amendment_names_every_epic_it_changes() {
    let dir = repo();
    let root = dir.path();
    two_epic_amendment(root);
    assert_blocks(
        &ci(root, PLAN, "Task: EPC-001"),
        "an unnamed epic",
        &[
            "work.planning_amendment",
            "TSK-002 belongs to EPC-002, which the `Task:` line does not name",
        ],
    );
    git(root, &["switch", "-C", "plan/standalone", "main"]);
    loosen(root, "TSK-001", Some("EPC-001"), "main");
    loosen(root, "TSK-003", None, "main");
    commit(root, "docs(records): amend EPC-001 and a standalone task");
    assert_passes(
        &ci(root, "plan/standalone", "Task: EPC-001"),
        "a standalone record is listed",
        &[
            "EPC-001: TSK-001 criteria delta",
            "no epic: TSK-003 criteria delta",
        ],
    );
    // An epic it names must exist and must not be cancelled before.
    assert_blocks(
        &ci(root, "plan/standalone", "Task: EPC-001, EPC-009"),
        "an epic with no record",
        &["work.classification", "EPC-009, which has no epic record"],
    );
}

/// AC-8: a task id never takes a list, and a list of epics is one class.
#[test]
fn a_task_line_list_takes_only_distinct_epics() {
    let dir = repo();
    let root = dir.path();
    two_epic_amendment(root);
    for line in [
        "Task: TSK-001, TSK-002",
        "Task: EPC-001, EPC-001",
        "Task: EPC-001, TSK-002",
    ] {
        assert_blocks(
            &ci(root, PLAN, line),
            line,
            &["work.classification", "a task id never takes a list"],
        );
    }
}

const LINE: &str = "integration/EPC-001-line";

/// Run `codeflow` with `args`: whether it succeeded, and its output.
fn run(root: &Path, args: &[&str]) -> (bool, String) {
    let out = codeflow().args(args).current_dir(root).output().unwrap();
    (
        out.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

/// AC-6 (journey): the amendment lands on `main`; a line task added by it
/// does not start on the line until the line merges `main`, then starts and
/// reads the amended criteria. Before landing, CI flags a record that
/// changed on the line since it last merged `main`.
#[test]
fn a_line_takes_the_amendment_by_merging_main() {
    let dir = repo();
    let root = dir.path();
    write(
        root,
        &record_path("TSK-001"),
        &task("TSK-001", Some("EPC-001"), LINE, "todo", CRITERIA),
    );
    commit(root, "docs(records): bind TSK-001 to the line");
    git(root, &["branch", LINE, "main"]);
    // The line edits TSK-001's record after it was cut.
    git(root, &["switch", LINE]);
    let on_line = task("TSK-001", Some("EPC-001"), LINE, "todo", CRITERIA)
        .replace("Work.\n", "Work, as the line found it.\n");
    write(root, &record_path("TSK-001"), &on_line);
    commit(root, "docs(records): describe TSK-001 on the line");

    // The amendment on main: TSK-001's AC-1 loosens, TSK-004 joins EPC-001
    // on the line, and TSK-002 in EPC-002 loosens.
    git(root, &["switch", "-C", PLAN, "main"]);
    loosen(root, "TSK-001", Some("EPC-001"), LINE);
    loosen(root, "TSK-002", Some("EPC-002"), "main");
    write(
        root,
        &record_path("TSK-004"),
        &task("TSK-004", Some("EPC-001"), LINE, "todo", CRITERIA),
    );
    commit(root, "docs(records): amend EPC-001 and EPC-002");
    // A maintainer registers the ids, so the added record is not refused.
    assert!(run(root, &["ids", "seed"]).0, "ids seed");
    assert_passes(
        &ci(root, PLAN, BOTH),
        "the amendment before it lands",
        &[
            "EPC-001: TSK-004 added (task record, todo)",
            &format!("EPC-001: TSK-001 is newer on refs/heads/{LINE} than on the target"),
        ],
    );
    git(root, &["switch", "main"]);
    git(
        root,
        &["merge", "--no-ff", "-m", "merge: the amendment", PLAN],
    );

    // On the line, before it merges main, TSK-004 cannot start.
    git(root, &["switch", "-C", "task/TSK-004-work", LINE]);
    let (started, said) = run(root, &["work", "start", "TSK-004"]);
    assert!(!started && said.contains("TSK-004"), "{said}");
    write(root, "src/lib.rs", "pub fn line_work() {}\n");
    commit(root, "feat: build TSK-004");
    assert_blocks(
        &ci_on(root, LINE, "task/TSK-004-work", Some("Task: TSK-004")),
        "the task pull request names the merge",
        &[
            "TSK-004 is not present at the merge-base",
            "merge the epic's planning change into",
        ],
    );

    // The line merges main; the task starts and reads the amended criteria.
    git(root, &["switch", LINE]);
    git(
        root,
        &[
            "merge",
            "--no-ff",
            "-m",
            "merge: take the amendment",
            "main",
        ],
    );
    git(root, &["switch", "-C", "task/TSK-004-work", LINE]);
    let (started, said) = run(root, &["work", "start", "TSK-004"]);
    assert!(started, "{said}");
    let anchor = said
        .split_whitespace()
        .skip_while(|word| *word != "at")
        .nth(1)
        .unwrap_or_default()
        .to_string();
    let amended = git_out(
        root,
        &["show", &format!("{anchor}:{}", record_path("TSK-001"))],
    );
    assert!(amended.contains("shall mostly work"), "{amended}");
    assert!(amended.contains("as the line found it"), "{amended}");
}

/// Review round 1 (AC-2, AC-4): an instruction file under a planning
/// folder, a path the target's policy (not the head's) names as product,
/// and a document reached through a symbolic link all keep a range out of
/// the planning class, with a body and on push.
#[test]
fn a_planning_amendment_refuses_aliases_and_reads_the_targets_policy() {
    let dir = repo();
    let root = dir.path();
    // An instruction file inside a planning folder.
    two_epic_amendment(root);
    write(root, "docs/plan/AGENTS.md", "Agents, read this.\n");
    commit(root, "docs: hide instructions in the plan");
    assert_blocks(
        &ci(root, PLAN, BOTH),
        "an instruction file under docs/plan",
        &["touches a product path: docs/plan/AGENTS.md"],
    );

    // The target marks docs/tool.md as product after the branch was cut;
    // the branch keeps the older policy.
    git(root, &["switch", "-C", "plan/old-policy", "main"]);
    write(root, "docs/tool.md", "A tool page.\n");
    commit(root, "docs: change a page");
    git(root, &["switch", "main"]);
    write(
        root,
        ".codeflow/policy.json",
        "{\n  \"schema_version\": 1,\n  \"git\": {\"product_paths\": [\"src/**\", \"docs/tool.md\"]}\n}\n",
    );
    commit(root, "chore: name the tool page as product");
    git(root, &["switch", "plan/old-policy"]);
    assert_blocks(
        &ci(root, "plan/old-policy", "Task: EPC-001"),
        "a path the target's policy names as product",
        &["touches a product path: docs/tool.md"],
    );

    // A document a symbolic link carries into the instructions.
    #[cfg(unix)]
    {
        git(root, &["switch", "main"]);
        write(root, "docs/instructions.md", &agents("linked"));
        std::os::unix::fs::symlink("docs/instructions.md", root.join("CLAUDE.md")).unwrap();
        commit(root, "docs: link the harness instructions");
        git(root, &["switch", "-C", "plan/through-a-link", "main"]);
        write(
            root,
            "docs/instructions.md",
            &agents("linked").replace("managed rules", "other rules"),
        );
        commit(root, "docs: change the linked instructions");
        assert_blocks(
            &ci(root, "plan/through-a-link", "Task: EPC-001"),
            "a document behind a link",
            &["holds the symbolic link CLAUDE.md"],
        );
        // On push the range is not an amendment, so no criteria may move.
        loosen(root, "TSK-001", Some("EPC-001"), "main");
        commit(root, "docs(records): loosen AC-1 behind the link");
        assert_blocks(
            &ci_push(root, "plan/through-a-link"),
            "a document behind a link on push",
            &["work.criteria_frozen", "TSK-001 changes its criteria"],
        );

        // Review round 2: a link whose target differs only in letter case,
        // as a case-insensitive checkout resolves it, and an absolute link
        // into the checkout reach the document as well.
        git(root, &["switch", "main"]);
        write(root, "docs/guide.md", &agents("guide"));
        write(root, "docs/absolute.md", &agents("absolute"));
        std::os::unix::fs::symlink("DOCS/Guide.md", root.join("GEMINI.md")).unwrap();
        std::os::unix::fs::symlink(root.join("docs/absolute.md"), root.join("absolute-link"))
            .unwrap();
        commit(root, "docs: link two more instruction files");
        for (doc, link) in [
            ("docs/guide.md", "GEMINI.md"),
            ("docs/absolute.md", "absolute-link"),
        ] {
            let branch = format!("plan/through-{link}");
            git(root, &["switch", "-C", &branch, "main"]);
            let text = std::fs::read_to_string(root.join(doc)).unwrap();
            write(root, doc, &text.replace("managed rules", "other rules"));
            commit(root, "docs: change the linked instructions");
            assert_blocks(
                &ci(root, &branch, "Task: EPC-001"),
                doc,
                &["holds the symbolic link"],
            );
            loosen(root, "TSK-001", Some("EPC-001"), "main");
            commit(root, "docs(records): loosen AC-1 behind the link");
            assert_blocks(
                &ci_push(root, &branch),
                doc,
                &["work.criteria_frozen", "TSK-001 changes its criteria"],
            );
        }
    }
}

/// Review rounds 3 and 4 (AC-2, AC-3, AC-4): a range that carries a doc or
/// `AGENTS.md` needs trees with no symbolic link or submodule, so no chain
/// of links, case alias or submodule can present that text elsewhere; a
/// range of records and plans only keeps its 3.0.0 behaviour, and a link
/// or submodule entry is never changed by an amendment.
#[cfg(unix)]
#[test]
fn links_and_submodules_keep_text_out_of_an_amendment() {
    let dir = repo();
    let root = dir.path();
    std::os::unix::fs::symlink("AGENTS.md", root.join("CLAUDE.md")).unwrap();
    commit(root, "docs: link the harness instructions to AGENTS.md");
    // Records and plans only: admitted as in 3.0.0.
    two_epic_amendment(root);
    assert_passes(&ci(root, PLAN, BOTH), "records beside a link", &[]);
    assert_passes(&ci_push(root, PLAN), "records beside a link on push", &[]);
    // The project section of AGENTS.md, or any doc: refused, naming the link.
    for (path, content) in [
        ("AGENTS.md", agents("mine, with the amended plan")),
        ("docs/reading.md", "Read this.\n".to_string()),
    ] {
        git(root, &["switch", "-C", "plan/text", PLAN]);
        write(root, path, &content);
        commit(root, "docs: carry text beside a link");
        assert_blocks(
            &ci(root, "plan/text", BOTH),
            path,
            &[&format!(
                "carries {path}, and the tree at the target holds the symbolic link CLAUDE.md"
            )],
        );
        assert_blocks(&ci_push(root, "plan/text"), path, &["work.criteria_frozen"]);
    }

    // A submodule on the target: a doc beside it is refused, and the
    // submodule entry itself is never changed by an amendment.
    git(root, &["switch", "main"]);
    std::fs::remove_file(root.join("CLAUDE.md")).unwrap();
    commit(root, "docs: drop the link");
    let first = git_out(root, &["rev-parse", "HEAD"]);
    git(
        root,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{first},docs/reference"),
        ],
    );
    git(
        root,
        &["commit", "-q", "-m", "docs: add a reference submodule"],
    );
    git(root, &["switch", "-C", "plan/beside-a-submodule", "main"]);
    loosen(root, "TSK-001", Some("EPC-001"), "main");
    write(root, "docs/reading.md", "Read this.\n");
    git(root, &["add", "project-management", "docs/reading.md"]);
    git(
        root,
        &["commit", "-q", "-m", "docs: amend beside a submodule"],
    );
    assert_blocks(
        &ci(root, "plan/beside-a-submodule", "Task: EPC-001"),
        "a doc beside a submodule",
        &["holds the submodule docs/reference"],
    );
    git(root, &["switch", "-C", "plan/moves-a-submodule", "main"]);
    loosen(root, "TSK-001", Some("EPC-001"), "main");
    git(root, &["add", "project-management"]);
    let second = git_out(root, &["rev-parse", "HEAD~2"]);
    git(
        root,
        &[
            "update-index",
            "--cacheinfo",
            &format!("160000,{second},docs/reference"),
        ],
    );
    git(root, &["commit", "-q", "-m", "docs: move the submodule"]);
    assert_blocks(
        &ci(root, "plan/moves-a-submodule", "Task: EPC-001"),
        "a moved submodule",
        &["changes the submodule docs/reference"],
    );
    assert_blocks(
        &ci_push(root, "plan/moves-a-submodule"),
        "a moved submodule on push",
        &["work.criteria_frozen"],
    );
}

/// Stage a symbolic link entry named by raw `name` bytes and pointing at
/// `target`, through Git's own plumbing, since a file system may refuse
/// the name.
#[cfg(unix)]
fn stage_link(root: &Path, name: &[u8], target: &str) {
    use std::os::unix::ffi::OsStringExt;
    write(root, "link.tmp", target);
    let blob = git_out(root, &["hash-object", "-w", "link.tmp"]);
    std::fs::remove_file(root.join("link.tmp")).unwrap();
    let mut info = format!("120000,{},", blob.trim()).into_bytes();
    info.extend_from_slice(name);
    let status = Command::new("git")
        .args(["update-index", "--add", "--cacheinfo"])
        .arg(std::ffi::OsString::from_vec(info))
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .unwrap();
    assert!(status.success());
}

/// Review round 5 (AC-3, AC-4): entries are judged from Git's own diff and
/// tree modes, so a name that is not UTF-8 hides no link, and a changed
/// entry with such a name is refused, with a body and on push.
#[cfg(unix)]
#[test]
fn entries_are_judged_whatever_their_names() {
    let record = record_path("TSK-001");
    // A link on the target whose name is not UTF-8, beside an amended doc.
    let dir = repo();
    let root = dir.path();
    write(root, "docs/runtime.md", "Runtime.\n");
    git(root, &["add", "docs/runtime.md"]);
    stage_link(root, b"src/\xFF", "../docs/runtime.md");
    git(root, &["commit", "-q", "-m", "docs: link the runtime page"]);
    git(root, &["switch", "-C", "plan/odd-tree", "main"]);
    loosen(root, "TSK-001", Some("EPC-001"), "main");
    write(root, "docs/runtime.md", "Other runtime.\n");
    git(root, &["add", &record, "docs/runtime.md"]);
    git(
        root,
        &["commit", "-q", "-m", "docs: amend beside an odd link"],
    );
    assert_blocks(
        &ci(root, "plan/odd-tree", "Task: EPC-001"),
        "a doc beside a link with an odd name",
        &["holds the symbolic link src/"],
    );
    assert_blocks(
        &ci_push(root, "plan/odd-tree"),
        "a doc beside a link with an odd name on push",
        &["work.criteria_frozen"],
    );

    // A changed link entry whose name is not UTF-8, in a records range.
    let dir = repo();
    let root = dir.path();
    stage_link(root, b"project-management/ref\xFF", "one");
    git(root, &["commit", "-q", "-m", "docs: add an odd link"]);
    git(root, &["switch", "-C", "plan/odd-entry", "main"]);
    loosen(root, "TSK-001", Some("EPC-001"), "main");
    git(root, &["add", &record]);
    stage_link(root, b"project-management/ref\xFF", "two");
    git(root, &["commit", "-q", "-m", "docs: move the odd link"]);
    assert_blocks(
        &ci(root, "plan/odd-entry", "Task: EPC-001"),
        "a changed entry with an odd name",
        &["whose name is not plain UTF-8 or holds a backslash"],
    );
    assert_blocks(
        &ci_push(root, "plan/odd-entry"),
        "a changed entry with an odd name on push",
        &["work.criteria_frozen"],
    );
}

/// TSK-234 (design D3): a task record the target deleted after its
/// completion landed is judged against its newest judged version when an
/// amendment adds it again. That version names its epic, so re-adding it
/// as a standalone task still needs that epic named. A re-added complete
/// record would be a new completion, which the binding judges, so it is
/// added back as planned with its criteria unchanged.
#[test]
fn an_amendment_that_readds_a_deleted_landed_record_keeps_its_epic() {
    let dir = repo();
    let root = dir.path();
    let reviewed = git_out(root, &["rev-parse", "HEAD"]);
    write(
        root,
        &record_path("TSK-002"),
        &completed("TSK-002", CRITERIA, &reviewed),
    );
    commit(root, "docs: complete TSK-002");
    std::fs::remove_file(root.join(record_path("TSK-002"))).unwrap();
    commit(root, "docs: remove the TSK-002 record");
    git(root, &["switch", "-c", PLAN]);
    write(
        root,
        &record_path("TSK-002"),
        &task("TSK-002", None, "main", "todo", CRITERIA),
    );
    commit(root, "docs(records): add TSK-002 again, standalone");
    // A maintainer registers the ids, so the added record is not refused.
    assert!(run(root, &["ids", "seed"]).0, "ids seed");
    assert_blocks(
        &ci(root, PLAN, "Task: EPC-001"),
        "a deleted landed record added again",
        &["TSK-002 belongs to EPC-002", "work.planning_amendment"],
    );
    assert_passes(
        &ci(root, PLAN, BOTH),
        "a deleted landed record added again, its epic named",
        &[
            "EPC-002: TSK-002 is added again",
            "EPC-002: TSK-002 moves from epic EPC-002 to none",
        ],
    );
}

/// TSK-234 (SPC-013 R-119): an amendment may change the criteria of a task
/// the target reopened after its completion landed; CI flags it as a
/// landed task, so the reviewer confirms the criteria its next completion
/// is held to.
#[test]
fn an_amendment_flags_a_landed_task_reopened_on_the_target() {
    let dir = repo();
    let root = dir.path();
    let reviewed = git_out(root, &["rev-parse", "HEAD"]);
    write(
        root,
        &record_path("TSK-002"),
        &completed("TSK-002", CRITERIA, &reviewed),
    );
    commit(root, "docs: complete TSK-002");
    write(
        root,
        &record_path("TSK-002"),
        &task("TSK-002", Some("EPC-002"), "main", "todo", CRITERIA),
    );
    commit(root, "docs: reopen TSK-002 on the target");
    git(root, &["switch", "-c", PLAN]);
    write(
        root,
        &record_path("TSK-002"),
        &task("TSK-002", Some("EPC-002"), "main", "todo", LOOSER),
    );
    commit(root, "docs(records): loosen TSK-002");
    assert_passes(
        &ci(root, PLAN, BOTH),
        "an amendment to a landed task reopened on the target",
        &[
            "EPC-002: TSK-002 criteria delta: AC-1 changed",
            "EPC-002: TSK-002 landed a completion earlier",
        ],
    );
}

/// TSK-234 review round 5: an amendment that restores a deleted landed
/// record and then reopens it with changed criteria reopens a landed task,
/// which the reopen freeze refuses whatever the range's class.
#[test]
fn an_amendment_cannot_reopen_a_deleted_landed_task_with_new_criteria() {
    let dir = repo();
    let root = dir.path();
    let reviewed = git_out(root, &["rev-parse", "HEAD"]);
    let original = completed("TSK-002", CRITERIA, &reviewed);
    write(root, &record_path("TSK-002"), &original);
    commit(root, "docs: complete TSK-002");
    std::fs::remove_file(root.join(record_path("TSK-002"))).unwrap();
    commit(root, "docs: remove the TSK-002 record");
    git(root, &["switch", "-c", PLAN]);
    write(root, &record_path("TSK-002"), &original);
    commit(root, "docs: restore the completed record");
    let reopened = original
        .replace("status: complete", "status: todo")
        .replace(CRITERIA, LOOSER)
        .replace(
            "acceptance:\n",
            "acceptance_superseded:\n  reason: fix the task\n",
        );
    write(root, &record_path("TSK-002"), &reopened);
    commit(root, "docs: reopen TSK-002 with changed criteria");
    assert!(run(root, &["ids", "seed"]).0, "ids seed");
    assert_blocks(
        &ci(root, PLAN, BOTH),
        "an amendment that reopens a deleted landed task",
        &[
            "work.criteria_frozen",
            "TSK-002: a reopened task keeps its criteria",
        ],
    );
    // Added again in one step, reopened, the record is refused the same way.
    git(root, &["switch", "-c", "plan/readd-reopened", "main"]);
    write(
        root,
        &record_path("TSK-002"),
        &task("TSK-002", Some("EPC-002"), "main", "todo", LOOSER),
    );
    commit(root, "docs: add TSK-002 again, reopened");
    assert!(run(root, &["ids", "seed"]).0, "ids seed");
    assert_blocks(
        &ci(root, "plan/readd-reopened", BOTH),
        "a deleted landed task added again reopened",
        &[
            "work.criteria_frozen",
            "TSK-002: a reopened task keeps its criteria",
        ],
    );
}
