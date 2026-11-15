//! Tests of the policy baseline (TSK-1057).
use std::path::{Path, PathBuf};
use std::process::Command;

use git2::{Oid, Repository, Signature};

use super::*;
use crate::hooks::landed_policy::{load_with, Authority};
use crate::hooks::PolicyLevel;

const STRICT: &str = r#"{"schema_version": 1, "git": {"push_to_protected": "block", "protected_branches": ["main", "release"]}}"#;
const WEAK: &str = r#"{"schema_version": 1, "git": {"push_to_protected": "warn", "protected_branches": ["main"]}}"#;
const STRICTER: &str = r#"{"schema_version": 1, "git": {"push_to_protected": "block", "protected_branches": ["main", "release", "stable"]}}"#;

/// A commit whose tree holds `policy` at `.codeflow/policy.json`.
fn commit(repo: &Repository, policy: &str, parents: &[Oid]) -> Oid {
    let blob = repo.blob(policy.as_bytes()).unwrap();
    let mut inner = repo.treebuilder(None).unwrap();
    inner.insert("policy.json", blob, 0o100_644).unwrap();
    let inner = inner.write().unwrap();
    let mut outer = repo.treebuilder(None).unwrap();
    outer.insert(".codeflow", inner, 0o040_000).unwrap();
    let tree = repo.find_tree(outer.write().unwrap()).unwrap();
    let sig = Signature::now("Test", "test@example.invalid").unwrap();
    let parents: Vec<_> = parents
        .iter()
        .map(|p| repo.find_commit(*p).unwrap())
        .collect();
    let parents: Vec<_> = parents.iter().collect();
    repo.commit(None, &sig, &sig, "test: policy", &tree, &parents)
        .unwrap()
}

/// A working repository whose `origin/main` names a strict-policy commit,
/// with a baseline store in its own directory.
struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    store: Store,
    base: Oid,
}

const URL: &str = "file:///nonexistent/origin.git";

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap().join("work");
        let repo = Repository::init(&root).unwrap();
        let base = commit(&repo, STRICT, &[]);
        repo.reference("refs/heads/main", base, true, "test")
            .unwrap();
        repo.set_head("refs/heads/main").unwrap();
        repo.remote("origin", URL).unwrap();
        repo.reference("refs/remotes/origin/main", base, true, "test")
            .unwrap();
        repo.reference_symbolic(
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
            true,
            "test",
        )
        .unwrap();
        let store = Store::at(dir.path().join("authority").join("baselines"));
        Self {
            _dir: dir,
            root,
            store,
            base,
        }
    }

    fn repo(&self) -> Repository {
        Repository::open(&self.root).unwrap()
    }

    fn record(&self) -> Record {
        let repo = self.repo();
        initial(
            &Fresh {
                common_dir: common_dir(&repo).unwrap(),
                remote: "origin".into(),
                remote_url: URL.into(),
                authority: "refs/remotes/origin/main".into(),
                commit: self.base.to_string(),
                policy_text: Some(STRICT.into()),
                policy_digest: digest(STRICT),
            },
            "2026-11-15T00:00:00Z",
        )
    }

    fn initialize(&self) -> PathBuf {
        self.store.write(&self.record()).unwrap()
    }

    /// Point `origin/main` at `oid`, as a direct ref write would.
    fn rewrite_main(&self, oid: Oid) {
        self.repo()
            .reference("refs/remotes/origin/main", oid, true, "test: rewrite")
            .unwrap();
    }

    fn level(&self) -> Result<PolicyLevel, String> {
        load_with(&self.root, Some(&self.store)).map(|l| l.policy.git.push_to_protected)
    }
}

fn policy(text: &str) -> Policy {
    serde_json::from_str(text).unwrap()
}

// ---------------------------------------------------------------------------
// The stricter of two (AC-1)
// ---------------------------------------------------------------------------

#[test]
fn the_stricter_policy_takes_higher_levels_and_the_union_of_denials() {
    let merged = stricter(&policy(WEAK), &policy(STRICTER)).unwrap();
    assert_eq!(merged.git.push_to_protected, PolicyLevel::Block);
    assert_eq!(merged.git.protected_branches, ["main", "release", "stable"]);
    let merged = stricter(&policy(STRICT), &policy(WEAK)).unwrap();
    assert_eq!(merged.git.push_to_protected, PolicyLevel::Block);
    assert_eq!(merged.git.protected_branches, ["main", "release"]);
}

#[test]
fn the_stricter_policy_narrows_allowances_and_keeps_the_strict_text_rule() {
    let base = policy(
        r#"{"schema_version": 1, "security": {"text_guard": "relaxed", "trusted_client_configs": ["curl", "wget"]}}"#,
    );
    let other = policy(
        r#"{"schema_version": 1, "security": {"text_guard": "strict", "trusted_client_configs": ["curl", "xh"]}}"#,
    );
    let merged = stricter(&base, &other).unwrap();
    assert!(merged.security.text_guard.is_strict());
    assert_eq!(merged.security.trusted_client_configs, ["curl"]);
    // Relaxed only when both say so.
    let merged = stricter(&base, &base).unwrap();
    assert!(!merged.security.text_guard.is_strict());
}

/// A key left at its default is omitted when serialized; the merge reads
/// the omission as that default, so the other side's relaxation does not
/// survive, in either order.
#[test]
fn an_omitted_default_merges_as_the_default() {
    let relaxed = policy(
        r#"{"schema_version": 1, "security": {"text_guard": "relaxed", "trusted_client_configs": ["curl"]}, "git": {"ignored_files": {"level": "off", "allow": [{"path": "a", "reason": "r"}]}}}"#,
    );
    let plain = policy(r#"{"schema_version": 1}"#);
    for (left, right) in [(&relaxed, &plain), (&plain, &relaxed)] {
        let merged = stricter(left, right).unwrap();
        assert!(merged.security.text_guard.is_strict());
        assert!(merged.security.trusted_client_configs.is_empty());
        let ignored = merged.git.ignored_files.unwrap_or_default();
        assert_eq!(ignored.level, PolicyLevel::Warn);
        assert!(ignored.allow.is_empty());
    }
    assert!(weaker(&relaxed, &plain).unwrap());
    assert!(!weaker(&plain, &plain).unwrap());
}

/// More product paths require more journey criteria, so a ref that drops
/// one is weaker: the merge keeps the baseline's paths. A ref list over a
/// baseline that leaves the key at its stack default (`null`) cannot
/// replace that default either.
#[test]
fn a_ref_that_drops_a_product_path_keeps_the_baselines_paths() {
    let base =
        policy(r#"{"schema_version": 1, "git": {"product_paths": ["crates/**", "assets/**"]}}"#);
    let other = policy(r#"{"schema_version": 1, "git": {"product_paths": ["crates/**"]}}"#);
    let merged = stricter(&base, &other).unwrap();
    assert_eq!(
        merged.git.product_paths.as_deref(),
        Some(["crates/**".to_owned(), "assets/**".to_owned()].as_slice())
    );
    assert!(weaker(&other, &base).unwrap());
    let unset = policy(r#"{"schema_version": 1}"#);
    let narrow = policy(r#"{"schema_version": 1, "git": {"product_paths": ["nothing/**"]}}"#);
    let merged = stricter(&unset, &narrow).unwrap();
    assert_eq!(merged.git.product_paths, None);
}

#[test]
fn any_other_value_is_the_baselines() {
    let base = policy(
        r#"{"schema_version": 1, "git": {"commit_desc_max_len": 50, "root_branch": "main"}}"#,
    );
    let other = policy(
        r#"{"schema_version": 1, "git": {"commit_desc_max_len": 500, "root_branch": "off"}}"#,
    );
    let merged = stricter(&base, &other).unwrap();
    assert_eq!(merged.git.commit_desc_max_len, 50);
    assert_eq!(merged.git.root_branch, "main");
}

/// Every list in the policy is classed: a new list key must say whether
/// more entries refuse more or allow more, or this test fails. The schema
/// registry names every key, including those `null` or omitted by default;
/// the serialized default catches a list the registry does not type as one.
#[test]
fn every_policy_list_is_classed() {
    use super::super::policy_schema::{KeyKind, SCHEMA};
    // Objects whose values the baseline keeps whole: the PR-section mapping
    // and the size rule (its limit and exempt prefixes are the baseline's).
    let baseline_only: &[&str] = &["git.pr_section_mapping", "git.max_file_bytes"];
    let classed = |key: &str| {
        UNION_LISTS.contains(&key) || INTERSECT_LISTS.contains(&key) || baseline_only.contains(&key)
    };
    let mut unclassed = Vec::new();
    for spec in SCHEMA {
        let listy = match spec.kind {
            KeyKind::StringList
            | KeyKind::Profiles
            | KeyKind::PathRules
            | KeyKind::RetryEntries
            | KeyKind::SectionMapping => classed(spec.path),
            // The ignored-file rule's allowance list is classed.
            KeyKind::ContentRule => classed(spec.path) || classed(&format!("{}.allow", spec.path)),
            _ => true,
        };
        if !listy {
            unclassed.push(spec.path.to_owned());
        }
    }
    let mut list_keys = Vec::new();
    collect_lists(
        &explicit(serde_json::to_value(Policy::default()).unwrap()),
        "",
        &mut list_keys,
    );
    unclassed.extend(list_keys.into_iter().filter(|key| !classed(key)));
    assert!(
        unclassed.is_empty(),
        "unclassed policy lists: {unclassed:?}"
    );
}

fn collect_lists(value: &Value, path: &str, out: &mut Vec<String>) {
    if let Value::Object(map) = value {
        for (key, child) in map {
            let path = if path.is_empty() {
                key.clone()
            } else {
                format!("{path}.{key}")
            };
            if child.is_array() {
                out.push(path);
            } else {
                collect_lists(child, &path, out);
            }
        }
    }
}

#[test]
fn a_weaker_candidate_is_named_weaker() {
    assert!(weaker(&policy(WEAK), &policy(STRICT)).unwrap());
    assert!(!weaker(&policy(STRICTER), &policy(STRICT)).unwrap());
    assert!(!weaker(&policy(STRICT), &policy(STRICT)).unwrap());
}

// ---------------------------------------------------------------------------
// AC-2: a rewritten tracking ref keeps the stricter policy
// ---------------------------------------------------------------------------

/// Fail before: without a baseline the read takes the rewritten ref's weak
/// policy, as every read did before this change. Pass after: with the
/// baseline the strict policy holds.
#[test]
fn a_weaker_rewrite_of_origin_main_keeps_the_strict_policy() {
    let fixture = Fixture::new();
    fixture.initialize();
    let weak = commit(&fixture.repo(), WEAK, &[]);
    fixture.rewrite_main(weak);
    let before = load_with(&fixture.root, None).unwrap();
    assert_eq!(before.policy.git.push_to_protected, PolicyLevel::Warn);
    let after = load_with(&fixture.root, Some(&fixture.store)).unwrap();
    assert_eq!(after.policy.git.push_to_protected, PolicyLevel::Block);
    assert!(
        matches!(after.baseline, State::Diverged { .. }),
        "{:?}",
        after.baseline
    );
    assert!(after.source.contains("baseline"), "{}", after.source);
}

// ---------------------------------------------------------------------------
// AC-5: one test per baseline state
// ---------------------------------------------------------------------------

#[test]
fn a_repository_never_initialized_reads_the_authority_with_a_note() {
    let fixture = Fixture::new();
    let landed = load_with(&fixture.root, Some(&fixture.store)).unwrap();
    assert_eq!(landed.baseline, State::Never);
    assert_eq!(landed.policy.git.push_to_protected, PolicyLevel::Block);
    let note = landed.baseline.note().unwrap();
    assert!(note.contains("codeflow policy baseline init"), "{note}");
}

#[test]
fn a_current_baseline_changes_nothing() {
    let fixture = Fixture::new();
    fixture.initialize();
    let landed = load_with(&fixture.root, Some(&fixture.store)).unwrap();
    assert_eq!(landed.baseline, State::Current);
    assert!(landed.baseline.note().is_none());
}

#[test]
fn a_missing_baseline_for_an_initialized_repository_blocks() {
    let fixture = Fixture::new();
    let path = fixture.initialize();
    std::fs::remove_file(path).unwrap();
    let error = fixture.level().unwrap_err();
    assert!(error.contains("is missing"), "{error}");
}

#[test]
fn a_malformed_baseline_blocks() {
    let fixture = Fixture::new();
    let path = fixture.initialize();
    std::fs::write(&path, "{not json").unwrap();
    let error = fixture.level().unwrap_err();
    assert!(error.contains("cannot be parsed"), "{error}");
    // A record whose policy text no longer matches its digest.
    let mut record = fixture.record();
    record.policy_text = Some(WEAK.into());
    std::fs::write(&path, serde_json::to_string(&record).unwrap()).unwrap();
    let error = fixture.level().unwrap_err();
    assert!(error.contains("digest"), "{error}");
}

#[cfg(unix)]
#[test]
fn a_baseline_others_can_write_blocks() {
    use std::os::unix::fs::PermissionsExt as _;
    let fixture = Fixture::new();
    let path = fixture.initialize();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
    let error = fixture.level().unwrap_err();
    assert!(error.contains("written by others"), "{error}");
}

#[test]
fn a_baseline_for_another_authority_blocks() {
    let fixture = Fixture::new();
    fixture.initialize();
    let repo = fixture.repo();
    repo.reference("refs/remotes/origin/trunk", fixture.base, true, "test")
        .unwrap();
    repo.reference_symbolic(
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/trunk",
        true,
        "test: re-point",
    )
    .unwrap();
    let error = fixture.level().unwrap_err();
    assert!(error.contains("bound to authority"), "{error}");
}

#[test]
fn a_baseline_for_another_remote_url_blocks() {
    let fixture = Fixture::new();
    fixture.initialize();
    fixture
        .repo()
        .remote_set_url("origin", "file:///nonexistent/other.git")
        .unwrap();
    let error = fixture.level().unwrap_err();
    assert!(error.contains("now points at"), "{error}");
}

/// Replay: a record copied from another repository under this
/// repository's key names the other common directory and blocks.
#[test]
fn a_baseline_replayed_from_another_repository_blocks() {
    let fixture = Fixture::new();
    let other = Fixture::new();
    let path = other.initialize();
    let repo = fixture.repo();
    let ours = fixture
        .store
        .dir()
        .join(format!("{}.json", key(&common_dir(&repo).unwrap())));
    std::fs::create_dir_all(fixture.store.dir()).unwrap();
    std::fs::copy(path, &ours).unwrap();
    let error = fixture.level().unwrap_err();
    assert!(error.contains("names"), "{error}");
}

#[test]
fn a_stale_baseline_enforces_the_stricter_of_the_two() {
    let fixture = Fixture::new();
    fixture.initialize();
    let repo = fixture.repo();
    // A descendant that weakens one rule and adds a protected branch.
    let next = commit(
        &repo,
        r#"{"schema_version": 1, "git": {"push_to_protected": "warn", "protected_branches": ["main", "release", "stable"]}}"#,
        &[fixture.base],
    );
    fixture.rewrite_main(next);
    let landed = load_with(&fixture.root, Some(&fixture.store)).unwrap();
    assert!(
        matches!(landed.baseline, State::Stale { .. }),
        "{:?}",
        landed.baseline
    );
    assert_eq!(landed.policy.git.push_to_protected, PolicyLevel::Block);
    assert_eq!(
        landed.policy.git.protected_branches,
        ["main", "release", "stable"]
    );
    assert!(landed.baseline.note().unwrap().contains("refresh"));
}

#[test]
fn a_non_descendant_ref_enforces_the_baseline() {
    let fixture = Fixture::new();
    fixture.initialize();
    let other = commit(&fixture.repo(), STRICTER, &[]);
    fixture.rewrite_main(other);
    let landed = load_with(&fixture.root, Some(&fixture.store)).unwrap();
    assert!(matches!(landed.baseline, State::Diverged { .. }));
    // The baseline's own policy, not the stricter ref's.
    assert_eq!(landed.policy.git.protected_branches, ["main", "release"]);
}

// ---------------------------------------------------------------------------
// AC-3: the no-remote rule
// ---------------------------------------------------------------------------

#[test]
fn with_a_baseline_and_no_remote_the_baseline_is_enforced() {
    let fixture = Fixture::new();
    fixture.initialize();
    let repo = fixture.repo();
    let weak = commit(&repo, WEAK, &[]);
    repo.reference("refs/heads/main", weak, true, "test")
        .unwrap();
    repo.remote_delete("origin").unwrap();
    let landed = load_with(&fixture.root, Some(&fixture.store)).unwrap();
    assert!(
        matches!(&landed.baseline, State::RemoteRemoved { remote, .. } if remote == "origin"),
        "{:?}",
        landed.baseline
    );
    assert_eq!(landed.policy.git.push_to_protected, PolicyLevel::Block);
    assert!(landed.baseline.note().unwrap().contains("is gone"));
}

#[test]
fn without_a_baseline_and_no_remote_the_state_is_local_only() {
    let fixture = Fixture::new();
    let repo = fixture.repo();
    let weak = commit(&repo, WEAK, &[]);
    repo.reference("refs/heads/main", weak, true, "test")
        .unwrap();
    repo.remote_delete("origin").unwrap();
    let landed = load_with(&fixture.root, Some(&fixture.store)).unwrap();
    assert_eq!(landed.baseline, State::Never);
    assert!(landed.source.contains("local only"), "{}", landed.source);
    assert_eq!(landed.policy.git.push_to_protected, PolicyLevel::Warn);
}

/// TSK-1077's refusal states hold with a baseline in place: an unset
/// remote `HEAD` still refuses before any baseline is read.
#[test]
fn the_authority_refusals_hold_with_a_baseline() {
    let fixture = Fixture::new();
    fixture.initialize();
    let repo = fixture.repo();
    repo.find_reference("refs/remotes/origin/HEAD")
        .unwrap()
        .delete()
        .unwrap();
    let error = fixture.level().unwrap_err();
    assert!(error.contains("the policy authority is unset"), "{error}");
}

// ---------------------------------------------------------------------------
// AC-6: attacks
// ---------------------------------------------------------------------------

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

fn fresh_at(fixture: &Fixture, oid: Oid, text: &str) -> Fresh {
    Fresh {
        common_dir: common_dir(&fixture.repo()).unwrap(),
        remote: "origin".into(),
        remote_url: URL.into(),
        authority: "refs/remotes/origin/main".into(),
        commit: oid.to_string(),
        policy_text: Some(text.into()),
        policy_digest: digest(text),
    }
}

#[test]
fn a_refresh_to_a_non_descendant_needs_a_reset() {
    let fixture = Fixture::new();
    let record = fixture.record();
    let other = commit(&fixture.repo(), STRICTER, &[]);
    let why = refresh_refusal(
        &fixture.repo(),
        &record,
        &fresh_at(&fixture, other, STRICTER),
    )
    .unwrap()
    .unwrap();
    assert!(why.contains("not a descendant"), "{why}");
}

#[test]
fn a_refresh_to_a_weaker_descendant_needs_a_reset() {
    let fixture = Fixture::new();
    let record = fixture.record();
    let next = commit(&fixture.repo(), WEAK, &[fixture.base]);
    let why = refresh_refusal(&fixture.repo(), &record, &fresh_at(&fixture, next, WEAK))
        .unwrap()
        .unwrap();
    assert!(why.contains("weaker"), "{why}");
}

#[test]
fn a_refresh_to_a_stricter_descendant_proceeds_and_a_reset_is_recorded() {
    let fixture = Fixture::new();
    let record = fixture.record();
    let next = commit(&fixture.repo(), STRICTER, &[fixture.base]);
    let fresh = fresh_at(&fixture, next, STRICTER);
    assert!(refresh_refusal(&fixture.repo(), &record, &fresh)
        .unwrap()
        .is_none());
    let reset = refreshed(
        &record,
        &fresh,
        "2026-11-15T01:00:00Z",
        Some("policy moved"),
    );
    assert_eq!(reset.created, record.created);
    assert_eq!(reset.resets.len(), 1);
    assert_eq!(reset.resets[0].reason, "policy moved");
}

#[test]
fn a_refresh_to_another_remote_needs_a_reset() {
    let fixture = Fixture::new();
    let record = fixture.record();
    let mut fresh = fresh_at(&fixture, fixture.base, STRICT);
    fresh.remote_url = "file:///nonexistent/other.git".into();
    let why = refresh_refusal(&fixture.repo(), &record, &fresh)
        .unwrap()
        .unwrap();
    assert!(why.contains("bound to"), "{why}");
}

#[allow(clippy::unnecessary_wraps)] // The shape `Probe::ancestors` takes.
fn no_ancestors() -> Result<Vec<String>, String> {
    Ok(vec!["zsh".into(), "login".into(), "Terminal".into()])
}

#[test]
fn the_writer_refuses_inside_a_session() {
    let none = |_: &str| None;
    let marked = |name: &str| (name == "CLAUDECODE").then(|| "1".into());
    let harness = || Ok(vec!["zsh".into(), "claude".into()]);
    let unreadable = || Err("denied".to_string());
    let cases: [(&Probe<'_>, &str); 4] = [
        (
            &Probe {
                env: &marked,
                ancestors: &no_ancestors,
                stdin_is_terminal: true,
            },
            "CLAUDECODE is set",
        ),
        (
            &Probe {
                env: &none,
                ancestors: &harness,
                stdin_is_terminal: true,
            },
            "an agent harness",
        ),
        (
            &Probe {
                env: &none,
                ancestors: &unreadable,
                stdin_is_terminal: true,
            },
            "cannot be read",
        ),
        (
            &Probe {
                env: &none,
                ancestors: &no_ancestors,
                stdin_is_terminal: false,
            },
            "not a terminal",
        ),
    ];
    for (probe, expected) in cases {
        let error = require_operator_terminal(probe).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
    let operator = Probe {
        env: &none,
        ancestors: &no_ancestors,
        stdin_is_terminal: true,
    };
    assert!(require_operator_terminal(&operator).is_ok());
}

/// The live ancestry reads as names, or, where a sandbox hides a process,
/// as an error; the writer check then refuses (fails closed).
#[cfg(unix)]
#[test]
fn the_live_ancestry_reads_or_the_writer_refuses() {
    match process_ancestors() {
        Ok(names) => assert!(!names.is_empty()),
        Err(why) => {
            assert!(why.contains("cannot be read"), "{why}");
            let none = |_: &str| None;
            let probe = Probe {
                env: &none,
                ancestors: &process_ancestors,
                stdin_is_terminal: true,
            };
            let error = require_operator_terminal(&probe).unwrap_err();
            assert!(error.contains("ancestry cannot be read"), "{error}");
        }
    }
}

/// Each named in-session command is refused by git-guard's authority check
/// today; the ordinary reads and fetches stay allowed.
#[test]
fn git_guard_refuses_each_command_that_moves_the_authority() {
    let fixture = Fixture::new();
    let words = |line: &str| -> Vec<String> { line.split(' ').map(str::to_owned).collect() };
    for line in [
        "git remote remove origin",
        "git remote rm origin",
        "git remote set-url origin file:///elsewhere.git",
        "git remote set-head origin trunk",
        "git symbolic-ref refs/remotes/origin/HEAD refs/remotes/origin/trunk",
        "git symbolic-ref -d refs/remotes/origin/HEAD",
        "git update-ref -d refs/remotes/origin/main",
        "git update-ref -d refs/remotes/origin/HEAD",
    ] {
        let args = words(line);
        assert!(
            super::super::ref_authority::check(&fixture.root, &args[1..]).is_some(),
            "{line} is allowed"
        );
    }
    for line in [
        "git remote -v",
        "git remote get-url origin",
        "git symbolic-ref refs/remotes/origin/HEAD",
        "git update-ref refs/heads/feature HEAD",
    ] {
        let args = words(line);
        assert!(
            super::super::ref_authority::check(&fixture.root, &args[1..]).is_none(),
            "{line} is refused"
        );
    }
}

/// The same commands run outside a session reach the repository; the
/// baseline catches each one at the next policy read.
#[test]
fn the_baseline_catches_each_command_run_outside_a_session() {
    // remote remove and rm: no remote, the baseline is enforced.
    for sub in ["remove", "rm"] {
        let fixture = Fixture::new();
        fixture.initialize();
        git(&fixture.root, &["remote", sub, "origin"]);
        let landed = load_with(&fixture.root, Some(&fixture.store)).unwrap();
        assert!(
            matches!(landed.baseline, State::RemoteRemoved { .. }),
            "{sub}"
        );
    }
    // set-url: the binding blocks.
    let fixture = Fixture::new();
    fixture.initialize();
    git(
        &fixture.root,
        &[
            "remote",
            "set-url",
            "origin",
            "file:///nonexistent/other.git",
        ],
    );
    assert!(fixture.level().unwrap_err().contains("now points at"));
    // set-head and symbolic-ref: a re-pointed remote HEAD blocks.
    for args in [
        ["remote", "set-head", "origin", "trunk"].as_slice(),
        [
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/trunk",
        ]
        .as_slice(),
    ] {
        let fixture = Fixture::new();
        fixture.initialize();
        fixture
            .repo()
            .reference("refs/remotes/origin/trunk", fixture.base, true, "test")
            .unwrap();
        git(&fixture.root, args);
        assert!(
            fixture.level().unwrap_err().contains("bound to authority"),
            "{args:?}"
        );
    }
    // update-ref -d of the default branch: the authority cannot be read.
    let fixture = Fixture::new();
    fixture.initialize();
    git(
        &fixture.root,
        &["update-ref", "-d", "refs/remotes/origin/main"],
    );
    assert!(fixture.level().is_err());
    // update-ref to a weaker commit: the baseline holds (AC-2).
    let fixture = Fixture::new();
    fixture.initialize();
    let weak = commit(&fixture.repo(), WEAK, &[]);
    git(
        &fixture.root,
        &["update-ref", "refs/remotes/origin/main", &weak.to_string()],
    );
    assert_eq!(fixture.level().unwrap(), PolicyLevel::Block);
}

#[test]
fn the_ledger_survives_a_second_write_and_lists_each_repository_once() {
    let fixture = Fixture::new();
    fixture.initialize();
    fixture.initialize();
    let ledger = std::fs::read_to_string(fixture.store.dir().join(LEDGER)).unwrap();
    assert_eq!(ledger.lines().count(), 1);
}

#[test]
fn applying_without_a_store_changes_nothing() {
    let fixture = Fixture::new();
    let repo = fixture.repo();
    let authority = Authority {
        reference: "refs/remotes/origin/main".into(),
        sha: fixture.base.to_string(),
    };
    let bound = Bound {
        remote: "origin",
        authority: Some(&authority),
    };
    let applied = apply(&repo, None, Some(&bound), &policy(WEAK)).unwrap();
    assert!(applied.policy.is_none());
    assert_eq!(applied.state, State::Unsupported);
}
