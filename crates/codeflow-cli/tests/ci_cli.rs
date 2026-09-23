//! End-to-end tests for the `codeflow ci` PR-body structure gate
//! (`git.pr_sections`): a lazy PR body fails CI mechanically, docs-only
//! ranges are exempt from the code sections, template remnants warn, and a
//! local run without a PR body is untouched. The policy-character rule
//! (`git.policy_characters`, ADR-0067) is pinned here too: it judges the PR
//! body and the lines a range adds, never the bytes a range leaves alone.
//!
//! Each test runs the real binary (`CARGO_BIN_EXE_codeflow`) in a tempdir git
//! repo (the `policy_cli.rs` pattern), pinning the exit-code and message
//! contracts a consumer of the binary (no source) relies on.

use std::path::Path;
use std::process::{Command, Output};

/// Shared isolated `CODEFLOW_HOME` (the `hooks_cli.rs` pattern): keeps the
/// per-command registry touch out of the developer's real `~/.codeflow`.
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
        // The structure check reads this when no --pr-body flag is given; the
        // developer's shell must not leak a body into the no-body tests.
        .env_remove("CODEFLOW_PR_BODY")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE");
    cmd
}

fn run_in(dir: &Path, args: &[&str]) -> Output {
    codeflow()
        .args(args)
        .current_dir(dir)
        .output()
        .expect("binary runs")
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}");
}

/// A repo with a `main` base commit and a `feat/x` branch carrying one commit
/// that touches `kind` — `"code"` (a .rs file) or `"docs"` (a .md file).
fn repo_with_range(dir: &Path, kind: &str) {
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "user.email", "t@example.com"]);
    git(dir, &["config", "user.name", "t"]);
    std::fs::write(dir.join("base.txt"), "base\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "chore: init"]);
    git(dir, &["checkout", "-b", "feat/x"]);
    if kind == "code" {
        std::fs::write(dir.join("thing.rs"), "fn main() {}\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-m", "feat: add thing"]);
    } else {
        std::fs::write(dir.join("guide.md"), "# guide\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-m", "docs: add guide"]);
    }
}

fn ci_with_body(dir: &Path, body: &str) -> Output {
    run_in(
        dir,
        &[
            "ci",
            "--base",
            "main",
            "--head",
            "HEAD",
            "--branch",
            "feat/x",
            "--pr-body",
            body,
        ],
    )
}

#[test]
fn ci_blocks_task_whose_planning_record_is_not_on_target() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    git(dir.path(), &["config", "user.email", "t@example.com"]);
    git(dir.path(), &["config", "user.name", "t"]);
    std::fs::write(dir.path().join("base.txt"), "base\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "chore: init"]);
    git(dir.path(), &["switch", "-c", "task/TSK-001-unanchored"]);
    let tasks = dir.path().join("project-management/tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    std::fs::write(
        tasks.join("TSK-001.md"),
        "---\nid: TSK-001\nepic_id: null\nstandalone_reason: branch-only task\nintegration_target: main\ntitle: unanchored\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: []\ncreated: 2026-07-29\n---\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("thing.rs"), "fn work() {}\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "feat: add unanchored work"]);

    let output = run_in(
        dir.path(),
        &[
            "ci",
            "--base",
            "main",
            "--head",
            "HEAD",
            "--branch",
            "task/TSK-001-unanchored",
            "--pr-body",
            "## Summary\nUnanchored work.\n\n## Changes\n- add work\n\n## Testing\n```text\nnot run\n```",
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("work.stable_planning_anchor"), "{err}");
}

#[test]
fn ci_blocks_an_invalid_visible_workgraph_on_a_task_branch() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    git(dir.path(), &["config", "user.email", "t@example.com"]);
    git(dir.path(), &["config", "user.name", "t"]);
    let tasks = dir.path().join("project-management/tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    std::fs::write(
        tasks.join("TSK-001.md"),
        "---\nid: TSK-001\nepic_id: null\nstandalone_reason: bounded repair\nintegration_target: main\ntitle: repair\nstatus: todo\nwork_type: fix\nspecs: []\ndepends_on: []\ncreated: 2026-07-29\n---\n\n## Description\nRepair the implementation.\n\n## Acceptance Criteria\n- [ ] repair verified\n",
    )
    .unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "plan: anchor repair task"]);
    git(dir.path(), &["switch", "-c", "task/TSK-001-repair"]);

    let epics = dir.path().join("project-management/epics");
    std::fs::create_dir_all(&epics).unwrap();
    std::fs::write(
        epics.join("EPC-999.md"),
        "---\nid: EPC-998\ntitle: mismatch\nstatus: planning\nwork_type: feat\ncreated: 2026-07-29\n---\n\n## Summary\nMismatch.\n\n## Acceptance Criteria\n- [ ] fixed\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("thing.rs"), "fn work() {}\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "fix: repair work"]);

    let output = run_in(
        dir.path(),
        &[
            "ci",
            "--base",
            "main",
            "--head",
            "HEAD",
            "--branch",
            "task/TSK-001-repair",
            "--pr-body",
            "## Summary\nRepair.\n\n## Changes\n- repair work\n\n## Testing\n- focused test",
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("work.valid_graph"), "{err}");
    assert!(err.contains("EPC-999.md"), "{err}");
}

#[test]
fn ci_keeps_task_prefix_available_without_durable_work_tracking() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_range(dir.path(), "code");
    let foreign_tasks = dir.path().join("project-management/tasks");
    std::fs::create_dir_all(&foreign_tasks).unwrap();
    std::fs::write(foreign_tasks.join("notes.md"), "External tracker notes").unwrap();
    let output = run_in(
        dir.path(),
        &[
            "ci",
            "--base",
            "main",
            "--head",
            "HEAD",
            "--branch",
            "task/tidy-the-logger",
            "--pr-body",
            "## Summary\nBounded task.\n\n## Changes\n- tidy logger\n\n## Testing\n- focused test",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("work-start"), "{stdout}");
}

#[test]
fn ci_blocks_indeterminate_state_without_a_task_directory() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_range(dir.path(), "code");
    let state_dir = dir.path().join(".codeflow");
    std::fs::create_dir_all(&state_dir).unwrap();
    std::fs::write(state_dir.join("project.toml"), "tier = [invalid").unwrap();

    let output = run_in(
        dir.path(),
        &[
            "ci",
            "--base",
            "main",
            "--head",
            "HEAD",
            "--branch",
            "task/TSK-001-repair",
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("work.tracking_state"), "{err}");
    assert!(
        !err.contains("[invalid"),
        "state contents must not be echoed: {err}"
    );
}

#[test]
fn ci_recognizes_nested_only_historical_task() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_range(dir.path(), "code");
    let nested = dir.path().join("project-management/epics/EPC-001/tasks");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(
        nested.join("TSK-001-001.md"),
        "---\nid: TSK-001-001\nepic_id: null\nstandalone_reason: historical task\nintegration_target: main\ntitle: historical\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: []\ncreated: 2026-07-29\n---\n\n## Description\nNested historical task.\n\n## Acceptance Criteria\n- [ ] anchored first\n",
    )
    .unwrap();

    let output = run_in(
        dir.path(),
        &[
            "ci",
            "--base",
            "main",
            "--head",
            "HEAD",
            "--branch",
            "task/TSK-001-001-unanchored",
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("work.stable_planning_anchor"), "{err}");
}

/// A body satisfying every default-required section with real content.
const FULL_BODY: &str = "## Summary\n\n- adds a thing\n\n## Changes\n\n- one change\n\n\
                         ## Testing\n\n- cargo test: 12 passed\n";

#[test]
fn ci_absent_pr_body_skips_structure_check() {
    // A local run without a PR body must be untouched by the structure gate —
    // the check is skipped exactly like the attribution/emoji PR-body scan.
    let dir = tempfile::tempdir().unwrap();
    repo_with_range(dir.path(), "code");
    let out = run_in(
        dir.path(),
        &[
            "ci", "--base", "main", "--head", "HEAD", "--branch", "feat/x",
        ],
    );
    assert_eq!(out.status.code(), Some(0));
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!all.contains("pr_sections"), "{all}");
    assert!(
        !all.contains("PR-body"),
        "the summary must not claim a check that never ran: {all}"
    );
}

#[test]
fn ci_blocks_lazy_pr_body_naming_the_section() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_range(dir.path(), "code");
    // No Summary, and Testing missing while the range touches code.
    let out = ci_with_body(dir.path(), "## Changes\n\n- one change\n");
    assert_eq!(out.status.code(), Some(1), "a lazy body must fail CI");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("git.pr_sections"), "{stderr}");
    assert!(stderr.contains("'## Summary'"), "{stderr}");
    assert!(stderr.contains("'## Testing'"), "{stderr}");
    assert!(stderr.contains("touches code"), "{stderr}");
}

#[test]
fn ci_empty_section_reported_as_present_but_empty() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_range(dir.path(), "code");
    let body = "## Summary\n\n<!-- template comment -->\n\n-\n\n## Changes\n\n- one change\n\n\
                ## Testing\n\n- ran the tests\n";
    let out = ci_with_body(dir.path(), body);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("present but empty"), "{stderr}");
}

#[test]
fn ci_docs_only_range_does_not_require_code_sections() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_range(dir.path(), "docs");
    let out = ci_with_body(
        dir.path(),
        "## Summary\n\n- docs\n\n## Changes\n\n- reword a guide\n",
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "docs-only range must not require Testing"
    );
}

#[test]
fn ci_executable_documentation_requires_testing_for_the_entire_range() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_range(dir.path(), "docs");
    std::fs::create_dir_all(dir.path().join("docs/examples")).unwrap();
    std::fs::write(
        dir.path().join("docs/examples/install.sh"),
        "#!/bin/sh\nprintf 'install example\\n'\n",
    )
    .unwrap();
    git(dir.path(), &["add", "."]);
    git(
        dir.path(),
        &["commit", "-m", "fix: update installer example"],
    );
    // A final prose-only commit must not hide the earlier executable change.
    std::fs::write(dir.path().join("guide.md"), "# Updated guide\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "docs: clarify guide"]);

    let missing = ci_with_body(
        dir.path(),
        "## Summary\n\nUpdate installation.\n\n## Changes\n\n- Update example.\n",
    );
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("'## Testing'"));
    let complete = ci_with_body(dir.path(), FULL_BODY);
    assert_eq!(complete.status.code(), Some(0));
}

#[test]
fn ci_warn_level_structure_reports_and_proceeds() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_range(dir.path(), "code");
    let cf = dir.path().join(".codeflow");
    std::fs::create_dir_all(&cf).unwrap();
    std::fs::write(
        cf.join("policy.json"),
        r#"{"schema_version":1,"git":{"pr_sections":"warn"}}"#,
    )
    .unwrap();
    let out = ci_with_body(dir.path(), "## Changes\n\n- one change\n");
    assert_eq!(
        out.status.code(),
        Some(0),
        "warn-level structure must not fail CI"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("git.pr_sections"), "{stderr}");
    assert!(stderr.contains("warning"), "{stderr}");
}

#[test]
fn ci_template_remnants_warn_but_pass() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_range(dir.path(), "code");
    let body = format!("{FULL_BODY}\n(paste the real test summary output here)\n|  |  |\n- CAP-\n");
    let out = ci_with_body(dir.path(), &body);
    assert_eq!(
        out.status.code(),
        Some(0),
        "remnants alone must never fail CI"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("template remnant"), "{stderr}");
    assert!(stderr.contains("paste-your-output"), "{stderr}");
    assert!(stderr.contains("empty cells"), "{stderr}");
    assert!(stderr.contains("linked-work"), "{stderr}");
}

#[test]
fn ci_full_body_on_code_range_is_clean() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_range(dir.path(), "code");
    let out = ci_with_body(dir.path(), FULL_BODY);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("PR-body"),
        "the summary names the check that ran: {stdout}"
    );
}

// -- policy characters (ADR-0067) --------------------------------------------

/// A repo whose `main` already carries `docs/old.md` with an em dash on its
/// second line (grandfathered bytes), then a `feat/x` branch; the caller adds
/// the branch commit.
fn repo_with_grandfathered_dash(dir: &Path) {
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "user.email", "t@example.com"]);
    git(dir, &["config", "user.name", "t"]);
    std::fs::create_dir_all(dir.join("docs")).unwrap();
    std::fs::write(
        dir.join("docs/old.md"),
        "# Old\n\nAn old line \u{2014} kept as is.\n\nClosing line.\n",
    )
    .unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "docs: add old page"]);
    git(dir, &["checkout", "-b", "feat/x"]);
}

fn ci_range(dir: &Path) -> Output {
    run_in(
        dir,
        &[
            "ci", "--base", "main", "--head", "HEAD", "--branch", "feat/x",
        ],
    )
}

#[test]
fn ci_grandfathered_policy_character_on_unchanged_line_passes() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_grandfathered_dash(dir.path());
    // Edit another line of the same file and add a clean page: the old dash
    // sits on an unchanged line, so the range adds no policy character.
    std::fs::write(
        dir.path().join("docs/old.md"),
        "# Old\n\nAn old line \u{2014} kept as is.\n\nClosing line, edited.\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("docs/new.md"), "# New\n\nA plain line.\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "docs: edit old page"]);
    let out = ci_range(dir.path());
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(0), "{all}");
    assert!(!all.contains("git.policy_characters"), "{all}");
    assert!(
        all.contains("added-lines"),
        "the summary names the check: {all}"
    );
}

#[test]
fn ci_added_policy_character_blocks_naming_file_and_line() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_grandfathered_dash(dir.path());
    std::fs::create_dir_all(dir.path().join("project-management/tasks")).unwrap();
    std::fs::write(
        dir.path().join("project-management/tasks/notes.md"),
        "# Notes\n\nPages 1\u{2013}3 cover it.\n",
    )
    .unwrap();
    // A dash outside the named trees is not judged.
    std::fs::write(dir.path().join("fixture.txt"), "x \u{2014} y\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "docs: add notes"]);
    let out = ci_range(dir.path());
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("git.policy_characters"), "{stderr}");
    assert!(
        stderr.contains("project-management/tasks/notes.md:3 adds an en dash (U+2013)"),
        "{stderr}"
    );
    assert!(stderr.contains("a comma, colon"), "{stderr}");
    assert!(!stderr.contains("fixture.txt"), "{stderr}");
    assert!(!stderr.contains("docs/old.md"), "{stderr}");
}

#[test]
fn ci_changed_line_keeping_a_policy_character_blocks() {
    // A changed line is new text: editing the grandfathered line while keeping
    // its dash makes the range add it.
    let dir = tempfile::tempdir().unwrap();
    repo_with_grandfathered_dash(dir.path());
    std::fs::write(
        dir.path().join("docs/old.md"),
        "# Old\n\nAn old line \u{2014} now edited.\n\nClosing line.\n",
    )
    .unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "docs: edit old line"]);
    let out = ci_range(dir.path());
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("docs/old.md:3 adds an em dash (U+2014)"),
        "{stderr}"
    );
}

#[test]
fn ci_pr_body_policy_character_blocks() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_range(dir.path(), "code");
    let body = FULL_BODY.replace("adds a thing", "adds a thing \u{2014} and more");
    let out = ci_with_body(dir.path(), &body);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("git.policy_characters"), "{stderr}");
    assert!(stderr.contains("PR body line 3"), "{stderr}");
}

#[test]
fn ci_commit_message_policy_character_blocks() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_grandfathered_dash(dir.path());
    std::fs::write(dir.path().join("docs/new.md"), "# New\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(
        dir.path(),
        &[
            "commit",
            "-m",
            "docs: add new page",
            "-m",
            "- one \u{2014} two",
        ],
    );
    let out = ci_range(dir.path());
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("git.policy_characters"), "{stderr}");
    assert!(stderr.contains("commit message line 3"), "{stderr}");
}

fn ci_range_output(dir: &Path) -> (Option<i32>, String) {
    let out = ci_range(dir);
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code(), all)
}

// Codex EPC-017 review, finding 3: git keeps a `#` line given with `-m`,
// and a merge message is committed text too; CI scans both as stored.
#[test]
fn ci_committed_hash_line_and_merge_message_are_scanned() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_grandfathered_dash(dir.path());
    std::fs::write(dir.path().join("docs/new.md"), "# New\n").unwrap();
    git(dir.path(), &["add", "."]);
    let body = "# one \u{2014} two";
    git(
        dir.path(),
        &["commit", "-m", "docs: add new page", "-m", body],
    );
    let (code, all) = ci_range_output(dir.path());
    assert_eq!(code, Some(1), "{all}");
    assert!(all.contains("commit message line 3"), "{all}");

    git(dir.path(), &["reset", "--hard", "main"]);
    git(dir.path(), &["checkout", "-b", "feat/y"]);
    std::fs::write(dir.path().join("docs/new.md"), "# New\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "docs: add new page"]);
    git(dir.path(), &["checkout", "feat/x"]);
    let subject = "Merge branch 'feat/y' \u{2014} tidy";
    git(dir.path(), &["merge", "--no-ff", "feat/y", "-m", subject]);
    let (code, all) = ci_range_output(dir.path());
    assert_eq!(code, Some(1), "{all}");
    assert!(all.contains("commit subject contains an em dash"), "{all}");
    assert!(all.contains("1 merge(s)"), "{all}");
}
