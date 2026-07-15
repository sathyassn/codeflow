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
        "**Evidence outranks agreement.**",
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
    assert!(delegate.contains("--permission-mode plan"));
    assert!(delegate.contains("sandbox.failIfUnavailable"));
    assert!(delegate.contains("Never enumerate or capture unrelated"));
    assert!(!delegate.contains("two identical captures"));

    for required in [
        "codeflow hook delegate-turn",
        "owner-only",
        "tmux wait-for codeflow-delegate-review-42",
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
