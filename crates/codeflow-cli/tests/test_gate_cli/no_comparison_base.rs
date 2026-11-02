//! TSK-1039: a quick gate with no comparison base runs every target and
//! says why. No local record earns selection credit (ADR-0089), so the
//! output says local narrowing is unavailable and never recommends `--since`.

use super::{git, repo, run, stderr};
use std::path::Path;

// The targets append with `echo name>>runs`: under `cmd` on Windows,
// `echo name >> runs` would write the name with a trailing space.
const CONFIG: &str = r#"{"schema_version":"1.0","targets":[
  {"name":"alpha","runner":"custom","narrow":["a/**"],"modes":{"quick":{"command":"echo alpha>>runs"}}},
  {"name":"beta","runner":"custom","narrow":["b/**"],"modes":{"quick":{"command":"echo beta>>runs"}}},
  {"name":"gamma","runner":"custom","narrow":["c/**"],"modes":{"quick":{"command":"echo gamma>>runs"}}}]}"#;

fn committed_repo() -> tempfile::TempDir {
    let dir = repo(CONFIG);
    std::fs::write(dir.path().join(".gitignore"), "target/\nruns\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "test: three narrow targets"]);
    dir
}

fn task_record(dir: &Path, target: Option<&str>) {
    let tasks = dir.join("project-management/tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    let target = target.map_or(String::new(), |t| format!("integration_target: {t}\n"));
    std::fs::write(
        tasks.join("TSK-001.md"),
        format!("---\nid: TSK-001\nepic_id: null\nstandalone_reason: \"x\"\n{target}title: t\nstatus: todo\nwork_type: fix\nspecs: []\ndepends_on: []\ncreated: 2026-11-02\n---\n\n## Description\nx\n\n## Acceptance Criteria\n- AC-1 x\n"),
    )
    .unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "docs: add the task record"]);
}

fn assert_every_target_runs_with_the_reason(dir: &Path, case: &str) {
    let home = tempfile::tempdir().unwrap();
    let output = run(dir, home.path(), &["test", "--mode", "quick"]);
    let text = stderr(&output);
    assert!(output.status.success(), "{case}: {text}");
    assert!(
        text.contains("every target: local narrowing is unavailable, since no local run record earns selection credit (ADR-0089)"),
        "{case}: {text}"
    );
    assert!(
        !text.contains("--since"),
        "{case}: recommends --since: {text}"
    );
    let runs = std::fs::read_to_string(dir.join("runs")).unwrap();
    for target in ["alpha", "beta", "gamma"] {
        assert!(runs.lines().any(|l| l == target), "{case}: {runs}");
    }
    std::fs::remove_file(dir.join("runs")).unwrap();
}

#[test]
fn a_branch_with_no_task_runs_every_target_and_recommends_no_since() {
    let dir = committed_repo();
    git(dir.path(), &["switch", "-c", "plan/some-plan"]);
    assert_every_target_runs_with_the_reason(dir.path(), "plan branch");
    git(dir.path(), &["switch", "-c", "chore/housekeeping"]);
    assert_every_target_runs_with_the_reason(dir.path(), "chore branch");
}

#[test]
fn a_detached_head_runs_every_target_and_recommends_no_since() {
    let dir = committed_repo();
    git(dir.path(), &["switch", "--detach", "HEAD"]);
    assert_every_target_runs_with_the_reason(dir.path(), "detached head");
}

#[test]
fn a_task_with_no_declared_target_runs_every_target_and_recommends_no_since() {
    let dir = committed_repo();
    git(dir.path(), &["switch", "-c", "task/TSK-001-fix"]);
    task_record(dir.path(), None);
    assert_every_target_runs_with_the_reason(dir.path(), "task without a target");
    let declared = committed_repo();
    git(declared.path(), &["switch", "-c", "task/TSK-001-fix"]);
    task_record(declared.path(), Some("main"));
    assert_every_target_runs_with_the_reason(declared.path(), "task with a target");
}

#[test]
fn a_given_since_still_runs_every_target_and_says_it_narrows_nothing() {
    let dir = committed_repo();
    git(dir.path(), &["switch", "-c", "plan/some-plan"]);
    std::fs::create_dir_all(dir.path().join("a")).unwrap();
    std::fs::write(dir.path().join("a/x"), "x").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "test: change a"]);
    let home = tempfile::tempdir().unwrap();
    let output = run(
        dir.path(),
        home.path(),
        &["test", "--mode", "quick", "--since", "HEAD~1"],
    );
    let text = stderr(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("--since HEAD~1 narrows nothing; local narrowing is unavailable"),
        "{text}"
    );
    let runs = std::fs::read_to_string(dir.path().join("runs")).unwrap();
    assert_eq!(runs.lines().count(), 3, "{runs}");
}

#[test]
fn a_shallow_clone_runs_every_target_and_recommends_no_since() {
    let source = committed_repo();
    let parent = tempfile::tempdir().unwrap();
    let url = format!("file://{}", source.path().display());
    codeflow_fixture::clone(parent.path(), &url, "shallow")
        .depth(1)
        .run();
    let clone = parent.path().join("shallow");
    assert_every_target_runs_with_the_reason(&clone, "shallow clone");
}
