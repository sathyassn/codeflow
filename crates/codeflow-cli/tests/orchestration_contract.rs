//! Behavioral contract for the host-neutral Claude+Codex duo.
//!
//! Mirror tests prove byte parity. These assertions pin the semantics that
//! must survive wording refactors and scaffold updates.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(repo_root().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

#[test]
fn orchestrator_is_host_neutral_with_fixed_roles() {
    let skill = read("assets/base/agents/skills/cf-model-orchestrator/SKILL.md");

    for required in [
        "Detect capabilities, not model identity.",
        "Claude Code",
        "Codex App or interactive Codex CLI",
        "Other harness, including Hermes",
        "**Both think independently.**",
        "**Claude leads design.**",
        "**Codex implements.**",
        "**Claude final-reviews.**",
        "latest available Fable-class Claude",
        "model at xhigh effort",
        "current Opus-class subagents",
        "strongest",
        "supported Codex coding model at xhigh",
        "**Evidence outranks agreement.**",
        "**Bounded parallelism.**",
        "Plan v1",
        "at most two rounds",
    ] {
        assert!(
            skill.contains(required),
            "orchestrator lost required behavior marker: {required}"
        );
    }

    for stale_pin in ["Fable 5", "gpt-5.6-sol"] {
        assert!(
            !skill.contains(stale_pin),
            "orchestrator must not hard-code model pin {stale_pin}"
        );
    }
}

#[test]
fn every_non_trivial_task_is_stage_aware_and_uses_effective_autonomy() {
    let skill = read("assets/base/agents/skills/cf-model-orchestrator/SKILL.md");
    let agents = read("assets/base/AGENTS.md.tmpl");

    for required in [
        "Use the duo for every non-trivial repository task.",
        "## Outcome modes",
        "**Research / analysis:**",
        "**Plan / design:**",
        "**Implementation:**",
        "**Review / verification:**",
        "**Substantive documentation:**",
        "--model fable --effort xhigh --permission-mode auto",
        "--settings '{\"autoMode\":{\"classifyAllShell\":true}}'",
        "never fall through to bypass mode on an ordinary host",
        "session in auto mode under the same fail-closed sandbox",
        "not plan or bypass",
        "public network and live search are enabled",
        "Authenticated tools use their broker/OAuth/keychain/credential-mask path",
    ] {
        assert!(
            skill.contains(required),
            "stage/autonomy contract lost marker: {required}"
        );
    }

    for required in [
        "Every non-trivial repository task **must begin with**",
        "`/cf-model-orchestrator`",
        "research- or planning-only task stops before implementation",
        "supporting flows, not",
        "When uncertain whether work is trivial, treat",
    ] {
        assert!(
            agents.contains(required),
            "AGENTS template lost duo entry-point marker: {required}"
        );
    }
}

#[test]
fn quality_contract_pins_evidence_coverage_and_ui() {
    let contract =
        read("assets/base/agents/skills/cf-model-orchestrator/resources/quality-contract.md");

    for required in [
        "PLAN_VERSION:",
        "CLAUDE_APPROVAL:",
        "CODEX_APPROVAL:",
        "Model agreement is not evidence.",
        "hard floor of **80%**",
        "normal target is **90% or higher**",
        "Playwright",
        "Computer Use",
        "UI: N/A",
        "failing or missing gate cannot be overridden by model consensus",
        "DEPENDENCY_GRAPH:",
        "TASK_BRANCH_WORKTREE_OWNER:",
        "SHARED_FILE_OWNER:",
        "HOST_RESOURCE_BUDGET:",
        "one writer, branch, and worktree",
        "aggregate gates on the final",
        "combined diff",
    ] {
        assert!(
            contract.contains(required),
            "quality contract lost required marker: {required}"
        );
    }
}

#[test]
fn reverse_lane_uses_hook_completion_not_pane_stability() {
    let delegate = read("assets/base/claude/skills/cf-delegate/SKILL.md");
    let adapter = read("assets/base/claude/skills/cf-delegate/resources/claude-turn-completion.md");

    assert!(delegate.contains("task-scoped Claude"));
    assert!(delegate.contains("StopFailure"));
    assert!(delegate.contains("last_assistant_message"));
    assert!(delegate.contains("--model fable --effort xhigh --permission-mode auto"));
    assert!(delegate.contains("autoMode.classifyAllShell"));
    assert!(delegate.contains("sandbox.failIfUnavailable"));
    assert!(delegate.contains("Never enumerate or capture unrelated"));
    assert!(!delegate.contains("two identical captures"));

    for required in [
        "codeflow hook delegate-turn",
        "owner-only",
        "tmux wait-for codeflow-delegate-review-42",
        "\"classifyAllShell\": true",
        "--permission-mode auto",
        "schema_version",
        "dedicated pane only for bounded diagnosis",
    ] {
        assert!(
            adapter.contains(required),
            "completion adapter lost required marker: {required}"
        );
    }
}

#[test]
fn batch_workflow_is_honestly_single_vendor_and_validates_edges() {
    let source = read("assets/base/claude/workflows/pipeline.workflow.js");
    let mirror = read(".claude/workflows/pipeline.workflow.js");
    assert_eq!(source, mirror, "dogfood workflow must match shipped source");

    for required in [
        "'single-vendor-assurance'",
        "batch duo is unsupported",
        "A.maxRework must be a positive integer",
        "Object.prototype.hasOwnProperty.call(STAGES, name)",
        "an approved plan-audit must return a non-empty contract",
        "required: [...VERDICT.required, 'attack_log']",
        "attack_log: out?.attack_log ?? null",
    ] {
        assert!(
            source.contains(required),
            "pipeline lost required behavior marker: {required}"
        );
    }
    assert!(!source.contains("duo: ['plan-align'"));
}
