//! TSK-280 (issue 118): a policy key has one placement rule. A new key lives
//! in the policy schema and the code default only. The shipped template
//! (`assets/base/policy.json`) and this repository's installed policy
//! (`.codeflow/policy.json`) may name only keys the last released binary
//! already knows, because `codeflow update` copies template keys into an
//! adopter's policy and an older binary rejects an unknown key, which blocks
//! every commit. The rule is written in the module documentation of
//! `crates/codeflow-core/src/hooks/policy_schema.rs` and in
//! `docs/policy-reference.md` ("Where a new key goes").
//!
//! The last released binary's keys are the committed list
//! `tests/fixtures/policy-keys-last-release.txt`. This file holds:
//!
//! - the check over both policy files, with a planted key proving it fails;
//! - a shape guard on the list;
//! - the journey: a project at the last release runs `codeflow update` with
//!   this build's template, and the policy file it ends with names only keys
//!   the last release knows.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use codeflow_core::hooks::policy_schema::{schema, DEPRECATED_KEYS};
use codeflow_core::scaffold::{
    self, AssetSource, DirSource, InitAnswers, InitOptions, Tier, UpdateOptions,
};
use serde_json::Value;

/// The keys of the last released binary, one dotted path per line.
const LAST_RELEASE_KEYS: &str = include_str!("fixtures/policy-keys-last-release.txt");

/// The template the last release shipped (v3.1.1), as a project at that
/// release holds it.
const LAST_RELEASE_TEMPLATE: &str = include_str!("fixtures/policy-last-release.json");

/// The two policy files the rule covers, repository-relative.
const GUARDED_FILES: [&str; 2] = ["assets/base/policy.json", ".codeflow/policy.json"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn last_release_keys() -> BTreeSet<String> {
    LAST_RELEASE_KEYS
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// The dotted leaf paths a policy file names, in the schema's own terms:
/// `git`, `security` and `guidance` are sections whose members are leaves
/// (`git.pr_summary`), and every other top-level key is a leaf by itself.
fn named_keys(policy_text: &str) -> Vec<String> {
    let value: Value = serde_json::from_str(policy_text).expect("policy file is JSON");
    let mut keys = Vec::new();
    for (key, member) in value.as_object().expect("policy file is an object") {
        match (key.as_str(), member.as_object()) {
            ("git" | "security" | "guidance", Some(section)) => {
                keys.extend(section.keys().map(|leaf| format!("{key}.{leaf}")));
            }
            _ => keys.push(key.clone()),
        }
    }
    keys
}

/// The keys `policy_text` names that `released` does not list.
fn unknown_to_release(policy_text: &str, released: &BTreeSet<String>) -> Vec<String> {
    named_keys(policy_text)
        .into_iter()
        .filter(|key| !released.contains(key))
        .collect()
}

/// The failure text: the rule, the keys and the sanctioned fix.
fn placement_message(label: &str, unknown: &[String]) -> String {
    format!(
        "{label} names {}, which the last released binary does not know. A new policy key \
         lives in the schema (crates/codeflow-core/src/hooks/policy_schema.rs) and the code \
         default only, so `codeflow update` never writes it into an adopter's policy for an \
         older pinned binary to reject. Remove the key from the file. If the release that \
         introduced it has published, refresh tests/fixtures/policy-keys-last-release.txt \
         from that release tag first. See \"Where a new key goes\" in docs/policy-reference.md.",
        unknown.join(", ")
    )
}

#[test]
fn shipped_and_installed_policy_name_only_keys_the_last_release_knows() {
    let released = last_release_keys();
    let root = repo_root();
    for rel in GUARDED_FILES {
        let unknown = unknown_to_release(&read(&root.join(rel)), &released);
        assert!(unknown.is_empty(), "{}", placement_message(rel, &unknown));
    }
}

#[test]
fn a_planted_key_fails_the_check() {
    // Fixed inputs, so this test judges the check and not the repository's files.
    let released = last_release_keys();
    assert!(
        unknown_to_release(LAST_RELEASE_TEMPLATE, &released).is_empty(),
        "the last release's own template must pass"
    );

    // A key no release knows.
    let mut value: Value = serde_json::from_str(LAST_RELEASE_TEMPLATE).unwrap();
    value["git"]["planted_future_key"] = Value::from("block");
    value["planted_top_level_key"] = Value::from(1);
    assert_eq!(
        unknown_to_release(&serde_json::to_string(&value).unwrap(), &released),
        ["git.planted_future_key", "planted_top_level_key"]
    );

    // A key this build's schema knows but the last release does not: the list
    // minus that key, and a file that names it.
    let mut older = released.clone();
    assert!(older.remove("git.pr_summary"));
    let mut value: Value = serde_json::from_str(LAST_RELEASE_TEMPLATE).unwrap();
    value["git"]["pr_summary"] = Value::from("block");
    assert_eq!(
        unknown_to_release(&serde_json::to_string(&value).unwrap(), &older),
        ["git.pr_summary"]
    );
}

#[test]
fn the_last_release_key_list_is_well_formed() {
    let lines: Vec<&str> = LAST_RELEASE_KEYS
        .lines()
        .filter(|line| !line.starts_with('#'))
        .collect();
    assert!(
        lines.iter().all(|line| !line.trim().is_empty()),
        "blank line in the key list"
    );
    let mut sorted = lines.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(lines, sorted, "keep the list sorted, one key per line");
    // A typo in the list would let a key that no binary knows into the
    // template, so each entry is a schema key or a deprecated one.
    let known: BTreeSet<&str> = schema()
        .iter()
        .map(|spec| spec.path)
        .chain(DEPRECATED_KEYS.iter().map(|(key, _)| *key))
        .collect();
    for line in &lines {
        assert!(
            known.contains(line),
            "{line} is in the last release key list but this build's schema does not know it"
        );
    }
}

/// The current asset tree with some assets replaced, serving a project as an
/// older release scaffolded it.
struct Overlay {
    current: DirSource,
    overrides: BTreeMap<String, Vec<u8>>,
}

impl AssetSource for Overlay {
    fn read(&self, path: &str) -> std::io::Result<Option<Vec<u8>>> {
        match self.overrides.get(path) {
            Some(value) => Ok(Some(value.clone())),
            None => self.current.read(path),
        }
    }
}

fn isolate_git() {
    static ISOLATE: std::sync::Once = std::sync::Once::new();
    ISOLATE.call_once(|| {
        std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
        std::env::set_var("GIT_CONFIG_SYSTEM", "/dev/null");
    });
}

#[test]
fn update_leaves_a_last_release_policy_readable_by_the_last_release() {
    isolate_git();
    let current = DirSource::new(repo_root().join("assets"));
    let last_release = Overlay {
        current: DirSource::new(repo_root().join("assets")),
        overrides: BTreeMap::from([(
            "base/policy.json".to_string(),
            LAST_RELEASE_TEMPLATE.as_bytes().to_vec(),
        )]),
    };

    // A project at the last release: its scaffold, with the policy file that
    // release shipped.
    let project = tempfile::tempdir().expect("project tempdir");
    scaffold::init(
        &last_release,
        project.path(),
        &InitOptions {
            tier: Some(Tier::Minimal),
            force: false,
            binary_version: "3.1.1".to_string(),
            answers: InitAnswers {
                product_one_liner: Some("A billing service for small clinics.".to_string()),
                areas: Some(vec!["api".to_string()]),
                permission_preset: None,
            },
        },
    )
    .expect("init at the last release succeeds");
    let policy_path = project.path().join(".codeflow/policy.json");
    let released = last_release_keys();
    let before = unknown_to_release(&read(&policy_path), &released);
    assert!(
        before.is_empty(),
        "the fixture project is not at the last release: {before:?}"
    );

    // `codeflow update` with this build's template.
    scaffold::update(
        &current,
        project.path(),
        &UpdateOptions {
            force: false,
            binary_version: env!("CARGO_PKG_VERSION").to_string(),
            diff_out: None,
        },
    )
    .expect("update succeeds");

    // The last release's schema must still load the result.
    let after = read(&policy_path);
    let unknown = unknown_to_release(&after, &released);
    assert!(
        unknown.is_empty(),
        "after `codeflow update` the policy names {}, which the last released binary rejects \
         as unknown, blocking every commit for a project pinned to it.\n{}",
        unknown.join(", "),
        placement_message("assets/base/policy.json", &unknown)
    );
}
