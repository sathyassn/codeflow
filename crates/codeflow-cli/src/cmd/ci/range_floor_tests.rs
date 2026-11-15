use super::*;
use git2::{Repository, Signature};
use serde_json::json;

#[derive(clap::Parser)]
struct Options {
    #[command(flatten)]
    ci: CiArgs,
}

fn fixture(stale: bool) -> (tempfile::TempDir, tempfile::TempDir, git2::Oid) {
    let dir = tempfile::tempdir().unwrap();
    let remote = tempfile::tempdir().unwrap();
    Repository::init_bare(remote.path()).unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    repo.set_head("refs/heads/task/check").unwrap();
    std::fs::create_dir(dir.path().join(".codeflow")).unwrap();
    std::fs::create_dir_all(dir.path().join("project-management/tasks")).unwrap();
    std::fs::write(dir.path().join("project-management/tasks/TSK-001.md"),
        "---\nid: TSK-001\nuid: 00000000-0000-4000-8000-000000000001\nepic_id: null\nstandalone_reason: fixture\nintegration_target: main\ntitle: fixture\nstatus: todo\nwork_type: fix\nspecs: []\ndepends_on: []\ncreated: 2026-11-14\n---\n").unwrap();
    let path = dir.path().join(".codeflow/policy.json");
    let commit = || {
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(".codeflow/policy.json")).unwrap();
        index
            .add_path(Path::new("project-management/tasks/TSK-001.md"))
            .unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = Signature::now("Test", "test@example.invalid").unwrap();
        let parent = repo.head().ok().and_then(|head| head.peel_to_commit().ok());
        let parents: Vec<_> = parent.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, "test: seed", &tree, &parents)
            .unwrap()
    };
    std::fs::write(
        &path,
        json!({"git":{"protected_branches":["main"], "branch_prefixes":["task/"]}}).to_string(),
    )
    .unwrap();
    let base = commit();
    repo.remote("origin", remote.path().to_str().unwrap())
        .unwrap();
    repo.reference("refs/remotes/origin/main", base, true, "fixture")
        .unwrap();
    repo.reference_symbolic(
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
        true,
        "fixture",
    )
    .unwrap();
    std::fs::write(
        &path,
        json!({"git":{"protected_branches":[], "branch_prefixes":["task/", "loose/"]}}).to_string(),
    )
    .unwrap();
    if stale {
        commit();
    }
    (dir, remote, base)
}

fn check_range(stale: bool) {
    use clap::Parser;
    let (dir, _remote, base) = fixture(stale);
    let (policy, _) = range_policy(dir.path()).unwrap();
    assert_eq!(policy.git.protected_branches, ["main"]);
    assert_eq!(policy.git.branch_prefixes, ["task/"]);
    let options = Options::parse_from(["test", "--branch", "task/check"]);
    let context = context::resolve(
        dir.path(),
        &options.ci,
        &BTreeMap::new(),
        &policy.git.protected_branches,
        &policy.git.branch_prefixes,
    )
    .unwrap();
    assert_eq!(context.base.as_deref(), Some(base.to_string().as_str()));
    // The checkout's added prefix really would admit a declared target.
    let head = context.head.as_str();
    let branch = "loose/TSK-001-check";
    assert!(matches!(
        context::judged_declared_target(
            dir.path(),
            branch,
            head,
            &policy.git.protected_branches,
            &["task/".into(), "loose/".into()]
        )
        .unwrap(),
        Some(Declared::Eligible(_))
    ));
    assert!(context::judged_declared_target(
        dir.path(),
        branch,
        head,
        &policy.git.protected_branches,
        &policy.git.branch_prefixes
    )
    .unwrap()
    .is_none());
}

#[test]
fn range_floor_stale_checkout_cannot_drop_protection_or_add_prefix() {
    check_range(true);
}

#[test]
fn range_floor_uncommitted_policy_cannot_drop_protection_or_add_prefix() {
    check_range(false);
}

#[test]
fn range_floor_unreadable_authority_refuses_range_selection() {
    let (dir, _remote, _) = fixture(false);
    std::fs::write(
        dir.path().join(".git/refs/remotes/origin/main"),
        format!("{}\n", "1".repeat(40)),
    )
    .unwrap();
    assert!(range_policy(dir.path()).is_err());
}
