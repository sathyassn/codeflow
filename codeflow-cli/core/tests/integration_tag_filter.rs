//! Integration test for per-test-file tag filtering.
//!
//! Exercises the runner's `only_tags`/`skip_tags` composition using real
//! `sh -c <command>` invocations against a tempdir, verifying that:
//!
//! * `--only-tag critical` runs only critical-tagged targets;
//! * `--skip-tag low` excludes low-tagged targets;
//! * tag filtering composes with name filtering (AND semantics);
//! * skip wins over only when both match;
//! * untagged targets are included when no positive tag filter is active
//!   and excluded when any `--only-tag` is set.

use std::collections::BTreeMap;

use codeflow_core::testing::config::{ModeCommand, RunnerType, Tag, TargetConfig};
use codeflow_core::testing::runner;

fn tagged(name: &str, tags: &[Tag]) -> TargetConfig {
    TargetConfig {
        name: name.to_string(),
        enabled: true,
        cwd: None,
        env: BTreeMap::new(),
        runner: RunnerType::Custom,
        modes: BTreeMap::from([(
            "full".to_string(),
            ModeCommand {
                command: format!("echo {name}"),
            },
        )]),
        report: None,
        coverage: None,
        ci_skip: None,
        ci_skip_reason: None,
        structural: None,
        tags: tags.to_vec(),
        test_files: Vec::new(),
    }
}

fn untagged(name: &str) -> TargetConfig {
    tagged(name, &[])
}

fn ran_targets(
    results: &[Result<runner::TargetRunResult, codeflow_core::testing::error::TestingError>],
) -> Vec<String> {
    results
        .iter()
        .filter_map(|r| r.as_ref().ok())
        .map(|r| r.target_name.clone())
        .collect()
}

#[test]
fn only_tag_critical_runs_only_critical_targets() {
    let dir = tempfile::tempdir().unwrap();
    let targets = vec![
        tagged("a-crit", &[Tag::Critical]),
        tagged("b-high", &[Tag::High]),
        tagged("c-low", &[Tag::Low]),
    ];
    let results = runner::run_all_targets(
        &targets,
        "full",
        dir.path(),
        false,
        false,
        &[],
        &[],
        &[Tag::Critical],
        &[],
    );
    let ran = ran_targets(&results);
    assert_eq!(ran, vec!["a-crit".to_string()]);
}

#[test]
fn skip_tag_low_excludes_low_targets() {
    let dir = tempfile::tempdir().unwrap();
    let targets = vec![
        tagged("a-crit", &[Tag::Critical]),
        tagged("b-high", &[Tag::High]),
        tagged("c-low", &[Tag::Low]),
    ];
    let results = runner::run_all_targets(
        &targets,
        "full",
        dir.path(),
        false,
        false,
        &[],
        &[],
        &[],
        &[Tag::Low],
    );
    let ran = ran_targets(&results);
    assert_eq!(ran.len(), 2);
    assert!(ran.contains(&"a-crit".to_string()));
    assert!(ran.contains(&"b-high".to_string()));
    assert!(!ran.contains(&"c-low".to_string()));
}

#[test]
fn only_tag_composes_with_only_name_filter_and() {
    let dir = tempfile::tempdir().unwrap();
    let targets = vec![
        tagged("a-crit", &[Tag::Critical]),
        tagged("b-crit", &[Tag::Critical]),
        tagged("c-crit", &[Tag::Critical]),
    ];
    // --only=b,c AND --only-tag=critical → intersection { b, c }
    let results = runner::run_all_targets(
        &targets,
        "full",
        dir.path(),
        false,
        false,
        &["b-crit".to_string(), "c-crit".to_string()],
        &[],
        &[Tag::Critical],
        &[],
    );
    let mut ran = ran_targets(&results);
    ran.sort();
    assert_eq!(ran, vec!["b-crit".to_string(), "c-crit".to_string()]);
}

#[test]
fn skip_tag_wins_over_only_tag_on_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let targets = vec![tagged("a", &[Tag::Critical])];
    // Both filters match → skip wins → zero targets.
    let results = runner::run_all_targets(
        &targets,
        "full",
        dir.path(),
        false,
        false,
        &[],
        &[],
        &[Tag::Critical],
        &[Tag::Critical],
    );
    assert!(ran_targets(&results).is_empty());
}

#[test]
fn untagged_target_excluded_when_only_tag_nonempty() {
    let dir = tempfile::tempdir().unwrap();
    let targets = vec![tagged("a-crit", &[Tag::Critical]), untagged("b-plain")];
    let results = runner::run_all_targets(
        &targets,
        "full",
        dir.path(),
        false,
        false,
        &[],
        &[],
        &[Tag::Critical],
        &[],
    );
    let ran = ran_targets(&results);
    assert_eq!(ran, vec!["a-crit".to_string()]);
}

#[test]
fn untagged_target_included_when_no_only_tag() {
    let dir = tempfile::tempdir().unwrap();
    let targets = vec![tagged("a-crit", &[Tag::Critical]), untagged("b-plain")];
    let results = runner::run_all_targets(
        &targets,
        "full",
        dir.path(),
        false,
        false,
        &[],
        &[],
        &[],
        &[],
    );
    let mut ran = ran_targets(&results);
    ran.sort();
    assert_eq!(ran, vec!["a-crit".to_string(), "b-plain".to_string()]);
}

#[test]
fn target_level_tag_unions_with_per_file_tags() {
    use codeflow_core::testing::config::TestFileEntry;
    let mut t = tagged("mixed", &[Tag::High]);
    t.test_files = vec![TestFileEntry {
        path: "a.sh".to_string(),
        tags: vec![Tag::Critical],
    }];
    // Union of { High } ∪ { Critical } == { Critical, High }. A
    // --only-tag=critical filter should admit this target even though the
    // target-level tag is High.
    assert!(runner::target_tags(&t).contains(&Tag::Critical));
    let dir = tempfile::tempdir().unwrap();
    let results = runner::run_all_targets(
        std::slice::from_ref(&t),
        "full",
        dir.path(),
        false,
        false,
        &[],
        &[],
        &[Tag::Critical],
        &[],
    );
    assert_eq!(ran_targets(&results), vec!["mixed".to_string()]);
}
