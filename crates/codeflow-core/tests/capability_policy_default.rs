//! TSK-1136 AC-1: the tier default for `git.pr_capability_impact`.
//!
//! `init` and `update` write `off` at minimal and `warn` at standard and
//! full, and only when the release that reads the policy knows the key: the
//! running binary at `init`, the project's recorded pin at `update`. A
//! binary older than the key rejects the whole policy file ("Where a new key
//! goes" in docs/policy-reference.md), so a project still pinned to 3.x gets
//! the key on the update after the one that raises its pin. A level the
//! project chose is never moved.

use std::collections::BTreeMap;
use std::path::Path;

use codeflow_core::scaffold::{
    self, AssetSource, DirSource, InitAnswers, InitOptions, Report, Tier, UpdateOptions,
};
use serde_json::Value;

#[path = "support/git_isolation.rs"]
mod git_isolation;
use git_isolation::isolate_git;

const KEY: &str = "/git/pr_capability_impact";
const CURRENT: &str = env!("CARGO_PKG_VERSION");
const LAST_RELEASE_TEMPLATE: &str = include_str!("fixtures/policy-last-release.json");

fn assets() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

/// The current asset tree with the policy template the last release shipped.
struct LastRelease {
    current: DirSource,
    overrides: BTreeMap<String, Vec<u8>>,
}

impl LastRelease {
    fn new() -> Self {
        Self {
            current: DirSource::new(assets()),
            overrides: BTreeMap::from([(
                "base/policy.json".to_string(),
                LAST_RELEASE_TEMPLATE.as_bytes().to_vec(),
            )]),
        }
    }
}

impl AssetSource for LastRelease {
    fn read(&self, path: &str) -> std::io::Result<Option<Vec<u8>>> {
        match self.overrides.get(path) {
            Some(value) => Ok(Some(value.clone())),
            None => self.current.read(path),
        }
    }
}

fn init(source: &dyn AssetSource, root: &Path, tier: Tier, version: &str) {
    git_isolation::own_repository(root);
    scaffold::init(
        source,
        root,
        &InitOptions {
            tier: Some(tier),
            force: false,
            binary_version: version.to_string(),
            answers: InitAnswers {
                product_one_liner: Some("A billing service for small clinics.".to_string()),
                areas: Some(vec!["api".to_string()]),
                permission_preset: None,
            },
        },
    )
    .expect("init succeeds");
}

fn update(root: &Path) -> Report {
    scaffold::update(
        &DirSource::new(assets()),
        root,
        &UpdateOptions {
            force: false,
            binary_version: CURRENT.to_string(),
            diff_out: None,
        },
    )
    .expect("update succeeds")
}

fn policy(root: &Path) -> Value {
    let text = std::fs::read_to_string(root.join(".codeflow/policy.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn level(root: &Path) -> Option<String> {
    policy(root)
        .pointer(KEY)
        .map(|v| v.as_str().unwrap().to_string())
}

fn set_level(root: &Path, level: &str) {
    let mut value = policy(root);
    value["git"]["pr_capability_impact"] = Value::from(level);
    let mut text = serde_json::to_string_pretty(&value).unwrap();
    text.push('\n');
    std::fs::write(root.join(".codeflow/policy.json"), text).unwrap();
}

/// Record `version` as the project's pin, as `codeflow update --pin` does.
fn set_pin(root: &Path, version: &str) {
    let path = root.join(".codeflow/project.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    let mut found = false;
    let out: String = text
        .lines()
        .map(|line| {
            if line.starts_with("scaffold_version") {
                found = true;
                format!("scaffold_version = \"{version}\"\n")
            } else {
                format!("{line}\n")
            }
        })
        .collect();
    assert!(found, "project.toml records scaffold_version");
    std::fs::write(path, out).unwrap();
}

fn expected(tier: Tier) -> &'static str {
    match tier {
        Tier::Minimal => "off",
        Tier::Standard | Tier::Full => "warn",
    }
}

const TIERS: [Tier; 3] = [Tier::Minimal, Tier::Standard, Tier::Full];

#[test]
fn a_fresh_init_writes_the_tier_level() {
    isolate_git();
    for tier in TIERS {
        let project = tempfile::tempdir().unwrap();
        init(&DirSource::new(assets()), project.path(), tier, CURRENT);
        assert_eq!(
            level(project.path()).as_deref(),
            Some(expected(tier)),
            "{tier:?}"
        );
        // A rerun of update changes nothing.
        update(project.path());
        assert_eq!(level(project.path()).as_deref(), Some(expected(tier)));
    }
}

#[test]
fn an_init_by_a_binary_older_than_the_key_never_writes_it() {
    isolate_git();
    let project = tempfile::tempdir().unwrap();
    init(&LastRelease::new(), project.path(), Tier::Full, "3.1.1");
    assert_eq!(level(project.path()), None);
}

#[test]
fn a_3x_project_gains_the_key_on_the_update_after_its_pin_rises() {
    isolate_git();
    for tier in TIERS {
        let project = tempfile::tempdir().unwrap();
        init(&LastRelease::new(), project.path(), tier, "3.1.1");
        assert_eq!(level(project.path()), None);

        // One step: the update runs while the project still pins 3.1.1, so
        // the key is withheld and the report says why.
        let report = update(project.path()).shown().to_string();
        assert_eq!(level(project.path()), None, "{tier:?}");
        assert!(
            report.contains("git.pr_capability_impact was not added"),
            "{tier:?}: {report}"
        );

        // The update recorded the new pin, so the next one adds the key.
        let report = update(project.path()).shown().to_string();
        assert_eq!(
            level(project.path()).as_deref(),
            Some(expected(tier)),
            "{tier:?}: {report}"
        );
        assert!(!report.contains("was not added"), "{report}");
    }
}

#[test]
fn a_3x_project_that_raised_its_pin_first_gains_the_key_at_once() {
    isolate_git();
    for tier in TIERS {
        let project = tempfile::tempdir().unwrap();
        init(&LastRelease::new(), project.path(), tier, "3.1.1");
        set_pin(project.path(), CURRENT);
        update(project.path());
        assert_eq!(
            level(project.path()).as_deref(),
            Some(expected(tier)),
            "{tier:?}"
        );
    }
}

#[test]
fn a_repeated_update_keeps_the_level_the_project_chose() {
    isolate_git();
    for (tier, chosen) in [
        (Tier::Standard, "block"),
        (Tier::Full, "block"),
        (Tier::Minimal, "block"),
        (Tier::Minimal, "warn"),
        (Tier::Standard, "off"),
        (Tier::Full, "off"),
    ] {
        let project = tempfile::tempdir().unwrap();
        init(&LastRelease::new(), project.path(), tier, "3.1.1");
        set_pin(project.path(), CURRENT);
        update(project.path());
        set_level(project.path(), chosen);
        update(project.path());
        update(project.path());
        assert_eq!(
            level(project.path()).as_deref(),
            Some(chosen),
            "{tier:?} {chosen}"
        );
    }
}

#[test]
fn a_project_that_removed_the_key_keeps_it_removed() {
    isolate_git();
    let project = tempfile::tempdir().unwrap();
    init(
        &DirSource::new(assets()),
        project.path(),
        Tier::Standard,
        CURRENT,
    );
    let mut value = policy(project.path());
    value["git"]
        .as_object_mut()
        .unwrap()
        .remove("pr_capability_impact");
    std::fs::write(
        project.path().join(".codeflow/policy.json"),
        serde_json::to_string_pretty(&value).unwrap() + "\n",
    )
    .unwrap();
    update(project.path());
    assert_eq!(level(project.path()), None);
}
