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
            &["which the symbolic link CLAUDE.md reaches"],
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
                &[&format!("which the symbolic link {link} reaches")],
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

/// A changed document, the links (path, target) that reach it, and the
/// link a refusal names.
#[cfg(unix)]
type LinkCase<'a> = (&'a str, &'a [(&'a str, &'a str)], &'a str);

/// Review round 3 (AC-2, AC-3): a link is followed through every link on
/// its way, before the `..` after it, so a chain through a folder link, an
/// absolute folder link or a backslash target still keeps the document it
/// reaches out of a planning amendment, with a body and on push; a link to
/// the root `AGENTS.md` carries only what `AGENTS.md` may.
#[cfg(unix)]
#[test]
fn a_link_chain_is_followed_before_its_dots() {
    use std::os::unix::fs::symlink;
    let dir = repo();
    let root = dir.path();
    // Control: CLAUDE.md -> AGENTS.md, and the project section changes.
    symlink("AGENTS.md", root.join("CLAUDE.md")).unwrap();
    commit(root, "docs: link the harness instructions to AGENTS.md");
    two_epic_amendment(root);
    write(root, "AGENTS.md", &agents("mine, with the amended plan"));
    commit(root, "docs: carry the instruction text");
    assert_passes(&ci(root, PLAN, BOTH), "a link to AGENTS.md", &[]);
    assert_passes(&ci_push(root, PLAN), "a link to AGENTS.md on push", &[]);

    let absolute = root.join("docs").to_string_lossy().into_owned();
    let cases: [LinkCase; 3] = [
        // docs/alias -> docs/another/subdir, so `..` lands in docs/another.
        (
            "docs/another/rules.md",
            &[
                ("docs/alias", "another/subdir"),
                ("CLAUDE.local.md", "docs/alias/../rules.md"),
            ],
            "CLAUDE.local.md",
        ),
        // An absolute folder link on the way.
        (
            "docs/abs.md",
            &[
                ("docs/outside", absolute.as_str()),
                ("CLAUDE.extra.md", "docs/outside/abs.md"),
            ],
            "CLAUDE.extra.md",
        ),
        // A backslash is a letter here and a separator on Windows.
        (
            "docs/instructions\\safe.md",
            &[("CLAUDE.back.md", "docs/instructions\\safe.md")],
            "CLAUDE.back.md",
        ),
    ];
    for (doc, links, link) in cases {
        git(root, &["switch", "main"]);
        write(root, doc, &agents("linked"));
        write(root, "docs/another/subdir/keep.md", "Kept.\n");
        for (at, target) in links {
            symlink(target, root.join(at)).unwrap();
        }
        commit(root, "docs: link an instruction file");
        let branch = format!("plan/chain-{link}");
        git(root, &["switch", "-C", &branch, "main"]);
        let text = std::fs::read_to_string(root.join(doc)).unwrap();
        write(root, doc, &text.replace("managed rules", "other rules"));
        commit(root, "docs: change the linked instructions");
        assert_blocks(
            &ci(root, &branch, "Task: EPC-001"),
            doc,
            &[&format!("which the symbolic link {link} reaches")],
        );
        loosen(root, "TSK-001", Some("EPC-001"), "main");
        commit(root, "docs(records): loosen AC-1 behind the link");
        assert_blocks(
            &ci_push(root, &branch),
            doc,
            &["work.criteria_frozen", "TSK-001 changes its criteria"],
        );
        // Drop the links so the next case is judged on its own.
        git(root, &["switch", "main"]);
        for (at, _) in links {
            std::fs::remove_file(root.join(at)).unwrap();
        }
        commit(root, "docs: drop the links");
    }
}
