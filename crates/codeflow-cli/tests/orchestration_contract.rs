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

fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn orchestrator_is_host_neutral_with_capability_routed_execution() {
    let skill = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-model-orchestrator/SKILL.md",
    ));
    let routing = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-model-orchestrator/resources/capability-routing.md",
    ));

    for required in [
        "Detect capabilities, not model identity.",
        "Claude Code",
        "Codex App or interactive Codex CLI",
        "Other harness, including Hermes",
        "**Both think independently.**",
        "**Claude leads design.**",
        "**Host routes execution.**",
        "**Review is producer-relative.**",
        "**Fable owns integrated Claude judgment.**",
        "task fit",
        "observed native usage signals only",
        "Codex supplies independent review",
        "owns the final quality verdict",
        "latest Fable-class Claude model directly",
        "directly at high effort by default",
        "Escalate either primary to xhigh",
        "Opus-class workers at medium",
        "GPT-5.6 Sol or its strongest supported successor Codex coding seat",
        "qualified Terra-class workers",
        "Fable alone owns Claude-side internal routing",
        "never let an internal worker replace either duo seat",
        "**One orchestration owner.**",
        "never starts a nested duo",
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

    for required in [
        "TASK_ID | PRODUCER seat@effort | CROSS_LINEAGE_REVIEWER seat@effort",
        "verified native availability and routing",
        "Unknown remains unknown",
        "never infer quota, availability, or a worker route",
        "is reassignment: create Plan vN+1",
        "same-seat high→xhigh escalation",
        "A model cannot independently review its own authored unit",
        "Peer and worker prompts explicitly forbid nested orchestration",
    ] {
        assert!(
            routing.contains(required),
            "capability-routing contract lost marker: {required}"
        );
    }

    for superseded in ["**Codex implements.**", "Fixed role binding"] {
        assert!(
            !skill.contains(superseded),
            "orchestrator retained superseded fixed-role marker: {superseded}"
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
fn independent_planning_cannot_degrade_to_plan_then_critique() {
    let skill = read("assets/base/agents/skills/cf-model-orchestrator/SKILL.md");
    let agents = read("assets/base/AGENTS.md.tmpl");
    let capabilities = read("docs/capabilities.md");
    let normalized = normalize_whitespace(&skill);

    for required in [
        "Both models independently research, analyze, and plan",
        "Claude and Codex research, analyze, identify risks, and draft a plan in parallel before seeing the other's conclusions.",
        "Give both seats the same immutable brief and repository scope.",
        "an implementation plan and test strategy;",
        "The host reconciles the two drafts into **Plan v1**",
        "Codex reviews the design for implementation feasibility, failure modes, security, testing, and maintainability.",
    ] {
        assert!(
            normalized.contains(required),
            "orchestrator lost the independent-plan contract: {required}"
        );
    }

    for anchored_flow in [
        "Codex supplies the implementation challenge",
        "Codex critiques Claude's plan",
        "Codex reviews Claude's plan instead of drafting",
    ] {
        assert!(
            !normalized.contains(anchored_flow),
            "orchestrator reintroduced a critique-only Codex seat: {anchored_flow}"
        );
    }

    assert!(
        normalize_whitespace(&agents).contains(
            "both independently research/analyze/plan; Claude leads design; the host assigns each task"
        ),
        "always-loaded AGENTS contract must expose independent planning"
    );
    assert!(
        normalize_whitespace(&capabilities).contains(
            "Both seats independently research, analyze risks, and draft complete plans from the same immutable brief before either sees the other's conclusions"
        ),
        "CAP-010 must preserve the anti-anchoring contract"
    );
}

#[test]
fn design_review_and_security_roles_cannot_silently_drift() {
    let skill = read("assets/base/agents/skills/cf-model-orchestrator/SKILL.md");
    let reviewer = read("assets/base/claude/agents/cf-reviewer.md");
    let security = read("assets/base/claude/agents/cf-security-reviewer.md");
    let quality =
        read("assets/base/agents/skills/cf-model-orchestrator/resources/quality-contract.md");
    let normalized = normalize_whitespace(&skill);

    for required in [
        "unless the brief already fixes a clear direction, compares 2–3 viable options",
        "When the brief already dictates one clear design direction, record that constraint and why option exploration was waived.",
        "independent inspection without self-review → Fable integrated verdict",
        "they do not replace the required other-lineage review or Fable judgment.",
        "separate interactive Claude session in auto mode under the same fail-closed sandbox—not plan or bypass mode",
    ] {
        assert!(
            normalized.contains(required),
            "orchestrator lost a fixed design/review role: {required}"
        );
    }
    assert!(
        !normalized.contains("brief or local convention")
            && !normalized.contains("brief or established convention"),
        "local convention must not waive independent design options"
    );

    for required in [
        "require at least 80% aggregate production-code line coverage",
        "the approved design",
        "Anything less is `changes_requested`.",
        "Material avoidable complexity or brittleness is major even when tests pass",
    ] {
        assert!(
            normalize_whitespace(&reviewer).contains(required),
            "reviewer lost a completion gate: {required}"
        );
    }

    for required in [
        "Defender lens — Claude",
        "Attacker lens — a second vendor",
        "custom or internal token shapes the regexes miss",
        "IaC / CI YAML",
        "CWE-79/89/78/94/77/22/1336",
        "Cross-vendor divergence escalates to the human at merge",
    ] {
        assert!(
            normalize_whitespace(&security).contains(required),
            "security reviewer lost a mandatory adversarial axis: {required}"
        );
    }

    for required in [
        "A changed plan invalidates both approvals",
        "hard floor of **80%**",
        "normal target is **90% or higher**",
        "failing or missing gate cannot be overridden by model consensus",
    ] {
        assert!(
            normalize_whitespace(&quality).contains(required),
            "quality contract lost a hard gate: {required}"
        );
    }
}

#[test]
fn always_loaded_reasoning_and_output_contract_survives_refactors() {
    let agents = normalize_whitespace(&read("assets/base/AGENTS.md.tmpl"));

    for required in [
        "then the best current external sources",
        "Steelman the rejected option before the decision stands",
        "Think independently — not a yes-man.",
        "Agreement without examination is a failure mode",
        "Think in depth, not at the surface.",
        "Decide by options and horizons.",
        "Write only what earns its keep.",
        "every material complexity maps to a current requirement",
        "DRY with judgment",
        "brittle under-design, not simplicity",
        "Shape the deliverable.",
        "check what it affects upstream and downstream",
        "ADRs and the ledger are never edited",
    ] {
        assert!(
            agents.contains(required),
            "always-loaded contract lost a load-bearing duty: {required}"
        );
    }
}

#[test]
fn every_non_trivial_task_is_stage_aware_and_uses_effective_autonomy() {
    let skill = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-model-orchestrator/SKILL.md",
    ));
    let agents = read("assets/base/AGENTS.md.tmpl");
    let claude = read("assets/base/CLAUDE.md.tmpl");

    for required in [
        "Use the duo for every non-trivial repository task.",
        "## Outcome modes",
        "**Research / analysis:**",
        "**Plan / design:**",
        "**Implementation:**",
        "**Review / verification:**",
        "**Substantive documentation:**",
        "--model fable --effort high --permission-mode auto",
        "replace `high` with `xhigh`",
        "/codex:rescue --effort high",
        "do not inherit an unobserved user default",
        "--settings '{\"autoMode\":{\"classifyAllShell\":true}}'",
        "Never fall through to bypass mode on an ordinary host",
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

    for required in [
        "## Routing gate",
        "Before repository or external research",
        "invoke\n`/cf-model-orchestrator`",
        "Do not inspect first and route later",
        "one obvious local check",
    ] {
        assert!(
            claude.contains(required),
            "CLAUDE template lost early routing marker: {required}"
        );
    }
}

#[test]
fn quality_contract_pins_evidence_coverage_and_ui() {
    let contract = normalize_whitespace(&format!(
        "{}\n{}",
        read("assets/base/agents/skills/cf-model-orchestrator/resources/quality-contract.md"),
        read("assets/base/agents/skills/cf-model-orchestrator/resources/capability-routing.md")
    ));

    for required in [
        "PLAN_VERSION:",
        "TASK_ASSIGNMENTS:",
        "CROSS_LINEAGE_REVIEWER seat@effort",
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
        "COMPLEXITY_JUSTIFICATION:",
        "smallest coherent solution",
        "Material avoidable complexity is `changes_requested`",
        "brittle under-design",
        "existing design system",
        "accessible primitives",
        "not the fewest lines",
        "A model cannot independently review its own authored unit",
    ] {
        assert!(
            contract.contains(required),
            "quality contract lost required marker: {required}"
        );
    }
}

#[test]
fn editorial_quality_is_contextual_on_demand_and_cross_harness() {
    let full_agents = normalize_whitespace(&read("assets/base/AGENTS.md.tmpl"));
    let minimal_agents = normalize_whitespace(&read("assets/base/AGENTS.minimal.md.tmpl"));
    let skill = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-editorial-review/SKILL.md",
    ));
    let smells = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-editorial-review/references/editorial-smells.md",
    ));

    for required in [
        "verified truth and policy outrank CodeFlow philosophy",
        "documented voice/examples",
        "Preserve technical meaning",
        "never fabricate personality",
    ] {
        assert!(
            full_agents.contains(required),
            "full contract lost editorial principle: {required}"
        );
    }
    for required in [
        "verified truth and policy outrank documented project voice",
        "preserve technical meaning",
        "never invent personality",
    ] {
        assert!(
            minimal_agents.contains(required),
            "minimal contract lost proportionate editorial principle: {required}"
        );
    }

    for required in [
        "verified truth, evidence, exact technical meaning, and governing policy",
        "the consuming project's documented voice and human-approved examples",
        "Never trade precision for fluency",
        "cluster items by shared purpose and audience",
        "There is no universal word, punctuation, formatting, or emoji blacklist",
        "Do not use an AI detector",
        "primary Fable seat reviews",
    ] {
        assert!(skill.contains(required), "editorial skill lost {required}");
    }
    for required in [
        "Use these as diagnostic prompts, not a checklist or blacklist",
        "Do not rewrite merely because text uses an em dash",
        "Do not replace an established project voice",
    ] {
        assert!(
            smells.contains(required),
            "editorial reference lost {required}"
        );
    }
    assert!(
        !skill.contains("Vale"),
        "editorial skill must not require Vale"
    );
}

#[test]
fn reverse_lane_uses_hook_completion_not_pane_stability() {
    let delegate = read("assets/base/claude/skills/cf-delegate/SKILL.md");
    let adapter = read("assets/base/claude/skills/cf-delegate/resources/claude-turn-completion.md");

    assert!(delegate.contains("task-scoped Claude"));
    assert!(delegate.contains("StopFailure"));
    assert!(delegate.contains("last_assistant_message"));
    assert!(delegate.contains("--model fable --effort high --permission-mode auto"));
    assert!(delegate.contains("xhigh for capability-sensitive"));
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
