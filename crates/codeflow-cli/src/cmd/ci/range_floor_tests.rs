use super::*;
use git2::{Repository, Signature};
use serde_json::json;

#[derive(clap::Parser)]
struct Options {
    #[command(flatten)]
    ci: CiArgs,
}

fn fixture(stale: bool) -> (tempfile::TempDir, tempfile::TempDir, git2::Oid) {
    fixture_with(stale, true)
}

/// `remote_head`: whether the clone records the remote's default branch, as
/// `git clone` does; a hosted `actions/checkout` with fetch-depth 0 leaves
/// the tracking refs without it.
fn fixture_with(
    stale: bool,
    remote_head: bool,
) -> (tempfile::TempDir, tempfile::TempDir, git2::Oid) {
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
    if remote_head {
        repo.reference_symbolic(
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
            true,
            "fixture",
        )
        .unwrap();
    }
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
    let options = Options::parse_from(["test", "--branch", "task/check"]);
    let (policy, _) = range_policy(dir.path(), &options.ci, &BTreeMap::new()).unwrap();
    assert_eq!(policy.git.protected_branches, ["main"]);
    assert_eq!(policy.git.branch_prefixes, ["task/"]);
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

/// A protected entry the checkout adds stays in the union, which only adds
/// prohibitions, and never reaches what grants trust or picks a range base
/// (TSK-1078 review F1).
#[test]
fn range_floor_added_protected_entry_grants_no_trust() {
    use clap::Parser;
    for stale in [false, true] {
        let (dir, _remote, _) = fixture(stale);
        std::fs::write(
            dir.path().join(".codeflow/policy.json"),
            json!({"git":{"protected_branches":["main", "integration/EPC-999-unbound"],
            "branch_prefixes":["task/"]}})
            .to_string(),
        )
        .unwrap();
        let options = Options::parse_from(["test", "--branch", "task/check"]);
        let (policy, _) = range_policy(dir.path(), &options.ci, &BTreeMap::new()).unwrap();
        assert_eq!(
            policy.git.protected_branches,
            ["main", "integration/EPC-999-unbound"]
        );
        assert_eq!(policy.git.trusted_protected_branches(), ["main"]);
        assert!(!policy
            .git
            .branch_is_landed_protected("integration/EPC-999-unbound"));
    }
}

#[test]
fn range_floor_unreadable_authority_refuses_range_selection() {
    let (dir, _remote, _) = fixture(false);
    std::fs::write(
        dir.path().join(".git/refs/remotes/origin/main"),
        format!("{}\n", "1".repeat(40)),
    )
    .unwrap();
    let options = <Options as clap::Parser>::parse_from(["test", "--branch", "task/check"]);
    assert!(range_policy(dir.path(), &options.ci, &BTreeMap::new()).is_err());
}

/// A hosted checkout (`actions/checkout` with fetch-depth 0) has the
/// remote's tracking refs but no remote default branch. The floor is then
/// read at the commit the run already pins: `--into`, `--base`,
/// `--policy-from` or the base the CI event names (TSK-1078). With nothing
/// pinned, the unset remote default still refuses.
#[test]
fn range_floor_hosted_clone_without_remote_head_reads_the_pinned_commit() {
    use clap::Parser;
    let (dir, _remote, _) = fixture_with(true, false);
    let none = BTreeMap::new();
    let event = BTreeMap::from([("GITHUB_BASE_REF".to_string(), "main".to_string())]);
    for (flags, environment) in [
        (&["--into", "main"][..], &none),
        (&["--base", "origin/main"][..], &none),
        (&["--policy-from", "origin/main"][..], &none),
        (&[][..], &event),
    ] {
        let mut argv = vec!["test", "--branch", "task/check"];
        argv.extend_from_slice(flags);
        let options = Options::parse_from(argv);
        let (policy, _) = range_policy(dir.path(), &options.ci, environment)
            .unwrap_or_else(|error| panic!("{flags:?}: {error}"));
        assert_eq!(policy.git.protected_branches, ["main"], "{flags:?}");
        assert_eq!(policy.git.branch_prefixes, ["task/"], "{flags:?}");
    }
    let unpinned = Options::parse_from(["test", "--branch", "task/check"]);
    assert!(range_policy(dir.path(), &unpinned.ci, &none).is_err());
}
