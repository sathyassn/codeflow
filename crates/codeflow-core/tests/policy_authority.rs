//! TSK-1077: the default branch, resolved to one commit, is the only local
//! policy authority. No line supplies a policy, an exception or a level,
//! and no state is guessed: an unset remote `HEAD`, an empty tracking
//! namespace and an unreadable authority refuse with their remedy.
use codeflow_core::hooks::landed_policy;
use codeflow_core::hooks::path_policy::{self, PathPolicy};
use git2::{Oid, Repository};

const RECORD: &str = "project-management/tasks/TSK-007.md";

/// A remote git cannot reach and that opens no socket and looks up no host,
/// so a test that asks it never leaves the machine (TSK-1156).
const UNREACHABLE_REMOTE: &str = "file:///nonexistent/codeflow-origin.git";

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
    let f = Fixture::new(UNREACHABLE_REMOTE);
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
    let f = Fixture::new(UNREACHABLE_REMOTE);
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
    let f = Fixture::new(UNREACHABLE_REMOTE);
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
        let f = Fixture::new(UNREACHABLE_REMOTE);
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
        let f = Fixture::new(UNREACHABLE_REMOTE);
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
    // TSK-1184 AC-7: refused by name before any query, not as a failed one.
    assert!(error.contains("the ext protocol"), "{error}");
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
    // TSK-1184 review: a configured upload program chooses what a later
    // fetch reaches, which the hermetic answer does not cover, so this is no
    // longer initialization; it refuses naming the setting, and still runs
    // nothing.
    let error = landed_policy::load(f.root())
        .err()
        .expect("a configured upload program is not initialization");
    assert!(error.contains("remote.origin.uploadpack"), "{error}");
    assert!(!ran.exists(), "the configured upload command ran");
}

/// A remote URL, the settings added to it, and the refusal it gives.
type Case<'a> = (&'a str, &'a [(&'a str, &'a str)], &'a str);

/// Each repository setting that git would run as a command on the way to
/// a remote: none runs when the authority asks the remote, and none can
/// make a remote with refs read as empty (each case refuses with the fetch
/// route). Review round 01 F1.
#[cfg(unix)]
#[test]
fn no_configured_transport_command_runs_or_fabricates_initialization() {
    let marker = tempfile::tempdir().unwrap();
    let ran = marker.path().join("ran");
    let touch = format!("sh -c 'touch {}' --", ran.display());
    let touch = touch.as_str();
    // The refusal each case gives: the fetch route when the remote cannot be
    // asked, or, since TSK-1184 AC-7, the allow-list when the effective
    // transport is outside it.
    let fetch = "git fetch origin";
    let listed = "outside the hermetic allow-list";
    let cases: [Case; 5] = [
        // An SSH command, for an SSH remote.
        (
            "ssh://127.0.0.1:9/repo.git",
            &[("core.sshCommand", touch)],
            fetch,
        ),
        // A proxy command, for a git:// remote.
        (
            "git://127.0.0.1:9/repo.git",
            &[("core.gitProxy", touch)],
            listed,
        ),
        // A remote helper by a `vcs` name the configuration allows.
        (
            "https://127.0.0.1:9/repo.git",
            &[
                ("remote.origin.vcs", "touchy"),
                ("protocol.touchy.allow", "always"),
            ],
            "remote.origin.vcs",
        ),
        // A URL rewritten to a command transport.
        (
            "fixture",
            &[
                ("url.ext::sh -c touch% /tmp/never.insteadOf", "fixture"),
                ("protocol.ext.allow", "always"),
            ],
            listed,
        ),
        // A credential helper and an askpass program.
        (
            "http://127.0.0.1:9/repo.git",
            &[("credential.helper", touch), ("core.askPass", touch)],
            fetch,
        ),
    ];
    for (url, settings, refusal) in cases {
        let f = Fixture::new(url);
        let mut config = f.repo.config().unwrap();
        for (key, value) in settings {
            config.set_str(key, value).unwrap();
        }
        let tip = f.commit(&[(".codeflow/policy.json", MAIN_POLICY)], &[]);
        f.on_branch(tip);
        let error = landed_policy::load(f.root())
            .err()
            .unwrap_or_else(|| panic!("{url}: an unasked remote is no initialization"));
        assert!(error.contains(refusal), "{url}: {error}");
        assert!(!error.contains("initialization"), "{url}: {error}");
        assert!(!ran.exists(), "{url}: a configured command ran");
    }
}

/// AC-5: read, parse and object errors at the authority refuse; a proven
/// absence of the policy file selects the shipped defaults even when the
/// working copy is weakened.
#[test]
fn authority_errors_refuse_and_a_proven_absence_selects_the_defaults() {
    let f = Fixture::new(UNREACHABLE_REMOTE);
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

/// TSK-1184: an empty repository at the configured URL and a populated one
/// that a configuration rewrite makes git use instead. Returns the fixture,
/// the empty URL and the populated URL; `rewrite` adds the setting that
/// makes them differ.
fn rewritten(
    rewrite: impl Fn(&Fixture, &str, &str),
) -> (Fixture, String, String, tempfile::TempDir) {
    let remotes = tempfile::tempdir().unwrap();
    let empty = remotes.path().join("empty.git");
    let populated = remotes.path().join("populated.git");
    upstream(&empty, true);
    upstream(&populated, false);
    let empty = empty.to_str().unwrap().to_string();
    let populated = populated.to_str().unwrap().to_string();
    let f = Fixture::new(&empty);
    rewrite(&f, &empty, &populated);
    let tip = f.commit(&[(".codeflow/policy.json", MAIN_POLICY)], &[]);
    f.on_branch(tip);
    (f, empty, populated, remotes)
}

fn refusal(f: &Fixture) -> String {
    match landed_policy::load(f.root()) {
        Ok(authority) => panic!("read policy from {}", authority.source),
        Err(error) => error,
    }
}

/// TSK-1184 AC-1: the configured URL is empty and an `insteadOf` rewrite
/// makes git fetch from a populated repository. Before the change this read
/// as initialization and `HEAD` judged; now it refuses and names both.
#[test]
fn an_insteadof_rewrite_to_a_populated_remote_refuses_naming_both() {
    let (f, empty, populated, _keep) = rewritten(|f, empty, populated| {
        let mut config = f.repo.config().unwrap();
        config
            .set_str(&format!("url.{populated}.insteadOf"), empty)
            .unwrap();
    });
    let error = refusal(&f);
    assert!(error.contains(&empty), "{error}");
    assert!(error.contains(&populated), "{error}");
    assert!(error.contains("never read from HEAD"), "{error}");
}

/// TSK-1184 AC-2 and AC-6: the push identity counts too: `pushInsteadOf`
/// and `pushurl` each refuse with both identities named.
#[test]
fn a_push_rewrite_or_push_url_refuses_naming_both() {
    type Rewrite = fn(&Fixture, &str, &str);
    let cases: [(&str, Rewrite); 2] = [
        ("pushInsteadOf", |f, empty, populated| {
            let mut config = f.repo.config().unwrap();
            config
                .set_str(&format!("url.{populated}.pushInsteadOf"), empty)
                .unwrap();
        }),
        ("pushurl", |f, _, populated| {
            let mut config = f.repo.config().unwrap();
            config.set_str("remote.origin.pushurl", populated).unwrap();
        }),
    ];
    for (what, rewrite) in cases {
        let (f, empty, populated, _keep) = rewritten(rewrite);
        let error = refusal(&f);
        assert!(error.contains(&empty), "{what}: {error}");
        assert!(error.contains(&populated), "{what}: {error}");
        assert!(error.contains("pushes to"), "{what}: {error}");
    }
}

/// TSK-1184 AC-6: a rewrite set in an included configuration file refuses
/// as one set directly.
#[test]
fn a_rewrite_from_an_included_file_refuses() {
    let include = tempfile::tempdir().unwrap();
    let path = include.path().join("rewrite.config");
    let (f, empty, populated, _keep) = rewritten(|f, empty, populated| {
        std::fs::write(
            &path,
            format!("[url \"{populated}\"]\n\tinsteadOf = {empty}\n"),
        )
        .unwrap();
        let mut config = f.repo.config().unwrap();
        config
            .set_str("include.path", path.to_str().unwrap())
            .unwrap();
    });
    let error = refusal(&f);
    assert!(error.contains(&empty), "{error}");
    assert!(error.contains(&populated), "{error}");
}

/// TSK-1184 AC-6: an empty fork as the effective remote is still not the
/// configured one: a rewrite to another empty repository refuses too, so
/// initialization needs the configured URL itself to be empty.
#[test]
fn a_rewrite_to_another_empty_repository_refuses() {
    let (f, empty, other, _keep) = rewritten(|f, empty, _| {
        let fork = std::path::Path::new(empty).with_file_name("fork.git");
        upstream(&fork, true);
        let mut config = f.repo.config().unwrap();
        config
            .set_str(&format!("url.{}.insteadOf", fork.display()), empty)
            .unwrap();
    });
    let error = refusal(&f);
    assert!(error.contains(&empty), "{error}");
    assert!(error.contains("fork.git"), "{error} ({other})");
}

/// TSK-1184 AC-7: an effective URL on a transport outside the hermetic
/// allow-list refuses naming the protocol and the list, whether the
/// configured URL holds it or a rewrite produces it; nothing runs.
#[test]
fn an_effective_url_outside_the_allow_list_refuses_naming_the_protocol() {
    let marker = tempfile::tempdir().unwrap();
    let ran = marker.path().join("ran");
    let ext = format!("ext::sh -c touch% {}", ran.display());
    let direct = |url: &str| {
        let f = Fixture::new(url);
        let tip = f.commit(&[(".codeflow/policy.json", MAIN_POLICY)], &[]);
        f.on_branch(tip);
        f
    };
    let rewritten_to = |url: &str| {
        let f = Fixture::new("fixture-remote");
        let mut config = f.repo.config().unwrap();
        config
            .set_str(&format!("url.{url}.insteadOf"), "fixture-remote")
            .unwrap();
        config.set_str("protocol.ext.allow", "always").unwrap();
        let tip = f.commit(&[(".codeflow/policy.json", MAIN_POLICY)], &[]);
        f.on_branch(tip);
        f
    };
    for (protocol, url) in [("ext", ext.as_str()), ("git", "git://127.0.0.1:9/repo.git")] {
        for f in [direct(url), rewritten_to(url)] {
            let error = refusal(&f);
            assert!(
                error.contains(&format!("the {protocol} protocol")),
                "{protocol}: {error}"
            );
            assert!(error.contains("file:http:https:ssh"), "{protocol}: {error}");
            assert!(error.contains("never widened"), "{protocol}: {error}");
        }
    }
    assert!(!ran.exists(), "the ext:: command ran");
}

/// TSK-1184 AC-2: when git cannot say which URL it uses, the load refuses
/// instead of asking the configured URL. A stand-in `git` fails only
/// `remote get-url`; the child test runs with it first on `PATH`.
#[cfg(unix)]
#[test]
fn an_unreadable_effective_identity_refuses() {
    use std::os::unix::fs::PermissionsExt as _;
    const CHILD: &str = "CODEFLOW_TSK1184_UNREADABLE_CHILD";
    if let Some(root) = std::env::var_os(CHILD) {
        let error = landed_policy::load(std::path::Path::new(&root))
            .err()
            .expect("an unreadable identity refuses");
        assert!(error.contains("cannot be read"), "{error}");
        assert!(error.contains("never read from HEAD"), "{error}");
        return;
    }
    let remote = tempfile::tempdir().unwrap();
    upstream(remote.path(), true);
    let f = Fixture::new(remote.path().to_str().unwrap());
    let tip = f.commit(&[(".codeflow/policy.json", MAIN_POLICY)], &[]);
    f.on_branch(tip);
    // The same fixture with the real git is initialization.
    assert!(landed_policy::load(f.root())
        .unwrap()
        .source
        .contains("initialization"));
    let bin = tempfile::tempdir().unwrap();
    let real = String::from_utf8(
        std::process::Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let real = real.trim();
    let git = bin.path().join("git");
    std::fs::write(
        &git,
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do [ \"$a\" = get-url ] && exit 1; done\nexec '{real}' \"$@\"\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        bin.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "an_unreadable_effective_identity_refuses",
            "--nocapture",
        ])
        .env(CHILD, f.root())
        .env("PATH", path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// TSK-1184 AC-3: matching identities and a verified-empty remote stay the
/// one initialization case, and a populated tracking namespace is never
/// asked: a rewrite does not stop a repository whose authority is fetched.
#[test]
fn matching_identities_keep_initialization_and_fetched_authority() {
    let remote = tempfile::tempdir().unwrap();
    upstream(remote.path(), true);
    let f = Fixture::new(remote.path().to_str().unwrap());
    let tip = f.commit(&[(".codeflow/policy.json", MAIN_POLICY)], &[]);
    f.on_branch(tip);
    let authority = landed_policy::load(f.root()).unwrap();
    assert!(
        authority
            .source
            .contains("initialization: origin advertises no refs"),
        "{}",
        authority.source
    );
    assert!(authority.authority.is_none());
    // Once the authority is fetched, the remote is not asked, so a rewrite
    // the fetch guard owns does not change where policy is read.
    let mut config = f.repo.config().unwrap();
    config
        .set_str(
            "url./nonexistent/elsewhere.git.insteadOf",
            remote.path().to_str().unwrap(),
        )
        .unwrap();
    f.tracking("main", tip);
    f.remote_head("refs/remotes/origin/main");
    let authority = landed_policy::load(f.root()).unwrap();
    assert_eq!(authority.source, "refs/remotes/origin/main");
    assert!(authority.authority.is_some());
}

/// TSK-1184 review finding 1: a rewrite included on a `gitdir` condition
/// that names the repository by a symbolic link applies to a command run
/// through the link, while a read of the resolved path does not see it.
/// Neither path is initialization: the class refuses, naming the file.
#[cfg(unix)]
#[test]
fn a_path_conditional_rewrite_refuses_by_every_path() {
    let top = tempfile::tempdir().unwrap();
    let empty = top.path().join("empty.git");
    let populated = top.path().join("populated.git");
    upstream(&empty, true);
    upstream(&populated, false);
    let real = top.path().join("real");
    let link = top.path().join("link");
    std::fs::create_dir(&real).unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let repo = Repository::init(&real).unwrap();
    repo.set_head("refs/heads/task/TSK-007-authority").unwrap();
    repo.remote("origin", empty.to_str().unwrap()).unwrap();
    let rewrite = top.path().join("rewrite.config");
    std::fs::write(
        &rewrite,
        format!(
            "[url \"{}\"]\n\tinsteadOf = {}\n",
            populated.display(),
            empty.display()
        ),
    )
    .unwrap();
    let mut config = repo.config().unwrap();
    config
        .set_str(
            &format!("includeIf.gitdir:{}/.git.path", link.display()),
            rewrite.to_str().unwrap(),
        )
        .unwrap();
    let f = Fixture { dir: top, repo };
    let tip = f.commit(&[(".codeflow/policy.json", MAIN_POLICY)], &[]);
    f.on_branch(tip);
    for root in [&link, &real] {
        let error = match landed_policy::load(root) {
            Ok(authority) => panic!("{}: read policy from {}", root.display(), authority.source),
            Err(error) => error,
        };
        assert!(
            error.contains("rewrite.config"),
            "{}: {error}",
            root.display()
        );
        assert!(
            error.contains("never read from HEAD"),
            "{}: {error}",
            root.display()
        );
        assert!(
            !error.contains("initialization"),
            "{}: {error}",
            root.display()
        );
    }
}

/// TSK-1184 review finding 2: a remote helper set for the remote, here
/// through an included file, replaces the transport while the URL stays the
/// empty, reachable repository. Git runs the helper for a fetch; the policy
/// load refuses naming the setting and the helper, and runs nothing.
#[cfg(unix)]
#[test]
fn a_remote_helper_for_a_reachable_empty_remote_refuses_naming_it() {
    use std::os::unix::fs::PermissionsExt as _;
    let remote = tempfile::tempdir().unwrap();
    upstream(remote.path(), true);
    let f = Fixture::new(remote.path().to_str().unwrap());
    let side = tempfile::tempdir().unwrap();
    let marker = side.path().join("helper-ran");
    let bin = side.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let helper = bin.join("git-remote-touchy");
    std::fs::write(
        &helper,
        format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let included = side.path().join("helper.config");
    std::fs::write(&included, "[remote \"origin\"]\n\tvcs = touchy\n").unwrap();
    let mut config = f.repo.config().unwrap();
    config
        .set_str("include.path", included.to_str().unwrap())
        .unwrap();
    let tip = f.commit(&[(".codeflow/policy.json", MAIN_POLICY)], &[]);
    f.on_branch(tip);
    // A fetch-side question by the remote's name runs the helper.
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let _ = std::process::Command::new("git")
        .current_dir(f.root())
        .env("PATH", path)
        .args(["ls-remote", "origin"])
        .output()
        .unwrap();
    assert!(marker.exists(), "git did not run the helper for the remote");
    std::fs::remove_file(&marker).unwrap();
    let error = refusal(&f);
    assert!(error.contains("remote.origin.vcs"), "{error}");
    assert!(error.contains("touchy"), "{error}");
    assert!(!error.contains("initialization"), "{error}");
    assert!(!marker.exists(), "the policy load ran the helper");
}

/// TSK-1184 sibling sweep: each transport setting of the remote's section
/// refuses on a reachable empty remote, set directly or through an include.
#[test]
fn every_transport_setting_of_the_remote_refuses() {
    for setting in ["vcs", "uploadpack", "receivepack"] {
        let remote = tempfile::tempdir().unwrap();
        upstream(remote.path(), true);
        let f = Fixture::new(remote.path().to_str().unwrap());
        let mut config = f.repo.config().unwrap();
        config
            .set_str(&format!("remote.origin.{setting}"), "fixture-program")
            .unwrap();
        let tip = f.commit(&[(".codeflow/policy.json", MAIN_POLICY)], &[]);
        f.on_branch(tip);
        let error = refusal(&f);
        assert!(
            error.contains(&format!("remote.origin.{setting}")),
            "{setting}: {error}"
        );
        assert!(!error.contains("initialization"), "{setting}: {error}");
    }
}

/// The shared transport check takes a remote's name or URL section and
/// refuses only that section's settings.
#[test]
fn the_shared_transport_check_reads_one_section() {
    let remote = tempfile::tempdir().unwrap();
    upstream(remote.path(), true);
    let url = remote.path().to_str().unwrap().to_string();
    let f = Fixture::new(&url);
    let limits = codeflow_core::git::remote_query::QueryLimits::guard();
    let check = |section: &str| {
        codeflow_core::git::remote_identity::require_native_transport(f.root(), section, limits)
    };
    assert!(check("remote.origin").is_ok());
    assert!(check(&format!("remote.{url}")).is_ok());
    assert!(check("core").is_err(), "not a remote section");
    let mut config = f.repo.config().unwrap();
    config
        .set_str(&format!("remote.{url}.vcs"), "touchy")
        .unwrap();
    let error = check(&format!("remote.{url}")).unwrap_err();
    assert!(error.contains("touchy"), "{error}");
    assert!(
        check("remote.origin").is_ok(),
        "another section is not read"
    );
}
