//! Merge candidates keep both refs fixed until the gate passes.

use codeflow_core::integrate::{integrate_with_mode, IntegrateError, IntegrateMode};
use std::path::Path;

fn git(root: &Path, args: &[&str]) -> String {
    let output = codeflow_core::git::command()
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn commit(root: &Path, file: &str, content: &str) {
    std::fs::write(root.join(file), content).unwrap();
    git(root, &["add", file]);
    git(root, &["commit", "-m", "test: change fixture"]);
}

fn fixture(conflict: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.name", "Test"]);
    git(root, &["config", "user.email", "test@example.test"]);
    commit(root, "base.txt", "base\n");
    git(root, &["checkout", "-b", "feat/reviewed"]);
    commit(root, "feature.txt", "reviewed\n");
    if conflict {
        commit(root, "base.txt", "source edit\n");
    }
    git(root, &["checkout", "main"]);
    commit(root, "base.txt", "line edit\n");
    dir
}

fn gate(root: &Path, command: &str) {
    std::fs::create_dir_all(root.join(".codeflow")).unwrap();
    let config = serde_json::json!({
        "schema_version": "1.0",
        "targets": [{"name": "merge-gate", "runner": "custom", "modes": {
            "full": {"command": command}
        }}]
    });
    std::fs::write(root.join(".codeflow/test-config.json"), config.to_string()).unwrap();
}

#[test]
fn merge_gates_both_parents_without_moving_either_ref() {
    let dir = fixture(false);
    let root = dir.path();
    let line = git(root, &["rev-parse", "main"]);
    let source = git(root, &["rev-parse", "feat/reviewed"]);
    gate(root, &format!(
        "test $(git rev-parse HEAD^1) = {line} && test $(git rev-parse HEAD^2) = {source} && test $(git rev-parse main) = {line} && test $(git rev-parse feat/reviewed) = {source} && git ls-files --error-unmatch feature.txt base.txt"
    ));
    integrate_with_mode(root, "feat/reviewed", "main", IntegrateMode::Merge).unwrap();
    assert_eq!(git(root, &["rev-parse", "main^1"]), line);
    assert_eq!(git(root, &["rev-parse", "main^2"]), source);
    assert_eq!(git(root, &["rev-parse", "feat/reviewed"]), source);
    assert_eq!(git(root, &["branch", "--show-current"]), "main");
}

#[test]
fn merge_adds_a_merge_commit_even_when_fast_forward_is_possible() {
    let dir = fixture(false);
    let root = dir.path();
    git(root, &["branch", "land", "feat/reviewed^1"]);
    let line = git(root, &["rev-parse", "land"]);
    let source = git(root, &["rev-parse", "feat/reviewed"]);
    integrate_with_mode(root, "feat/reviewed", "land", IntegrateMode::Merge).unwrap();
    assert_eq!(git(root, &["rev-parse", "land^1"]), line);
    assert_eq!(git(root, &["rev-parse", "land^2"]), source);
    assert_eq!(git(root, &["rev-parse", "feat/reviewed"]), source);
}

#[test]
fn merge_of_an_already_contained_head_keeps_the_target_tip() {
    let dir = fixture(false);
    let root = dir.path();
    git(root, &["branch", "feat/contained", "main^1"]);
    let line = git(root, &["rev-parse", "main"]);
    let outcome =
        integrate_with_mode(root, "feat/contained", "main", IntegrateMode::Merge).unwrap();
    assert_eq!(git(root, &["rev-parse", "main"]), line);
    assert_eq!(outcome.commits_landed, 0);
    assert_eq!(git(root, &["branch", "--show-current"]), "main");
}

#[test]
fn red_merge_gate_restores_checkout_and_preserves_both_refs() {
    let dir = fixture(false);
    let root = dir.path();
    let line = git(root, &["rev-parse", "main"]);
    let source = git(root, &["rev-parse", "feat/reviewed"]);
    git(root, &["checkout", "feat/reviewed"]);
    gate(root, "exit 7");
    let error =
        integrate_with_mode(root, "feat/reviewed", "main", IntegrateMode::Merge).unwrap_err();
    assert!(matches!(error, IntegrateError::TestGateFailed { .. }));
    assert!(error.to_string().contains("exit 7"));
    assert_eq!(git(root, &["rev-parse", "main"]), line);
    assert_eq!(git(root, &["rev-parse", "feat/reviewed"]), source);
    assert_eq!(git(root, &["branch", "--show-current"]), "feat/reviewed");
    assert!(git(root, &["status", "--porcelain", "--untracked-files=no"]).is_empty());
}

#[test]
fn merge_conflict_aborts_before_gate_and_preserves_both_refs() {
    let dir = fixture(true);
    let root = dir.path();
    let line = git(root, &["rev-parse", "main"]);
    let source = git(root, &["rev-parse", "feat/reviewed"]);
    gate(root, "echo ran > gate-ran.txt");
    let error =
        integrate_with_mode(root, "feat/reviewed", "main", IntegrateMode::Merge).unwrap_err();
    assert!(matches!(error, IntegrateError::CandidateMergeFailed { .. }));
    assert!(error.to_string().contains("base.txt"));
    assert!(error.to_string().contains("re-run integrate"));
    assert_eq!(git(root, &["rev-parse", "main"]), line);
    assert_eq!(git(root, &["rev-parse", "feat/reviewed"]), source);
    assert_eq!(git(root, &["branch", "--show-current"]), "main");
    assert!(!root.join("gate-ran.txt").exists());
    assert_eq!(
        git2::Repository::open(root).unwrap().state(),
        git2::RepositoryState::Clean
    );
    assert!(git(root, &["status", "--porcelain", "--untracked-files=no"]).is_empty());
}

#[test]
fn merge_refuses_source_moved_during_gate() {
    let dir = fixture(false);
    let root = dir.path();
    let line = git(root, &["rev-parse", "main"]);
    gate(
        root,
        "git update-ref refs/heads/feat/reviewed refs/heads/main",
    );
    let error =
        integrate_with_mode(root, "feat/reviewed", "main", IntegrateMode::Merge).unwrap_err();
    assert!(error.to_string().contains("moved during integrate"));
    assert_eq!(git(root, &["rev-parse", "main"]), line);
    assert_eq!(git(root, &["branch", "--show-current"]), "main");
}

#[test]
fn merge_refuses_candidate_moved_during_gate() {
    let dir = fixture(false);
    let root = dir.path();
    let line = git(root, &["rev-parse", "main"]);
    // Change only HEAD, leaving the candidate files intact so the gate's
    // file-generation guard does not reject the mutation first.
    gate(root, "git update-ref HEAD HEAD^1");
    let error =
        integrate_with_mode(root, "feat/reviewed", "main", IntegrateMode::Merge).unwrap_err();
    assert!(error.to_string().contains("candidate HEAD moved"));
    assert_eq!(git(root, &["rev-parse", "main"]), line);
}

#[test]
fn merge_refuses_target_moved_during_gate() {
    let dir = fixture(false);
    let root = dir.path();
    let source = git(root, &["rev-parse", "feat/reviewed"]);
    gate(
        root,
        "git update-ref refs/heads/main refs/heads/feat/reviewed",
    );
    let error =
        integrate_with_mode(root, "feat/reviewed", "main", IntegrateMode::Merge).unwrap_err();
    assert!(matches!(error, IntegrateError::MergeFailed { .. }));
    assert_eq!(
        git(root, &["rev-parse", "main"]),
        source,
        "the concurrent move is never overwritten"
    );
}

#[test]
fn merge_lands_from_linked_worktree_and_refreshes_target_checkout() {
    let dir = fixture(false);
    let parent = tempfile::tempdir().unwrap();
    let linked = parent.path().join("source");
    git(
        dir.path(),
        &["worktree", "add", linked.to_str().unwrap(), "feat/reviewed"],
    );
    let source = git(dir.path(), &["rev-parse", "feat/reviewed"]);
    let result =
        integrate_with_mode(&linked, "feat/reviewed", "main", IntegrateMode::Merge).unwrap();
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    assert_eq!(git(dir.path(), &["rev-parse", "main^2"]), source);
    assert_eq!(git(&linked, &["branch", "--show-current"]), "feat/reviewed");
    assert!(dir.path().join("feature.txt").exists());
    assert!(git(
        dir.path(),
        &["status", "--porcelain", "--untracked-files=no"]
    )
    .is_empty());
}
