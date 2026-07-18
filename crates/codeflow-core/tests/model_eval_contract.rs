//! Deterministic traceability guards for the shipped model-evaluation kit.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(root().join(relative))
        .unwrap_or_else(|error| panic!("read {relative}: {error}"))
}

fn json(relative: &str) -> Value {
    serde_json::from_str(&read(relative))
        .unwrap_or_else(|error| panic!("parse {relative}: {error}"))
}

fn normalized(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn every_hard_requirement_has_source_markers_and_behavioral_cases() {
    let requirements =
        json("assets/base/agents/skills/cf-evaluate-model/resources/requirements.json");
    let cases = json("assets/base/agents/skills/cf-evaluate-model/resources/cases.json");
    let requirement_entries = requirements["requirements"]
        .as_array()
        .expect("requirements array");
    let case_entries = cases["cases"].as_array().expect("cases array");

    let mut requirement_ids = BTreeSet::new();
    let mut hard_ids = BTreeSet::new();
    for requirement in requirement_entries {
        let id = requirement["id"].as_str().expect("requirement id");
        assert!(requirement_ids.insert(id), "duplicate requirement {id}");
        if requirement["level"] == "hard" {
            hard_ids.insert(id);
        }
        let sources = requirement["sources"].as_array().expect("sources array");
        assert!(!sources.is_empty(), "{id}: no sources");
        for source in sources {
            let path = source["path"].as_str().expect("source path");
            let content = normalized(&read(path));
            for marker in source["markers"].as_array().expect("marker array") {
                let marker = marker.as_str().expect("marker string");
                assert!(
                    content.contains(&normalized(marker)),
                    "{id}: marker missing from {path}: {marker}"
                );
            }
        }
    }

    let mut covered = BTreeSet::new();
    for case in case_entries {
        for requirement in case["requirements"].as_array().expect("case requirements") {
            let id = requirement.as_str().expect("case requirement id");
            assert!(requirement_ids.contains(id), "case names unknown {id}");
            covered.insert(id);
        }
    }
    let missing: Vec<_> = hard_ids.difference(&covered).collect();
    assert!(
        missing.is_empty(),
        "hard requirements without cases: {missing:?}"
    );
}

#[test]
fn cases_and_exact_fixture_registry_are_consistent() {
    let cases = json("assets/base/agents/skills/cf-evaluate-model/resources/cases.json");
    let fixtures = json("assets/base/agents/skills/cf-evaluate-model/resources/fixtures.json");
    assert!(cases["full_trials"].as_u64().unwrap_or_default() >= 3);

    let fixture_entries = fixtures["fixtures"].as_array().expect("fixtures array");
    let mut fixture_ids = BTreeSet::new();
    for fixture in fixture_entries {
        let id = fixture["id"].as_str().expect("fixture id");
        assert!(fixture_ids.insert(id), "duplicate fixture {id}");
        assert!(matches!(
            fixture["tier"].as_str(),
            Some("standard" | "full")
        ));
        let files = fixture["files"].as_object().expect("fixture files");
        assert!(!files.is_empty(), "{id}: empty file overlay");
        for path in files.keys() {
            assert!(!path.starts_with('/'), "{id}: absolute fixture path");
            assert!(
                !path.split('/').any(|part| part.is_empty() || part == ".."),
                "{id}: unsafe fixture path {path}"
            );
        }
    }

    let mut case_ids = BTreeSet::new();
    let mut used_fixtures = BTreeSet::new();
    let mut categories = BTreeSet::new();
    for case in cases["cases"].as_array().expect("cases array") {
        let id = case["id"].as_str().expect("case id");
        assert!(case_ids.insert(id), "duplicate case {id}");
        let fixture = case["fixture"].as_str().expect("case fixture");
        assert!(fixture_ids.contains(fixture), "{id}: unknown fixture");
        used_fixtures.insert(fixture);
        categories.insert(case["category"].as_str().expect("category"));
        let expected = case["expected"].as_object().expect("expected object");
        for field in ["routes", "signals", "references", "must_not"] {
            assert!(expected[field].is_array(), "{id}: expected.{field}");
        }
    }
    assert_eq!(fixture_ids, used_fixtures, "fixture/case drift");
    for category in [
        "routing",
        "orchestration",
        "design",
        "governance",
        "verification",
        "security",
        "parallelism",
        "references",
        "outputs",
        "shipping",
        "artifact_quality",
        "code_quality",
    ] {
        assert!(categories.contains(category), "missing category {category}");
    }
}

#[test]
fn canaries_pin_the_regressions_that_triggered_the_framework() {
    let cases = json("assets/base/agents/skills/cf-evaluate-model/resources/cases.json");
    let indexed: BTreeMap<&str, &Value> = cases["cases"]
        .as_array()
        .expect("cases array")
        .iter()
        .map(|case| (case["id"].as_str().expect("case id"), case))
        .collect();

    let plans = indexed["model-independent-plans"]["expected"]
        .as_object()
        .expect("plan expected");
    let plan_signals: BTreeSet<&str> = plans["signals"]
        .as_array()
        .expect("plan signals")
        .iter()
        .map(|value| value.as_str().expect("signal"))
        .collect();
    for signal in [
        "claude_complete_plan_before_exchange",
        "codex_complete_plan_before_exchange",
        "first_cross_exposure_after_both_plans",
        "matching_dual_approval",
    ] {
        assert!(plan_signals.contains(signal), "plan canary lost {signal}");
    }

    let compression = indexed["token-efficiency-cannot-delete-contract"];
    assert_eq!(compression["canary"], true);
    let required: BTreeSet<&str> = compression["requirements"]
        .as_array()
        .expect("compression requirements")
        .iter()
        .map(|value| value.as_str().expect("requirement"))
        .collect();
    for requirement in ["CF-MM-002", "CF-QA-001", "CF-SEC-001", "CF-EVAL-001"] {
        assert!(
            required.contains(requirement),
            "compression canary lost {requirement}"
        );
    }

    let right_sized = indexed["reject-overengineered-correct-change"];
    assert_eq!(right_sized["canary"], true);
    let right_sized_signals: BTreeSet<&str> = right_sized["expected"]["signals"]
        .as_array()
        .expect("right-sized signals")
        .iter()
        .map(|value| value.as_str().expect("signal"))
        .collect();
    for signal in [
        "simpler_equivalent_identified",
        "speculative_scope_rejected",
        "fable_design_and_code_review",
        "changes_requested_for_avoidable_complexity",
    ] {
        assert!(
            right_sized_signals.contains(signal),
            "right-sized canary lost {signal}"
        );
    }
}

#[test]
fn protocol_is_native_interactive_and_cleanup_is_fail_closed() {
    let skill = read("assets/base/agents/skills/cf-evaluate-model/SKILL.md");
    let normalized_skill = normalized(&skill);
    let script = read("assets/base/agents/skills/cf-evaluate-model/scripts/eval_kit.py");
    for required in [
        "fresh native interactive Codex App/CLI or Claude Code sessions",
        "Never use `codex exec`",
        "`claude -p` / `--print`",
        "removes this evaluation skill from the fixture",
        "never authorizes deleting a duty",
    ] {
        assert!(
            normalized_skill.contains(&normalized(required)),
            "evaluation skill lost {required}"
        );
    }
    for required in [
        "refusing unmarked cleanup target",
        "cleanup confirmation must exactly match the run_id",
        "refusing cleanup target that is itself a git repository",
        "remove_grader_material",
        "reset_fixture_history",
    ] {
        assert!(
            script.contains(required),
            "evaluation script lost {required}"
        );
    }
}
