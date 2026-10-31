//! TSK-397: the strict text rule is the default; `relaxed` is honoured only
//! from the protected remote-tracking refs, and a retry flag or native
//! Windows keeps a relaxed call strict.

use super::*;
use std::fs;

const RELAXED: &str = r#"{"security":{"text_guard":"relaxed"}}"#;

/// A clone of an origin whose `main` holds `origin_policy` (none when
/// `None`), with `origin/main` and `origin/HEAD` fetched.
struct Fixture {
    _dir: tempfile::TempDir,
    work: PathBuf,
}

fn commit(repo: &git2::Repository, files: &[(&str, &str)]) {
    let root = repo.workdir().unwrap().to_path_buf();
    let mut index = repo.index().unwrap();
    for (path, text) in files {
        let file = root.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, text).unwrap();
        index.add_path(Path::new(path)).unwrap();
    }
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let signature = git2::Signature::now("t", "t@example.invalid").unwrap();
    let parent = repo.head().ok().and_then(|head| head.peel_to_commit().ok());
    let parents: Vec<&git2::Commit> = parent.iter().collect();
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        "test: x",
        &tree,
        &parents,
    )
    .unwrap();
}

fn init(dir: &Path) -> git2::Repository {
    let mut options = git2::RepositoryInitOptions::new();
    options.initial_head("main");
    git2::Repository::init_opts(dir, &options).unwrap()
}

impl Fixture {
    fn new(origin_policy: Option<&str>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let origin = init(&base.join("origin"));
        let mut files = vec![("README.md", "x\n")];
        if let Some(policy) = origin_policy {
            files.push((".codeflow/policy.json", policy));
        }
        commit(&origin, &files);
        let work = base.join("work");
        git2::Repository::clone(base.join("origin").to_str().unwrap(), &work).unwrap();
        Self { _dir: dir, work }
    }

    fn host(&self) -> Host {
        Host {
            session: None,
            cwd: self.work.clone(),
            root: self.work.clone(),
            home: None,
            claude_config_dir: None,
            managed_file: None,
            managed_dir: None,
            native_windows: false,
        }
    }

    fn outcome(&self, host: &Host, retry: bool) -> Outcome {
        let authority = crate::hooks::landed_policy::load(&self.work).unwrap();
        let policy: Vec<String> = policy_reason(&authority, "the project policy")
            .into_iter()
            .collect();
        outcome(host, retry, &policy)
    }
}

fn assert_strict(outcome: &Outcome, needle: &str) {
    match outcome {
        Outcome::Strict(reason) => assert!(reason.contains(needle), "{reason}"),
        Outcome::Relaxed => panic!("expected the strict rule naming {needle}, got relaxed"),
    }
}

/// With no `text_guard` anywhere, every call is strict.
#[test]
fn the_strict_rule_is_the_default() {
    let fixture = Fixture::new(None);
    assert_strict(
        &fixture.outcome(&fixture.host(), false),
        "keeps the default `security.text_guard: strict`",
    );
    let fixture = Fixture::new(Some(r#"{"security":{"text_guard":"strict"}}"#));
    assert_strict(
        &fixture.outcome(&fixture.host(), false),
        "keeps the default",
    );
}

/// `relaxed` on the remote's default branch, read through its
/// remote-tracking ref, selects the line's rule.
#[test]
fn relaxed_from_a_protected_ref_is_honoured() {
    let fixture = Fixture::new(Some(RELAXED));
    let authority = crate::hooks::landed_policy::load(&fixture.work).unwrap();
    assert!(authority.remote_tracked, "{}", authority.source);
    assert_eq!(fixture.outcome(&fixture.host(), false), Outcome::Relaxed);
}

/// `relaxed` only in the working copy, or only in a local commit the
/// remote does not hold, is not honoured.
#[test]
fn relaxed_in_the_worktree_or_a_branch_is_ignored() {
    let fixture = Fixture::new(None);
    let policy = fixture.work.join(".codeflow/policy.json");
    fs::create_dir_all(policy.parent().unwrap()).unwrap();
    fs::write(&policy, RELAXED).unwrap();
    assert_strict(
        &fixture.outcome(&fixture.host(), false),
        "keeps the default",
    );
    let repo = git2::Repository::open(&fixture.work).unwrap();
    commit(&repo, &[(".codeflow/policy.json", RELAXED)]);
    assert_strict(
        &fixture.outcome(&fixture.host(), false),
        "keeps the default",
    );
}

/// A repository with no remote reads `HEAD`, which a local commit can
/// change, so `relaxed` there is not honoured.
#[test]
fn relaxed_from_head_without_a_remote_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap().join("solo");
    let repo = init(&root);
    commit(&repo, &[(".codeflow/policy.json", RELAXED)]);
    let authority = crate::hooks::landed_policy::load(&root).unwrap();
    assert!(!authority.remote_tracked, "{}", authority.source);
    let reason = policy_reason(&authority, "the project policy").unwrap();
    assert!(reason.contains("not honoured"), "{reason}");
}

/// Under `relaxed`, a call marked `dangerouslyDisableSandbox` and every
/// call on native Windows still take the strict rule.
#[test]
fn relaxed_still_tightens_a_retry_and_native_windows() {
    let fixture = Fixture::new(Some(RELAXED));
    assert_strict(
        &fixture.outcome(&fixture.host(), true),
        "dangerouslyDisableSandbox",
    );
    let mut host = fixture.host();
    host.native_windows = true;
    assert_strict(&fixture.outcome(&host, false), "native Windows");
}

/// A policy reason passed in always selects the strict rule.
#[test]
fn a_policy_reason_never_loosens() {
    let fixture = Fixture::new(Some(RELAXED));
    let reason = "the session project's policy keeps the default".to_string();
    assert_strict(
        &outcome(&fixture.host(), false, std::slice::from_ref(&reason)),
        "session project",
    );
}
