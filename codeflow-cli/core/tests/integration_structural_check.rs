//! Integration test for the `codeflow test structural-check` subcommand.
//!
//! Uses the library API (`codeflow_core::testing::structural`) directly —
//! equivalent to `codeflow test structural-check` but avoids shelling out to
//! a freshly-built binary. A planted-missing-test fixture confirms the check
//! detects real coverage gaps rather than silently passing.

use std::fs;
use std::path::Path;

use codeflow_core::testing::config::{
    ExclusionEntry, ModeCommand, PatternMapEntry, RunnerType, StructuralConfig,
    StructuralExclusions, TargetConfig, TestConfig,
};
use codeflow_core::testing::structural;

fn write_file(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
}

fn shell_target_with_structural(root_has_exclusions: bool) -> TargetConfig {
    let exclusions = if root_has_exclusions {
        Some(StructuralExclusions {
            no_test_required: vec![ExclusionEntry {
                pattern: "src/helpers/*".to_string(),
                reason: Some("helpers covered via dependent scripts".to_string()),
            }],
            orphan_allowed: vec![ExclusionEntry {
                pattern: "tests/framework/*".to_string(),
                reason: None,
            }],
        })
    } else {
        None
    };

    TargetConfig {
        name: "shell".to_string(),
        enabled: true,
        cwd: None,
        env: std::collections::BTreeMap::new(),
        runner: RunnerType::Custom,
        modes: std::collections::BTreeMap::from([(
            "full".to_string(),
            ModeCommand {
                command: "true".to_string(),
            },
        )]),
        report: None,
        coverage: None,
        ci_skip: None,
        ci_skip_reason: None,
        structural: Some(StructuralConfig {
            source_glob: vec!["src/**/*.sh".to_string()],
            test_glob: vec!["tests/test-*.sh".to_string()],
            pattern_map: vec![PatternMapEntry {
                source: r"^src/([^/]+)\.sh$".to_string(),
                test: "tests/test-$1.sh".to_string(),
            }],
            exclusions,
        }),
        tags: Vec::new(),
        test_files: Vec::new(),
    }
}

#[test]
fn end_to_end_pass_on_fully_covered_fixture() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    // Two sources with matching tests: fully covered.
    write_file(&root.join("src/one.sh"), "# one");
    write_file(&root.join("src/two.sh"), "# two");
    write_file(&root.join("tests/test-one.sh"), "# test one");
    write_file(&root.join("tests/test-two.sh"), "# test two");

    let target = shell_target_with_structural(false);
    let results = structural::validate_all(std::slice::from_ref(&target), root).unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].pass, "fully-covered fixture should pass");
    assert_eq!(results[0].missing.len(), 0);
    assert_eq!(results[0].orphans.len(), 0);
    assert_eq!(results[0].sources_scanned, 2);
    assert_eq!(results[0].tests_scanned, 2);
}

#[test]
fn end_to_end_detects_planted_missing_test() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    // Plant a source without its test — the structural check must catch it.
    write_file(&root.join("src/alpha.sh"), "# alpha");
    write_file(&root.join("src/beta.sh"), "# beta");
    write_file(&root.join("tests/test-alpha.sh"), "# test");
    // DELIBERATELY NOT writing tests/test-beta.sh

    let target = shell_target_with_structural(false);
    let results = structural::validate_all(std::slice::from_ref(&target), root).unwrap();
    assert_eq!(results.len(), 1);
    assert!(!results[0].pass, "missing test should fail");
    assert_eq!(results[0].missing.len(), 1);
    assert_eq!(results[0].missing[0].source, "src/beta.sh");
    assert_eq!(
        results[0].missing[0].expected_test.as_deref(),
        Some("tests/test-beta.sh")
    );
}

#[test]
fn end_to_end_detects_orphaned_test() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    // No source; orphan test present. The structural check must catch it.
    write_file(&root.join("tests/test-ghost.sh"), "# ghost");

    let target = shell_target_with_structural(false);
    let results = structural::validate_all(std::slice::from_ref(&target), root).unwrap();
    assert_eq!(results.len(), 1);
    assert!(!results[0].pass);
    assert_eq!(results[0].orphans.len(), 1);
    assert_eq!(results[0].orphans[0].test, "tests/test-ghost.sh");
}

#[test]
fn end_to_end_exclusions_suppress_both_directions() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    // helpers/ source with no test → suppressed by `no_test_required`.
    write_file(&root.join("src/helpers/util.sh"), "# helper");
    // tests/framework orphan → suppressed by `orphan_allowed`.
    write_file(&root.join("tests/framework/shared.sh"), "# framework");
    // A real pair that should pass.
    write_file(&root.join("src/main.sh"), "# main");
    write_file(&root.join("tests/test-main.sh"), "# test main");

    let target = shell_target_with_structural(true);
    // Extend source glob so helpers/ is covered too.
    let mut t = target;
    t.structural.as_mut().unwrap().source_glob = vec!["src/**/*.sh".to_string()];
    // Extend test glob to catch framework/.
    t.structural.as_mut().unwrap().test_glob =
        vec!["tests/**/*.sh".to_string(), "tests/test-*.sh".to_string()];

    let results = structural::validate_all(std::slice::from_ref(&t), root).unwrap();
    assert_eq!(results.len(), 1);
    assert!(
        results[0].pass,
        "exclusions should suppress all findings — got {:?}",
        results[0]
    );
    assert!(results[0].sources_excluded >= 1);
    assert!(results[0].tests_excluded >= 1);
}

#[test]
fn end_to_end_validate_all_skips_targets_without_structural_block() {
    let dir = tempfile::tempdir().unwrap();

    let with_block = shell_target_with_structural(false);
    let mut without_block = shell_target_with_structural(false);
    without_block.name = "no-check".to_string();
    without_block.structural = None;

    let results = structural::validate_all(&[with_block, without_block], dir.path()).unwrap();
    assert_eq!(
        results.len(),
        1,
        "target without structural block must be skipped"
    );
    assert_eq!(results[0].target, "shell");
}

#[test]
fn end_to_end_json_roundtrip_preserves_all_fields() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_file(&root.join("src/alpha.sh"), "# a");
    // no test

    let target = shell_target_with_structural(false);
    let results = structural::validate_all(std::slice::from_ref(&target), root).unwrap();

    // Serialize and deserialize — the CLI --format=json path depends on this.
    let json = serde_json::to_string(&results[0]).unwrap();
    let back: structural::StructuralResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back.target, "shell");
    assert_eq!(back.missing.len(), 1);
    assert_eq!(back.missing[0].source, "src/alpha.sh");
    assert!(!back.pass);
    assert_eq!(back.sources_scanned, 1);
}

/// Validates that the pattern `test-config.schema.json` supports a live
/// structural block by building a full `TestConfig` and serializing it.
/// This is a regression guard against schema vs. Rust model drift.
#[test]
fn end_to_end_full_config_with_structural_block_serializes_cleanly() {
    let target = shell_target_with_structural(true);
    let config = TestConfig {
        description: Some("integration test fixture".to_string()),
        schema_ref: Some(".codeflow/schemas/test-config.schema.json".to_string()),
        schema_version: "1.0".to_string(),
        execution: codeflow_core::testing::config::ExecutionConfig::default(),
        defaults: codeflow_core::testing::config::DefaultsConfig::default(),
        targets: vec![target],
    };

    let json = serde_json::to_string_pretty(&config).unwrap();
    assert!(
        json.contains("\"structural\""),
        "serialized config must include structural"
    );
    assert!(
        json.contains("\"pattern_map\""),
        "serialized config must include pattern_map"
    );
    assert!(
        json.contains("\"no_test_required\""),
        "serialized config must include no_test_required"
    );
}
