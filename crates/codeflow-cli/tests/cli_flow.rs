//! End-to-end tests for the g-flow subcommands, driving the real binary
//! in tempdir git repos: test (loud no-op vs configured stack), validate
//! --docs (AC #9), status across tiers, and integrate.

use std::path::Path;
use std::process::{Command, Output};

/// Runs the binary with its own isolated `CODEFLOW_HOME` per call, so no
/// registry touch reaches the developer's real `~/.codeflow` and parallel
/// full gates never meet on one machine-wide gate lock (TSK-134). The
/// harness's own `CARGO_TARGET_DIR` lies outside these temp repositories, so
/// it is removed, as the gate would refuse it.
fn codeflow(dir: &Path, args: &[&str]) -> Output {
    let home = tempfile::tempdir().expect("home tempdir");
    Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .args(args)
        .current_dir(dir)
        .env("CODEFLOW_HOME", home.path())
        .env_remove("CARGO_TARGET_DIR")
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
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
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
fn test_command_strict_no_stack_exits_nonzero_with_banner() {
    // Scripted/unattended callers pass --strict so a "nothing ran" outcome
    // does NOT read as green: it exits non-zero while keeping the loud banner.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());

    let output = codeflow(dir.path(), &["test", "--strict"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "strict no-stack must exit non-zero (2 == nothing ran)"
    );
    let err = stderr(&output);
    assert!(
        err.contains("WARNING"),
        "banner still printed under strict: {err}"
    );
    assert!(
        err.contains("No tests were executed"),
        "loud no-op text kept: {err}"
    );
    assert!(
        err.contains("--strict"),
        "explains the non-zero exit: {err}"
    );
}

#[test]
fn test_command_strict_with_targets_still_passes() {
    // --strict only escalates the NoTargets no-op; a real run that passes is
    // unaffected, so this repo's own configured gates keep exiting 0.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    write(
        dir.path(),
        ".codeflow/test-config.json",
        r#"{"schema_version": "1.0", "targets": [{"name": "gate", "runner": "custom", "modes": {"full": {"command": "exit 0"}, "quick": {"command": "exit 0"}}}]}"#,
    );

    let output = codeflow(dir.path(), &["test", "--strict"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "a passing configured gate exits 0 even under --strict: {}",
        stderr(&output)
    );
    assert!(
        !stderr(&output).contains("WARNING"),
        "no no-op banner when targets ran"
    );
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

#[test]
fn test_setup_writes_config_offline_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    // A Cargo.toml makes stack detection fire, so setup writes a populated config.
    write(
        dir.path(),
        "Cargo.toml",
        "[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(
        dir.path(),
        "src/lib.rs",
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn passes() {}\n}\n",
    );

    let first = codeflow(dir.path(), &["test", "setup"]);
    assert_eq!(
        first.status.code(),
        Some(0),
        "setup must succeed: {}",
        stderr(&first)
    );
    let cfg = dir.path().join(".codeflow/test-config.json");
    assert!(cfg.exists(), "setup must write .codeflow/test-config.json");
    let before = std::fs::read(&cfg).unwrap();

    // Idempotent: a second run leaves the populated config byte-for-byte
    // unchanged and still exits 0.
    let second = codeflow(dir.path(), &["test", "setup"]);
    assert_eq!(second.status.code(), Some(0), "second setup must exit 0");
    let after = std::fs::read(&cfg).unwrap();
    assert_eq!(
        before, after,
        "setup must not overwrite an already-populated config"
    );

    // The conservative auto-detected config must run on a stock Rust
    // toolchain. It must not assume cargo-nextest, a project-defined nextest
    // profile, a report plugin, or a coverage artifact.
    let gate = codeflow(dir.path(), &["test", "--mode", "full", "--strict"]);
    assert_eq!(
        gate.status.code(),
        Some(0),
        "auto-detected Rust config must be runnable:\nstdout: {}\nstderr: {}",
        stdout(&gate),
        stderr(&gate)
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
    let registry = std::fs::read_to_string(dir.path().join("docs/capabilities.md")).unwrap();
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
fn validate_docs_on_an_unscaffolded_tree_is_quiet() {
    // TSK-147: a tree no tier scaffolded never had the docs or record
    // layers, so there is nothing to note and nothing to act on.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());

    let output = codeflow(dir.path(), &["validate", "--docs"]);
    assert_eq!(output.status.code(), Some(0));
    let out = stdout(&output);
    assert!(!out.contains("note:"), "no layer was installed: {out}");
    assert!(out.contains("doc graph clean"), "{out}");
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
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains("integrated 'feat/x' into 'main'"), "{out}");
    assert!(out.contains("test gate: passed"), "{out}");
    assert!(dir.path().join("feature.txt").exists(), "merge landed");
}

// ---------------------------------------------------------------------------
// codeflow epic new / task new — collision-free id allocation
// ---------------------------------------------------------------------------

#[test]
fn epic_new_allocates_and_scaffolds_from_template() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());

    // Empty project starts at EPC-001.
    let first = codeflow(dir.path(), &["epic", "new", "Ship the thing"]);
    assert_eq!(first.status.code(), Some(0), "stderr: {}", stderr(&first));
    let out = stdout(&first);
    assert!(out.contains("EPC-001"), "prints allocated id: {out}");
    let epic_path = dir.path().join("project-management/epics/EPC-001.md");
    assert!(epic_path.exists(), "scaffolds the epic file");
    let body = std::fs::read_to_string(&epic_path).unwrap();
    assert!(body.contains("id: EPC-001"), "rendered id: {body}");
    assert!(!body.contains("format_id:"), "one stable id: {body}");
    assert!(body.contains("Ship the thing"), "rendered title: {body}");
    assert!(!body.contains("{{"), "no placeholder survives: {body}");

    // Next allocation sees the file just written → EPC-002.
    let second = codeflow(dir.path(), &["epic", "new", "Second"]);
    assert_eq!(second.status.code(), Some(0));
    assert!(
        stdout(&second).contains("EPC-002"),
        "max+1: {}",
        stdout(&second)
    );
}

#[test]
fn task_new_allocates_independent_ids_across_epics() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    codeflow(dir.path(), &["epic", "new", "Epic one"]); // EPC-001
    codeflow(dir.path(), &["epic", "new", "Epic two"]); // EPC-002

    let t1 = codeflow(dir.path(), &["task", "new", "--epic", "EPC-001", "First"]);
    assert_eq!(t1.status.code(), Some(0), "stderr: {}", stderr(&t1));
    assert!(stdout(&t1).contains("TSK-001"), "{}", stdout(&t1));
    let t2 = codeflow(dir.path(), &["task", "new", "--epic", "EPC-001", "Second"]);
    assert!(stdout(&t2).contains("TSK-002"), "{}", stdout(&t2));

    // Task ids remain globally independent of their parent epic.
    let other = codeflow(dir.path(), &["task", "new", "--epic", "EPC-002", "Other"]);
    assert!(stdout(&other).contains("TSK-003"), "{}", stdout(&other));

    let task_path = dir.path().join("project-management/tasks/TSK-001.md");
    let body = std::fs::read_to_string(&task_path).unwrap();
    assert!(
        body.contains("epic_id: EPC-001"),
        "links parent epic: {body}"
    );
    assert!(!body.contains("{{"), "no placeholder survives: {body}");
}

#[test]
fn task_new_rejects_unknown_epic() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());

    let output = codeflow(dir.path(), &["task", "new", "--epic", "EPC-404", "x"]);
    assert_eq!(output.status.code(), Some(1), "unknown epic must fail");
    assert!(stderr(&output).contains("not found"), "{}", stderr(&output));
}

#[test]
fn task_new_rejects_self_authorizing_task_target() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    codeflow(dir.path(), &["epic", "new", "Anchored work"]);

    let output = codeflow(
        dir.path(),
        &[
            "task",
            "new",
            "--epic",
            "EPC-001",
            "--into",
            "task/TSK-001-work",
            "Work",
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("stable non-task branch name"),
        "{}",
        stderr(&output)
    );
    assert!(!dir
        .path()
        .join("project-management/tasks/TSK-001.md")
        .exists());
}

#[test]
fn task_new_rejects_a_missing_integration_target() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    codeflow(dir.path(), &["epic", "new", "Anchored work"]);

    let output = codeflow(
        dir.path(),
        &[
            "task",
            "new",
            "--epic",
            "EPC-001",
            "--into",
            "integration/EPC-001-missing",
            "Work",
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("does not resolve"),
        "{}",
        stderr(&output)
    );
    assert!(!dir
        .path()
        .join("project-management/tasks/TSK-001.md")
        .exists());
}

#[test]
fn spec_new_allocates_and_links_from_its_consumer() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    codeflow(dir.path(), &["epic", "new", "Contracted work"]);

    let output = codeflow(
        dir.path(),
        &["spec", "new", "--for", "EPC-001", "Public contract"],
    );
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(stdout(&output).contains("SPC-001"), "{}", stdout(&output));
    let spec =
        std::fs::read_to_string(dir.path().join("project-management/specs/SPC-001.md")).unwrap();
    assert!(spec.contains("id: SPC-001"), "{spec}");
    assert!(!spec.contains("format_id:"), "{spec}");
    let epic =
        std::fs::read_to_string(dir.path().join("project-management/epics/EPC-001.md")).unwrap();
    assert!(epic.contains("specs: [SPC-001]"), "{epic}");
}

/// TSK-103 AC-8: one spec, several consumers, each written in the same
/// change; the spec keeps no consumer list; a missing consumer writes none.
#[test]
fn spec_new_links_every_consumer_or_none() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    codeflow(dir.path(), &["epic", "new", "Contracted work"]);
    codeflow(
        dir.path(),
        &[
            "task",
            "new",
            "--standalone-reason",
            "a one-off",
            "Side work",
        ],
    );
    let output = codeflow(
        dir.path(),
        &[
            "spec",
            "new",
            "--for",
            "EPC-001",
            "--for",
            "TSK-001",
            "Shared contract",
        ],
    );
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let read = |path: &str| std::fs::read_to_string(dir.path().join(path)).unwrap();
    assert!(read("project-management/epics/EPC-001.md").contains("specs: [SPC-001]"));
    assert!(read("project-management/tasks/TSK-001.md").contains("specs: [SPC-001]"));
    let spec = read("project-management/specs/SPC-001.md");
    assert!(
        !spec.contains("EPC-001") && !spec.contains("TSK-001"),
        "{spec}"
    );

    let before_epic = read("project-management/epics/EPC-001.md");
    let output = codeflow(
        dir.path(),
        &["spec", "new", "--for", "EPC-001,TSK-404", "Half linked"],
    );
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(read("project-management/epics/EPC-001.md"), before_epic);
    assert!(!dir
        .path()
        .join("project-management/specs/SPC-002.md")
        .exists());
}

#[test]
fn work_start_proves_a_merged_planning_anchor_without_mutation() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    codeflow(dir.path(), &["epic", "new", "Anchored work"]);
    codeflow(
        dir.path(),
        &["task", "new", "--epic", "EPC-001", "Implement"],
    );
    git(dir.path(), &["add", "project-management"]);
    git(dir.path(), &["commit", "-m", "plan: anchor durable task"]);
    git(dir.path(), &["switch", "-c", "task/TSK-001-implement"]);

    let before = codeflow(dir.path(), &["status"]);
    let output = codeflow(dir.path(), &["work", "start", "TSK-001"]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(
        stdout(&output).contains("TSK-001 anchored"),
        "{}",
        stdout(&output)
    );
    let after = codeflow(dir.path(), &["status"]);
    assert_eq!(stdout(&before), stdout(&after), "work start is read-only");
    let porcelain = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(porcelain.stdout.is_empty());
}

/// Make `remote` a configured remote and `main`'s upstream.
fn set_upstream(dir: &Path, remote: &str) {
    git(
        dir,
        &["config", &format!("remote.{remote}.url"), "/nowhere"],
    );
    git(
        dir,
        &[
            "config",
            "--replace-all",
            &format!("remote.{remote}.fetch"),
            &format!("+refs/heads/*:refs/remotes/{remote}/*"),
        ],
    );
    git(dir, &["config", "branch.main.remote", remote]);
    git(dir, &["config", "branch.main.merge", "refs/heads/main"]);
}

fn rev_parse(dir: &Path, rev: &str) -> String {
    let out = Command::new("git")
        .args(["rev-parse", rev])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn work_start_anchors_on_the_tracking_ref_past_a_stale_local_target() {
    // The planning record landed on the remote; local `main` was never
    // fast-forwarded. Anchoring on it would miss the record.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    git(dir.path(), &["switch", "-c", "plan/anchor"]);
    codeflow(dir.path(), &["epic", "new", "Anchored work"]);
    codeflow(
        dir.path(),
        &[
            "task",
            "new",
            "--epic",
            "EPC-001",
            "--into",
            "main",
            "Implement",
        ],
    );
    git(dir.path(), &["add", "project-management"]);
    git(dir.path(), &["commit", "-m", "plan: anchor durable task"]);
    git(
        dir.path(),
        &["update-ref", "refs/remotes/origin/main", "HEAD"],
    );
    git(dir.path(), &["switch", "-c", "task/TSK-001-implement"]);

    // AC-6: without an upstream, the newer fetched origin target is used.
    let output = codeflow(dir.path(), &["work", "start", "TSK-001"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("refs/remotes/origin/main"),
        "{}",
        stdout(&output)
    );

    // A fork: `main` tracks `upstream/main`, which lacks the planning. The
    // fork's newer `origin/main` is not the target.
    set_upstream(dir.path(), "upstream");
    let base = rev_parse(dir.path(), "main");
    git(
        dir.path(),
        &["update-ref", "refs/remotes/upstream/main", &base],
    );
    let output = codeflow(dir.path(), &["work", "start", "TSK-001"]);
    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    assert!(
        !stderr(&output).contains("anchoring on"),
        "{}",
        stderr(&output)
    );

    // `main` tracks `origin/main`, where the planning landed.
    set_upstream(dir.path(), "origin");
    let output = codeflow(dir.path(), &["work", "start", "TSK-001"]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(
        stdout(&output).contains("-> refs/remotes/origin/main"),
        "{}",
        stdout(&output)
    );
    assert!(
        stderr(&output).contains("behind its upstream 'refs/remotes/origin/main'"),
        "{}",
        stderr(&output)
    );

    // Local `main` gains a commit the remote lacks: diverged, refused.
    git(dir.path(), &["switch", "main"]);
    git(
        dir.path(),
        &["commit", "--allow-empty", "-m", "chore: local only"],
    );
    git(dir.path(), &["switch", "task/TSK-001-implement"]);
    let output = codeflow(dir.path(), &["work", "start", "TSK-001"]);
    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    assert!(
        stderr(&output).contains("'main' and 'refs/remotes/origin/main' have diverged"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn work_start_rejects_an_invalid_visible_workgraph() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    codeflow(
        dir.path(),
        &[
            "task",
            "new",
            "--standalone-reason",
            "bounded repair",
            "Repair",
        ],
    );
    git(dir.path(), &["add", "project-management"]);
    git(dir.path(), &["commit", "-m", "plan: anchor repair task"]);
    git(dir.path(), &["switch", "-c", "task/TSK-001-repair"]);

    write(
        dir.path(),
        "project-management/epics/EPC-999.md",
        "---\nid: EPC-998\ntitle: mismatch\nstatus: planning\nwork_type: feat\ncreated: 2026-07-29\n---\n\n## Summary\nMismatch.\n\n## Acceptance Criteria\n- AC-1 fixed\n",
    );

    let output = codeflow(dir.path(), &["work", "start", "TSK-001"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("current workgraph is invalid"),
        "{}",
        stderr(&output)
    );
    assert!(
        stderr(&output).contains("EPC-999.md"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn task_new_requires_a_parent_or_explicit_standalone_reason() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    let missing = codeflow(dir.path(), &["task", "new", "Orphan"]);
    assert_eq!(missing.status.code(), Some(2));

    let standalone = codeflow(
        dir.path(),
        &[
            "task",
            "new",
            "--standalone-reason",
            "one bounded durable correction",
            "Bounded correction",
        ],
    );
    assert_eq!(
        standalone.status.code(),
        Some(0),
        "stderr: {}",
        stderr(&standalone)
    );
    let body =
        std::fs::read_to_string(dir.path().join("project-management/tasks/TSK-001.md")).unwrap();
    assert!(body.contains("epic_id: null"), "{body}");
    assert!(
        body.contains("standalone_reason: \"one bounded durable correction\""),
        "{body}"
    );
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
