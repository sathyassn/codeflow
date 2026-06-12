//! End-to-end tests for the g-flow subcommands, driving the real binary
//! in tempdir git repos: test (loud no-op vs configured stack), validate
//! --docs (AC #9), status across tiers, and integrate.

use std::path::Path;
use std::process::{Command, Output};

fn codeflow(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("codeflow binary runs")
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_repo(dir: &Path) {
    git(dir, &["init", "-b", "main"]);
    std::fs::write(dir.join("README.md"), "hello\n").unwrap();
    git(dir, &["add", "README.md"]);
    git(dir, &["commit", "-m", "chore: initial commit"]);
}

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

// ---------------------------------------------------------------------------
// codeflow test — AC #7
// ---------------------------------------------------------------------------

#[test]
fn test_command_no_stack_is_loud_no_op_exit_zero() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());

    let output = codeflow(dir.path(), &["test"]);
    assert_eq!(output.status.code(), Some(0), "no-stack must exit 0");
    let err = stderr(&output);
    assert!(err.contains("WARNING"), "must warn loudly: {err}");
    assert!(err.contains("No tests were executed"), "loud no-op: {err}");
}

#[test]
fn test_command_configured_stack_runs_and_gates() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    write(
        dir.path(),
        ".codeflow/test-config.json",
        r#"{"schema_version": "1.0", "targets": [{"name": "gate", "runner": "custom", "modes": {"full": {"command": "echo running-tests && exit 0"}, "quick": {"command": "exit 0"}}}]}"#,
    );

    let pass = codeflow(dir.path(), &["test"]);
    assert_eq!(pass.status.code(), Some(0), "stderr: {}", stderr(&pass));
    let out = stdout(&pass);
    assert!(out.contains("gate: ok"), "per-target line: {out}");
    assert!(out.contains("test gate: passed"), "{out}");
    assert!(
        !stderr(&pass).contains("WARNING"),
        "configured stack must not warn"
    );

    // Failing target gates with exit 1.
    write(
        dir.path(),
        ".codeflow/test-config.json",
        r#"{"schema_version": "1.0", "targets": [{"name": "gate", "runner": "custom", "modes": {"full": {"command": "exit 7"}}}]}"#,
    );
    let fail = codeflow(dir.path(), &["test"]);
    assert_eq!(fail.status.code(), Some(1));
    assert!(stdout(&fail).contains("gate: FAILED"));
}

#[test]
fn test_command_quick_mode_honors_config() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    write(
        dir.path(),
        ".codeflow/test-config.json",
        r#"{"schema_version": "1.0", "targets": [{"name": "gate", "runner": "custom", "modes": {"full": {"command": "exit 1"}, "quick": {"command": "exit 0"}}}]}"#,
    );

    let output = codeflow(dir.path(), &["test", "--mode", "quick"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "quick mode must use the quick command: {}",
        stderr(&output)
    );
}

// ---------------------------------------------------------------------------
// codeflow validate --docs — AC #9
// ---------------------------------------------------------------------------

fn clean_docs_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    init_repo(root);
    write(
        root,
        "docs/capabilities.md",
        "# caps\n\n## CAP-001 — flow\n\n```yaml\nid: CAP-001\nname: flow\narea: engine\nstatus: shipped\nverified_by: [flow-core]\nepics: [EPC-001]\nadrs: [ADR-0001]\n```\n\nProse.\n",
    );
    write(
        root,
        "docs/decisions/ADR-0001-stack.md",
        "---\nid: ADR-0001\ntitle: stack\ndate: 2026-06-11\nstatus: accepted\nsuperseded_by: null\n---\n\n# ADR-0001\n",
    );
    write(
        root,
        "project-management/epics/EPC-001.md",
        "---\nid: \"epic-01a\"\nformat_id: \"EPC-001\"\ntitle: \"Build flow\"\nsummary: \"Flow work\"\nstatus: \"in_progress\"\nwork_type: \"feat\"\npriority: \"high\"\npr_number: null\ncreated_at: \"2026-06-11T00:00:00Z\"\nupdated_at: \"2026-06-11T00:00:00Z\"\ncapabilities: [CAP-001]\nadrs: [ADR-0001]\n---\n## Summary\nFlow.\n\n## Acceptance Criteria\nWorks.\n",
    );
    dir
}

#[test]
fn validate_docs_clean_graph_exits_zero() {
    let dir = clean_docs_repo();
    let output = codeflow(dir.path(), &["validate", "--docs"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        stdout(&output),
        stderr(&output)
    );
    assert!(stdout(&output).contains("doc graph clean"));
}

#[test]
fn validate_docs_dangling_epic_ref_exits_one_with_file_line() {
    let dir = clean_docs_repo();
    // Add a capability entry pointing at a nonexistent epic.
    let registry =
        std::fs::read_to_string(dir.path().join("docs/capabilities.md")).unwrap();
    write(
        dir.path(),
        "docs/capabilities.md",
        &format!(
            "{registry}\n## CAP-002 — ghost\n\n```yaml\nid: CAP-002\nname: ghost\narea: engine\nstatus: building\nverified_by: []\nepics: [EPC-404]\nadrs: []\n```\n"
        ),
    );

    let output = codeflow(dir.path(), &["validate", "--docs"]);
    assert_eq!(output.status.code(), Some(1), "dangling ref must exit 1");
    let err = stderr(&output);
    assert!(
        err.contains("docs/capabilities.md:") && err.contains("EPC-404"),
        "file:line-style message expected: {err}"
    );
}

#[test]
fn validate_docs_superseded_adr_without_superseded_by_exits_one() {
    let dir = clean_docs_repo();
    write(
        dir.path(),
        "docs/decisions/ADR-0002-old.md",
        "---\nid: ADR-0002\ntitle: old\ndate: 2026-06-11\nstatus: superseded\nsuperseded_by: null\n---\n\n# ADR-0002\n",
    );

    let output = codeflow(dir.path(), &["validate", "--docs"]);
    assert_eq!(output.status.code(), Some(1));
    let err = stderr(&output);
    assert!(
        err.contains("ADR-0002-old.md:") && err.contains("superseded_by"),
        "file:line-style message expected: {err}"
    );
}

#[test]
fn validate_docs_absent_tiers_skip_with_notes() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());

    let output = codeflow(dir.path(), &["validate", "--docs"]);
    assert_eq!(output.status.code(), Some(0));
    let out = stdout(&output);
    assert!(out.contains("note:"), "absent dirs must be noted: {out}");
}

// ---------------------------------------------------------------------------
// codeflow status — tiers
// ---------------------------------------------------------------------------

#[test]
fn status_minimal_tier_is_graceful() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());

    let output = codeflow(dir.path(), &["status"]);
    assert_eq!(output.status.code(), Some(0));
    let out = stdout(&output);
    assert!(out.contains("branch: main"), "{out}");
    assert!(out.contains("(no project-management tier)"), "{out}");
    assert!(out.contains("(no registry)"), "{out}");
}

#[test]
fn status_full_tier_shows_work_and_capability_table() {
    let dir = clean_docs_repo();

    let output = codeflow(dir.path(), &["status", "--capabilities"]);
    assert_eq!(output.status.code(), Some(0));
    let out = stdout(&output);
    assert!(out.contains("in_progress 1"), "epic counts: {out}");
    assert!(out.contains("EPC-001 Build flow"), "in-flight epic: {out}");
    assert!(out.contains("CAP-001"), "capability table: {out}");
    assert!(out.contains("shipped"), "{out}");
}

// ---------------------------------------------------------------------------
// codeflow integrate — happy path and dirty-tree block via the binary
// ---------------------------------------------------------------------------

#[test]
fn integrate_lands_branch_and_prints_report() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    git(dir.path(), &["checkout", "-b", "feat/x"]);
    write(dir.path(), "feature.txt", "feature\n");
    git(dir.path(), &["add", "feature.txt"]);
    git(dir.path(), &["commit", "-m", "feat: add feature"]);
    git(dir.path(), &["checkout", "main"]);
    write(
        dir.path(),
        ".codeflow/test-config.json",
        r#"{"schema_version": "1.0", "targets": [{"name": "gate", "runner": "custom", "modes": {"full": {"command": "exit 0"}}}]}"#,
    );

    let output = codeflow(dir.path(), &["integrate", "feat/x", "--into", "main"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr(&output)
    );
    let out = stdout(&output);
    assert!(out.contains("integrated 'feat/x' into 'main'"), "{out}");
    assert!(out.contains("test gate: passed"), "{out}");
    assert!(dir.path().join("feature.txt").exists(), "merge landed");
}

#[test]
fn integrate_refuses_dirty_tree_via_binary() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    git(dir.path(), &["branch", "feat/x", "main"]);
    write(dir.path(), "README.md", "uncommitted\n");

    let output = codeflow(dir.path(), &["integrate", "feat/x"]);
    assert_eq!(output.status.code(), Some(1));
    let err = stderr(&output);
    assert!(err.contains("dirty"), "{err}");
    assert!(err.contains("nothing was merged"), "{err}");
}
