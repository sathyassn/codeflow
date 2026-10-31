//! TSK-1077: the default branch, resolved to one commit, is the only local
//! policy authority. No line supplies a policy, an exception or a level,
//! and no state is guessed: an unset remote `HEAD`, an empty tracking
//! namespace and an unreadable authority refuse with their remedy.
use codeflow_core::hooks::landed_policy;
use codeflow_core::hooks::path_policy::{self, PathPolicy};
use git2::{Oid, Repository};

const RECORD: &str = "project-management/tasks/TSK-007.md";

struct Fixture {
    dir: tempfile::TempDir,
    repo: Repository,
}

impl Fixture {
    /// A repository on `task/TSK-007-authority` with the remote `origin`
    /// at `url` and no tracking refs yet.
    fn new(url: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        repo.set_head("refs/heads/task/TSK-007-authority").unwrap();
        repo.remote("origin", url).unwrap();
        Self { dir, repo }
    }

    fn root(&self) -> &std::path::Path {
        self.dir.path()
    }

    fn put(&self, path: &str, mode: u32, contents: &[u8]) {
        let mut index = self.repo.index().unwrap();
        index
            .add(&git2::IndexEntry {
                ctime: git2::IndexTime::new(0, 0),
                mtime: git2::IndexTime::new(0, 0),
                dev: 0,
                ino: 0,
                mode,
                uid: 0,
                gid: 0,
                file_size: u32::try_from(contents.len()).unwrap(),
                id: self.repo.blob(contents).unwrap(),
                flags: 0,
                flags_extended: 0,
                path: path.as_bytes().to_vec(),
            })
            .unwrap();
        index.write().unwrap();
    }

    fn drop_path(&self, path: &str) {
        let mut index = self.repo.index().unwrap();
        index.remove_path(std::path::Path::new(path)).unwrap();
        index.write().unwrap();
    }

    /// Commit the index with `files` written into it first.
    fn commit(&self, files: &[(&str, &str)], parents: &[Oid]) -> Oid {
        for (path, text) in files {
            self.put(path, 0o100_644, text.as_bytes());
        }
        let tree = self
            .repo
            .find_tree(self.repo.index().unwrap().write_tree().unwrap())
            .unwrap();
        let sig = git2::Signature::now("Tester", "test@example.invalid").unwrap();
        let parents: Vec<_> = parents
            .iter()
            .map(|p| self.repo.find_commit(*p).unwrap())
            .collect();
        self.repo
            .commit(
                None,
                &sig,
                &sig,
                "chore: authority fixture",
                &tree,
                &parents.iter().collect::<Vec<_>>(),
            )
            .unwrap()
    }

    fn tracking(&self, name: &str, id: Oid) {
        self.repo
            .reference(&format!("refs/remotes/origin/{name}"), id, true, "fixture")
            .unwrap();
    }

    fn remote_head(&self, target: &str) {
        self.repo
            .reference_symbolic("refs/remotes/origin/HEAD", target, true, "fixture")
            .unwrap();
    }

    /// The task branch at `id`, with `id`'s files in the working copy's
    /// policy file too when it holds one.
    fn on_branch(&self, id: Oid) {
        self.repo
            .reference("refs/heads/task/TSK-007-authority", id, true, "fixture")
            .unwrap();
    }
}

fn record(target: &str) -> String {
    format!("---\nid: TSK-007\nintegration_target: \"{target}\"\n---\n")
}

const MAIN_POLICY: &str = r#"{"git":{"force_push_unprotected":"allow"}}"#;
const LINE_POLICY: &str = r#"{"git":{"force_push_unprotected":"block","blocked_names":[{"pattern":"*.pem","except_under":["fixtures"],"reason":"line exception"}]}}"#;

/// `main` with a plain policy and `integration/line` whose policy adds a
/// `*.pem` exception under `fixtures/` and a stricter level, with the task
/// record on the line declaring it, and remote `HEAD` set to `main`.
fn line_fixture() -> (Fixture, Oid, Oid) {
    let f = Fixture::new("https://example.invalid/repo.git");
    let main = f.commit(&[(".codeflow/policy.json", MAIN_POLICY)], &[]);
    let line = f.commit(
        &[
            (".codeflow/policy.json", LINE_POLICY),
            (RECORD, &record("integration/line")),
        ],
        &[main],
    );
    f.tracking("main", main);
    f.tracking("integration/line", line);
    f.remote_head("refs/remotes/origin/main");
    f.on_branch(line);
    (f, main, line)
}

/// AC-1: the line's exception and its stricter level are ignored; the
/// default branch alone decides. Before TSK-1077 both applied.
#[test]
fn a_line_exception_and_stricter_level_are_ignored() {
    let (f, main, _) = line_fixture();
    let authority = landed_policy::load(f.root()).unwrap();
    assert_eq!(
        authority.source, "refs/remotes/origin/main",
        "{}",
        authority.source
    );
    assert_eq!(
        authority.policy.git.force_push_unprotected,
        codeflow_core::hooks::PolicyLevel::Allow,
        "the line's stricter level is not the authority's"
    );
    let paths = PathPolicy::from_git(&authority.policy.git).unwrap();
    f.put("fixtures/cert.pem", 0o100_644, b"public");
    let report = path_policy::staged(&f.repo, &paths).unwrap();
    assert!(
        !report.violations.is_empty(),
        "the line's exception does not admit fixtures/cert.pem: {report:?}"
    );
    let diagnostic = landed_policy::diagnostic(f.root()).unwrap();
    assert!(
        diagnostic.contains(&format!("refs/remotes/origin/main at {main}")),
        "{diagnostic}"
    );
}

/// The live case of 2026-10-30: `main` and a line hold the task's record
/// with different targets (a retarget made on the line only). Before
/// TSK-1077 every guarded command refused because the records disagreed;
/// records are no longer read, so the default branch judges.
#[test]
fn records_that_disagree_on_the_target_no_longer_refuse() {
    let f = Fixture::new("https://example.invalid/repo.git");
    let main = f.commit(
        &[
            (".codeflow/policy.json", MAIN_POLICY),
            (RECORD, &record("integration/EPC-022-old")),
        ],
        &[],
    );
    let line = f.commit(
        &[
            (".codeflow/policy.json", LINE_POLICY),
            (RECORD, &record("integration/EPC-024-new")),
        ],
        &[main],
    );
    f.tracking("main", main);
    f.tracking("integration/EPC-022-old", main);
    f.tracking("integration/EPC-024-new", line);
    f.remote_head("refs/remotes/origin/main");
    f.on_branch(line);
    let authority = landed_policy::load(f.root()).unwrap();
    assert_eq!(authority.source, "refs/remotes/origin/main");
    assert_eq!(
        authority.policy.git.force_push_unprotected,
        codeflow_core::hooks::PolicyLevel::Allow
    );
}

/// `git.root_branch` keeps its root-checkout role and is no policy source.
#[test]
fn the_root_branch_is_no_policy_source() {
    let f = Fixture::new("https://example.invalid/repo.git");
    let main = f.commit(
        &[(
            ".codeflow/policy.json",
            r#"{"git":{"root_branch":"integration/root","force_push_unprotected":"allow"}}"#,
        )],
        &[],
    );
    let line = f.commit(&[(".codeflow/policy.json", LINE_POLICY)], &[main]);
    f.tracking("main", main);
    f.tracking("integration/root", line);
    f.remote_head("refs/remotes/origin/main");
    f.on_branch(main);
    let authority = landed_policy::load(f.root()).unwrap();
    assert_eq!(authority.source, "refs/remotes/origin/main");
    assert_eq!(
        authority.policy.git.force_push_unprotected,
        codeflow_core::hooks::PolicyLevel::Allow
    );
}

/// AC-2: with remote `HEAD` unset, a sole `origin/main` or `origin/master`
/// is not taken as the default branch; the refusal names
/// `git remote set-head origin -a`.
#[test]
fn an_unset_remote_head_refuses_instead_of_guessing() {
    for name in ["main", "master"] {
        let f = Fixture::new("https://example.invalid/repo.git");
        let tip = f.commit(&[(".codeflow/policy.json", MAIN_POLICY)], &[]);
        f.tracking(name, tip);
        f.on_branch(tip);
        let error = landed_policy::load(f.root())
            .err()
            .unwrap_or_else(|| panic!("a sole origin/{name} is not an authority"));
        assert!(error.contains("git remote set-head origin -a"), "{error}");
    }
}

/// AC-2: only a fully qualified branch of the remote is an authority:
/// remote `HEAD` naming a tag, a local branch, a short name or a ref that
/// is not symbolic refuses.
/// A case that points remote `HEAD` somewhere other than a tracking branch.
type Spoil = fn(&Fixture, Oid);

#[test]
fn only_a_tracking_branch_is_an_authority() {
    let cases: [(&str, Spoil); 4] = [
        ("a tag", |f, tip| {
            f.repo
                .reference("refs/tags/v1", tip, true, "fixture")
                .unwrap();
            f.remote_head("refs/tags/v1");
        }),
        ("a local branch", |f, tip| {
            f.repo
                .reference("refs/heads/develop", tip, true, "fixture")
                .unwrap();
            f.remote_head("refs/heads/develop");
        }),
        ("a short name", |f, _| {
            std::fs::write(
                f.repo.path().join("refs/remotes/origin/HEAD"),
                "ref: main\n",
            )
            .unwrap();
        }),
        ("a direct ref", |f, tip| {
            f.repo
                .reference("refs/remotes/origin/HEAD", tip, true, "fixture")
                .unwrap();
        }),
    ];
    for (what, set) in cases {
        let f = Fixture::new("https://example.invalid/repo.git");
        let tip = f.commit(&[(".codeflow/policy.json", MAIN_POLICY)], &[]);
        f.tracking("main", tip);
        f.on_branch(tip);
        set(&f, tip);
        let error = landed_policy::load(f.root())
            .err()
            .unwrap_or_else(|| panic!("remote HEAD naming {what} is not an authority"));
        assert!(
            error.contains("git remote set-head origin -a"),
            "{what}: {error}"
        );
    }
}

/// A repository at `path` holding one commit, or none when `empty`.
fn upstream(path: &std::path::Path, empty: bool) {
    let repo = Repository::init_bare(path).unwrap();
    if !empty {
        let tree = repo
            .find_tree(repo.treebuilder(None).unwrap().write().unwrap())
            .unwrap();
        let sig = git2::Signature::now("Tester", "test@example.invalid").unwrap();
        repo.commit(
            Some("refs/heads/main"),
            &sig,
            &sig,
            "chore: seed",
            &tree,
            &[],
        )
        .unwrap();
    }
}

/// AC-3: a remote that has refs while the local tracking namespace is
/// empty refuses with the fetch route; before TSK-1077 `HEAD` judged.
#[test]
fn an_empty_namespace_for_a_remote_with_refs_refuses_with_the_fetch_route() {
    let remote = tempfile::tempdir().unwrap();
    upstream(remote.path(), false);
    let f = Fixture::new(remote.path().to_str().unwrap());
    let tip = f.commit(&[(".codeflow/policy.json", MAIN_POLICY)], &[]);
    f.on_branch(tip);
    let error = landed_policy::load(f.root())
        .err()
        .expect("an empty namespace for a remote with refs refuses");
    assert!(error.contains("git fetch origin"), "{error}");
}

/// AC-3: a remote that advertises no refs is labelled initialization, and
/// no remote at all is labelled local only.
#[test]
fn initialization_and_local_only_are_labelled() {
    let remote = tempfile::tempdir().unwrap();
    upstream(remote.path(), true);
    let f = Fixture::new(remote.path().to_str().unwrap());
    let tip = f.commit(&[(".codeflow/policy.json", MAIN_POLICY)], &[]);
    f.on_branch(tip);
    let authority = landed_policy::load(f.root()).unwrap();
    assert!(
        authority.source.contains("initialization"),
        "{}",
        authority.source
    );
    f.repo.remote_delete("origin").unwrap();
    let authority = landed_policy::load(f.root()).unwrap();
    assert!(
        authority.source.contains("local only"),
        "{}",
        authority.source
    );
}

/// AC-3: a remote that cannot be asked refuses, never guessing that it is
/// empty.
#[test]
fn an_unreachable_remote_with_an_empty_namespace_refuses() {
    let missing = tempfile::tempdir().unwrap();
    let f = Fixture::new(missing.path().join("absent.git").to_str().unwrap());
    let tip = f.commit(&[(".codeflow/policy.json", MAIN_POLICY)], &[]);
    f.on_branch(tip);
    let error = landed_policy::load(f.root())
        .err()
        .expect("an unreachable remote is not an empty one");
    assert!(error.contains("git fetch origin"), "{error}");
}

/// The question to the remote runs no command the repository configures:
/// neither an `ext::` URL the configuration allows nor a configured upload
/// command. The guards ask it on every command while the namespace is
/// empty, so it must not be a way to run code.
#[cfg(unix)]
#[test]
fn asking_the_remote_runs_no_configured_command() {
    let remote = tempfile::tempdir().unwrap();
    upstream(remote.path(), true);
    let marker = tempfile::tempdir().unwrap();
    let ran = marker.path().join("ran");
    let shown = ran.to_str().unwrap();
    // A URL that runs a command, which the configuration allows.
    let f = Fixture::new(&format!("ext::sh -c touch% {shown}"));
    let mut config = f.repo.config().unwrap();
    config.set_str("protocol.ext.allow", "always").unwrap();
    let tip = f.commit(&[(".codeflow/policy.json", MAIN_POLICY)], &[]);
    f.on_branch(tip);
    let error = landed_policy::load(f.root())
        .err()
        .expect("a transport that runs a command is not asked");
    assert!(error.contains("git fetch origin"), "{error}");
    assert!(!ran.exists(), "the ext:: command ran");
    // A configured upload command on a reachable empty repository.
    f.repo.remote_delete("origin").unwrap();
    f.repo
        .remote("origin", remote.path().to_str().unwrap())
        .unwrap();
    config
        .set_str(
            "remote.origin.uploadpack",
            &format!("sh -c 'touch {shown}; exec git-upload-pack \"$@\"' upload"),
        )
        .unwrap();
    let authority = landed_policy::load(f.root()).unwrap();
    assert!(
        authority.source.contains("initialization"),
        "{}",
        authority.source
    );
    assert!(!ran.exists(), "the configured upload command ran");
}

/// AC-5: read, parse and object errors at the authority refuse; a proven
/// absence of the policy file selects the shipped defaults even when the
/// working copy is weakened.
#[test]
fn authority_errors_refuse_and_a_proven_absence_selects_the_defaults() {
    let f = Fixture::new("https://example.invalid/repo.git");
    let absent = f.commit(&[("README.md", "readme\n")], &[]);
    f.tracking("main", absent);
    f.remote_head("refs/remotes/origin/main");
    f.on_branch(absent);
    std::fs::create_dir_all(f.root().join(".codeflow")).unwrap();
    std::fs::write(
        f.root().join(".codeflow/policy.json"),
        r#"{"git":{"path_policy":"off","secret_scan":"off"}}"#,
    )
    .unwrap();
    let authority = landed_policy::load(f.root()).unwrap();
    assert_eq!(
        serde_json::to_value(&authority.policy).unwrap(),
        serde_json::to_value(codeflow_core::hooks::Policy::default()).unwrap(),
        "a proven absence selects the shipped defaults"
    );
    assert!(authority.local_differs);

    // A parse error.
    let broken = f.commit(&[(".codeflow/policy.json", "{")], &[absent]);
    f.tracking("main", broken);
    assert!(
        landed_policy::load(f.root()).is_err(),
        "a parse error refuses"
    );

    // The policy path holds a link, not a file.
    f.drop_path(".codeflow/policy.json");
    f.put(".codeflow/policy.json", 0o120_000, b"../elsewhere.json");
    let link = f.commit(&[], &[absent]);
    f.tracking("main", link);
    assert!(landed_policy::load(f.root()).is_err(), "a link refuses");

    // The tracking ref names an object this clone does not hold.
    std::fs::write(
        f.repo.path().join("refs/remotes/origin/main"),
        format!("{}\n", "1".repeat(40)),
    )
    .unwrap();
    assert!(
        landed_policy::load(f.root()).is_err(),
        "an object error refuses"
    );
}
