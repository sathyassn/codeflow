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

/// TSK-129: the quality contract loads by section from an index, so a pin on
/// "the quality contract" reads the index and every section file together.
fn read_quality_contract() -> String {
    let resources = "assets/base/agents/skills/cf-model-orchestrator/resources";
    let mut sections: Vec<_> = std::fs::read_dir(repo_root().join(resources).join("quality"))
        .expect("quality sections")
        .map(|entry| entry.expect("section entry").path())
        .collect();
    sections.sort();
    let mut contract = read(&format!("{resources}/quality-contract.md"));
    for section in sections {
        contract.push('\n');
        contract.push_str(&std::fs::read_to_string(&section).expect("read section"));
    }
    contract
}

/// TSK-129: the orchestrator skill keeps what every task needs and moves
/// trigger-only text to `references/`; a pin on the skill reads both.
fn read_orchestrator_skill() -> String {
    let skill = "assets/base/agents/skills/cf-model-orchestrator";
    let mut references: Vec<_> = std::fs::read_dir(repo_root().join(skill).join("references"))
        .expect("orchestrator references")
        .map(|entry| entry.expect("reference entry").path())
        .collect();
    references.sort();
    let mut text = read(&format!("{skill}/SKILL.md"));
    for reference in references {
        text.push('\n');
        text.push_str(&std::fs::read_to_string(&reference).expect("read reference"));
    }
    text
}

/// TSK-129: capability-routing also loads by section from an index.
/// TSK-184: the six every-task routing files merged into the orchestrator's
/// seat section (the session-level routing read) and the plan's assignment
/// section, so the routing contract reads those homes too.
fn read_routing_contract() -> String {
    let resources = "assets/base/agents/skills/cf-model-orchestrator/resources";
    let mut sections: Vec<_> = std::fs::read_dir(repo_root().join(resources).join("routing"))
        .expect("routing sections")
        .map(|entry| entry.expect("section entry").path())
        .collect();
    sections.sort();
    let mut contract = read(&format!("{resources}/capability-routing.md"));
    for section in sections {
        contract.push('\n');
        contract.push_str(&std::fs::read_to_string(&section).expect("read section"));
    }
    for home in [
        "assets/base/agents/skills/cf-model-orchestrator/SKILL.md".to_string(),
        format!("{resources}/quality/plan.md"),
    ] {
        contract.push('\n');
        contract.push_str(&read(&home));
    }
    contract
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
    let skill = normalize_whitespace(&read_orchestrator_skill());
    let capability_routing = normalize_whitespace(&read_routing_contract());

    for required in [
        "Detect capabilities, not model identity.",
        "Claude Code",
        "Codex App or interactive Codex CLI",
        "Grok Build (interactive `grok` CLI)",
        "Other harness, including Hermes",
        "never a silent third vote",
        "Name extra families on trigger if available",
        "strongest capable permitted reasoning route",
        // TSK-150 (H24): the Grok host detail is a named read before launch.
        "Before a Grok preflight or launch, also read [the Grok host detail](resources/grok-host.md)",
        "**Both think independently.**",
        "**Claude leads design.**",
        "**Host routes execution.**",
        "**Review is author-relative.**",
        "**The Claude judgment primary owns integrated Claude judgment.**",
        "task fit",
        "verified native routing, and resources, on the plan's assignment line",
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
        // TSK-184: one plan, approved once; the round cap is gone.
        "Plan settlement ends when both seats approve one version or the host stops for the operator.",
        "approval of an older version does not carry forward",
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
    let routing = normalize_whitespace(&read_routing_contract());
    for required in [
        "The first line of every cross-family task declares `ROLE: peer`",
        "qualified primary at its default effort",
        "is only for same-family work its primary dispatches as a native subagent of its own session",
        "A caller never selects a foreign worker or passes worker escalation effort",
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
    let routing = normalize_whitespace(&read_routing_contract());
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
        "the observed model, effort and native provenance are recorded with the return",
        "Unknown usage is advisory",
        "Never combine apparently separate limits",
        "is a reassignment: it is recorded and reviewed by one other-lineage seat",
        "permitted worker change",
        "Default effort is high for primary seats, not a ceiling",
        "strongest capable permitted same-family reasoning route",
        "An xhigh trigger requires the owning primary to obtain xhigh reasoning",
        "Delegate substantial, well-specified routine implementation",
        "The primary critically integrates worker findings and retains approvals",
        "a lineage different from the actual author's reviews it independently",
        "`candidate` and `scoped-qualified` describe evidence status",
        "three fresh accepted trials",
        "Full primary-binding promotion",
        "economical-default",
        "a model cannot independently review its own authored unit",
        "delegating back to the host lineage",
        "generic same-lineage subagent never satisfies",
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
    let skill = normalize_whitespace(&read_orchestrator_skill());
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
    let routing = normalize_whitespace(&read_routing_contract());
    let quality = normalize_whitespace(&read_quality_contract());
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
        // TSK-129: the unknown-usage duty's one home is capability-routing.
        "Unknown usage is advisory while task fit, capability and quality still decide the route",
        "It blocks only when the brief declares an explicit hard limit",
    ] {
        assert!(
            routing.contains(required),
            "accountable routing contract lost marker: {required}"
        );
    }

    for required in [
        "Primary responsibility and actual execution are separate",
        "does not become its author",
    ] {
        assert!(
            quality.contains(required),
            "quality contract lost accountable-execution marker: {required}"
        );
    }

    // TSK-184: the design owner, fixture limit and override rule have one
    // home, the orchestrator's "Claude leads design" invariant; cf-design
    // points at it.
    for required in [
        "owns real design execution and fidelity",
        "Candidates run only disposable fixtures",
        "Claude absence is not one",
    ] {
        assert!(
            routing.contains(required),
            "design execution contract lost marker: {required}"
        );
    }
    for required in [
        "follow the orchestrator's \"Claude leads design\" invariant",
        "Turning settled product/UX/UI into components, layout, styles, or interactions is design implementation",
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

/// TSK-184: independent discovery comes first; Claude then drafts the one
/// plan and Codex challenges it against its own findings. The anti-anchoring
/// guard is that both seats return findings before either sees the other's.
#[test]
fn independent_discovery_precedes_the_one_challenged_plan() {
    let skill = read_orchestrator_skill();
    let capabilities = read("docs/capabilities.md");
    let normalized = normalize_whitespace(&skill);

    for required in [
        "Claude and Codex research, analyze, and identify risks in parallel and return their findings before seeing the other's conclusions.",
        "Give both seats the same immutable brief and repository scope.",
        "each seat independently returns its findings",
        "Claude drafts the one plan **from its native session**",
        "Codex challenges that plan against its own findings: feasibility, failure modes, security, testing, maintainability",
        "There is no second plan and no reconciliation round.",
    ] {
        assert!(
            normalized.contains(required),
            "orchestrator lost the independent-discovery contract: {required}"
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

    // TSK-127 moved the entry-point cell's doctrine into the owning skill.
    assert!(
        normalized.contains(
            "Both families independently research and analyze; Claude drafts the plan and owns design and integrated judgment, Codex challenges it; the host assigns"
        ),
        "the orchestrator skill must expose independent discovery in its description"
    );
    assert!(
        normalize_whitespace(&capabilities).contains("nineteen health checks"),
        "CAP-008 must count the grok and reading doctor checks"
    );
    let readme = normalize_whitespace(&read("README.md"));
    let architecture = normalize_whitespace(&read("docs/architecture.md"));
    assert!(
        readme.contains("Health checks (19): hooks, claude, codex, grok, config"),
        "README must list the grok doctor check"
    );
    assert!(
        architecture.contains("19 checks: hooks, claude, codex, grok, config"),
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
fn readme_distinguishes_installed_and_effective_discipline() {
    let readme = normalize_whitespace(&read("README.md"));

    for required in [
        "Integrated, proportional discipline",
        "Full tier adds durable epics, tasks, and specs plus referential checks",
        "Trivial or conversational work needs no new artifact",
        "Session summaries are captured automatically only when a supported harness's SessionEnd hook is installed and actually executes",
        "Installed files alone do not make those planes effective",
        "Minimal init scaffolds local hooks, in-session settings, and CI while preserving an existing hook manager; it does not configure remote branch protection",
        "Verify hook execution, harness trust and event support, required CI results, and actual remote rules, permissions, and bypasses",
        "Codex-driven work receives the git-hook plane where those hooks are installed and executed",
        "CI becomes a merge gate when the remote requires its result",
    ] {
        assert!(
            readme.contains(required),
            "README lost an installed-versus-effective or proportionality qualifier: {required}"
        );
    }

    for unsupported in [
        "none of it survives the session",
        "those prescribe a per-change authoring ceremony",
        "re-runs the gates as the authoritative perimeter",
        "Codex-driven work is bound unconditionally by the git-hook plane",
    ] {
        assert!(
            !readme.contains(unsupported),
            "README reintroduced an unsupported categorical claim: {unsupported}"
        );
    }
}

#[test]
fn design_review_and_security_roles_cannot_silently_drift() {
    let skill = read_orchestrator_skill();
    let reviewer = read("assets/base/claude/agents/cf-reviewer.md");
    let security = read("assets/base/claude/agents/cf-security-reviewer.md");
    let quality = read_quality_contract();
    let normalized = normalize_whitespace(&skill);

    for required in [
        "unless the brief already fixes a clear direction, compares 2 to 3 viable options",
        "the design options and recommendation, or the recorded constraint when the brief already dictates one clear direction",
        "a lineage different from the actual author's reviews it independently. Self-review is never independent.",
        "The Claude judgment primary owns integrated Claude judgment.",
        "they do not replace the required other-lineage review or primary judgment.",
        "separate interactive Claude session in auto mode under the same fail-closed sandbox (not plan or bypass mode)",
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
        // TSK-184: the reviewer applies the project's configured floor; the
        // quality contract keeps the 80% floor and 90% target (below).
        "coverage against the project's configured floor",
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
        "a later change needs fresh approval only when it changes the outcome, a cross-task interface, the dependency graph or a safety boundary",
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
fn responsible_autonomy_has_detailed_quality_and_security_owners() {
    let quality = normalize_whitespace(&read_quality_contract());
    let security = normalize_whitespace(&read("assets/base/claude/agents/cf-security-reviewer.md"));

    for required in [
        "Apply authority to effects, not tool verbs",
        // TSK-184: the authority binding is stated once in the workflow
        // discipline rules (pinned below); the section points there.
        "are stated once in the workflow discipline rules (acting within legitimate intent and bounded authority)",
        "Use personal or confidential data only when necessary and authorized",
        "A qualified provider or route is not blanket authority",
        "Missing a required hard control withholds that risky lane",
        "same idempotency key where supported",
        "A compensating action reduces harm but is not guaranteed reversal",
        "preserve the minimum protected evidence",
    ] {
        assert!(
            quality.contains(required),
            "quality contract lost responsible-autonomy duty: {required}"
        );
    }

    let discipline = normalize_whitespace(&read("assets/base/rules/workflow-discipline.md"));
    for required in [
        "Continue unchanged safe steps only for their authorized instance/count, without re-asking",
        "an identical tuple grants no standing authority",
    ] {
        assert!(
            discipline.contains(required),
            "workflow discipline lost responsible-autonomy duty: {required}"
        );
    }

    for required in [
        "necessary and authorized before it reaches a provider/tool",
        "prompt, URL, log, screenshot, trace, feedback, Git record, or peer",
        "Route qualification is not data authority",
        "purpose, action, resource, data, destination/recipient, and side effects",
        "GET/read may disclose or mutate",
        "Inspect uncertain non-idempotent outcomes before retry",
        "preserve bounded incident evidence",
    ] {
        assert!(
            security.contains(required),
            "security reviewer lost responsible-autonomy duty: {required}"
        );
    }
}

#[test]
fn always_loaded_reasoning_and_output_contract_survives_refactors() {
    // TSK-127: the map keeps the one-line kernel; the full text is one hop
    // away in the shared references every tier installs.
    let map = normalize_whitespace(&read("assets/base/AGENTS.md.tmpl"));
    for required in [
        "Evidence and honest analysis outrank agreement",
        "Find broadly; act by materiality.",
        "Prove it where it runs.",
        ".codeflow/rules/workflow-discipline.md",
        ".codeflow/rules/writing.md",
    ] {
        assert!(
            map.contains(required),
            "rule map lost its kernel line: {required}"
        );
    }
    let agents = normalize_whitespace(&format!(
        "{}\n{}",
        read("assets/base/rules/workflow-discipline.md"),
        read("assets/base/rules/writing.md")
    ));

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
            "workflow and writing references lost a load-bearing duty: {required}"
        );
    }
}

#[test]
fn every_non_trivial_task_is_stage_aware_and_uses_effective_autonomy() {
    let skill = normalize_whitespace(&read_orchestrator_skill());
    let agents = read("assets/base/AGENTS.md.tmpl");
    // Git may materialize text assets with CRLF on Windows. This contract
    // pins the authored line break, not the checkout's newline convention.
    let claude = read("assets/base/CLAUDE.md.tmpl").replace("\r\n", "\n");
    let ensemble =
        read("assets/base/agents/skills/cf-model-orchestrator/resources/current-ensemble.json");
    let capability_routing = normalize_whitespace(&read_routing_contract());
    let herdr = normalize_whitespace(&read("assets/base/agents/skills/cf-herdr/SKILL.md"));
    let claude_completion = normalize_whitespace(&read(
        "assets/base/claude/skills/cf-delegate/resources/claude-turn-completion.md",
    ));

    for required in [
        "Use the duo for routed work, decided by touched paths as AGENTS.md states; when unsure, route.",
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

    // TSK-127: entry is decided by touched paths (operator decision of
    // 2026-09-26); the map states it, CLAUDE.md carries the stage detail.
    let agents = normalize_whitespace(&agents);
    for required in [
        "Orchestration entry is decided by touched paths",
        "`/cf-model-orchestrator`",
        "when unsure, route",
    ] {
        assert!(
            agents.contains(required),
            "AGENTS map lost duo entry-point marker: {required}"
        );
    }
    // TSK-184: research or analysis that will drive a change starts with the
    // orchestrator (the map's routing rule); the CLAUDE notes carry the
    // Claude mechanism for that row, and the research-only exit lives in
    // the orchestrator's outcome modes.
    assert!(
        agents.contains(
            "research or analysis that will drive one, and plan, design, security or irreversible work start with `/cf-model-orchestrator`, once per brief"
        ),
        "AGENTS map lost the early routing rule"
    );
    assert!(
        skill.contains(
            "independent discovery, evidence comparison, settled findings, then stop without edits"
        ),
        "orchestrator lost the research-only exit"
    );
    let claude_normalized = normalize_whitespace(&claude);
    for required in [
        "`/cf-model-orchestrator` once per brief",
        "are supporting or solo flows, not alternate entry points",
        "when unsure, route",
    ] {
        assert!(
            claude_normalized.contains(required),
            "CLAUDE template lost duo entry-point marker: {required}"
        );
    }
}

#[test]
fn solo_fallback_requires_fresh_context_independent_review() {
    let skill = normalize_whitespace(&read_orchestrator_skill());
    let agents = normalize_whitespace(&read("assets/base/AGENTS.md.tmpl"));

    for (owner, contract, alternate) in [
        ("orchestrator", &skill, "else a separate read-only pass"),
        (
            "AGENTS",
            &agents,
            "else `cf-reviewer` with reduced assurance recorded",
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
}

#[test]
fn quality_contract_pins_evidence_coverage_and_ui() {
    let contract = normalize_whitespace(&format!(
        "{}\n{}",
        read_quality_contract(),
        read_routing_contract()
    ));

    for required in [
        // TSK-184: the plan form keeps assignments and named-version approvals.
        "ASSIGNMENTS:",
        "CROSS_LINEAGE_REVIEWER seat@effort",
        "APPROVALS: <each seat and the plan version it approved>",
        "A model claim, consensus, or approval never substitutes for a source",
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
        // TSK-184: one parallel field list; the batch candidate is gated once.
        "PARALLEL_TASKS:",
        "OWNER_BRANCH_WORKTREE:",
        "HOTSPOT_OWNER:",
        "CONCURRENCY_CAP:",
        "one writer, branch and worktree per task",
        "one full gate runs on that exact candidate before the integration line moves",
        "Both seats grade design proportionality before approval",
        "smallest coherent solution",
        "Material avoidable complexity is `changes_requested`",
        "brittle under-design",
        "existing design system",
        "accessible primitives",
        "stated once in the workflow discipline rules, \"Write only what earns its keep\"",
        "a model cannot independently review its own authored unit",
    ] {
        assert!(
            contract.contains(required),
            "quality contract lost required marker: {required}"
        );
    }
}

#[test]
fn task_graph_and_verification_strength_are_proportionate_contracts() {
    let skill = normalize_whitespace(&read_orchestrator_skill());
    let plan = normalize_whitespace(&read("assets/base/agents/skills/cf-plan/SKILL.md"));
    let develop = normalize_whitespace(&read("assets/base/agents/skills/cf-develop/SKILL.md"));
    let graph = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-model-orchestrator/resources/task-graph.md",
    ));
    let verification = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-model-orchestrator/resources/verification-selection.md",
    ));

    for required in [
        "the approval covers the canonical task graph",
        "Only a change of outcome, cross-task interface, dependency graph or safety boundary creates a new plan version under the task-graph contract",
        "Ordinary steps and bounded implementation choices inside an approved node stay in the ledger",
        "names a technique only when one is selected",
    ] {
        assert!(
            skill.contains(required),
            "orchestrator lost graph/verification marker: {required}"
        );
    }

    for required in [
        "TASK_ID | OUTCOME | RESPONSIBLE_PRIMARY seat@effort | EXEC_MODE | EXECUTION | AUTHORSHIP",
        "Every active bare edge means B cannot be accepted or land until A has landed",
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
        "Create a new plan version, approved by both primary seats before dependent work continues",
        "These ride in one batched epic amendment on a `plan/` branch, reviewed by one other-lineage seat",
        "different valid topological order",
        "Log material dependency or decision changes and meaningful checkpoints in the execution ledger",
        "without turning CodeFlow into a scheduler",
    ] {
        assert!(
            graph.contains(required),
            "task graph lost marker: {required}"
        );
    }

    assert!(
        plan.contains("run `codeflow validate --docs`"),
        "planning must run the task-graph validator before approval"
    );
    assert!(
        develop.contains("`codeflow validate --docs`, each cited with revision and command"),
        "delivery must run the task-graph validator before completion"
    );

    for required in [
        "when none is selected, write nothing",
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
        "stop; a new plan version or the batched epic amendment",
        "Completion never retroactively legitimizes a material deviation",
        "Shared engineering, security, testing, design, and review doctrine stays in `AGENTS.md`",
        "codeflow work start TSK-NNN",
        "planning PR",
    ] {
        assert!(
            reference.contains(required),
            "project organization lost required marker: {required}"
        );
    }

    for required in [
        // TSK-184: the closeout narrative is gone; review-relevant scope and
        // recovery notes live in the description, completion in the block.
        "external_refs: []",
        "Review-relevant scope, interfaces and recovery notes go here",
        "for complete the fenced `yaml` acceptance block",
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
    // TSK-127: one writing reference serves every tier.
    let full_agents = normalize_whitespace(&read("assets/base/rules/writing.md"));
    let minimal_agents = full_agents.clone();
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
    // TSK-184: the reply and writing policy moved from the lifecycle to the
    // writing reference (pinned below); the lifecycle points there.
    assert!(
        lifecycle
            .contains("`.codeflow/rules/writing.md`). That file is the one home of the reply and"),
        "workflow lifecycle lost its pointer to the writing reference"
    );
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
    // TSK-129: the reverse lane lives in the lifecycle lane file; the core is
    // read too so the legacy-protocol negatives still cover the whole skill.
    let delegate = normalize_whitespace(&format!(
        "{}\n{}",
        read("assets/base/claude/skills/cf-delegate/SKILL.md"),
        read("assets/base/claude/skills/cf-delegate/resources/lane-lifecycle.md")
    ));
    let adapter = read("assets/base/claude/skills/cf-delegate/resources/claude-turn-completion.md");

    // TSK-163: the launch sequence and turn detection are stated once, in the
    // adapter; the lane points there before launch.
    assert!(delegate.contains("before launching Claude, read and follow the shipped"));
    let launch = normalize_whitespace(&adapter);
    assert!(launch.contains("codeflow delegate init"));
    assert!(launch.contains("StopFailure"));
    assert!(launch.contains("--until terminal"));
    assert!(launch.contains("--model $CLAUDE_MODEL --effort $CLAUDE_EFFORT"));
    assert!(launch.contains("For consult/no-edit, use the same launch with --permission-mode auto"));
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

// TSK-131: the holistic-fix doctrine salvaged from the EPC-015 line (D19 to
// D23) and the working method of 2026-09-25, each sentence in its one home on
// this line. The classification of every EPC-015 pin (present, carried or
// dropped with a reason) is in `docs/verification/tsk-131-duty-map.md`.
const TSK131_FINDINGS: &str =
    "assets/base/agents/skills/cf-model-orchestrator/resources/quality/findings.md";
const TSK131_BLOCKERS: &str =
    "assets/base/agents/skills/cf-model-orchestrator/resources/quality/blockers-and-gates.md";
const TSK131_DESIGN: &str =
    "assets/base/agents/skills/cf-model-orchestrator/resources/quality/design-implementation.md";
const TSK131_QUALITY_INDEX: &str =
    "assets/base/agents/skills/cf-model-orchestrator/resources/quality-contract.md";
const TSK131_ORCHESTRATOR: &str = "assets/base/agents/skills/cf-model-orchestrator/SKILL.md";
const TSK131_DEVELOP: &str = "assets/base/agents/skills/cf-develop/SKILL.md";
const TSK131_REVIEWER: &str = "assets/base/claude/agents/cf-reviewer.md";
const TSK131_CONSULT: &str = "assets/base/agents/skills/cf-consult/SKILL.md";
const TSK131_DISCIPLINE: &str = "assets/base/rules/workflow-discipline.md";
const TSK131_MAP: &str = "assets/base/AGENTS.md.tmpl";
const TSK131_EVAL_REQUIREMENTS: &str =
    "assets/base/agents/skills/cf-evaluate-model/resources/requirements.json";

/// (EPC-015 pin, owner sentence on this line, owner file): one row per D19
/// to D23 obligation that is present or carried.
const HOLISTIC_FIX_PINS: &[(&str, &str, &str)] = &[
    ("D19 mechanism", "states the evidenced mechanism, the relevant conditions and the remaining uncertainty in one sentence", TSK131_FINDINGS),
    ("D19 regression", "adds a regression test that fails before the fix and passes after (Required verification, regression tests for each fixed defect)", TSK131_FINDINGS),
    ("D19 probe", "that probe is one command already run that fails on the **exact reported symptom** before hypothesising", TSK131_BLOCKERS),
    ("D19 sufficiency", "cause counts as found only when the evidenced conditions are sufficient to produce the failure", TSK131_FINDINGS),
    ("D19 reproduction", "reproduce safely where feasible, otherwise record the strongest evidence and the reproduction limit", TSK131_FINDINGS),
    ("D19 no masking", "Never mask a violated contract to make a check pass: a catch, fallback, weakened or skipped test, or retry is a fix only when it implements the contract's required failure behavior.", TSK131_FINDINGS),
    ("D19 probe citation", "For a defect that resists a first glance, run the probe in [blocker navigation](blockers-and-gates.md) before hypothesising.", TSK131_FINDINGS),
    ("D19 develop", "When the change fixes a defect, apply Repair in `cf-model-orchestrator/resources/quality/findings.md`: state the evidenced mechanism and add a regression test that fails before the fix and passes after", TSK131_DEVELOP),
    ("D19 develop probe", "for a defect that resists a first glance, first run one command that fails on the exact reported symptom", TSK131_DEVELOP),
    ("D19 reviewer", "the mechanism sentence and a regression test that fails before the fix and passes after", TSK131_REVIEWER),
    ("D20 every task", "Before selecting a fix or a design, name the bounded impact set (callers, consumers, inputs, effects, same-mechanism siblings, data, configuration, tests, docs)", TSK131_DESIGN),
    ("D20 adjacent", "verify the adjacent behavior the change could disturb as well as the changed path.", TSK131_DESIGN),
    ("D20 candidate set", "candidate impact set (callers, consumers, upstream inputs, downstream effects, sibling code with the same mechanism, data and schema, configuration, tests, docs)", TSK131_FINDINGS),
    ("D20 actual set", "refine it during verification into the actual impact set", TSK131_FINDINGS),
    ("D20 classification", "classify analogous occurrences by mechanism and materiality", TSK131_FINDINGS),
    ("D20 siblings", "Same-cause occurrences inside the authorized scope are repaired together", TSK131_FINDINGS),
    ("D20 discovery", "other discoveries route through `fix now`, `track once` or `drop` under [materiality](materiality.md)", TSK131_FINDINGS),
    ("D20 stacking", "separately authorized work stacks only where a dependency justifies it", TSK131_FINDINGS),
    ("D20 selection body", "Run the tests of the change's dependents and consumers and of the journey the change sits in", TSK131_FINDINGS),
    ("D20 selection scope", "verify each same-mechanism sibling repaired with it", TSK131_FINDINGS),
    ("D20 agents pointer", "what it touches upstream and downstream", TSK131_MAP),
    ("D20 develop", "name the bounded impact set (quality contract)", TSK131_DEVELOP),
    ("D20 reviewer", "Require the named impact set", TSK131_REVIEWER),
    ("D21 pre-apply", "the departure form, for a change that newly departs from the approved contract, scope, authority or risk boundary (a public contract break, a moved security boundary, scope growth, an irreversible action)", TSK131_BLOCKERS),
    ("D21 form", "situation with evidence, the boundary crossed, options with cost and reversibility, and one recommendation", TSK131_BLOCKERS),
    ("D21 withheld", "the dependent action waits while authorized independent work continues", TSK131_DISCIPLINE),
    ("D21 reuse", "An already approved departure is reused and not asked again.", TSK131_BLOCKERS),
    ("D21 compatibility", "Compatibility is judged by the git rules' breaking-change rule and the cf-ship release-policy reference (affected consumers, migration or deprecation, mixed-version operation, recovery).", TSK131_BLOCKERS),
    ("D21 trigger", "| when a step is blocked or would depart from what was approved, or a check or CI job is red or did not finish |", TSK131_QUALITY_INDEX),
    ("D21 minimal", "Before applying a change that newly departs from the approved contract, scope, authority, or risk boundary, stop and surface it in that form first", TSK131_DISCIPLINE),
    ("D22 remedy", "Every blocker or major finding carries the smallest evidenced remedy and its verification criterion", TSK131_FINDINGS),
    ("D22 uncertain", "when the remedy is uncertain, the required outcome and a bounded diagnostic next step", TSK131_FINDINGS),
    ("D22 options", "a finding that is an operator decision or a design question carries the options instead", TSK131_FINDINGS),
    ("D22 read-only", "The reviewer stays read-only, never writes a redesign or a new `DESIGN_INTENT` to fill the field, and never reopens settled design without a demonstrated defect", TSK131_FINDINGS),
    ("D22 minor", "A missing remedy on a minor is not incompleteness.", TSK131_FINDINGS),
    ("D22 no second flag", "Blocking is derived from the existing severity, confidence and gate policy; no second flag.", TSK131_FINDINGS),
    ("D22 security", "The security reviewer's `remediation` field is the same duty under its existing name.", TSK131_FINDINGS),
    ("D22 reviewer field", "remedy: <blocker and major: smallest evidenced fix and its verification criterion", TSK131_REVIEWER),
    ("D22 routing brief", "the remedy expected on every blocker and major finding, and the provenance the reply must carry", TSK131_FINDINGS),
    ("D22 routing return", "returns the verdict, the findings with their remedy, what was verified and what was not verified", TSK131_FINDINGS),
    ("D22 routing scope", "Cross-lineage and Herdr briefs follow this contract.", TSK131_FINDINGS),
    ("D22 consult", "the smallest evidenced remedy and its verification criterion, or the options when the fix is an operator decision", TSK131_CONSULT),
    ("D23 batch", "collects the findings into one dependency-ordered batch with provenance preserved, deduplicates them by mechanism", TSK131_FINDINGS),
    ("D23 evaluation", "evaluates each proposed remedy against the diagnosed mechanism and the impact set", TSK131_FINDINGS),
    ("D23 disposition", "records accept, modify or reject with the reason", TSK131_FINDINGS),
    ("D23 rejection", "Rejecting a remedy never closes the finding or waives a gate; a disputed finding returns with evidence to the reviewer who raised it.", TSK131_FINDINGS),
    ("D23 conflict", "Conflicting remedies are investigated against the mechanism, the impact evidence and the accepted contract.", TSK131_FINDINGS),
    ("D23 escalation", "One consolidated decision goes to the operator in the departure form under [blocker navigation](blockers-and-gates.md) only when resolution needs operator-owned intent, authority or risk acceptance.", TSK131_FINDINGS),
    ("D23 one cycle", "Apply the accepted batch as one apply-and-verify cycle (stacked dependents from Change impact stay separate) and re-verify the impact set.", TSK131_FINDINGS),
    ("D23 re-review", "The finder confirms each material fix on the affected scope, widened when the impact or the prior evidence is uncertain", TSK131_FINDINGS),
    ("D23 probe rerun", "A small fix whose finding came with a failing probe is confirmed by rerunning that probe and the affected tests", TSK131_FINDINGS),
    ("D23 failed cycle", "A cycle that introduces an attributable regression is a failed cycle.", TSK131_FINDINGS),
    ("D23 gates unchanged", "The required gates and the [completion](completion.md) section are unchanged.", TSK131_FINDINGS),
    // TSK-184 removed the cycle cap (change list WP3 findings row, WP4
    // cf-develop row); the progress rule and the strategy change replace it.
    ("D23 progress", "Continue while repairs produce relevant evidence; diagnose a stalled mechanism, an invalid assumption or a materially changed scope", TSK131_FINDINGS),
    ("D23 strategic", "then split, redesign or take the intent question to the operator. That decision is never made by a round counter and never by automatic acceptance.", TSK131_FINDINGS),
    ("D23 develop", "No cycle count decides: continue while repairs produce relevant evidence", TSK131_DEVELOP),
    ("D23 develop owner", "act on the round's findings as `cf-model-orchestrator/resources/quality/findings.md` sets out", TSK131_DEVELOP),
    ("D23 eval kit", "\"diagnose a stalled mechanism, an invalid assumption or a materially changed scope\"", TSK131_EVAL_REQUIREMENTS),
];

/// TSK-131 AC-1: every present or carried D19 to D23 obligation keeps its
/// owner sentence.
#[test]
fn holistic_fix_doctrine_keeps_one_owner_sentence_each() {
    let mut owners: BTreeMap<&str, String> = BTreeMap::new();
    for (pin, needle, file) in HOLISTIC_FIX_PINS {
        let text = owners
            .entry(file)
            .or_insert_with(|| normalize_whitespace(&read(file)));
        assert!(
            text.contains(&normalize_whitespace(needle)),
            "{pin}: owner sentence missing in {file}: {needle}"
        );
    }
}

/// Deleting any pinned duty from its owner text fails the pin: each needle
/// is removed in memory and the owner text is checked again.
#[test]
fn holistic_fix_pins_fail_when_a_duty_is_deleted() {
    for (pin, needle, file) in HOLISTIC_FIX_PINS {
        let wanted = normalize_whitespace(needle);
        let mutated = normalize_whitespace(&read(file)).replacen(&wanted, "", 1);
        assert!(
            !mutated.contains(&wanted),
            "{pin}: deleting the duty from {file} left the pin satisfied"
        );
    }
}

/// TSK-131 AC-2: the shipped skills state the working method, and every
/// round bound agrees with the change-class bound in the findings section.
#[test]
fn working_method_rounds_agree_across_the_shipped_skills() {
    let findings = normalize_whitespace(&read(TSK131_FINDINGS));
    // TSK-184: one holistic pass per revision, finder confirmation, no round
    // or cycle cap.
    for required in [
        "Review is one holistic pass per revision: every assigned reviewer reviews the whole change in parallel on that revision, with no minimum or maximum number of passes.",
        "The finder confirms each material fix on the affected scope",
        "Nits need no confirmation.",
        "That decision is never made by a round counter and never by automatic acceptance.",
    ] {
        assert!(
            findings.contains(required),
            "findings section lost the working method: {required}"
        );
    }
    let orchestrator = normalize_whitespace(&read(TSK131_ORCHESTRATOR));
    for required in [
        "Use the duo for routed work, decided by touched paths as AGENTS.md states; when unsure, route.",
        "Plan settlement ends when both seats approve one version or the host stops for the operator.",
        "When review findings are acted on, they are batched per [findings](resources/quality/findings.md)",
        "as [findings](resources/quality/findings.md) sets out, fixed in the same open PR.",
    ] {
        assert!(
            orchestrator.contains(required),
            "orchestrator lost the working method: {required}"
        );
    }
    assert!(
        normalize_whitespace(&read(TSK131_DEVELOP))
            .contains("No cycle count decides: continue while repairs produce relevant evidence"),
        "cf-develop lost the progress rule"
    );
    // Fable review of d60ce13f0: the defect-fix duty is stated where every
    // build happens, not only after a review bounces the change.
    let develop = normalize_whitespace(&read(TSK131_DEVELOP));
    let build = develop
        .split_once("a. **Build**")
        .and_then(|(_, rest)| rest.split_once("b. **Review**"))
        .map(|(build, _)| build)
        .expect("cf-develop step 5a");
    for required in [
        "When the change fixes a defect",
        "a regression test that fails before the fix and passes after",
        "exact reported symptom",
    ] {
        assert!(
            build.contains(required),
            "cf-develop Build lost the defect-fix duty: {required}"
        );
    }
    assert!(
        normalize_whitespace(&read(TSK131_DISCIPLINE))
            .contains("The finder confirms each material fix on the affected scope")
            && normalize_whitespace(&read(TSK131_DISCIPLINE)).contains(
                "There is no round cap: continue while repairs produce relevant evidence"
            ),
        "the discipline reference lost the review rule"
    );
    assert!(
        normalize_whitespace(&read(TSK131_QUALITY_INDEX)).contains(
            "| [Review findings and repair](quality/findings.md) | when a defect is fixed, or review findings are briefed, written or acted on |"
        ),
        "the quality index lost the findings row"
    );

    // Agreement: no shipped instruction keeps a superseded round bound.
    let mut files = Vec::new();
    collect_markdown(&repo_root().join("assets/base"), &mut files);
    for superseded in [
        "Maximum 3 evidence-moving cycles",
        "rework are each bounded to at most two rounds",
        "Rework is bounded to two rounds",
        "post-review repair is bounded to two evidence-moving cycles in every route",
        // TSK-184 removed every round and cycle cap.
        "Maximum 2 evidence-moving cycles",
        "bounded to at most two rounds",
        "two review rounds per submitted version",
    ] {
        let holders: Vec<String> = files
            .iter()
            .filter(|path| {
                normalize_whitespace(&std::fs::read_to_string(path).unwrap()).contains(superseded)
            })
            .map(|path| path.display().to_string())
            .collect();
        assert!(
            holders.is_empty(),
            "superseded round bound `{superseded}` still shipped in {holders:?}"
        );
    }
}

fn collect_markdown(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect_markdown(&path, out);
        } else if path
            .extension()
            .is_some_and(|ext| ext == "md" || ext == "tmpl" || ext == "json")
        {
            out.push(path);
        }
    }
}

/// TSK-131 AC-3: the copy guide sections of TSK-073, in order, in the one
/// writing reference.
const COPY_GUIDE_SECTIONS: &[&str] = &[
    "Voice",
    "Sentences",
    "Words",
    "Titles and headings",
    "Leads",
    "Captions",
    "Summaries",
    "Bullets and tables",
    "Microcopy",
    "Replies",
];

/// Each copy guide section's example: the named source and the quoted text,
/// or every section that has none, several, or a quote its source lacks.
fn copy_guide_examples(writing: &str) -> Result<Vec<(String, String, String)>, Vec<String>> {
    const EXAMPLE: &str = "Example, from CodeFlow's `";
    let guide = writing
        .split_once("\n## Copy guide\n")
        .map(|(_, guide)| guide)
        .ok_or_else(|| vec!["no `## Copy guide` section".to_string()])?;
    let guide = guide.split("\n## ").next().unwrap_or_default();
    let mut errors = Vec::new();
    let mut examples = Vec::new();
    let sections: Vec<&str> = guide.split("\n### ").skip(1).collect();
    let titles: Vec<&str> = sections
        .iter()
        .map(|section| section.lines().next().unwrap_or_default())
        .collect();
    if titles != COPY_GUIDE_SECTIONS {
        errors.push(format!("copy guide sections {titles:?}"));
    }
    for (title, section) in titles.iter().zip(&sections) {
        let found: Vec<&str> = section
            .match_indices(EXAMPLE)
            .map(|(i, _)| &section[i..])
            .collect();
        if found.len() != 1 {
            errors.push(format!("{title}: {} examples, want one", found.len()));
            continue;
        }
        let rest = &found[0][EXAMPLE.len()..];
        let (source, after) = rest.split_once("`:").expect("example source");
        let quote: Vec<&str> = after
            .lines()
            .skip_while(|line| line.trim().is_empty())
            .take_while(|line| line.starts_with('>'))
            .map(|line| line.trim_start_matches('>').trim())
            .collect();
        let quote = quote.join(" ");
        if quote.is_empty() {
            errors.push(format!("{title}: the example quotes nothing"));
            continue;
        }
        let text = std::fs::read_to_string(repo_root().join(source))
            .map(|text| normalize_whitespace(&text))
            .unwrap_or_default();
        if !text.contains(&normalize_whitespace(&quote)) {
            errors.push(format!("{title}: `{quote}` is not in {source}"));
        }
        examples.push(((*title).to_string(), source.to_string(), quote));
    }
    if errors.is_empty() {
        Ok(examples)
    } else {
        Err(errors)
    }
}

#[test]
fn writing_reference_carries_the_copy_guide_with_sourced_examples() {
    let writing = read("assets/base/rules/writing.md");
    let examples = copy_guide_examples(&writing).unwrap_or_else(|errors| panic!("{errors:#?}"));
    assert_eq!(examples.len(), COPY_GUIDE_SECTIONS.len());
    let guide = writing.split_once("\n## Copy guide\n").unwrap().1;
    assert!(
        !guide.contains(['\u{2013}', '\u{2014}']),
        "the copy guide carries a policy dash"
    );
    let normalized = normalize_whitespace(guide);
    for required in [
        "none asks a short answer for a lead, a heading or a sentence about its own format",
        "A short answer is its own summary and takes no lead.",
        "A simple answer stays simple: no figure, no headings, no recap, and a one-line answer stays one line.",
    ] {
        assert!(
            normalized.contains(required),
            "copy guide lost the short-answer rule: {required}"
        );
    }

    // Negative controls: a quote its source lacks, a missing example and a
    // reordered section each fail.
    let (_, _, quote) = &examples[0];
    let altered = writing.replacen(&format!("> {quote}"), &format!("> {quote} Always."), 1);
    let errors = copy_guide_examples(&altered).expect_err("an altered quote must fail");
    assert!(errors.iter().any(|e| e.starts_with("Voice:")), "{errors:?}");
    let missing = writing.replacen("Example, from CodeFlow's `docs/adoption.md`:", "", 1);
    let errors = copy_guide_examples(&missing).expect_err("a missing example must fail");
    assert!(
        errors.iter().any(|e| e.starts_with("Titles and headings:")),
        "{errors:?}"
    );
    let reordered = writing.replacen("### Leads", "### Lead paragraphs", 1);
    assert!(copy_guide_examples(&reordered).is_err());
}

/// Every shipped Markdown or template text under `assets/base`, whitespace
/// normalised, keyed by its path.
fn shipped_texts() -> BTreeMap<String, String> {
    fn walk(dir: &std::path::Path, out: &mut BTreeMap<String, String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, out);
            } else if path
                .extension()
                .is_some_and(|ext| ext == "md" || ext == "tmpl")
            {
                let rel = path
                    .strip_prefix(repo_root())
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(
                    rel,
                    normalize_whitespace(&std::fs::read_to_string(&path).unwrap()),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(&repo_root().join("assets/base"), &mut out);
    out
}

/// The `## The work lifecycle` section of cf-method's project-organization
/// reference, up to the next level-two heading.
fn lifecycle_section(text: &str) -> String {
    let start = text
        .find("\n## The work lifecycle\n")
        .expect("cf-method has one work lifecycle section");
    let rest = &text[start + 1..];
    let end = rest[3..].find("\n## ").map_or(rest.len(), |at| at + 3);
    rest[..end].to_string()
}

/// R-63, R-65, R-66: each rule is stated in one place across every
/// shipped text, and that place is the lifecycle reference. Each rule is
/// matched by the phrasings a restatement would use, case-insensitively,
/// so a paraphrase elsewhere counts as a second statement (Codex review of
/// TSK-108: cf-method once restated the standalone test in its own words).
fn each_rule_is_stated_once(texts: &BTreeMap<String, String>) {
    for (rule, phrasings) in [
        (
            "research folder",
            &["one file per question", "a file per question"][..],
        ),
        ("spec and epic", &["many to many", "many-to-many"][..]),
        (
            "standalone test",
            &[
                "reviewable pull request",
                "reviewable pr",
                "one pull request",
                "single pull request",
            ][..],
        ),
    ] {
        let holders: Vec<&String> = texts
            .iter()
            .filter(|(_, text)| {
                let text = text.to_lowercase();
                phrasings.iter().any(|phrase| text.contains(phrase))
            })
            .map(|(path, _)| path)
            .collect();
        assert_eq!(
            holders,
            vec!["assets/base/claude/skills/cf-method/references/project-organization.md"],
            "the {rule} rule must be stated once, in the lifecycle reference"
        );
    }
}

#[test]
fn lifecycle_guidance_is_one_section_the_skills_follow() {
    // TSK-108 AC-1 to AC-3 (SPC-013 R-34, R-43, R-63, R-65, R-66, R-112).
    let organization =
        read("assets/base/claude/skills/cf-method/references/project-organization.md");
    assert_eq!(
        organization.matches("\n## The work lifecycle\n").count(),
        1,
        "cf-method holds exactly one work lifecycle section"
    );
    let section = normalize_whitespace(&lifecycle_section(&organization));
    for required in [
        // The verbs of R-34 as shipped.
        "codeflow epic new",
        "--integration",
        "codeflow spec new --for",
        "codeflow task new --epic",
        "--standalone-reason",
        "--follow-up-of",
        "codeflow adr new",
        "codeflow work next",
        "codeflow work claim",
        "codeflow work start",
        "codeflow task status",
        "--acceptance",
        "--owner",
        "--revisit",
        "--scope",
        "codeflow spec status",
        "codeflow epic status",
        // Planning and dependency rules.
        "planning PR",
        "kind: research",
        "pin:",
        "awaiting_selection",
        "execution base",
        // Status rules.
        "in_progress",
        "`implemented` is derived",
    ] {
        assert!(
            section.contains(required),
            "the work lifecycle section lost: {required}"
        );
    }

    let texts = shipped_texts();
    each_rule_is_stated_once(&texts);
    for stale in [
        "`status: implemented` when",
        "frozen (`status: implemented`)",
        "It lists every direct candidate predecessor, including mutually exclusive guarded candidates",
    ] {
        let holders: Vec<&String> = texts
            .iter()
            .filter(|(_, text)| text.contains(stale))
            .map(|(path, _)| path)
            .collect();
        assert!(holders.is_empty(), "stale lifecycle text `{stale}` in {holders:?}");
    }

    // AC-2: the stage skills follow the section instead of restating it.
    for skill in [
        "assets/base/claude/skills/cf-method/SKILL.md",
        "assets/base/agents/skills/cf-plan/SKILL.md",
        "assets/base/agents/skills/cf-develop/SKILL.md",
        "assets/base/agents/skills/cf-ship/SKILL.md",
        "assets/base/agents/skills/cf-customize/SKILL.md",
    ] {
        let text = normalize_whitespace(&read(skill));
        assert!(
            text.contains("project-organization.md#the-work-lifecycle"),
            "{skill} must point to the work lifecycle section"
        );
    }
    let customize = normalize_whitespace(&read("assets/base/agents/skills/cf-customize/SKILL.md"));
    assert!(
        customize.contains("A change to `.codeflow/policy.json` needs a human"),
        "cf-customize states that policy edits need a human"
    );
    let graph = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-model-orchestrator/resources/task-graph.md",
    ));
    for required in [
        "Only the selected branch of a decision is written into `depends_on`",
        "awaiting_selection",
        "kind: research | decision",
        "lands only by the batched epic amendment",
    ] {
        assert!(graph.contains(required), "task-graph.md lost: {required}");
    }
}

/// TSK-169: the two texts that state how an approved spec changes, each
/// with every duty of the rule: Plan vN+1 amended in place, a dated note
/// naming its resolution with the editorial exception, the superseded
/// decision kept, each bound consumer's disposition, and the `implemented`
/// boundary with both later routes.
const SPEC_RULE_OWNERS: &[(&str, &[&str])] = &[
    (
        "assets/base/claude/skills/cf-method/references/project-organization.md",
        &[
            "a change to it is Plan vN+1, amended in place through a reviewed planning change",
            "each change of meaning gets a dated note naming its resolution",
            "(an editorial change needs none)",
            "the superseded decision stays visible as history",
            "names each consumer bound to a changed requirement and its disposition: unaffected, criteria amended in the same change, or reopened",
            "An `implemented` spec is frozen, so a later change gets a new spec or an explicit superseding record",
        ],
    ),
    (
        "assets/base/pm/spec.md.tmpl",
        &[
            "a change to an approved spec is Plan vN+1, amended in place through a reviewed planning change",
            "each change of meaning gets a dated note naming its resolution",
            "(an editorial change needs none)",
            "the superseded decision stays visible as history",
            "names each consumer bound to a changed requirement and its disposition: unaffected, criteria amended in the same change, or reopened",
            "Once implemented the spec is frozen, so a later change gets a new spec that lists `supersedes: [SPC-old]`",
            "`codeflow spec status SPC-old superseded --by SPC-new`, or an explicit superseding record",
        ],
    ),
];

/// Text an owner must not carry: approval freezing the criteria, or a new
/// spec for any later change without the `implemented` boundary.
const SPEC_RULE_FORBIDDEN: &[&str] = &[
    "freezes the criteria",
    "Later semantic change gets a new spec",
];

fn spec_rule_gaps(text: &str, needles: &[&str]) -> Vec<String> {
    let text = normalize_whitespace(text);
    let mut gaps: Vec<String> = needles
        .iter()
        .filter(|needle| !text.contains(&normalize_whitespace(needle)))
        .map(|needle| format!("missing: {needle}"))
        .collect();
    gaps.extend(
        SPEC_RULE_FORBIDDEN
            .iter()
            .filter(|phrase| text.contains(*phrase))
            .map(|phrase| format!("forbidden: {phrase}")),
    );
    gaps
}

/// TSK-169 AC-4: an approved spec is amended in place until it is
/// `implemented` and frozen after, in the lifecycle guidance and the spec
/// template, and neither says approval freezes the criteria.
#[test]
fn an_approved_spec_is_amended_until_implemented_and_frozen_after() {
    for (file, needles) in SPEC_RULE_OWNERS {
        let gaps = spec_rule_gaps(&read(file), needles);
        assert!(gaps.is_empty(), "{file}: {gaps:?}");
    }
}

/// The negative control: removing the rule or its boundary from each file,
/// or restoring the approval freeze, fails the pin.
#[test]
fn the_spec_amendment_pin_fails_when_the_rule_is_lost() {
    for (file, needles) in SPEC_RULE_OWNERS {
        let text = normalize_whitespace(&read(file));
        for needle in *needles {
            let removed = text.replacen(&normalize_whitespace(needle), "", 1);
            assert!(
                !spec_rule_gaps(&removed, needles).is_empty(),
                "{file}: removing {needle:?} left the pin satisfied"
            );
        }
        let frozen = format!("{text} Approval freezes the criteria.");
        assert!(
            !spec_rule_gaps(&frozen, needles).is_empty(),
            "{file}: an approval freeze left the pin satisfied"
        );
    }
}
