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

#[path = "ci_cli/acceptance_fix.rs"]
mod acceptance_fix;
#[path = "ci_cli/change_class_probes.rs"]
mod change_class_probes;
#[path = "ci_cli/pr_body_fixtures.rs"]
mod pr_body_fixtures;

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
        .env_remove("GITHUB_EVENT_NAME")
        .env_remove("GITHUB_HEAD_REF")
        .env_remove("CI_PIPELINE_SOURCE")
        .env_remove("CI_MERGE_REQUEST_IID")
        .env_remove("BITBUCKET_PR_ID")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE");
    cmd
}

fn run_in(dir: &Path, args: &[&str]) -> Output {
    // These fixtures isolate section checks; grammar negatives live in classification_cli.
    let mut named: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    if let Some(at) = named
        .iter()
        .position(|arg| arg == "--pr-body")
        .map(|at| at + 1)
    {
        if let Some(body) = named
            .get(at)
            .filter(|body| !body.trim().is_empty() && !body.contains("Task:"))
        {
            let branch = args
                .windows(2)
                .find(|pair| pair[0] == "--branch")
                .map_or("", |pair| pair[1]);
            let id = codeflow_core::workgraph::task_id_from_branch(dir, branch)
                .unwrap_or_else(|| "TSK-001".into());
            named[at] = format!("Task: {id}\n{body}");
        }
    }
    codeflow()
        .args(&named)
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
        "---\nid: TSK-001\nepic_id: EPC-001\nstandalone_reason: null\nintegration_target: main\ntitle: unanchored\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: []\ncreated: 2026-07-29\n---\n",
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
            "## Summary\nUnanchored work.\n\n- one change\n\n## Reviews\nNone: pending review.\n## Release impact\n- Impact: patch\n- Breaking: no\n- Rationale: Preserve public behavior.\n- Migration: none\n## Changes\n- add work\n\n## Testing\n```text\nnot run\n```",
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
        "---\nid: TSK-001\nepic_id: null\nstandalone_reason: bounded repair\nintegration_target: main\ntitle: repair\nstatus: todo\nwork_type: fix\nspecs: []\ndepends_on: []\ncreated: 2026-07-29\n---\n\n## Description\nRepair the implementation.\n\n## Acceptance Criteria\n- AC-1 repair verified\n",
    )
    .unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "plan: anchor repair task"]);
    git(dir.path(), &["switch", "-c", "task/TSK-001-repair"]);

    let epics = dir.path().join("project-management/epics");
    std::fs::create_dir_all(&epics).unwrap();
    std::fs::write(
        epics.join("EPC-999.md"),
        "---\nid: EPC-998\ntitle: mismatch\nstatus: planning\nwork_type: feat\ncreated: 2026-07-29\n---\n\n## Summary\nMismatch.\n\n## Acceptance Criteria\n- AC-1 fixed\n",
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
            "## Summary\nRepair.\n\n- one change\n\n## Reviews\nNone: pending review.\n## Release impact\n- Impact: patch\n- Breaking: no\n- Rationale: Preserve public behavior.\n- Migration: none\n## Changes\n- repair work\n\n## Testing\n- focused test",
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
            "## Summary\nBounded task.\n\n- one change\n\n## Reviews\nNone: pending review.\n## Release impact\n- Impact: patch\n- Breaking: no\n- Rationale: Preserve public behavior.\n- Migration: none\n## Changes\n- tidy logger\n\n## Testing\n- focused test",
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
        "---\nid: TSK-001-001\nepic_id: null\nstandalone_reason: historical task\nintegration_target: main\ntitle: historical\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: []\ncreated: 2026-07-29\n---\n\n## Description\nNested historical task.\n\n## Acceptance Criteria\n- AC-1 anchored first\n",
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
const FULL_BODY: &str = "## Summary\n\nAdds a thing so the command explains itself.\n\n- adds a thing\n\n## Reviews\nNone: pending review.\n## Release impact\n- Impact: patch\n- Breaking: no\n- Rationale: Preserve public behavior.\n- Migration: none\n## Changes\n\n- one change\n\n\
                         ## Testing\n\n- cargo test: 12 passed\nNot tested: Windows.\n";

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
    let out = ci_with_body(dir.path(), "## Reviews\nNone: pending review.\n## Release impact\n- Impact: patch\n- Breaking: no\n- Rationale: Preserve public behavior.\n- Migration: none\n## Changes\n\n- one change\n");
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
    let body = "## Summary\n\n<!-- template comment -->\n\n-\n\n## Reviews\nNone: pending review.\n## Release impact\n- Impact: patch\n- Breaking: no\n- Rationale: Preserve public behavior.\n- Migration: none\n## Changes\n\n- one change\n\n\
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
        "## Summary\n\nDocs.\n\n- docs\n\n## Reviews\nNone: pending review.\n## Release impact\n- Impact: patch\n- Breaking: no\n- Rationale: Preserve public behavior.\n- Migration: none\n## Changes\n\n- reword a guide\n",
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "docs-only range must not require Testing"
    );
}

/// TSK-135 AC-1: the PR body a range needs is scaled to its change class.
/// The shipped required list is configured and Release impact is set to
/// block, so an absent section would fail the run.
#[test]
fn ci_scales_the_pr_body_sections_to_the_change_class() {
    const LIGHT: &str =
        "## Summary\n\nRewords the guide.\n\n- reword the guide\n\n## Changes\n\n- one file\n";
    let blocking = |dir: &Path| {
        let cf = dir.join(".codeflow");
        std::fs::create_dir_all(&cf).unwrap();
        std::fs::write(
            cf.join("policy.json"),
            r#"{"schema_version":1,"git":{"pr_release_impact":"block","pr_required_sections":["Summary","Changes","Reviews","Release impact"]}}"#,
        )
        .unwrap();
    };
    // Markdown under docs/ (a guide) or a plan under docs/plan/, on a
    // branch cut from the base commit.
    let light_range = |dir: &Path, path: &str| {
        repo_with_range(dir, "docs");
        git(dir, &["reset", "-q", "--hard", "main"]);
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, "# Notes\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-m", "docs: add notes"]);
    };
    // Docs-only and planning-only ranges: Summary and Changes suffice, and
    // an absent Release impact reads as none.
    for (class, path) in [
        ("docs-only", "docs/guide.md"),
        ("planning-only", "docs/plan/roadmap.md"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        light_range(dir.path(), path);
        blocking(dir.path());
        git(dir.path(), &["branch", "integration/line", "main"]);
        let out = run_in(
            dir.path(),
            &[
                "ci",
                "--base",
                "integration/line",
                "--branch",
                "feat/x",
                "--pr-body",
                LIGHT,
            ],
        );
        let all = combined(&out);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(0), "{class}: {all}");
        assert!(!stderr.contains("git.pr_sections"), "{class}: {all}");
        assert!(!stderr.contains("git.pr_release_impact"), "{class}: {all}");
    }
    // A code range is unchanged: every configured section, Testing, and a
    // Release impact declaration.
    let dir = tempfile::tempdir().unwrap();
    repo_with_range(dir.path(), "code");
    blocking(dir.path());
    let out = ci_with_body(dir.path(), LIGHT);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    for section in ["'## Reviews'", "'## Release impact'", "'## Testing'"] {
        assert!(stderr.contains(section), "{section}: {stderr}");
    }
    assert!(stderr.contains("git.pr_release_impact"), "{stderr}");
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
        "## Summary\n\nUpdate installation.\n\n- one change\n\n## Reviews\nNone: pending review.\n## Release impact\n- Impact: patch\n- Breaking: no\n- Rationale: Preserve public behavior.\n- Migration: none\n## Changes\n\n- Update example.\n",
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
    let out = ci_with_body(dir.path(), "## Reviews\nNone: pending review.\n## Release impact\n- Impact: patch\n- Breaking: no\n- Rationale: Preserve public behavior.\n- Migration: none\n## Changes\n\n- one change\n");
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
    let body = format!(
        "{FULL_BODY}\n(paste the real test summary output here)\n|  |  |\n- Breaking: yes | no\n"
    );
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
    assert!(
        stderr.contains("unresolved template alternatives"),
        "{stderr}"
    );
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

/// The Summary shape (`git.pr_summary`, ADR-0071 note of 2026-10-03) blocks
/// by default through the real binary, names what it found and clears once
/// the Summary is a lead then bullets; a project lowers it to warn or off.
#[test]
fn ci_summary_shape_blocks_prose_only_and_follows_its_level() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_range(dir.path(), "code");
    let prose = FULL_BODY.replacen(
        "- adds a thing\n",
        "It also explains the thing at length.\n",
        1,
    );
    assert_ne!(prose, FULL_BODY, "the fixture names its Summary list");
    let out = ci_with_body(dir.path(), &prose);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("git.pr_summary (block)"), "{stderr}");
    assert!(stderr.contains("(found: paragraph, paragraph)"), "{stderr}");
    assert!(stderr.contains("writing.md` \"Summaries\""), "{stderr}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("level pr_summary = block (shipped default)"),
        "{stdout}"
    );

    let out = ci_with_body(dir.path(), FULL_BODY);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let cf = dir.path().join(".codeflow");
    std::fs::create_dir_all(&cf).unwrap();
    for (level, code) in [("warn", 0), ("off", 0)] {
        std::fs::write(
            cf.join("policy.json"),
            format!(r#"{{"schema_version":1,"git":{{"pr_summary":"{level}"}}}}"#),
        )
        .unwrap();
        let out = ci_with_body(dir.path(), &prose);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(code), "{level}: {stderr}");
        assert_eq!(
            stderr.contains("git.pr_summary"),
            level == "warn",
            "{level}: {stderr}"
        );
        assert!(
            String::from_utf8_lossy(&out.stdout)
                .contains(&format!("level pr_summary = {level} (configured)")),
            "{level}"
        );
    }
}

// -- policy characters (ADR-0067) --------------------------------------------

/// Set `git.policy_characters` to `block`, the level this repository uses;
/// the shipped default is `warn`, which never fails a run.
fn block_policy_characters(dir: &Path) {
    std::fs::create_dir_all(dir.join(".codeflow")).unwrap();
    std::fs::write(
        dir.join(".codeflow/policy.json"),
        r#"{"schema_version":1,"git":{"policy_characters":"block"}}"#,
    )
    .unwrap();
}

/// A repo whose `main` already carries `docs/old.md` with an em dash on its
/// second line (grandfathered bytes) and a policy that blocks the character,
/// then a `feat/x` branch; the caller adds the branch commit.
fn repo_with_grandfathered_dash(dir: &Path) {
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "user.email", "t@example.com"]);
    git(dir, &["config", "user.name", "t"]);
    block_policy_characters(dir);
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
    block_policy_characters(dir.path());
    let body = FULL_BODY.replace("adds a thing", "adds a thing \u{2014} and more");
    let out = ci_with_body(dir.path(), &body);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("git.policy_characters"), "{stderr}");
    assert!(stderr.contains("PR body line 6"), "{stderr}");
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

// Codex EPC-017 review, finding 4: a `-diff` attribute turned an added
// Markdown line into a binary-files summary with no hunk, and the check
// reported success. Text is now decided by content, not by attributes.
#[test]
fn ci_diff_attribute_does_not_hide_an_added_policy_character() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_grandfathered_dash(dir.path());
    std::fs::write(dir.path().join(".gitattributes"), "docs/*.md -diff\n").unwrap();
    std::fs::write(
        dir.path().join("docs/new.md"),
        "# New\n\nA line \u{2014} added.\n",
    )
    .unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "docs: add new page"]);
    let (code, all) = ci_range_output(dir.path());
    assert_eq!(code, Some(1), "{all}");
    assert!(
        all.contains("docs/new.md:3 adds an em dash (U+2014)"),
        "{all}"
    );
    assert!(!all.contains("docs/old.md"), "{all}");
}

#[test]
fn ci_genuine_binary_under_a_named_tree_is_not_scanned() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_grandfathered_dash(dir.path());
    // A NUL in the first bytes marks real binary content, whatever bytes
    // that happen to spell a dash follow it.
    let mut blob = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\n".to_vec();
    blob.extend("\u{2014}\n".as_bytes());
    std::fs::write(dir.path().join("docs/figure.png"), blob).unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "docs: add figure"]);
    let (code, all) = ci_range_output(dir.path());
    assert_eq!(code, Some(0), "{all}");
    assert!(!all.contains("git.policy_characters"), "{all}");
}

// Codex EPC-017 review round 2, N1: Git quotes a name holding `"`, `\` or a
// control byte in patch headers. Binary content is classified by blob id,
// and the quoted name is decoded, so neither shape is misjudged.
#[cfg(unix)]
#[test]
fn ci_quoted_name_binary_passes_and_quoted_name_text_fails() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_grandfathered_dash(dir.path());
    let mut blob = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\n".to_vec();
    blob.extend("\u{2014}\n".as_bytes());
    std::fs::write(dir.path().join("docs/release\"preview.png"), blob).unwrap();
    std::fs::write(dir.path().join("docs/tab\there.png"), b"\0\xe2\x80\x93\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "docs: add quoted figures"]);
    let (code, all) = ci_range_output(dir.path());
    assert_eq!(code, Some(0), "{all}");
    assert!(!all.contains("git.policy_characters"), "{all}");

    std::fs::write(
        dir.path().join("docs/release\"notes.md"),
        "# Notes\n\nA line \u{2014} added.\n",
    )
    .unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "docs: add quoted notes"]);
    let (code, all) = ci_range_output(dir.path());
    assert_eq!(code, Some(1), "{all}");
    assert!(
        all.contains("docs/release\"notes.md:3 adds an em dash (U+2014)"),
        "{all}"
    );
    assert!(!all.contains("preview.png"), "{all}");
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

// Operator direction 2026-09-25 (ADR-0067 note): the dash rule is a writing
// guideline. Without a policy file the built-in default warns and passes.
#[test]
fn ci_default_level_warns_on_an_added_dash_and_passes() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_range(dir.path(), "docs");
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::fs::write(
        dir.path().join("docs/notes.md"),
        "# Notes\n\nA line \u{2014} added.\n",
    )
    .unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "docs: add notes"]);
    let (code, all) = ci_range_output(dir.path());
    assert_eq!(code, Some(0), "{all}");
    assert!(
        all.contains("policy rule git.policy_characters (warn)"),
        "{all}"
    );
    assert!(all.contains("warning(s) only"), "{all}");
    assert!(
        all.contains("docs/notes.md:3 adds an em dash (U+2014)"),
        "{all}"
    );
}

// -- managed content is CodeFlow's (ADR-0067 note, 2026-09-25) ----------------

/// Run a git command with the binary under test first on `PATH`, so the
/// hooks `codeflow init` wires run this build.
fn git_with_binary(dir: &Path, args: &[&str]) {
    let exe = Path::new(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(exe.parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("PATH", path)
        .env("CODEFLOW_HOME", isolated_home())
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
}

/// A `trunk` commit, then `codeflow init --standard` committed on
/// `feat/x`, with the scaffolded policy raised to `block` so any finding
/// would fail the run. The range `trunk..HEAD` is the scaffold pull request.
fn scaffolded_standard(dir: &Path) {
    git(dir, &["init", "-b", "trunk"]);
    git(dir, &["config", "user.email", "t@example.com"]);
    git(dir, &["config", "user.name", "t"]);
    std::fs::write(dir.join("README.md"), "# sample\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "chore: init"]);
    git(dir, &["checkout", "-b", "feat/x"]);
    let out = run_in(dir, &["init", "--standard", "--yes"]);
    assert!(
        out.status.success(),
        "init: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    git_with_binary(dir, &["add", "-A"]);
    git_with_binary(dir, &["commit", "-q", "-m", "chore: scaffold codeflow"]);
    let path = dir.join(".codeflow/policy.json");
    let mut policy: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    policy["git"]["policy_characters"] = "block".into();
    std::fs::write(&path, serde_json::to_vec_pretty(&policy).unwrap()).unwrap();
}

fn ci_scaffold_range(dir: &Path) -> (Option<i32>, String) {
    let out = run_in(
        dir,
        &[
            "ci", "--base", "trunk", "--head", "HEAD", "--branch", "feat/x",
        ],
    );
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code(), all)
}

/// A managed file that still ships an em dash: the evaluation fixtures quote
/// dashed text on purpose, while the skill prose itself carries none.
const DASHED_MANAGED: &str = ".agents/skills/cf-evaluate-model/resources/fixtures.json";

/// The 1-based line of the first em dash in `text`.
fn first_dash_line(text: &str) -> usize {
    text.lines()
        .position(|line| line.contains('\u{2014}'))
        .expect("fixture carries an em dash")
        + 1
}

#[test]
fn ci_scaffold_range_skips_unmodified_managed_files() {
    let dir = tempfile::tempdir().unwrap();
    scaffolded_standard(dir.path());
    // The fixture must really carry a dash, or the skip proves nothing.
    let skill = std::fs::read_to_string(dir.path().join(DASHED_MANAGED)).unwrap();
    assert!(skill.contains('\u{2014}'), "fixture lost its em dash");
    // The managed skills are exactly what this binary ships for those paths,
    // and the user-owned starter docs carry no policy character, so the
    // scaffold range is clean even when the policy blocks.
    let (code, all) = ci_scaffold_range(dir.path());
    assert_eq!(code, Some(0), "{all}");
    assert!(!all.contains("git.policy_characters"), "{all}");
    assert!(all.contains("added-lines"), "{all}");
}

#[test]
fn ci_adopter_edited_managed_file_is_scanned() {
    let dir = tempfile::tempdir().unwrap();
    scaffolded_standard(dir.path());
    let path = dir.path().join(DASHED_MANAGED);
    let mut skill = std::fs::read_to_string(&path).unwrap();
    let line = first_dash_line(&skill);
    skill.push_str("\nA local note, added by the adopter.\n");
    std::fs::write(&path, skill).unwrap();
    git_with_binary(dir.path(), &["add", "."]);
    git_with_binary(
        dir.path(),
        &["commit", "-m", "docs: note the evaluation fixtures"],
    );
    let (code, all) = ci_scaffold_range(dir.path());
    assert_eq!(code, Some(1), "{all}");
    assert!(
        all.contains(&format!("{DASHED_MANAGED}:{line} adds an em dash (U+2014)")),
        "{all}"
    );
    // The untouched mirror is still CodeFlow's bytes.
    assert!(
        !all.contains(".claude/skills/cf-evaluate-model/resources/fixtures.json"),
        "{all}"
    );
}

#[test]
fn ci_non_managed_docs_file_in_a_scaffolded_repo_is_scanned() {
    let dir = tempfile::tempdir().unwrap();
    scaffolded_standard(dir.path());
    std::fs::write(
        dir.path().join("docs/notes.md"),
        "# Notes\n\nA line \u{2014} added.\n",
    )
    .unwrap();
    git_with_binary(dir.path(), &["add", "."]);
    git_with_binary(dir.path(), &["commit", "-m", "docs: add notes"]);
    let (code, all) = ci_scaffold_range(dir.path());
    assert_eq!(code, Some(1), "{all}");
    assert!(
        all.contains("docs/notes.md:3 adds an em dash (U+2014)"),
        "{all}"
    );
    assert!(!all.contains("skills/"), "{all}");
}

/// Codex R1 and Grok D1 on the first cut: the change writes
/// `.codeflow/manifest.json` itself, so a record in it proves nothing. An
/// authored page with a dash and a matching forged record, for every
/// ownership value, is still reported under `block`.
#[test]
fn ci_forged_manifest_record_does_not_hide_an_authored_dash() {
    for ownership in ["managed", "managed-region", "user-owned"] {
        let dir = tempfile::tempdir().unwrap();
        repo_with_grandfathered_dash(dir.path());
        let page = "# Notes\n\nA line \u{2014} added.\n";
        std::fs::write(dir.path().join("docs/notes.md"), page).unwrap();
        std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
        let digest = codeflow_core::scaffold::sha256_hex(page.as_bytes());
        let manifest = serde_json::json!({
            "schema_version": 1,
            "scaffold_version": "3.0.0",
            "files": {
                "docs/notes.md": {
                    "src": "agents/skills/cf-consult/SKILL.md",
                    "ownership": ownership,
                    "sha256": digest,
                    "exec": false
                }
            }
        });
        std::fs::write(
            dir.path().join(".codeflow/manifest.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "docs: add notes"]);
        let (code, all) = ci_range_output(dir.path());
        assert_eq!(code, Some(1), "{ownership}: {all}");
        assert!(
            all.contains("docs/notes.md:3 adds an em dash (U+2014)"),
            "{ownership}: {all}"
        );
    }
}

/// The exemption is the shipped asset itself: a real managed skill at its
/// shipped path passes with no installed-file record at all, while the same
/// bytes at a path the scaffold does not install are scanned.
#[test]
fn ci_shipped_skill_bytes_are_exempt_only_at_their_shipped_path() {
    let shipped = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/base/agents/skills/cf-evaluate-model/resources/fixtures.json"),
    )
    .unwrap();
    let line = first_dash_line(&String::from_utf8_lossy(&shipped));
    let dir = tempfile::tempdir().unwrap();
    repo_with_grandfathered_dash(dir.path());
    let skill = dir
        .path()
        .join(".agents/skills/cf-evaluate-model/resources");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(skill.join("fixtures.json"), &shipped).unwrap();
    git(dir.path(), &["add", "."]);
    git(
        dir.path(),
        &["commit", "-m", "docs: add the evaluation fixtures"],
    );
    let (code, all) = ci_range_output(dir.path());
    assert_eq!(code, Some(0), "{all}");
    assert!(!all.contains("git.policy_characters"), "{all}");

    std::fs::write(dir.path().join("docs/fixtures.json"), &shipped).unwrap();
    git(dir.path(), &["add", "."]);
    git(
        dir.path(),
        &["commit", "-m", "docs: copy the evaluation fixtures"],
    );
    let (code, all) = ci_range_output(dir.path());
    assert_eq!(code, Some(1), "{all}");
    assert!(
        all.contains(&format!("docs/fixtures.json:{line} adds an em dash")),
        "{all}"
    );
    assert!(!all.contains(".agents/skills/cf-evaluate-model"), "{all}");
}

/// A task record under `project-management/tasks/`, valid on its own, with
/// the journey criterion a product-path range needs (TSK-105).
fn planned_task(id: &str, depends_on: &str) -> String {
    format!(
        "---\nid: {id}\nepic_id: null\nstandalone_reason: bounded work\nintegration_target: main\ntitle: work\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: [{depends_on}]\ncreated: 2026-07-29\n---\n\n## Description\nWork.\n\n## Acceptance Criteria\n- AC-1 When run, the system shall work.\n- AC-2 (journey) On a fresh project, the command shall succeed.\n"
    )
}

/// TSK-133 AC-2: the planning checks run for every work prefix that
/// carries a task id, at the `git.work_planning` level, never a hard-coded
/// block. The target holds the policy and two planned tasks; TSK-001 waits
/// on the open TSK-002, so its anchored preflight fails on `task/`, `fix/`
/// and `feat/` alike.
#[test]
fn ci_runs_the_planning_checks_on_every_work_prefix_at_the_policy_level() {
    for level in ["block", "warn"] {
        for prefix in ["task", "fix", "feat"] {
            let dir = tempfile::tempdir().unwrap();
            git(dir.path(), &["init", "-b", "main"]);
            git(dir.path(), &["config", "user.email", "t@example.com"]);
            git(dir.path(), &["config", "user.name", "t"]);
            std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
            std::fs::write(
                dir.path().join(".codeflow/policy.json"),
                format!(r#"{{"git": {{"work_planning": "{level}"}}}}"#),
            )
            .unwrap();
            let tasks = dir.path().join("project-management/tasks");
            std::fs::create_dir_all(&tasks).unwrap();
            std::fs::write(tasks.join("TSK-001.md"), planned_task("TSK-001", "TSK-002")).unwrap();
            std::fs::write(tasks.join("TSK-002.md"), planned_task("TSK-002", "")).unwrap();
            git(dir.path(), &["add", "."]);
            git(dir.path(), &["commit", "-m", "chore: plan two tasks"]);
            let branch = format!("{prefix}/TSK-001-early");
            git(dir.path(), &["switch", "-c", &branch]);
            std::fs::write(dir.path().join("thing.rs"), "fn work() {}\n").unwrap();
            git(dir.path(), &["add", "."]);
            git(
                dir.path(),
                &["commit", "-m", "feat: start before the dependency"],
            );
            let output = run_in(
                dir.path(),
                &[
                    "ci", "--base", "main", "--head", "HEAD", "--branch", &branch,
                ],
            );
            let out = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let shown = format!("work.stable_planning_anchor ({level})");
            assert!(out.contains(&shown), "{branch} at {level}: {out}");
            assert!(out.contains("TSK-002"), "{branch} at {level}: {out}");
            let expected = i32::from(level == "block");
            assert_eq!(
                output.status.code(),
                Some(expected),
                "{branch} at {level}: {out}"
            );
        }
    }
}

/// TSK-133 AC-2: a pull request naming its task with `Task:` from a branch
/// that carries no task id gets the same anchored preflight, at the same
/// `git.work_planning` level; classification itself still blocks.
#[test]
fn ci_reports_a_named_task_anchor_at_the_policy_level() {
    for level in ["block", "warn"] {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-b", "main"]);
        git(dir.path(), &["config", "user.email", "t@example.com"]);
        git(dir.path(), &["config", "user.name", "t"]);
        std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
        std::fs::write(
            dir.path().join(".codeflow/policy.json"),
            format!(r#"{{"git": {{"work_planning": "{level}"}}}}"#),
        )
        .unwrap();
        let tasks = dir.path().join("project-management/tasks");
        std::fs::create_dir_all(&tasks).unwrap();
        std::fs::write(tasks.join("TSK-001.md"), planned_task("TSK-001", "TSK-002")).unwrap();
        std::fs::write(tasks.join("TSK-002.md"), planned_task("TSK-002", "")).unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "chore: plan two tasks"]);
        git(dir.path(), &["switch", "-c", "feat/early-work"]);
        std::fs::write(dir.path().join("thing.rs"), "fn work() {}\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(
            dir.path(),
            &["commit", "-m", "feat: start before the dependency"],
        );
        let body = format!("Task: TSK-001\n\n{FULL_BODY}");
        let output = run_in(
            dir.path(),
            &[
                "ci",
                "--base",
                "main",
                "--head",
                "HEAD",
                "--branch",
                "feat/early-work",
                "--pr-body",
                &body,
            ],
        );
        let out = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            out.contains("pull request class: tracked TSK-001"),
            "{level}: {out}"
        );
        let shown = format!("work.stable_planning_anchor ({level})");
        assert!(out.contains(&shown), "{level}: {out}");
        assert_eq!(
            output.status.code(),
            Some(i32::from(level == "block")),
            "{level}: {out}"
        );
    }
}

/// A repository whose target holds the `work_planning` level, a planned
/// TSK-001 (waiting on the open TSK-002 when `waits`), and, when `bad_graph`,
/// an unrelated epic file whose id does not match its name; the branch
/// `fix/TSK-001-work` adds code. Returns the branch head.
fn planning_repo(dir: &Path, level: &str, waits: bool, bad_graph: bool) -> String {
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "user.email", "t@example.com"]);
    git(dir, &["config", "user.name", "t"]);
    std::fs::create_dir_all(dir.join(".codeflow")).unwrap();
    std::fs::write(
        dir.join(".codeflow/policy.json"),
        format!(r#"{{"git": {{"work_planning": "{level}"}}}}"#),
    )
    .unwrap();
    let tasks = dir.join("project-management/tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    let dep = if waits { "TSK-002" } else { "" };
    std::fs::write(tasks.join("TSK-001.md"), planned_task("TSK-001", dep)).unwrap();
    if waits {
        std::fs::write(tasks.join("TSK-002.md"), planned_task("TSK-002", "")).unwrap();
    }
    if bad_graph {
        let epics = dir.join("project-management/epics");
        std::fs::create_dir_all(&epics).unwrap();
        std::fs::write(
            epics.join("EPC-999.md"),
            "---\nid: EPC-998\ntitle: mismatch\nstatus: planning\nwork_type: feat\ncreated: 2026-07-29\n---\n\n## Summary\nMismatch.\n\n## Acceptance Criteria\n- AC-1 fixed\n",
        )
        .unwrap();
    }
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "chore: anchor planning"]);
    git(dir, &["switch", "-c", "fix/TSK-001-work"]);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/lib.rs"), "fn work() {}\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "fix: implement work"]);
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn combined(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// TSK-133 review R1: tracked work gets the visible-workgraph check once,
/// at the `git.work_planning` level, whether the task comes from the branch
/// (`fix/TSK-001-work`) or only from the `Task:` line (`fix/work`), and from
/// the target checkout as the shipped workflow runs it.
#[test]
fn ci_checks_the_visible_graph_once_for_any_tracked_context() {
    for level in ["block", "warn"] {
        let dir = tempfile::tempdir().unwrap();
        let head = planning_repo(dir.path(), level, false, true);
        let body = format!("Task: TSK-001\n\n{FULL_BODY}");
        let run = |branch: &str, head: &str| {
            run_in(
                dir.path(),
                &[
                    "ci",
                    "--base",
                    "main",
                    "--head",
                    head,
                    "--branch",
                    branch,
                    "--pr-body",
                    &body,
                ],
            )
        };
        let shown = format!("work.valid_graph ({level})");
        let mut cases = vec![
            ("own", run("fix/TSK-001-work", "HEAD")),
            ("named", run("fix/work", "HEAD")),
        ];
        git(dir.path(), &["checkout", "-q", "--detach", "main"]);
        cases.push(("target checkout", run("fix/work", &head)));
        for (case, output) in cases {
            let out = combined(&output);
            assert!(
                out.contains("pull request class: tracked TSK-001"),
                "{case} {level}: {out}"
            );
            assert_eq!(
                out.matches("work.valid_graph (").count(),
                1,
                "{case} {level}: {out}"
            );
            assert!(
                out.contains(&shown) && out.contains("EPC-999.md"),
                "{case} {level}: {out}"
            );
            assert_eq!(
                output.status.code(),
                Some(i32::from(level == "block")),
                "{case} {level}: {out}"
            );
        }
    }
}

/// TSK-133 review R2: an unreadable tracking state blocks `work start` and
/// CI alike, whatever `git.work_planning` says; with the state readable, a
/// genuine planning finding still only warns at `warn`.
#[test]
fn an_unreadable_tracking_state_blocks_work_start_and_ci_at_any_level() {
    for level in ["block", "warn"] {
        let dir = tempfile::tempdir().unwrap();
        planning_repo(dir.path(), level, true, false);
        std::fs::write(dir.path().join(".codeflow/project.toml"), "not valid [").unwrap();
        let start = run_in(dir.path(), &["work", "start", "TSK-001"]);
        let out = combined(&start);
        assert_eq!(start.status.code(), Some(1), "{level}: {out}");
        assert!(
            out.contains("cannot determine durable-work tracking"),
            "{level}: {out}"
        );
        assert!(!out.contains("continuing"), "{level}: {out}");
        assert!(!out.contains("not valid ["), "state is not echoed: {out}");
        let ci = run_in(
            dir.path(),
            &[
                "ci",
                "--base",
                "main",
                "--head",
                "HEAD",
                "--branch",
                "fix/TSK-001-work",
            ],
        );
        let out = combined(&ci);
        assert_eq!(ci.status.code(), Some(1), "{level}: {out}");
        assert!(
            out.contains("work.tracking_state (block)"),
            "{level}: {out}"
        );

        std::fs::remove_file(dir.path().join(".codeflow/project.toml")).unwrap();
        let start = run_in(dir.path(), &["work", "start", "TSK-001"]);
        let out = combined(&start);
        assert_eq!(
            start.status.code(),
            Some(i32::from(level == "block")),
            "{level}: {out}"
        );
        assert!(out.contains("TSK-002"), "{level}: {out}");
    }
}

// -- watched contract paths (TSK-147 AC-4) -----------------------------------

/// A code range whose commit touches `thing.rs`, which the policy watches as a
/// contract surface.
fn repo_touching_a_watched_path(dir: &Path) {
    repo_with_range(dir, "code");
    std::fs::create_dir_all(dir.join(".codeflow")).unwrap();
    std::fs::write(
        dir.join(".codeflow/policy.json"),
        r#"{"git": {"breaking_watch_paths": ["thing.rs"]}}"#,
    )
    .unwrap();
}

#[test]
fn a_body_declaring_no_break_with_a_rationale_settles_a_watched_path() {
    let dir = tempfile::tempdir().unwrap();
    repo_touching_a_watched_path(dir.path());
    let out = ci_with_body(dir.path(), FULL_BODY);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(!text.contains("contract surface"), "{text}");
    assert!(!text.contains("git.breaking_watch_paths"), "{text}");
}

#[test]
fn a_bodyless_run_notes_a_watched_path_and_points_at_release_impact() {
    let dir = tempfile::tempdir().unwrap();
    repo_touching_a_watched_path(dir.path());
    let out = ci_range(dir.path());
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(
        text.contains("codeflow ci: note: commit touches a declared contract surface (thing.rs)"),
        "{text}"
    );
    assert!(
        text.contains("`Breaking: no` with a `Rationale` under Release impact"),
        "{text}"
    );
    assert!(
        !text.contains("warning — policy rule git.breaking_watch_paths"),
        "{text}"
    );
}

#[test]
fn a_quoted_example_of_the_fields_keeps_the_watched_path_warning() {
    // TSK-147 F4: fields quoted from another pull request are an example,
    // not this change's assessment.
    let dir = tempfile::tempdir().unwrap();
    repo_touching_a_watched_path(dir.path());
    let fields = "- Impact: patch\n- Breaking: no\n- Rationale: Preserve public behavior.\n- Migration: none\n";
    let quoted = fields.replace("- ", "> - ");
    let body = FULL_BODY.replace(
        fields,
        &format!("Not assessed yet; an example from another pull request:\n\n{quoted}\n"),
    );
    assert_ne!(body, FULL_BODY);
    let out = ci_with_body(dir.path(), &body);
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(
        text.contains("warning — policy rule git.breaking_watch_paths (warn)"),
        "{text}"
    );
}

#[test]
fn a_body_without_the_declaration_keeps_the_watched_path_warning() {
    let dir = tempfile::tempdir().unwrap();
    repo_touching_a_watched_path(dir.path());
    let body = FULL_BODY.replace("- Rationale: Preserve public behavior.\n", "");
    let out = ci_with_body(dir.path(), &body);
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(
        text.contains("warning — policy rule git.breaking_watch_paths (warn)"),
        "{text}"
    );
}

// -- conflict markers (TSK-170) ------------------------------------------------

/// A marker line built at run time, so this file holds none itself.
fn marker(fill: char, size: usize, label: &str) -> String {
    format!("{}{label}", fill.to_string().repeat(size))
}

fn output_text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// TSK-170 AC-4: `codeflow ci` judges the lines a range adds to every text
/// path with the hook's matcher, level, messages and attribute rule, and
/// names the check it ran. One finding per case of AC-1 and AC-3.
#[test]
fn ci_refuses_conflict_markers_one_finding_per_case() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    repo_with_range(root, "code");
    // A clean range names the check among those that passed.
    let clean = ci_range(root);
    let text = output_text(&clean);
    assert_eq!(clean.status.code(), Some(0), "{text}");
    assert!(text.contains("conflict-markers"), "{text}");
    let cases = [
        ("opening.txt", marker('<', 7, " HEAD")),
        ("opening-bare.txt", marker('<', 7, "")),
        ("closing.txt", marker('>', 7, " feat/y")),
        ("closing-bare.txt", marker('>', 7, "")),
        ("base.txt.d", marker('|', 7, " merged common ancestors")),
        ("base-bare.txt.d", marker('|', 7, "")),
        ("fixtures/sized.txt", marker('<', 32, " HEAD")),
    ];
    for (path, line) in &cases {
        let full = root.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, format!("before\n{line}\nafter\n")).unwrap();
    }
    // A separator between an opening and a closing marker.
    std::fs::write(
        root.join("separator.md"),
        format!("text\n{}\n", marker('=', 7, "")),
    )
    .unwrap();
    std::fs::write(
        root.join("pair.md"),
        format!(
            "{}\nours\n{}\ntheirs\n{}\n",
            marker('<', 7, " a"),
            marker('=', 7, ""),
            marker('>', 7, " b")
        ),
    )
    .unwrap();
    // Seven-character markers under a path whose attribute sets 32.
    std::fs::write(
        root.join("fixtures/seven.txt"),
        format!("{}\nx\n{}\n", marker('<', 7, " a"), marker('>', 7, " b")),
    )
    .unwrap();
    std::fs::write(
        root.join(".gitattributes"),
        "fixtures/** conflict-marker-size=32\n",
    )
    .unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-m", "feat: add the fixtures"]);

    let out = ci_range(root);
    let all = output_text(&out);
    assert_eq!(out.status.code(), Some(1), "{all}");
    for (path, _) in &cases {
        assert_eq!(
            all.matches(&format!("{path}:2 adds an unresolved")).count(),
            1,
            "{path}: {all}"
        );
    }
    // The setext underline alone is text; the pair's three lines are found.
    assert!(!all.contains("separator.md"), "{all}");
    assert!(
        all.contains("pair.md:1 adds an unresolved opening"),
        "{all}"
    );
    assert!(
        all.contains("pair.md:3 adds an unresolved separator"),
        "{all}"
    );
    assert!(
        all.contains("pair.md:5 adds an unresolved closing"),
        "{all}"
    );
    assert!(!all.contains("fixtures/seven.txt"), "{all}");
    assert_eq!(
        all.matches("policy rule git.conflict_markers (block)")
            .count(),
        cases.len() + 3,
        "{all}"
    );
    assert!(
        all.contains(
            "resolve the conflict and restage, or set conflict-marker-size for the path in .gitattributes"
        ),
        "{all}"
    );

    // At warn the same findings warn and the run passes; at off it is silent.
    std::fs::create_dir_all(root.join(".codeflow")).unwrap();
    for (level, code) in [("warn", 0), ("off", 0)] {
        std::fs::write(
            root.join(".codeflow/policy.json"),
            format!(r#"{{"schema_version":1,"git":{{"conflict_markers":"{level}"}}}}"#),
        )
        .unwrap();
        let out = ci_range(root);
        let all = output_text(&out);
        assert_eq!(out.status.code(), Some(code), "{level}: {all}");
        let expected = if level == "warn" { cases.len() + 3 } else { 0 };
        assert_eq!(
            all.matches("policy rule git.conflict_markers (warn)")
                .count(),
            expected,
            "{level}: {all}"
        );
    }
}

/// TSK-170 review P3: a git without `check-attr --source` (older than 2.40)
/// leaves the check incomplete, and the remedy is to upgrade Git, not to
/// fetch. The old git is simulated by a wrapper that refuses only that
/// option; every other call reaches the real git.
#[cfg(unix)]
#[test]
fn ci_names_a_git_upgrade_when_check_attr_has_no_source() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    repo_with_range(&root, "code");
    let real = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    let real = String::from_utf8(real.stdout).unwrap().trim().to_string();
    let wrapper = dir.path().join("old-git");
    std::fs::create_dir(&wrapper).unwrap();
    std::fs::write(
        wrapper.join("git"),
        format!(
            "#!/bin/sh\nfor arg in \"$@\"; do\n  case \"$arg\" in --source=*) echo \"error: unknown option \\`source'\" >&2; exit 129;; esac\ndone\nexec {real} \"$@\"\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(wrapper.join("git"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(
        std::iter::once(wrapper.clone()).chain(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        )),
    )
    .unwrap();
    let out = codeflow()
        .args([
            "ci", "--base", "main", "--head", "HEAD", "--branch", "feat/x",
        ])
        .current_dir(&root)
        .env("PATH", path)
        .output()
        .unwrap();
    let all = output_text(&out);
    assert_eq!(out.status.code(), Some(1), "{all}");
    let at = all
        .find("conflict-marker check incomplete")
        .unwrap_or_else(|| panic!("{all}"));
    let finding = &all[at..all[at..]
        .find("policy file:")
        .map_or(all.len(), |end| at + end)];
    assert!(finding.contains("unknown option"), "{finding}");
    assert!(
        finding.contains("Git release build of 2.40 or later"),
        "{finding}"
    );
    assert!(!finding.contains("fetch the whole range"), "{finding}");
}

/// Every CI platform variable the product reads is blank in the test
/// environment (`.cargo/config.toml` `[env]`), so a pull request run's real
/// event never reaches a codeflow the tests start; a test that needs one sets
/// it on its own command. A new variable read in the sources fails here until
/// it is blanked too.
#[test]
fn the_test_environment_blanks_every_ci_variable_the_product_reads() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config = std::fs::read_to_string(workspace.join(".cargo/config.toml")).unwrap();
    let name = regex::Regex::new(r#""((?:GITHUB|GITLAB|BITBUCKET|CI)_[A-Z0-9_]+|CI)""#).unwrap();
    let mut read = std::collections::BTreeSet::new();
    let mut pending: Vec<_> = std::fs::read_dir(workspace.join("crates"))
        .unwrap()
        .map(|entry| entry.unwrap().path().join("src"))
        .filter(|src| src.is_dir())
        .collect();
    assert!(pending.len() >= 3, "{pending:?}");
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                read.extend(name.captures_iter(&text).map(|found| found[1].to_string()));
            }
        }
    }
    assert!(
        read.contains("GITHUB_BASE_REF") && read.contains("CI"),
        "{read:?}"
    );
    for variable in &read {
        assert!(
            config.contains(&format!("\n{variable} = {{ value = \"\", force = true }}")),
            "{variable} is not blanked in .cargo/config.toml"
        );
        assert_eq!(
            std::env::var(variable).unwrap_or_default(),
            "",
            "{variable}"
        );
    }
}
