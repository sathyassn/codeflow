//! Behavioral contract for the host-neutral Claude+Codex duo.
//!
//! Mirror tests prove byte parity. These assertions pin the semantics that
//! must survive wording refactors and scaffold updates.

use std::collections::{BTreeMap, BTreeSet};
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

fn assert_internal_routes(
    role: &str,
    binding: &serde_json::Value,
    selectors: &serde_json::Map<String, serde_json::Value>,
) {
    let mut route_ids = BTreeSet::new();
    for route in binding["internal_routes"]
        .as_array()
        .expect("internal routes")
    {
        let route_id = route["route_id"].as_str().expect("route id");
        assert!(
            route_ids.insert(route_id),
            "duplicate route {role}/{route_id}"
        );
        assert_eq!(route["status"], "candidate");
        assert!(route["evidence"].as_array().expect("evidence").is_empty());
        let efforts = route["efforts"].as_array().expect("efforts");
        assert!(!efforts.is_empty());
        assert!(efforts.contains(&route["default_effort"]));
        assert!(!route["workloads"].as_array().expect("workloads").is_empty());
        for harness in route["native_selectors"]
            .as_object()
            .expect("route native selectors")
            .keys()
        {
            assert!(
                selectors.contains_key(harness),
                "route {role}/{route_id} escapes its parent harnesses"
            );
        }
    }
}

#[test]
fn current_ensemble_uses_only_capability_supported_harnesses() {
    let ensemble: serde_json::Value = serde_json::from_str(&read(
        "assets/base/agents/skills/cf-model-orchestrator/resources/current-ensemble.json",
    ))
    .expect("current ensemble JSON");
    let harnesses: serde_json::Value = serde_json::from_str(&read(
        "assets/base/agents/skills/cf-evaluate-model/resources/harnesses.json",
    ))
    .expect("harness catalog JSON");
    assert_eq!(ensemble["schema_version"], 4);
    assert_eq!(
        ensemble["design_execution_owner"],
        "claude-judgment-primary"
    );
    assert!(ensemble["rules"].as_array().unwrap().iter().any(|rule| {
        rule.as_str() == Some("High triggers set a minimum reasoning level for a unit, not an instruction to escalate a primary already at high or spawn a redundant high worker.")
    }), "high reasoning floor must not mandate redundant escalation");
    let supported: BTreeMap<&str, (&str, &str)> = harnesses["harnesses"]
        .as_array()
        .expect("harnesses")
        .iter()
        .map(|harness| {
            assert_eq!(
                harness["status"], "capability-supported",
                "ensemble harnesses require catalog capability support"
            );
            (
                harness["id"].as_str().expect("harness id"),
                (
                    harness["provider"].as_str().expect("provider"),
                    harness["lineage"].as_str().expect("lineage"),
                ),
            )
        })
        .collect();
    let standing: BTreeSet<&str> = ensemble["standing_roles"]
        .as_array()
        .expect("standing_roles")
        .iter()
        .map(|role| role.as_str().expect("standing role"))
        .collect();
    assert_eq!(
        standing,
        BTreeSet::from(["claude-judgment-primary", "codex-engineering-primary"])
    );
    let bindings = ensemble["bindings"].as_array().expect("bindings");
    assert!(
        bindings.len() >= 2,
        "the current ensemble must include the standing pair"
    );
    let mut seats = BTreeSet::new();
    let mut lineages = BTreeSet::new();
    let mut roles = BTreeSet::new();
    for binding in bindings {
        let role = binding["role"].as_str().expect("role");
        let seat = binding["seat"].as_str().expect("seat");
        let provider = binding["provider"].as_str().expect("provider");
        let lineage = binding["lineage"].as_str().expect("lineage");
        assert!(roles.insert(role), "duplicate ensemble role {role}");
        assert!(seats.insert(seat), "duplicate ensemble seat {seat}");
        assert!(
            lineages.insert(lineage),
            "duplicate primary lineage {lineage}"
        );
        assert!(!binding["responsibilities"]
            .as_array()
            .expect("responsibilities")
            .is_empty());
        let selectors = binding["native_selectors"]
            .as_object()
            .expect("native selectors");
        assert!(!selectors.is_empty(), "{seat} has no native selector");
        for (harness, selector) in selectors {
            assert!(
                !selector.as_str().unwrap_or_default().is_empty(),
                "{seat} has an empty selector for {harness}"
            );
            assert_eq!(
                supported.get(harness.as_str()),
                Some(&(provider, lineage)),
                "{seat} selects an unsupported or mismatched harness {harness}"
            );
        }
        assert_internal_routes(role, binding, selectors);
    }
    assert!(lineages.contains("claude") && lineages.contains("codex"));
    assert!(lineages.contains("grok"), "catalog family grok is missing");
    assert!(roles.contains("claude-judgment-primary"));
    assert!(roles.contains("codex-engineering-primary"));
    assert!(roles.contains("grok-engineering-primary"));
    assert!(!ensemble["xhigh_triggers"]
        .as_array()
        .expect("xhigh triggers")
        .is_empty());
}

#[test]
fn orchestrator_is_host_neutral_with_capability_routed_execution() {
    let skill = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-model-orchestrator/SKILL.md",
    ));
    let capability_routing = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-model-orchestrator/resources/capability-routing.md",
    ));

    for required in [
        "Detect capabilities, not model identity.",
        "Claude Code",
        "Codex App or interactive Codex CLI",
        "Grok Build (interactive `grok` CLI)",
        "Other harness, including Hermes",
        "never a silent third vote",
        "Name extra families on trigger if available",
        "strongest capable permitted reasoning route",
        "[detail](resources/grok-host.md)",
        "**Both think independently.**",
        "**Claude leads design.**",
        "**Host routes execution.**",
        "**Review is author-relative.**",
        "**The Claude judgment primary owns integrated Claude judgment.**",
        "task fit",
        "resources, and observed usage",
        "Codex supplies independent review",
        "owns the final quality verdict",
        "**Accountable route use.**",
        "Use the concrete selectors",
        "invoke each primary directly",
        "retain its planning, integration, and approval duties",
        "workers replace no primary",
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
    assert!(
        capability_routing
            .contains("The Claude design owner produces direction and real design implementation"),
        "capability routing lost Claude's design execution ownership"
    );
}

#[test]
fn grok_hosted_duo_canary_record_exists_and_stays_unqualified() {
    let grok_host = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-model-orchestrator/resources/grok-host.md",
    ));
    assert!(
        grok_host.contains("not a qualified `grok-engineering-primary` binding"),
        "grok-host.md must keep the unqualified-binding limit"
    );
    assert!(
        grok_host.contains("Consuming scaffolds do not ship that file"),
        "grok-host.md must not require a CodeFlow-only verification path"
    );
    let canary = repo_root().join("docs/verification/grok-host-duo-canary-2026-09-07.md");
    assert!(
        canary.is_file(),
        "dated Grok-hosted duo canary record must exist"
    );
    let canary_text = normalize_whitespace(&read(
        "docs/verification/grok-host-duo-canary-2026-09-07.md",
    ));
    assert!(canary_text.contains("GROK_HOST_SCHEMAV2_OK"));
    assert!(canary_text.contains("GROK_HOST_CODEX_OK"));
    assert!(canary_text.contains("not a full native-interactive promotion suite"));
}

#[test]
fn cross_family_entry_preserves_receiving_primary_ownership() {
    let routing = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-model-orchestrator/resources/capability-routing.md",
    ));
    for required in [
        "The first line of every cross-family task declares `ROLE: peer`",
        "qualified primary at its default effort",
        "only for same-family work owned and dispatched by that family's primary",
        "a caller never selects a foreign worker directly",
        "Later primary-approval prose cannot repair an incorrect initial dispatch",
    ] {
        assert!(
            routing.contains(required),
            "primary-entry contract lost: {required}"
        );
    }
}

#[test]
fn current_ensemble_and_routing_pin_grok_catalog() {
    let routing = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-model-orchestrator/resources/capability-routing.md",
    ));
    let ensemble = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-model-orchestrator/resources/current-ensemble.json",
    ));
    let policy = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-model-orchestrator/resources/routing-policy.json",
    ));
    for required in [
        "\"seat\": \"claude-primary\"",
        "\"role\": \"claude-judgment-primary\"",
        "\"model_class\": \"latest-fable\"",
        "\"model_class\": \"latest-opus\"",
        "\"seat\": \"codex-primary\"",
        "\"role\": \"codex-engineering-primary\"",
        "\"model_class\": \"latest-astra-coding\"",
        "\"model_class\": \"latest-sol\"",
        "\"model_class\": \"latest-terra\"",
        "\"seat\": \"grok-primary\"",
        "\"role\": \"grok-engineering-primary\"",
        "\"model_class\": \"latest-grok-coding\"",
        "\"grok-cli\": \"grok-4.6\"",
        "\"default_effort\": \"high\"",
        "\"escalation_effort\": \"xhigh\"",
        "Primary seats retain independent planning and approval duties.",
        "The standing pair is the usual quality floor; extra catalog families never vote silently.",
        "Internal workers never replace a primary or named cross-lineage reviewer.",
        "Claude-side internal routing belongs to the Claude primary.",
        "Grok-side internal routing is a CodeFlow instruction to the Grok primary, not a vendor-secret router.",
        "Claude produces design in its native interactive session regardless of host.",
        "A changed concrete binding requires native-interactive qualification before promotion.",
        "A high primary retains orchestration; use same-family workers when delegation adds value or an xhigh trigger requires stronger reasoning.",
        "Name the extra catalog family when a routing-policy trigger fires and it is available; its output is evidence, never a silent vote.",
    ] {
        assert!(
            ensemble.contains(required),
            "current ensemble lost binding marker: {required}"
        );
    }

    for required in [
        "\"never_silent_vote\": true",
        "\"requires_named_assignment\": true",
        "\"invoke_when_available\": true",
        "complex architecture",
        "security-sensitive change",
        "standing pair cannot reach justified confidence",
    ] {
        assert!(
            policy.contains(required),
            "routing policy lost extra-family marker: {required}"
        );
    }

    for required in [
        "TASK_ID | RESPONSIBLE_PRIMARY seat@effort | EXEC_MODE | EXECUTION",
        "requested from observed",
        "Unknown usage is advisory",
        "Never combine apparently separate limits",
        "is reassignment: create Plan vN+1",
        "permitted worker change",
        "Default effort is high for primary seats, not a ceiling",
        "strongest capable permitted same-family reasoning route",
        "An xhigh trigger requires the owning primary to obtain xhigh reasoning",
        "Delegate substantial, well-specified routine implementation",
        "both families still plan independently and cross-lineage review remains mandatory",
        "`candidate` and `scoped-qualified` describe evidence status",
        "three fresh accepted trials",
        "Full primary-binding promotion",
        "economical-default",
        "A model cannot independently review its own authored unit",
        "delegating back to the host lineage",
        "generic same-lineage subagent cannot satisfy",
        "never a silent third vote",
        "available-and-named or unavailable-with-limitation",
        "A Grok Build host coordinates the standing pair through Herdr",
        "implementer check",
        "Default UI assignment is Claude as responsible primary and executor",
        "Playwright remains the deterministic web driver",
        "preferred plugin or qualified official native client",
        "spawns same-family workers at that",
    ] {
        assert!(
            routing.contains(required),
            "capability-routing contract lost marker: {required}"
        );
    }
}

#[test]
fn orchestrator_skill_avoids_superseded_roles_and_model_pins() {
    let skill = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-model-orchestrator/SKILL.md",
    ));
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
fn accountable_execution_preserves_design_and_evidence_boundaries() {
    let routing = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-model-orchestrator/resources/capability-routing.md",
    ));
    let quality = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-model-orchestrator/resources/quality-contract.md",
    ));
    let design = normalize_whitespace(&read("assets/base/agents/skills/cf-design/SKILL.md"));
    let cases = read("assets/base/agents/skills/cf-evaluate-model/resources/cases.json");
    let packs = read("assets/base/agents/skills/cf-evaluate-model/resources/packs.json");

    for required in [
        "RESPONSIBLE_PRIMARY seat@effort | EXEC_MODE | EXECUTION",
        "`candidate` and `scoped-qualified` describe evidence status, not native reachability",
        "A configured candidate is usable for bounded non-design work",
        "no prior completed workload canary is required",
        "first bounded assignment may itself supply start, return",
        "Executable presence alone is not readiness",
        "three fresh accepted trials for every pre-registered case and qualifying arm",
        "Comparison outcomes inform claim scope but do not themselves gate qualification",
        "Full primary-binding promotion still requires the existing complete suite",
        "Savings or economical-default recommendations separately require measured all-attempt",
        "review lineage is opposite the session that authored the work",
    ] {
        assert!(
            routing.contains(required),
            "accountable routing contract lost marker: {required}"
        );
    }

    for required in [
        "Primary responsibility and actual execution are separate",
        "does not become its author",
        "unknown and advisory unless an explicit hard limit depends on it",
    ] {
        assert!(
            quality.contains(required),
            "quality contract lost accountable-execution marker: {required}"
        );
    }

    for required in [
        "same Claude owner authors and implements real design and retains fidelity judgment",
        "candidates run only disposable fixtures",
        "Turning settled product/UX/UI into components, layout, styles, or interactions is design implementation",
        "Claude absence is not one",
    ] {
        assert!(
            design.contains(required),
            "design execution contract lost marker: {required}"
        );
    }

    for case in [
        "unverified-worker-route-is-unavailable",
        "proven-candidate-route-is-bounded-executor",
        "candidate-design-route-cannot-implement-product",
        "explicit-design-family-override-is-valid",
        "unknown-usage-is-advisory-without-hard-limit",
        "observed-hard-usage-limit-allows-bounded-recovery",
        "catalog-owner-resolves-route-evidence",
        "observed-route-overrides-requested-label",
        "mixed-authorship-reassigns-independent-review",
        "mixed-lineage-contributions-use-per-unit-review",
    ] {
        assert!(
            cases.contains(case),
            "missing accountable routing case: {case}"
        );
        assert!(packs.contains(case), "focused pack omits case: {case}");
    }
}

#[test]
fn independent_planning_cannot_degrade_to_plan_then_critique() {
    let skill = read("assets/base/agents/skills/cf-model-orchestrator/SKILL.md");
    let agents = read("assets/base/AGENTS.md.tmpl");
    let capabilities = read("docs/capabilities.md");
    let normalized = normalize_whitespace(&skill);

    for required in [
        "Both families independently research, analyze, and plan",
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
        normalize_whitespace(&capabilities).contains("fifteen health checks"),
        "CAP-008 must count the grok doctor check"
    );
    let readme = normalize_whitespace(&read("README.md"));
    let architecture = normalize_whitespace(&read("docs/architecture.md"));
    assert!(
        readme.contains("Health checks (15): hooks, claude, codex, grok, config"),
        "README must list the grok doctor check"
    );
    assert!(
        architecture.contains("15 checks — hooks, claude, codex, grok, config"),
        "architecture must list the grok doctor check"
    );
    assert!(
        normalize_whitespace(&capabilities).contains(
            "use proportionate worker effort when useful, and obtain same-family xhigh reasoning on trigger mid-session rather than restarting the host"
        ),
        "CAP-010 must preserve proportionate workers and mid-session escalation without host restart"
    );
    assert!(
        normalize_whitespace(&capabilities).contains("named when a routing-policy trigger fires"),
        "CAP-010 must pin extra-family invoke-when-available"
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
        "a lineage different from the actual author's reviews it independently. Self-review is never independent.",
        "The Claude judgment primary owns integrated Claude judgment.",
        "they do not replace the required other-lineage review or primary judgment.",
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
        "A **gate** is the verification check",
        "missing *job* evidence, not a failed check",
        "An infra-incomplete duplicate job does not",
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
        "steelman the strongest alternative",
        "Challenge decisions independently.",
        "Evidence and honest analysis outrank agreement",
        "trace causes and consequences across affected domains",
        "compare short- and long-term routes",
        "An unfinished CI job is missing evidence",
        "Honor a red check.",
        "Write only what earns its keep.",
        "every material complexity maps to a current requirement",
        "DRY with judgment",
        "brittle under-design, not simplicity",
        "Shape the deliverable.",
        "check what it affects upstream and downstream",
        "accepted ADRs and the ledger are append-only",
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
    // Git may materialize text assets with CRLF on Windows. This contract
    // pins the authored line break, not the checkout's newline convention.
    let claude = read("assets/base/CLAUDE.md.tmpl").replace("\r\n", "\n");
    let ensemble =
        read("assets/base/agents/skills/cf-model-orchestrator/resources/current-ensemble.json");
    let capability_routing = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-model-orchestrator/resources/capability-routing.md",
    ));
    let herdr = normalize_whitespace(&read("assets/base/agents/skills/cf-herdr/SKILL.md"));
    let claude_completion = normalize_whitespace(&read(
        "assets/base/claude/skills/cf-delegate/resources/claude-turn-completion.md",
    ));

    for required in [
        "Use the duo for every non-trivial repository task.",
        "## Outcome modes",
        "**Research/analysis:**",
        "**Plan/design:**",
        "**Implementation:**",
        "**Review/verification:**",
        "**Substantive docs:**",
        "/codex:rescue --model <primary-selector> --effort <primary-default>",
        "an unobserved user default is not selection evidence",
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
    assert!(
        capability_routing.contains("spawns same-family workers at that effort"),
        "capability routing lost worker-effort ownership"
    );
    assert!(
        herdr.contains("--model <selector> --effort <effort> --permission-mode bypassPermissions"),
        "Herdr lost its Claude production launch contract"
    );
    for required in [
        "Consult and no-edit review keep",
        "Make `autoMode.classifyAllShell` effective at user scope",
        "repeated `--settings` flags are not a supported composition mechanism",
    ] {
        assert!(
            claude_completion.contains(required),
            "Claude turn-completion contract lost autonomy marker: {required}"
        );
    }
    for required in [
        "\"default_effort\": \"high\"",
        "\"escalation_effort\": \"xhigh\"",
        "\"claude-code\": \"fable\"",
        "\"codex-cli\": \"gpt-6-astra\"",
        "\"grok-cli\": \"grok-4.6\"",
    ] {
        assert!(
            ensemble.contains(required),
            "current ensemble lost effective binding marker: {required}"
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
    let normalized_agents = normalize_whitespace(&agents);
    for (owner, contract, alternate) in [
        ("orchestrator", &skill, "else a separate read-only pass"),
        (
            "AGENTS",
            &normalized_agents,
            "otherwise a separate read-only pass",
        ),
    ] {
        assert!(
            contract.contains("fresh-context independent review")
                && contract.contains(alternate)
                && contract.contains("self-review is not review"),
            "{owner} solo fallback lost its unconditional independent-review floor"
        );
        assert!(
            !contract.contains("review where possible"),
            "{owner} solo fallback made independent review optional"
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
        "Tests must be capable of failing for a material regression",
        "Coverage measures exercised lines; it never proves test integrity",
        "For performance-, scale-, or concurrency-sensitive paths",
        "N+1 access",
        "race/concurrency test",
        "Playwright",
        "Computer Use",
        "Playwright remains the deterministic web driver",
        "implementer check",
        "every interactive control",
        "UI: N/A",
        "failing or missing gate cannot be overridden by model consensus",
        "A **gate** is the verification check",
        "missing *job* evidence, not a failed check",
        "SETTLED_TASK_GRAPH:",
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
fn task_graph_and_verification_strength_are_proportionate_contracts() {
    let skill = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-model-orchestrator/SKILL.md",
    ));
    let plan = normalize_whitespace(&read("assets/base/agents/skills/cf-plan/SKILL.md"));
    let develop = normalize_whitespace(&read("assets/base/agents/skills/cf-develop/SKILL.md"));
    let graph = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-model-orchestrator/resources/task-graph.md",
    ));
    let verification = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-model-orchestrator/resources/verification-selection.md",
    ));

    for required in [
        "both approvals cover the same canonical task graph",
        "material node, dependency, decision guard, ownership, acceptance, interface, or safety-boundary change creates Plan vN+1",
        "Ordinary steps and bounded implementation choices inside an approved node remain ledger evidence",
        "records `none selected`",
    ] {
        assert!(
            skill.contains(required),
            "orchestrator lost graph/verification marker: {required}"
        );
    }

    for required in [
        "TASK_ID | OUTCOME | RESPONSIBLE_PRIMARY seat@effort | EXEC_MODE | EXECUTION | AUTHORSHIP",
        "Every active bare edge means B cannot start or be accepted until A has landed",
        "A guard that merely restates a standard quality",
        "Every unselected alternative records `not_selected`",
        "non-executable structural topology",
        "It never interprets guards, readiness, completion, or scheduling",
        "Every task assignment appears exactly once as a node",
        "The execution graph is acyclic",
        "eligible for parallel work, not automatically parallel",
        "TASK_GRAPH: N/A (single task)",
        "`START` is plan-only and is never written to task metadata",
        "A root task reached from `START` records `depends_on: []`",
        "An observed outcome that matches no approved guard is a graph mutation",
        "Create Plan vN+1 and obtain fresh approval from both primary seats",
        "different valid topological order",
        "Classify **and persist** each such occurrence in the execution ledger",
        "without turning CodeFlow into a scheduler",
    ] {
        assert!(
            graph.contains(required),
            "task graph lost marker: {required}"
        );
    }

    assert!(
        plan.contains("Run `codeflow validate --docs`"),
        "planning must run the task-graph validator before approval"
    );
    assert!(
        develop.contains("`codeflow test` and `codeflow validate --docs` green"),
        "delivery must run the task-graph validator before completion"
    );

    for required in [
        "`none selected` is a valid and common result",
        "Select them when all of these hold",
        "Keep explicit examples for known singular boundaries",
        "Use a targeted, time-bounded mutation run",
        "First require a deterministic base suite",
        "Add a project-owned deterministic fitness check",
        "Do not duplicate a compiler",
        "Concrete tools, thresholds, commands, and CI cadence belong to the consuming project",
    ] {
        assert!(
            verification.contains(required),
            "verification selection lost marker: {required}"
        );
    }
}

#[test]
fn project_organization_has_one_authority_and_honest_closeout() {
    let reference = normalize_whitespace(&read(
        "assets/base/claude/skills/cf-method/references/project-organization.md",
    ));
    let task = normalize_whitespace(&read("assets/base/pm/task.md.tmpl"));
    let epic = normalize_whitespace(&read("assets/base/pm/epic.md.tmpl"));

    for required in [
        "epics/EPC-NNN.md",
        "tasks/TSK-NNN.md",
        "specs/SPC-NNN.md",
        "independent, repo-wide sequences",
        "The authority owns status, acceptance, and lifecycle",
        "never mirrored status",
        "must remain rebuildable from Git Markdown",
        "Do not import or paraphrase an equivalent authoritative tree",
        "reconcile and dual-approve Plan vN+1",
        "codeflow work start TSK-NNN",
        "planning PR",
    ] {
        assert!(
            reference.contains(required),
            "project organization lost required marker: {required}"
        );
    }

    for required in [
        "external_refs: []",
        "Bounded discoveries/deviations",
        "never legalized here after implementation",
        "Shared engineering/security/testing doctrine stays in AGENTS.md",
    ] {
        assert!(
            task.contains(required),
            "task template lost required marker: {required}"
        );
    }
    assert!(epic.contains("external_refs: []"));
    assert!(epic.contains("Affected surfaces and interfaces"));
}

#[test]
fn editorial_quality_is_contextual_on_demand_and_cross_harness() {
    let full_agents = normalize_whitespace(&read("assets/base/AGENTS.md.tmpl"));
    let minimal_agents = normalize_whitespace(&read("assets/base/AGENTS.minimal.md.tmpl"));
    let lifecycle = normalize_whitespace(&read(
        "assets/base/claude/skills/cf-method/references/workflow-lifecycle.md",
    ));
    let skill = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-editorial-review/SKILL.md",
    ));
    let smells = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-editorial-review/references/editorial-smells.md",
    ));

    assert!(
        full_agents.contains(
            "verified truth, policy, technical meaning, project voice, and accessibility"
        ) && full_agents.contains("`cf-editorial-review`"),
        "full entry contract lost its compact editorial route"
    );
    for required in [
        "verified truth and policy outrank CodeFlow philosophy",
        "documented voice/examples",
        "Preserve technical meaning",
        "never fabricate personality",
    ] {
        assert!(
            lifecycle.contains(required),
            "workflow lifecycle lost editorial principle: {required}"
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
        "claude-judgment-primary` reviews",
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
    let delegate = normalize_whitespace(&read("assets/base/claude/skills/cf-delegate/SKILL.md"));
    let adapter = read("assets/base/claude/skills/cf-delegate/resources/claude-turn-completion.md");

    assert!(delegate.contains("codeflow delegate init"));
    assert!(delegate.contains("StopFailure"));
    assert!(delegate.contains("--until terminal"));
    assert!(
        delegate.contains("--model $CLAUDE_MODEL --effort $CLAUDE_EFFORT --permission-mode auto")
    );
    assert!(delegate.contains("Launch with default effort"));
    assert!(delegate.contains("workers take escalation"));
    assert!(delegate.contains("current-ensemble.json"));
    assert!(delegate.contains("autoMode.classifyAllShell"));
    assert!(delegate.contains("sandbox.failIfUnavailable"));
    assert!(delegate.contains("capture unrelated tmux sessions"));
    assert!(!delegate.contains("two identical captures"));
    assert!(!delegate.contains("tmux wait-for"));

    for required in [
        "codeflow hook delegate-turn",
        "owner-only",
        "--until terminal",
        "last_assistant_message",
        "autoMode.classifyAllShell",
        "--permission-mode auto",
        "schema_version",
        "only for bounded diagnosis",
    ] {
        assert!(
            adapter.contains(required),
            "completion adapter lost required marker: {required}"
        );
    }
    assert!(!adapter.contains("tmux wait-for"));
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
