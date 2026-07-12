//! End-to-end tests for the `codeflow ci` PR-body structure gate
//! (`git.pr_sections`): a lazy PR body fails CI mechanically, docs-only
//! ranges are exempt from the code sections, template remnants warn, and a
//! local run without a PR body is untouched.
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
            "ci", "--base", "main", "--head", "HEAD", "--branch", "feat/x", "--pr-body", body,
        ],
    )
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
        &["ci", "--base", "main", "--head", "HEAD", "--branch", "feat/x"],
    );
    assert_eq!(out.status.code(), Some(0));
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!all.contains("pr_sections"), "{all}");
    assert!(!all.contains("PR-body"), "the summary must not claim a check that never ran: {all}");
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
    let out = ci_with_body(dir.path(), "## Summary\n\n- docs\n\n## Changes\n\n- reword a guide\n");
    assert_eq!(out.status.code(), Some(0), "docs-only range must not require Testing");
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
    assert_eq!(out.status.code(), Some(0), "warn-level structure must not fail CI");
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
    assert_eq!(out.status.code(), Some(0), "remnants alone must never fail CI");
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
    assert!(stdout.contains("PR-body"), "the summary names the check that ran: {stdout}");
}
