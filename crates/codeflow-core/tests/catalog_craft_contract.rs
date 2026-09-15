//! Pins Wave-1 skill-catalog craft: descriptions as triggers, plan/review
//! craft, tracker/spike rules, and PR bodies derived from the whole branch
//! with measured test evidence.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(root().join(relative))
        .unwrap_or_else(|error| panic!("read {relative}: {error}"))
}

fn normalized(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn assert_contains(relative: &str, markers: &[&str]) {
    let content = normalized(&read(relative));
    for marker in markers {
        assert!(
            content.contains(&normalized(marker)),
            "{relative} lost marker: {marker}"
        );
    }
}

#[test]
fn plan_synthesizes_settled_ground_and_asks_only_live_questions() {
    assert_contains(
        "assets/base/agents/skills/cf-plan/SKILL.md",
        &[
            "synthesize that settled ground",
            "not already answered by the brief or Plan vN",
        ],
    );
}

#[test]
fn develop_and_quality_contract_require_a_failing_symptom_command() {
    assert_contains(
        "assets/base/agents/skills/cf-develop/SKILL.md",
        &["named interfaces first", "exact reported symptom"],
    );
    assert_contains(
        "assets/base/agents/skills/cf-model-orchestrator/resources/quality-contract.md",
        &["exact reported symptom"],
    );
}

#[test]
fn develop_is_an_orchestrated_stage_or_recorded_solo_fallback() {
    assert_contains(
        "assets/base/agents/skills/cf-develop/SKILL.md",
        &[
            "inside cf-model-orchestrator",
            "recorded solo fallback",
            "Do not use as an alternate entry point",
            "records the required interactive seat unavailable",
        ],
    );
}

#[test]
fn task_graph_prefers_a_narrow_complete_path() {
    assert_contains(
        "assets/base/agents/skills/cf-model-orchestrator/resources/task-graph.md",
        &[
            "narrow complete path",
            "expand the new form beside the old",
            "Not yet specified",
            "Out of scope",
        ],
    );
}

#[test]
fn project_organization_projects_trackers_and_keeps_spikes_off_protected() {
    assert_contains(
        "assets/base/claude/skills/cf-method/references/project-organization.md",
        &["one-way projection", "spikes/", "external_refs"],
    );
}

#[test]
fn editorial_smells_inspect_chatbot_and_puffery_without_a_blacklist() {
    assert_contains(
        "assets/base/agents/skills/cf-editorial-review/references/editorial-smells.md",
        &["Chatbot openers", "Stock puffery", "Vague attribution"],
    );
}

#[test]
fn skill_authoring_keeps_tty_host_out_of_the_description() {
    assert_contains(
        "assets/base/claude/skills/cf-method/references/skill-authoring.md",
        &[
            "load trigger",
            "Do not name a TTY host",
            "evals/skill-triggers/",
        ],
    );
}

#[test]
fn ship_and_pr_template_require_whole_branch_summary_and_measured_coverage() {
    assert_contains(
        "assets/base/agents/skills/cf-ship/SKILL.md",
        &[
            "Prepare the whole-branch PR using",
            "[references/pr-evidence.md](references/pr-evidence.md)",
            "attribute measured evidence to its revision and scope",
            "Missing required evidence keeps the PR draft",
        ],
    );
    assert_contains(
        "assets/base/agents/skills/cf-ship/references/pr-evidence.md",
        &[
            "git log --oneline",
            "git diff --stat",
            "not the last conversation",
            "Inspect the actual diff as well",
            "Report measured coverage TOTALs, metric, scope, and governing floor",
            "completed attributable CI run",
            "a job's `PASS` is not a coverage number",
            "Do not relabel subset coverage as workspace coverage",
            "unavailable or stale evidence is a gap",
            "codeflow test --mode essential --strict",
        ],
    );
    // Projects may extend their PR template without changing the shared scaffold.
    // Both still owe the same evidence contract.
    for path in [
        "assets/base/ci/pull_request_template.md",
        ".github/pull_request_template.md",
    ] {
        assert_contains(
            path,
            &[
                "git log --oneline",
                "git diff --stat",
                "Do not write from the last",
                "measured TOTAL from the project's command",
                "name revision, command, metric, and scope",
                "CI PASS alone is insufficient",
            ],
        );
    }
}

#[test]
fn ship_returns_failures_to_their_owner_without_waiving_configured_gates() {
    assert_contains(
        "assets/base/agents/skills/cf-ship/SKILL.md",
        &[
            "every mandatory project, CodeFlow, CI, and adopted-policy gate is green",
            "including for docs-only changes",
            "not invented code coverage or product behavior",
            "Return only to the failed owner",
            "Never restart the whole lifecycle",
            "already-frozen specs remain historical",
        ],
    );
}

#[test]
fn reviewer_labels_axis_and_disposition() {
    assert_contains(
        "assets/base/claude/agents/cf-reviewer.md",
        &["axis: standards", "axis: spec", "fix now", "track once"],
    );
}

#[test]
fn method_loads_skill_authoring_when_editing_skills() {
    assert_contains(
        "assets/base/claude/skills/cf-method/SKILL.md",
        &["references/skill-authoring.md", "description-trigger"],
    );
}

#[test]
fn trigger_suite_is_wired_into_the_test_gate() {
    let config = read(".codeflow/test-config.json");
    assert!(
        config.contains("evals/skill-triggers/test_triggers.py"),
        "skill-triggers must run in codeflow test"
    );
    assert!(
        config.contains("evals/herdr-delivery/test_delivery.py"),
        "herdr-delivery must run in codeflow test"
    );
}
