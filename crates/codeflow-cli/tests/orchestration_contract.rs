//! Behavioral contract for the host-neutral Claude+Codex duo.
//!
//! Mirror tests prove byte parity. These assertions pin the semantics that
//! must survive wording refactors and scaffold updates.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use codeflow_core::model_catalog::{
    Adoption, Alternative, Catalog, Effort, EligibilityRequest, Lifecycle, ParticipantLabel,
    Resolution, ResolveRequest, Target,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(repo_root().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[path = "../../codeflow-core/tests/support/catalog_fixture.rs"]
mod fixture_data;

const CATALOG: &str =
    "assets/base/agents/skills/cf-model-orchestrator/resources/current-ensemble.json";
/// The operator's seat designation of the managed roster (EPC-018 Q2).
const OPERATOR_DESIGNATION: &str = "2026-09-23";
const AUTHORS: [Option<&str>; 4] = [None, Some("claude"), Some("codex"), Some("grok")];

fn managed_catalog() -> Catalog {
    Catalog::parse(read(CATALOG).as_bytes()).expect("managed catalog validates")
}

fn fictional_catalog() -> Catalog {
    let mut catalog =
        Catalog::parse(fixture_data::fixture().to_string().as_bytes()).expect("fictional catalog");
    // TSK-079's fixture keeps a design-implementation duty that is open until
    // scoped evidence exists; the managed catalog defines no such duty.
    catalog.duties.remove("design-implementation");
    catalog
}

fn lineage_of_seat<'a>(catalog: &'a Catalog, seat: &str) -> &'a str {
    let family = &catalog
        .seats
        .iter()
        .find(|s| s.id == seat)
        .expect("seat")
        .family;
    &catalog
        .families
        .iter()
        .find(|f| &f.id == family)
        .expect("family")
        .lineage
}

fn hosts(catalog: &Catalog) -> Vec<&str> {
    catalog
        .families
        .iter()
        .flat_map(|family| family.harnesses.iter().map(String::as_str))
        .collect()
}

fn resolve(catalog: &Catalog, duty: &str, host: &str, author: Option<&str>) -> Resolution {
    let observed = BTreeMap::new();
    catalog
        .resolve(&ResolveRequest {
            duty,
            task: "",
            host_harness: host,
            author_lineage: author,
            exclusions: &[],
            observed_ids: &observed,
            trigger_facts: &[],
            requested_override: None,
            operator_override: None,
        })
        .unwrap_or_else(|error| panic!("{duty} on {host}: {error}"))
}

/// Every line a seat lists has a version that can hold that seat at high.
fn every_seat_line_can_be_eligible(catalog: &Catalog) -> Result<(), String> {
    let observed = BTreeMap::new();
    for seat in &catalog.seats {
        for id in &seat.lines {
            let line = catalog
                .lines
                .iter()
                .find(|l| &l.id == id)
                .ok_or(id.clone())?;
            let eligible = line.versions.iter().any(|version| {
                version.selectors.keys().any(|harness| {
                    catalog
                        .eligible(&EligibilityRequest {
                            duty: "orchestrate",
                            line: &line.id,
                            version: &version.id,
                            seat: Some(&seat.id),
                            harness,
                            effort: Effort::High,
                            exclusions: &[],
                            observed_ids: &observed,
                        })
                        .is_ok()
                })
            });
            if !eligible {
                return Err(format!(
                    "seat {} line {id} has no eligible version",
                    seat.id
                ));
            }
        }
    }
    Ok(())
}

/// Every duty is filled on at least one supported host for some author.
fn every_duty_resolves_on_some_host(catalog: &Catalog) -> Result<(), String> {
    for duty in catalog.duties.keys() {
        let filled = hosts(catalog).into_iter().any(|host| {
            AUTHORS
                .iter()
                .any(|author| !resolve(catalog, duty, host, *author).is_open())
        });
        if !filled {
            return Err(format!("duty {duty} resolves on no supported host"));
        }
    }
    Ok(())
}

/// Claude-authored units owe a Codex reviewer on every host.
fn unit_review_owes_codex_for_claude(catalog: &Catalog) -> Result<(), String> {
    for host in hosts(catalog) {
        let result = resolve(catalog, "unit-review", host, Some("claude"));
        let reviewers: Vec<_> = result
            .participants
            .iter()
            .filter(|p| p.label == ParticipantLabel::Required)
            .map(|p| p.lineage.as_str())
            .collect();
        if result.is_open() || !reviewers.contains(&"codex") || reviewers.contains(&"claude") {
            return Err(format!("unit-review on {host} owes {reviewers:?}"));
        }
    }
    Ok(())
}

/// The combined body is reviewed by both standing seats.
fn body_review_owes_both_standing_seats(catalog: &Catalog) -> Result<(), String> {
    for host in hosts(catalog) {
        let result = resolve(catalog, "body-review", host, None);
        let seats: BTreeSet<_> = result
            .participants
            .iter()
            .filter_map(|p| p.seat.as_deref())
            .collect();
        let owed: BTreeSet<_> = catalog.standing_seats.iter().map(String::as_str).collect();
        if result.is_open() || !owed.is_subset(&seats) {
            return Err(format!("body-review on {host} owes {seats:?}"));
        }
    }
    Ok(())
}

/// The design owner is the standing Claude seat and design uses its first line.
fn design_owner_is_claude(catalog: &Catalog) -> Result<(), String> {
    let owner = &catalog.design_owner;
    if lineage_of_seat(catalog, owner) != "claude" || !catalog.standing_seats.contains(owner) {
        return Err(format!(
            "design owner {owner} is not the standing Claude seat"
        ));
    }
    let first = &catalog
        .seats
        .iter()
        .find(|s| &s.id == owner)
        .expect("owner")
        .lines[0];
    let design = resolve(catalog, "design", "claude-code", None);
    match design.participants.as_slice() {
        [p] if p.seat.as_deref() == Some(owner) && &p.line == first => Ok(()),
        other => Err(format!("design resolved to {other:?}")),
    }
}

/// Only non-retired versions in seat-listed lines carry the operator's
/// designation; qualification starts empty everywhere.
fn designations_match_seat_lines(catalog: &Catalog) -> Result<(), String> {
    for line in &catalog.lines {
        let seats: Vec<_> = catalog
            .seats
            .iter()
            .filter(|seat| seat.lines.contains(&line.id))
            .map(|seat| seat.id.as_str())
            .collect();
        for version in &line.versions {
            let expected: Vec<_> = if version.lifecycle == Lifecycle::Retired {
                Vec::new()
            } else {
                seats.clone()
            };
            let actual: Vec<_> = version
                .designations
                .iter()
                .map(|d| d.seat.as_str())
                .collect();
            if actual != expected
                || version
                    .designations
                    .iter()
                    .any(|d| d.date != OPERATOR_DESIGNATION || d.record.trim().is_empty())
            {
                return Err(format!(
                    "{} designated {actual:?}, expected {expected:?} dated {OPERATOR_DESIGNATION}",
                    version.id
                ));
            }
            if !version.qualification.is_empty() {
                return Err(format!("{} carries qualification evidence", version.id));
            }
        }
    }
    Ok(())
}

type StructuralCheck = fn(&Catalog) -> Result<(), String>;

const STRUCTURAL_CHECKS: [(&str, StructuralCheck); 6] = [
    ("seat lines", every_seat_line_can_be_eligible),
    ("duties", every_duty_resolves_on_some_host),
    ("unit review", unit_review_owes_codex_for_claude),
    ("body review", body_review_owes_both_standing_seats),
    ("design owner", design_owner_is_claude),
    ("designations", designations_match_seat_lines),
];

#[test]
fn managed_catalog_passes_every_structural_check() {
    let catalog = managed_catalog();
    for (name, check) in STRUCTURAL_CHECKS {
        check(&catalog).unwrap_or_else(|error| panic!("{name}: {error}"));
    }
    // The fictional fixture used below is itself a valid control.
    let fixture = fictional_catalog();
    for (name, check) in STRUCTURAL_CHECKS {
        check(&fixture).unwrap_or_else(|error| panic!("fixture {name}: {error}"));
    }
}

#[test]
fn each_structural_check_fails_on_a_catalog_that_breaks_it() {
    type Break = fn(&mut Catalog);
    let breaks: [(&str, Break); 8] = [
        ("seat lines", |c| {
            c.lines[1].versions[0].designations.clear();
        }),
        ("duties", |c| {
            // Parses, but no light-execution worker supports the medium entry effort.
            let line = c
                .lines
                .iter_mut()
                .find(|l| l.id == "quartz-worker")
                .unwrap();
            line.versions[0].efforts = vec![Effort::High];
        }),
        ("unit review", |c| {
            let review = c.duties.get_mut("unit-review").unwrap();
            review.required[0]
                .alternatives
                .retain(|a| a.target != Target::Seat("quartz-seat".into()));
            review.required[0].alternatives.insert(
                0,
                Alternative {
                    target: Target::Seat("cinder-seat".into()),
                    harness: None,
                    effort: Effort::High,
                },
            );
        }),
        ("body review", |c| {
            c.duties.get_mut("body-review").unwrap().required.pop();
        }),
        ("design owner", |c| c.design_owner = "quartz-seat".into()),
        ("designations", |c| {
            // A worker-only line designated for a seat that does not list it.
            let designations = c.lines[2].versions[0].designations.clone();
            let line = c
                .lines
                .iter_mut()
                .find(|l| l.id == "quartz-worker")
                .unwrap();
            line.versions[0].designations = designations;
        }),
        ("designations", |c| {
            let mut retired = c.lines[2].versions[0].clone();
            retired.id = "quartz-retired".into();
            retired.pinned_id = "quartz-retired-pin".into();
            retired.lifecycle = Lifecycle::Retired;
            c.lines[2].versions.insert(0, retired);
        }),
        ("designations", |c| {
            c.lines[0].versions[0].qualification = c.lines[0].versions[0]
                .designations
                .iter()
                .map(|_| {
                    serde_json::from_value(serde_json::json!({
                        "harness": "claude-code", "selector": "orchid-one-pin",
                        "effort": "high", "duty": "orchestrate",
                        "record": "record.md", "evidence": ["evidence.json"]
                    }))
                    .unwrap()
                })
                .collect();
        }),
    ];
    for (name, mutate) in breaks {
        let mut catalog = fictional_catalog();
        mutate(&mut catalog);
        let (_, check) = STRUCTURAL_CHECKS
            .iter()
            .find(|(check, _)| *check == name)
            .unwrap();
        assert!(check(&catalog).is_err(), "{name} accepted a broken catalog");
    }
}

#[test]
fn managed_catalog_uses_only_capability_supported_harnesses() {
    let catalog = managed_catalog();
    let harnesses: serde_json::Value = serde_json::from_str(&read(
        "assets/base/agents/skills/cf-evaluate-model/resources/harnesses.json",
    ))
    .expect("harness catalog JSON");
    let supported: BTreeMap<&str, (&str, &str)> = harnesses["harnesses"]
        .as_array()
        .expect("harnesses")
        .iter()
        .map(|harness| {
            assert_eq!(harness["status"], "capability-supported");
            (
                harness["id"].as_str().expect("harness id"),
                (
                    harness["provider"].as_str().expect("provider"),
                    harness["lineage"].as_str().expect("lineage"),
                ),
            )
        })
        .collect();
    for family in &catalog.families {
        for harness in &family.harnesses {
            assert_eq!(
                supported.get(harness.as_str()),
                Some(&(family.provider.as_str(), family.lineage.as_str())),
                "{} selects an unsupported or mismatched harness {harness}",
                family.id
            );
        }
    }
    let lineages: BTreeSet<&str> = catalog
        .standing_seats
        .iter()
        .map(|seat| lineage_of_seat(&catalog, seat))
        .collect();
    assert_eq!(lineages, BTreeSet::from(["claude", "codex"]));
    assert!(catalog.families.iter().any(|f| f.lineage == "grok"));
    let roles: BTreeSet<&str> = catalog.seats.iter().map(|s| s.role.as_str()).collect();
    for role in [
        "claude-judgment-primary",
        "codex-engineering-primary",
        "grok-engineering-primary",
    ] {
        assert!(roles.contains(role), "missing standing role {role}");
    }
    assert!(!catalog.xhigh_triggers.is_empty());
    assert!(catalog.rules.iter().any(|rule| {
        rule == "High triggers set a minimum reasoning level for a unit, not an instruction to escalate a primary already at high or spawn a redundant high worker."
    }), "high reasoning floor must not mandate redundant escalation");
    // The Grok trigger policy is today's routing policy until the operator answers Q4.
    let policy: serde_json::Value = serde_json::from_str(&read(
        "assets/base/agents/skills/cf-model-orchestrator/resources/routing-policy.json",
    ))
    .expect("routing policy");
    let triggers: Vec<String> =
        serde_json::from_value(policy["extra_family_review"]["triggers"].clone()).unwrap();
    assert_eq!(catalog.named_policies["extra-family-review"], triggers);
    assert!(catalog
        .lines
        .iter()
        .all(|line| line.adoption == Adoption::Manual));
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
fn catalog_rules_and_routing_policy_keep_extra_family_review() {
    let routing = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-model-orchestrator/resources/capability-routing.md",
    ));
    let policy = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-model-orchestrator/resources/routing-policy.json",
    ));
    let rules = managed_catalog().rules.join(" ");
    for required in [
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
            rules.contains(required),
            "managed catalog lost rule: {required}"
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
    // Catalog-derived: no alias, pinned id or selector of any catalog version.
    let catalog = managed_catalog();
    for version in catalog.lines.iter().flat_map(|line| &line.versions) {
        for token in std::iter::once(&version.alias)
            .chain(std::iter::once(&version.pinned_id))
            .chain(version.selectors.values())
        {
            assert!(
                !skill
                    .split(|c: char| !(c.is_alphanumeric() || "-_.".contains(c)))
                    .any(|word| word.trim_end_matches('.') == token),
                "orchestrator must not hard-code model {token}"
            );
        }
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
fn responsible_autonomy_has_detailed_quality_and_security_owners() {
    let quality = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-model-orchestrator/resources/quality-contract.md",
    ));
    let security = normalize_whitespace(&read("assets/base/claude/agents/cf-security-reviewer.md"));

    for required in [
        "Apply authority to effects, not tool verbs",
        "the authorized instance/count of unchanged safe steps without re-asking",
        "an identical tuple is not a standing grant",
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
fn solo_fallback_requires_fresh_context_independent_review() {
    let skill = normalize_whitespace(&read(
        "assets/base/agents/skills/cf-model-orchestrator/SKILL.md",
    ));
    let agents = normalize_whitespace(&read("assets/base/AGENTS.md.tmpl"));

    for (owner, contract, alternate) in [
        ("orchestrator", &skill, "else a separate read-only pass"),
        ("AGENTS", &agents, "otherwise a separate read-only pass"),
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
    assert!(delegate.contains("--model $CLAUDE_MODEL --effort $CLAUDE_EFFORT"));
    assert!(
        delegate.contains("For consult/no-edit, use the same launch with --permission-mode auto")
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

/// TSK-151: the per-stage model example names the placeholder the catalog
/// resolves, never a model. The catalog scan bars catalog selectors; this pin
/// also bars the stale `sonnet`, which is not in the catalog.
#[test]
fn pipeline_example_names_the_resolved_selector_not_a_model() {
    for path in [
        "assets/base/claude/workflows/pipeline.workflow.js",
        ".claude/workflows/pipeline.workflow.js",
        ".codeflow/.baseline/.claude/workflows/pipeline.workflow.js",
    ] {
        let text = normalize_whitespace(&read(path));
        assert!(
            text.contains("per-stage model, e.g. { build: '<selector>' }, where the // selector comes from `codeflow models resolve`"),
            "{path}: the per-stage model example must name the resolved selector"
        );
        assert!(
            !text.contains("sonnet"),
            "{path}: the stale 'sonnet' example is back"
        );
    }
}
