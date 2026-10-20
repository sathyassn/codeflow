//! Merge candidates keep both refs fixed until the gate passes.

use codeflow_core::integrate::{integrate_with_mode, IntegrateError, IntegrateMode};
use std::path::{Path, PathBuf};

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

struct Fixture {
    dir: tempfile::TempDir,
    linked: PathBuf,
}

impl Fixture {
    fn path(&self) -> &Path {
        &self.linked
    }
}

fn fixture(conflict: bool) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-b", "main"]);
    // The helper ignores host Git config; integrate's Git processes inherit it.
    // Pin Windows checkout rules locally so both use the same conversion, and
    // exercise CRLF restoration on every platform.
    git(root, &["config", "core.autocrlf", "true"]);
    git(root, &["config", "core.filemode", "false"]);
    git(root, &["config", "user.name", "Test"]);
    git(root, &["config", "user.email", "test@example.test"]);
    commit(root, ".gitignore", ".worktrees/\n");
    commit(root, "base.txt", "base\n");
    git(root, &["checkout", "-b", "feat/reviewed"]);
    commit(root, "feature.txt", "reviewed\n");
    if conflict {
        commit(root, "base.txt", "source edit\n");
    }
    git(root, &["checkout", "main"]);
    commit(root, "base.txt", "line edit\n");
    let linked = root.join(".worktrees/integrator");
    git(
        root,
        &[
            "worktree",
            "add",
            "-b",
            "chore/integrator",
            linked.to_str().unwrap(),
        ],
    );
    Fixture { dir, linked }
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
    gate(
        root,
        "git rev-parse main feat/reviewed HEAD > gated-refs.txt && git ls-files --error-unmatch feature.txt base.txt",
    );
    integrate_with_mode(root, "feat/reviewed", "main", IntegrateMode::Merge).unwrap();
    let landed = git(root, &["rev-parse", "main"]);
    let gated = std::fs::read_to_string(root.join("gated-refs.txt")).unwrap();
    assert_eq!(
        gated.lines().collect::<Vec<_>>(),
        [line.as_str(), source.as_str(), landed.as_str()]
    );
    assert_eq!(git(root, &["rev-parse", "main^1"]), line);
    assert_eq!(git(root, &["rev-parse", "main^2"]), source);
    assert_eq!(
        git(root, &["log", "-1", "--format=%s", "main"]),
        "Merge branch 'feat/reviewed' into main"
    );
    assert_eq!(git(root, &["rev-parse", "feat/reviewed"]), source);
    assert_eq!(git(root, &["branch", "--show-current"]), "chore/integrator");
}

#[test]
fn merge_adds_a_merge_commit_even_when_fast_forward_is_possible() {
    let dir = fixture(false);
    let root = dir.path();
    git(root, &["branch", "land", "feat/reviewed^1"]);
    let line = git(root, &["rev-parse", "land"]);
    let source = git(root, &["rev-parse", "feat/reviewed"]);
    gate(root, "git ls-files --error-unmatch base.txt feature.txt");
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
    gate(root, "git ls-files --error-unmatch base.txt");
    let outcome =
        integrate_with_mode(root, "feat/contained", "main", IntegrateMode::Merge).unwrap();
    assert_eq!(git(root, &["rev-parse", "main"]), line);
    assert_eq!(outcome.commits_landed, 0);
    assert_eq!(git(root, &["branch", "--show-current"]), "chore/integrator");
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
    assert_eq!(git(root, &["branch", "--show-current"]), "chore/integrator");
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
    assert_eq!(git(root, &["branch", "--show-current"]), "chore/integrator");
}

#[test]
fn merge_refuses_candidate_moved_during_gate() {
    let dir = fixture(false);
    let root = dir.path();
    let line = git(root, &["rev-parse", "main"]);
    // Change only HEAD, leaving the candidate files intact so the gate's
    // file-generation guard does not reject the mutation first.
    gate(root, "git update-ref HEAD \"HEAD^1\"");
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
        dir.dir.path(),
        &["worktree", "add", linked.to_str().unwrap(), "feat/reviewed"],
    );
    let source = git(dir.dir.path(), &["rev-parse", "feat/reviewed"]);
    gate(&linked, "git ls-files --error-unmatch base.txt feature.txt");
    let result =
        integrate_with_mode(&linked, "feat/reviewed", "main", IntegrateMode::Merge).unwrap();
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    assert_eq!(git(dir.dir.path(), &["rev-parse", "main^2"]), source);
    assert_eq!(git(&linked, &["branch", "--show-current"]), "feat/reviewed");
    assert!(dir.dir.path().join("feature.txt").exists());
    assert_eq!(
        std::fs::read(dir.dir.path().join("feature.txt")).unwrap(),
        b"reviewed\r\n",
        "the target refresh must exercise Git's CRLF conversion"
    );
    assert!(git(
        dir.dir.path(),
        &["status", "--porcelain", "--untracked-files=no"]
    )
    .is_empty());
}

#[test]
fn merge_commit_failure_does_not_suggest_resolving_a_conflict() {
    let dir = fixture(false);
    let root = dir.path();
    let line = git(root, &["rev-parse", "main"]);
    // An invalid signing program refuses commit creation without an unmerged index.
    git(root, &["config", "commit.gpgsign", "true"]);
    git(
        root,
        &[
            "config",
            "gpg.program",
            "missing-codeflow-test-signing-program",
        ],
    );
    let error =
        integrate_with_mode(root, "feat/reviewed", "main", IntegrateMode::Merge).unwrap_err();
    assert!(matches!(error, IntegrateError::CandidateMergeFailed { .. }));
    assert!(
        !error.to_string().contains("resolve on the source branch"),
        "{error}"
    );
    assert_eq!(git(root, &["rev-parse", "main"]), line);
    assert_eq!(git(root, &["branch", "--show-current"]), "chore/integrator");
    assert_eq!(
        git2::Repository::open(root).unwrap().state(),
        git2::RepositoryState::Clean
    );
}
