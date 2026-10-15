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
//! The last released binary's schema is the committed file
//! `tests/fixtures/policy-schema-last-release.tsv`: each key with its kind and
//! valid values, as v3.1.1 declares them. This file holds:
//!
//! - the check over both policy files: every key is one the last release
//!   knows, and every value has the kind and valid values the last release
//!   requires, with a planted key and a planted bad value proving it fails;
//! - a shape guard on the pinned schema;
//! - the journey: a project at the last release runs `codeflow update` with
//!   this build's template, and the policy file it ends with passes the same
//!   check.
//!
//! The pinned schema stands in for the released validator. It does not repeat
//! that validator's key-specific checks (a regular expression, a remote name,
//! a non-empty `git.required_checks`, the shape inside a profile entry) or its
//! final whole-file deserialize, and a real v3.1.1 binary is not run, so these
//! are fast unit checks. The authority for AC-4 is the released-binary journey
//! in `crates/codeflow-cli/tests/policy_last_release_journey.rs`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use codeflow_core::hooks::policy_schema::{schema, DEPRECATED_KEYS};
use codeflow_core::scaffold::{
    self, AssetSource, DirSource, InitAnswers, InitOptions, Tier, UpdateOptions,
};
use serde_json::Value;

/// The last released binary's schema: `key<TAB>kind` per line.
const LAST_RELEASE_SCHEMA: &str = include_str!("fixtures/policy-schema-last-release.tsv");

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

/// The pinned schema rows, `(key, kind)`, in file order.
fn schema_rows() -> Vec<(&'static str, &'static str)> {
    LAST_RELEASE_SCHEMA
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .map(|line| {
            line.split_once('\t')
                .unwrap_or_else(|| panic!("schema line without a tab: {line:?}"))
        })
        .collect()
}

/// The last release's keys with their kinds.
fn last_release_schema() -> BTreeMap<String, String> {
    schema_rows()
        .into_iter()
        .map(|(key, kind)| (key.to_string(), kind.to_string()))
        .collect()
}

/// Why the last release would refuse `value` for a key of `kind`, if it would.
fn kind_problem(kind: &str, value: &Value) -> Option<String> {
    let ok = match kind {
        "level" => value
            .as_str()
            .is_some_and(|s| ["off", "warn", "allow", "block"].contains(&s)),
        "uint" => value.as_u64().is_some_and(|n| u32::try_from(n).is_ok()),
        "string-list" => value
            .as_array()
            .is_some_and(|items| items.iter().all(Value::is_string)),
        "string" => value.is_string(),
        "profiles" | "retry-entries" => value.is_array(),
        "mapping" => value.is_object(),
        "deprecated" => true,
        _ => {
            let allowed = kind
                .strip_prefix("enum:")
                .unwrap_or_else(|| panic!("unknown kind {kind:?} in the pinned schema"));
            value
                .as_str()
                .is_some_and(|s| allowed.split('|').any(|a| a == s))
        }
    };
    (!ok).then(|| format!("{value} is not a valid {kind}"))
}

/// Every way the last release would refuse `policy_text`: a key it does not
/// know, or a value without the kind and valid values it requires. Each entry
/// leads with the key.
fn release_problems(policy_text: &str, released: &BTreeMap<String, String>) -> Vec<String> {
    let value: Value = serde_json::from_str(policy_text).expect("policy file is JSON");
    let mut problems = Vec::new();
    for (key, member) in value.as_object().expect("policy file is an object") {
        let leaves: Vec<(String, &Value)> = match (key.as_str(), member.as_object()) {
            ("git" | "security" | "guidance", Some(section)) => section
                .iter()
                .map(|(leaf, v)| (format!("{key}.{leaf}"), v))
                .collect(),
            _ => vec![(key.clone(), member)],
        };
        for (path, leaf) in leaves {
            match released.get(&path) {
                None => problems.push(format!("{path} (unknown to the last release)")),
                Some(kind) => problems
                    .extend(kind_problem(kind, leaf).map(|problem| format!("{path} ({problem})"))),
            }
        }
    }
    problems
}

/// The failure text: the rule, the problems and the sanctioned fix.
fn placement_message(label: &str, problems: &[String]) -> String {
    format!(
        "{label} holds what the last released binary would refuse: {}. A new policy key \
         lives in the schema (crates/codeflow-core/src/hooks/policy_schema.rs) and the code \
         default only, so `codeflow update` never writes it into an adopter's policy for an \
         older pinned binary to reject. Remove a new key from the file, or restore a changed value. If the release that \
         introduced it has published, refresh tests/fixtures/policy-schema-last-release.tsv \
         from that release tag first. See \"Where a new key goes\" in docs/policy-reference.md.",
        problems.join(", ")
    )
}

#[test]
fn shipped_and_installed_policy_hold_only_what_the_last_release_accepts() {
    let released = last_release_schema();
    let root = repo_root();
    for rel in GUARDED_FILES {
        let problems = release_problems(&read(&root.join(rel)), &released);
        assert!(problems.is_empty(), "{}", placement_message(rel, &problems));
    }
}

#[test]
fn a_planted_key_fails_the_check() {
    // Fixed inputs, so this test judges the check and not the repository's files.
    let released = last_release_schema();
    assert!(
        release_problems(LAST_RELEASE_TEMPLATE, &released).is_empty(),
        "the last release's own template must pass"
    );

    // A key no release knows.
    let mut value: Value = serde_json::from_str(LAST_RELEASE_TEMPLATE).unwrap();
    value["git"]["planted_future_key"] = Value::from("block");
    value["planted_top_level_key"] = Value::from(1);
    assert_eq!(
        release_problems(&serde_json::to_string(&value).unwrap(), &released),
        [
            "git.planted_future_key (unknown to the last release)",
            "planted_top_level_key (unknown to the last release)"
        ]
    );

    // A key this build's schema knows but the last release does not: the
    // pinned schema minus that key, and a file that names it.
    let mut older = released.clone();
    assert!(older.remove("git.pr_summary").is_some());
    let mut value: Value = serde_json::from_str(LAST_RELEASE_TEMPLATE).unwrap();
    value["git"]["pr_summary"] = Value::from("block");
    assert_eq!(
        release_problems(&serde_json::to_string(&value).unwrap(), &older),
        ["git.pr_summary (unknown to the last release)"]
    );
}

#[test]
fn a_bad_value_under_a_known_key_fails_the_check() {
    // The released validator also rejects a wrong kind or an unlisted value
    // for a key it knows; a check on key names alone would pass these.
    let released = last_release_schema();
    for (section, key, bad) in [
        ("git", "commit_desc_max_len", serde_json::json!("fifty")),
        ("git", "commit_desc_max_len", serde_json::json!(-1)),
        ("git", "commit_format", serde_json::json!("strict")),
        ("git", "commit_types", serde_json::json!(["feat", 2])),
        ("git", "pr_breaking_level", serde_json::json!("huge")),
        ("git", "automation_profiles", serde_json::json!({})),
        ("security", "dangerous_commands", serde_json::json!("warn")),
    ] {
        let mut value: Value = serde_json::from_str(LAST_RELEASE_TEMPLATE).unwrap();
        value[section][key] = bad.clone();
        let problems = release_problems(&serde_json::to_string(&value).unwrap(), &released);
        assert_eq!(problems.len(), 1, "{section}.{key} = {bad}: {problems:?}");
        assert!(
            problems[0].starts_with(&format!("{section}.{key} (")),
            "{problems:?}"
        );
    }
}

#[test]
fn the_pinned_schema_is_well_formed() {
    let rows = schema_rows();
    let keys: Vec<&str> = rows.iter().map(|(key, _)| *key).collect();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(keys, sorted, "keep the schema sorted, one key per line");
    for (key, kind) in &rows {
        assert!(
            [
                "level",
                "uint",
                "string-list",
                "string",
                "profiles",
                "mapping",
                "retry-entries",
                "deprecated"
            ]
            .contains(kind)
                || kind
                    .strip_prefix("enum:")
                    .is_some_and(|values| !values.is_empty()),
            "{key} has the kind {kind:?}, which the check does not read"
        );
    }
    // A typo in the schema would let a key that no binary knows into the
    // template, so each entry is a schema key or a deprecated one.
    let known: BTreeSet<&str> = schema()
        .iter()
        .map(|spec| spec.path)
        .chain(DEPRECATED_KEYS.iter().map(|(key, _)| *key))
        .collect();
    for key in keys {
        assert!(
            known.contains(key),
            "{key} is in the last release schema but this build's schema does not know it"
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
fn update_leaves_a_last_release_policy_accepted_by_the_last_release_schema() {
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
    let released = last_release_schema();
    let before = release_problems(&read(&policy_path), &released);
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

    // The last release's schema must still accept the result.
    let problems = release_problems(&read(&policy_path), &released);
    assert!(
        problems.is_empty(),
        "after `codeflow update` the policy would be refused by the last released binary, \
         blocking every commit for a project pinned to it.\n{}",
        placement_message(".codeflow/policy.json", &problems)
    );
}
