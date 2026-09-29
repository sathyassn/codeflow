//! `codeflow update` carries the deny-only presets and the new policy
//! defaults to existing adopters without losing their own choices
//! (ADR-0075, TSK-171 AC-7 and AC-9).
//!
//! The settings cases run the update merge against the presets adopters
//! actually hold: the 2.1.0 presets (`fixtures/presets-2.1.0/`) and the
//! 3.0.0 staging presets the design started from
//! (`docs/verification/evidence/permission-presets/current/`). The policy and
//! Codex cases run the whole update against a project initialized from the
//! real shipped assets.

use std::path::{Path, PathBuf};

use codeflow_core::scaffold::settings_merge::merge_settings_from_baseline;
use codeflow_core::scaffold::{self, DirSource, InitAnswers, InitOptions, Tier, UpdateOptions};
use serde_json::Value;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn shipped(name: &str) -> String {
    text(&repo().join("assets/base/settings").join(name))
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The prior baselines an adopter's update starts from, each with the
/// shipped preset that replaces it.
fn prior_baselines() -> Vec<(String, String, String)> {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/presets-2.1.0");
    let staging = repo().join("docs/verification/evidence/permission-presets/current");
    vec![
        (
            "2.1.0 default".to_string(),
            text(&fixtures.join("default.json")),
            shipped("default.json"),
        ),
        (
            "2.1.0 bypass-sandboxed".to_string(),
            text(&fixtures.join("bypass-sandboxed.json")),
            shipped("bypass-sandboxed.json"),
        ),
        (
            "3.0.0 staging default".to_string(),
            text(&staging.join("default.json")),
            shipped("default.json"),
        ),
        (
            "3.0.0 staging acceptEdits".to_string(),
            text(&staging.join("acceptEdits.json")),
            shipped("acceptEdits.json"),
        ),
    ]
}

/// Add the adopter's own entries to a pristine prior preset.
fn with_adopter_entries(prior: &str) -> String {
    let mut value = json(prior);
    let permissions = value["permissions"].as_object_mut().unwrap();
    for (key, entry) in [
        ("allow", "Bash(my-tool *)"),
        ("ask", "Bash(deploy *)"),
        ("deny", "Read(private/**)"),
    ] {
        permissions
            .entry(key)
            .or_insert_with(|| Value::Array(vec![]))
            .as_array_mut()
            .unwrap()
            .push(Value::from(entry));
    }
    serde_json::to_string_pretty(&value).unwrap()
}

#[test]
fn update_from_prior_presets_retires_the_asks_and_keeps_adopter_entries() {
    for (label, prior, incoming) in prior_baselines() {
        let prior_value = json(&prior);
        let incoming_value = json(&incoming);
        let current = with_adopter_entries(&prior);

        let mut report = vec![];
        let merged =
            merge_settings_from_baseline(&current, Some(&prior), &incoming, &mut report).unwrap();
        let merged_value = json(&merged);
        let perms = &merged_value["permissions"];

        // Every shipped ask is gone; only the adopter's own ask remains.
        assert_eq!(strings(&perms["ask"]), ["Bash(deploy *)"], "{label}");
        assert!(
            strings(&prior_value["permissions"]["ask"]).len() > 5,
            "{label}: the prior preset really asked"
        );
        // The deny array is the new preset's plus the adopter's own entry.
        let mut expected_deny = strings(&incoming_value["permissions"]["deny"]);
        let old_deny = strings(&prior_value["permissions"]["deny"]);
        let new_deny = strings(&incoming_value["permissions"]["deny"]);
        let kept_old: Vec<String> = old_deny
            .iter()
            .filter(|rule| new_deny.contains(rule))
            .cloned()
            .collect();
        assert!(!kept_old.is_empty(), "{label}: no shared deny entries");
        expected_deny.push("Read(private/**)".to_string());
        let mut actual_deny = strings(&perms["deny"]);
        actual_deny.sort();
        expected_deny.sort();
        assert_eq!(actual_deny, expected_deny, "{label}");
        // Allow keeps the adopter's entry and loses nothing the preset ships.
        let allow = strings(&perms["allow"]);
        assert!(allow.contains(&"Bash(my-tool *)".to_string()), "{label}");
        for rule in strings(&incoming_value["permissions"]["allow"]) {
            assert!(allow.contains(&rule), "{label}: allow lost {rule}");
        }
        assert!(
            report
                .iter()
                .any(|l| l.contains("removed retired permissions.ask")),
            "{label}: {report:?}"
        );

        // A second run changes nothing.
        let mut again = vec![];
        let twice =
            merge_settings_from_baseline(&merged, Some(&incoming), &incoming, &mut again).unwrap();
        assert_eq!(twice, merged, "{label}");
        assert!(
            again
                .iter()
                .all(|l| l.contains("kept the project's removal")),
            "{label}: {again:?}"
        );
    }
}

#[test]
fn a_pristine_prior_preset_updates_to_exactly_the_new_arrays() {
    for (label, prior, incoming) in prior_baselines() {
        let mut report = vec![];
        let merged =
            merge_settings_from_baseline(&prior, Some(&prior), &incoming, &mut report).unwrap();
        let merged = json(&merged);
        let incoming = json(&incoming);
        assert!(merged["permissions"].get("ask").is_none(), "{label}");
        assert_eq!(
            merged["permissions"]["deny"], incoming["permissions"]["deny"],
            "{label}"
        );
        assert!(report
            .iter()
            .any(|l| l == "settings: removed permissions.ask: the preset no longer ships it"));
    }
}

#[test]
fn a_removed_deny_stays_removed_and_is_reported() {
    let preset = shipped("default.json");
    let mut adopter = json(&preset);
    adopter["permissions"]["deny"]
        .as_array_mut()
        .unwrap()
        .retain(|rule| rule != "Bash(sudo *)");
    let adopter = serde_json::to_string_pretty(&adopter).unwrap() + "\n";

    for _ in 0..2 {
        let mut report = vec![];
        let merged =
            merge_settings_from_baseline(&adopter, Some(&preset), &preset, &mut report).unwrap();
        assert_eq!(merged, adopter, "the removal stays and nothing else moves");
        assert!(
            report
                .iter()
                .any(|l| l.contains("kept the project's removal") && l.contains("Bash(sudo *)")),
            "{report:?}"
        );
    }
}

#[test]
fn without_a_baseline_new_entries_are_added_and_nothing_is_removed() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/presets-2.1.0");
    let prior = text(&fixtures.join("default.json"));
    let incoming = shipped("default.json");
    let mut report = vec![];
    let merged = merge_settings_from_baseline(&prior, None, &incoming, &mut report).unwrap();
    let merged = json(&merged);
    let prior = json(&prior);
    // Nothing can be told apart from the adopter's own choice, so nothing
    // is removed; every new deny is added.
    assert_eq!(merged["permissions"]["ask"], prior["permissions"]["ask"]);
    let deny = strings(&merged["permissions"]["deny"]);
    for rule in strings(&json(&incoming)["permissions"]["deny"]) {
        assert!(deny.contains(&rule), "missing {rule}");
    }
    assert!(!report.iter().any(|l| l.contains("removed")), "{report:?}");
}

// --- whole-update cases against the real assets ----------------------------

fn isolate_git() {
    static ISOLATE: std::sync::Once = std::sync::Once::new();
    ISOLATE.call_once(|| {
        std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
        std::env::set_var("GIT_CONFIG_SYSTEM", "/dev/null");
    });
}

fn assets() -> DirSource {
    DirSource::new(repo().join("assets"))
}

fn update_opts() -> UpdateOptions {
    UpdateOptions {
        force: false,
        binary_version: env!("CARGO_PKG_VERSION").to_string(),
        diff_out: None,
    }
}

fn init_minimal() -> (tempfile::TempDir, PathBuf) {
    isolate_git();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    scaffold::init(
        &assets(),
        &root,
        &InitOptions {
            tier: Some(Tier::Minimal),
            force: false,
            binary_version: env!("CARGO_PKG_VERSION").to_string(),
            answers: InitAnswers::default(),
        },
    )
    .unwrap();
    (dir, root)
}

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn policy(root: &Path) -> Value {
    json(&text(&root.join(".codeflow/policy.json")))
}

fn notes_for(report: &scaffold::Report, dest: &str) -> Vec<String> {
    report
        .files
        .iter()
        .find(|f| f.dest == dest)
        .map(|f| f.notes.clone())
        .unwrap_or_default()
}

/// An adopter still at the 3.0.0 staging defaults (`privilege_escalation`
/// and `headless_peer_runs` at warn) moves to block, reported; a value the
/// adopter chose that differs from the prior default is kept.
#[test]
fn update_moves_policy_scalars_still_at_the_prior_default() {
    let (_dir, root) = init_minimal();
    let staging = text(
        &repo().join("docs/verification/evidence/permission-presets/current/policy-asset.json"),
    );
    let mut adopter = json(&staging);
    adopter["git"]["dep_audit"] = Value::from("block");
    write(
        root.as_path(),
        ".codeflow/.baseline/.codeflow/policy.json",
        &staging,
    );
    write(
        root.as_path(),
        ".codeflow/policy.json",
        &(serde_json::to_string_pretty(&adopter).unwrap() + "\n"),
    );

    let report = scaffold::update(&assets(), &root, &update_opts()).unwrap();
    let after = policy(&root);
    assert_eq!(after["security"]["privilege_escalation"], "block");
    assert_eq!(after["security"]["headless_peer_runs"], "block");
    assert_eq!(after["git"]["dep_audit"], "block", "adopter value kept");
    assert_eq!(
        after["git"]["discard_uncommitted"], "block",
        "new key added"
    );
    let notes = notes_for(&report, ".codeflow/policy.json");
    for key in [
        "security.privilege_escalation",
        "security.headless_peer_runs",
    ] {
        assert!(
            notes
                .iter()
                .any(|n| n.starts_with(&format!("moved {key} from \"warn\" to \"block\""))),
            "{key}: {notes:?}"
        );
    }
    codeflow_core::hooks::policy_schema::validate_policy(&root).unwrap();

    let before = text(&root.join(".codeflow/policy.json"));
    scaffold::update(&assets(), &root, &update_opts()).unwrap();
    assert_eq!(
        text(&root.join(".codeflow/policy.json")),
        before,
        "second run"
    );
}

/// An adopter on the new defaults who removed the `sudo` deny and set
/// `privilege_escalation: warn` keeps both across updates.
#[test]
fn update_keeps_a_removed_sudo_deny_and_a_warn_privilege_level() {
    let (_dir, root) = init_minimal();
    let mut settings = json(&text(&root.join(".claude/settings.json")));
    settings["permissions"]["deny"]
        .as_array_mut()
        .unwrap()
        .retain(|rule| rule != "Bash(sudo *)");
    write(
        &root,
        ".claude/settings.json",
        &(serde_json::to_string_pretty(&settings).unwrap() + "\n"),
    );
    let mut adopter = policy(&root);
    adopter["security"]["privilege_escalation"] = Value::from("warn");
    write(
        &root,
        ".codeflow/policy.json",
        &(serde_json::to_string_pretty(&adopter).unwrap() + "\n"),
    );
    let settings_before = text(&root.join(".claude/settings.json"));
    let policy_before = text(&root.join(".codeflow/policy.json"));

    for _ in 0..2 {
        let report = scaffold::update(&assets(), &root, &update_opts()).unwrap();
        assert_eq!(text(&root.join(".claude/settings.json")), settings_before);
        assert_eq!(text(&root.join(".codeflow/policy.json")), policy_before);
        let notes = notes_for(&report, ".claude/settings.json");
        assert!(
            notes.iter().any(|n| n.contains("Bash(sudo *)")),
            "the kept removal is reported: {notes:?}"
        );
    }
}

/// AC-9: an adopter's own key in `.codex/config.toml` survives the managed
/// three-way merge that brings the new profiles.
#[test]
fn update_brings_the_codex_profiles_and_keeps_the_adopters_keys() {
    let (_dir, root) = init_minimal();
    let staging = text(
        &repo().join("docs/verification/evidence/permission-presets/current/codex-config.toml"),
    );
    write(&root, ".codeflow/.baseline/.codex/config.toml", &staging);
    let adopted = staging.replacen(
        "web_search = \"live\"\n",
        "web_search = \"live\"\nmodel = \"adopter-model\"\n",
        1,
    );
    assert_ne!(adopted, staging);
    write(&root, ".codex/config.toml", &adopted);
    // The recorded hash is the old baseline's, as after a real 3.0 staging
    // install.
    let manifest_path = root.join(".codeflow/manifest.json");
    let mut manifest = json(&text(&manifest_path));
    manifest["files"][".codex/config.toml"]["sha256"] =
        Value::from(scaffold::sha256_hex(staging.as_bytes()));
    write(
        &root,
        ".codeflow/manifest.json",
        &serde_json::to_string_pretty(&manifest).unwrap(),
    );

    let report = scaffold::update(&assets(), &root, &update_opts()).unwrap();
    assert!(!report.has_conflicts(), "{report}");
    let merged: toml::Value = toml::from_str(&text(&root.join(".codex/config.toml"))).unwrap();
    assert_eq!(merged["model"].as_str(), Some("adopter-model"));
    assert_eq!(merged["default_permissions"].as_str(), Some("cf-guard"));
    assert_eq!(merged["features"]["network_proxy"].as_bool(), Some(true));
    let builder = &merged["permissions"]["cf-builder"];
    assert_eq!(builder["extends"].as_str(), Some("cf-guard"));
    assert_eq!(
        builder["filesystem"][":workspace_roots"][".git"].as_str(),
        Some("write")
    );
    assert_eq!(
        merged["permissions"]["cf-guard"]["filesystem"][":workspace_roots"]
            [".codeflow/policy.json"]
            .as_str(),
        Some("read")
    );
}

/// AC-6: the Grok profile is installed when absent and never overwritten.
#[test]
fn the_grok_profile_is_installed_only_when_absent() {
    let (_dir, root) = init_minimal();
    let path = root.join(".grok/sandbox.toml");
    let shipped_profile = text(&repo().join("assets/base/grok/sandbox.toml"));
    assert_eq!(text(&path), shipped_profile, "init installs it");
    assert!(root.join(".codex/rules/codeflow.rules").is_file());

    let edited = format!("{shipped_profile}\n# adopter note\n");
    std::fs::write(&path, &edited).unwrap();
    scaffold::update(&assets(), &root, &update_opts()).unwrap();
    assert_eq!(text(&path), edited, "update never overwrites it");

    std::fs::remove_file(&path).unwrap();
    scaffold::update(&assets(), &root, &update_opts()).unwrap();
    assert_eq!(
        text(&path),
        shipped_profile,
        "update installs it when absent"
    );
}
