use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::*;

#[path = "../../tests/support/catalog_fixture.rs"]
mod fixture_data;
use fixture_data::{fixture, version};

fn parse(value: &Value) -> Result<Catalog, String> {
    Catalog::parse(&serde_json::to_vec(value).unwrap())
}
fn catalog() -> Catalog {
    parse(&fixture()).unwrap()
}
fn exclude(id: &str) -> Exclusion {
    Exclusion {
        scope: ExclusionScope::Version(id.into()),
        reason: "native canary failed".into(),
        fresh_native: true,
    }
}
fn request<'a>(duty: &'a str, observed: &'a BTreeMap<String, String>) -> ResolveRequest<'a> {
    ResolveRequest {
        duty,
        task: "TSK-900",
        host_harness: "claude-code",
        author_lineage: Some("claude"),
        exclusions: &[],
        observed_ids: observed,
        trigger_facts: &[],
        requested_override: None,
        operator_override: None,
    }
}
fn resolved(duty: &str) -> Resolution {
    catalog().resolve(&request(duty, &BTreeMap::new())).unwrap()
}
fn assert_invalid(mut change: impl FnMut(&mut Value), reason: &str) {
    let mut value = fixture();
    change(&mut value);
    let error = parse(&value).unwrap_err();
    assert!(error.contains(reason), "{error}, expected {reason}");
}

#[test]
fn schema_five_roundtrip_is_valid() {
    let catalog = catalog();
    assert_eq!(
        Catalog::parse(&serde_json::to_vec(&catalog).unwrap())
            .unwrap()
            .schema_version,
        5
    );
}
#[test]
fn unknown_fields_are_rejected_at_nested_boundary() {
    assert_invalid(
        |v| v["lines"][0]["versions"][0]["grant"] = json!(true),
        "unknown field",
    );
}
#[test]
fn duplicate_json_keys_are_rejected_in_selector_maps() {
    let json = serde_json::to_string(&fixture()).unwrap().replace(
        "\"claude-code\":\"orchid-one-pin\"",
        "\"claude-code\":\"orchid-one-pin\",\"claude-code\":\"other\"",
    );
    assert!(Catalog::parse(json.as_bytes())
        .unwrap_err()
        .contains("duplicate JSON key"));
}
#[test]
fn unsupported_harness_is_rejected() {
    assert_invalid(
        |v| v["families"][0]["harnesses"][0] = json!("shell"),
        "unsupported harness",
    );
}
#[test]
fn unsupported_probe_is_rejected() {
    assert_invalid(
        |v| v["families"][0]["probes"][0] = json!("shell-version"),
        "unsupported probe",
    );
}
#[test]
fn standing_seats_must_be_distinct() {
    assert_invalid(
        |v| v["standing_seats"][1] = json!("orchid-seat"),
        "two distinct lineages",
    );
}
#[test]
fn duplicate_family_lineage_is_rejected() {
    assert_invalid(
        |v| v["families"][1]["lineage"] = json!("claude"),
        "family lineage",
    );
}
#[test]
fn seat_id_equal_to_line_id_is_rejected() {
    // Rename a worker-only line, leaving every other reference valid, so the
    // sole failure is the ambiguous seat-or-line namespace used by --route.
    let value = fixture()
        .to_string()
        .replace("quartz-worker", "orchid-seat");
    assert_eq!(
        Catalog::parse(value.as_bytes()).unwrap_err(),
        "seat id orchid-seat collides with a line id"
    );
}
#[test]
fn non_claude_design_owner_is_rejected() {
    assert_invalid(
        |v| v["design_owner"] = json!("quartz-seat"),
        "standing Claude seat",
    );
}
#[test]
fn multiple_primary_seats_per_family_are_rejected() {
    assert_invalid(
        |v| v["seats"][1]["family"] = json!("orchid"),
        "primary family",
    );
}
#[test]
fn seat_line_without_eligible_version_is_rejected() {
    assert_invalid(
        |v| v["lines"][0]["versions"][0]["designations"] = json!([]),
        "no potentially eligible version",
    );
}
#[test]
fn retired_designation_is_rejected() {
    assert_invalid(
        |v| {
            let mut retired = version("orchid-retired", &["claude-code"], Some("orchid-seat"));
            retired["lifecycle"] = json!("retired");
            v["lines"][0]["versions"]
                .as_array_mut()
                .unwrap()
                .push(retired);
        },
        "designation on retired",
    );
}
#[test]
fn unlisted_line_designation_is_rejected() {
    assert_invalid(
        |v| {
            v["lines"][3]["versions"][0]["designations"] =
                json!([{"seat":"quartz-seat","date":"2026-09-23","record":"record.md"}]);
        },
        "not listed",
    );
}
#[test]
fn designation_requires_record() {
    assert_invalid(
        |v| v["lines"][0]["versions"][0]["designations"][0]["record"] = json!(""),
        "designation record",
    );
}
fn scoped(duty: &str, effort: &str) -> Value {
    json!({"harness":"claude-code","selector":"orchid-two-pin","effort":effort,"duty":duty,"record":"record.md","evidence":["evidence/native.json"]})
}
#[test]
fn evidence_requires_record() {
    assert_invalid(
        |v| {
            let mut evidence = scoped("design-implementation", "medium");
            evidence["record"] = json!("");
            v["lines"][1]["versions"][0]["qualification"] = json!([evidence]);
        },
        "evidence record",
    );
}
#[test]
fn seat_defaults_cannot_be_medium_but_bounded_workers_can() {
    assert_invalid(
        |v| {
            v["duties"]["orchestrate"]["required"][0]["alternatives"][0]["effort"] =
                json!("medium");
        },
        "below its effort floor",
    );
    assert_eq!(
        resolved("bounded-execution").participants[0].effort,
        Effort::Medium
    );
}
#[test]
fn unknown_trigger_and_unknown_route_are_rejected() {
    assert_invalid(
        |v| v["duties"]["unit-review"]["triggered"][0]["trigger"] = json!("unregistered"),
        "unknown participant trigger",
    );
    assert_invalid(
        |v| {
            v["duties"]["bounded-execution"]["required"][0]["alternatives"][0]["target"]["id"] =
                json!("absent");
        },
        "unknown line",
    );
}
#[test]
fn unit_review_is_author_relative_for_all_three_authors() {
    for (author, expected) in [
        ("claude", "quartz-seat"),
        ("codex", "orchid-seat"),
        ("grok", "quartz-seat"),
    ] {
        let observed = BTreeMap::new();
        let mut req = request("unit-review", &observed);
        req.author_lineage = Some(author);
        let result = catalog().resolve(&req).unwrap();
        assert!(!result.is_open());
        assert_eq!(result.participants.len(), 1);
        assert_eq!(result.participants[0].seat.as_deref(), Some(expected));
    }
}
#[test]
fn missing_independent_reviewer_never_substitutes_same_lineage() {
    let observed = BTreeMap::new();
    let excluded = [exclude("quartz-one")];
    let mut req = request("unit-review", &observed);
    req.exclusions = &excluded;
    let result = catalog().resolve(&req).unwrap();
    assert!(result.participants.is_empty());
    assert_eq!(result.open[0].participant, "independent");
    req.author_lineage = Some("grok");
    assert_eq!(
        catalog().resolve(&req).unwrap().participants[0]
            .seat
            .as_deref(),
        Some("orchid-seat")
    );
}
#[test]
fn extra_review_is_triggered_except_for_its_own_author() {
    let observed = BTreeMap::new();
    let facts = vec!["material".into()];
    for author in ["claude", "codex", "grok"] {
        let mut req = request("unit-review", &observed);
        req.trigger_facts = &facts;
        req.author_lineage = Some(author);
        let result = catalog().resolve(&req).unwrap();
        assert_eq!(
            result.participants.len(),
            if author == "grok" { 1 } else { 2 }
        );
        assert!(!result.is_open());
    }
}
#[test]
fn planning_and_body_review_return_both_standing_seats() {
    for duty in ["independent-plan", "body-review"] {
        let observed = BTreeMap::new();
        let excluded = [exclude("quartz-one")];
        let mut req = request(duty, &observed);
        let complete = catalog().resolve(&req).unwrap();
        assert_eq!(complete.participants.len(), 2);
        assert!(!complete.is_open());
        req.exclusions = &excluded;
        let missing = catalog().resolve(&req).unwrap();
        assert_eq!(missing.participants.len(), 1);
        assert!(missing.is_open());
    }
}
#[test]
fn scoped_test_evidence_never_qualifies_a_seat() {
    let mut c = catalog();
    c.lines[1].versions[0].designations.clear();
    c.lines[1].versions[0].qualification =
        vec![serde_json::from_value(scoped("test-authoring", "high")).unwrap()];
    let observed = BTreeMap::new();
    let req = EligibilityRequest {
        duty: "orchestrate",
        line: "orchid-support",
        version: "orchid-two",
        seat: Some("orchid-seat"),
        harness: "claude-code",
        effort: Effort::High,
        exclusions: &[],
        observed_ids: &observed,
    };
    assert!(c.eligible(&req).unwrap_err().contains("seat needs"));
}
#[test]
fn scoped_design_evidence_is_exact_in_duty_effort_and_harness() {
    let mut c = catalog();
    c.lines[1].versions[0].qualification =
        vec![serde_json::from_value(scoped("design-implementation", "medium")).unwrap()];
    let observed = BTreeMap::new();
    let req = EligibilityRequest {
        duty: "design-implementation",
        line: "orchid-support",
        version: "orchid-two",
        seat: None,
        harness: "claude-code",
        effort: Effort::Medium,
        exclusions: &[],
        observed_ids: &observed,
    };
    assert_eq!(c.eligible(&req).unwrap(), Eligibility::ScopedQualified);
    assert!(c
        .eligible(&EligibilityRequest {
            effort: Effort::High,
            ..req.clone()
        })
        .is_err());
    assert!(c
        .eligible(&EligibilityRequest {
            harness: "codex-cli",
            ..req.clone()
        })
        .is_err());
    assert!(c
        .eligible(&EligibilityRequest {
            duty: "another-design-task",
            effort: Effort::High,
            ..req.clone()
        })
        .is_err());
    for duty in ["design", "direction-approval", "fidelity-approval"] {
        assert!(c
            .eligible(&EligibilityRequest {
                duty,
                effort: Effort::High,
                ..req.clone()
            })
            .unwrap_err()
            .contains("workers never"));
    }
}
#[test]
fn unsupported_worker_effort_is_excluded_without_clamping() {
    let mut c = catalog();
    c.lines[3].versions[0].efforts = vec![Effort::High];
    let result = c
        .resolve(&request("light-execution", &BTreeMap::new()))
        .unwrap();
    assert!(result.is_open());
    assert!(result.open[0]
        .reasons
        .iter()
        .any(|r| r.contains("never clamped")));
}
#[test]
fn xhigh_adds_same_family_worker_and_keeps_seat_approval() {
    let observed = BTreeMap::new();
    let facts = vec!["deep".into()];
    let mut req = request("orchestrate", &observed);
    req.trigger_facts = &facts;
    let result = catalog().resolve(&req).unwrap();
    assert_eq!(result.participants[0].effort, Effort::High);
    assert_eq!(result.obligations[0].effort, Effort::Xhigh);
    assert_eq!(
        result.obligations[0].lineage,
        result.participants[0].lineage
    );
    assert_eq!(
        result.obligations[0].approval_owner.as_deref(),
        Some("orchid-seat")
    );
    assert!(result.obligations[0].seat.is_none());
    assert!(result.xhigh_trigger_met);
}
#[test]
fn seat_without_xhigh_stays_eligible() {
    let mut c = catalog();
    c.lines[0].versions[0].efforts = vec![Effort::High];
    let observed = BTreeMap::new();
    let facts = vec!["deep".into()];
    let mut req = request("orchestrate", &observed);
    req.trigger_facts = &facts;
    let result = c.resolve(&req).unwrap();
    assert_eq!(result.participants[0].version, "orchid-one");
    assert_eq!(result.obligations[0].version, "orchid-two");
    assert!(result.xhigh_trigger_met);
}
#[test]
fn absent_same_family_xhigh_worker_leaves_obligation_open() {
    let mut c = catalog();
    for line in c.lines.iter_mut().filter(|l| l.family == "orchid") {
        line.versions[0].efforts = vec![Effort::High];
    }
    let observed = BTreeMap::new();
    let facts = vec!["deep".into()];
    let mut req = request("orchestrate", &observed);
    req.trigger_facts = &facts;
    let result = c.resolve(&req).unwrap();
    assert_eq!(result.participants[0].effort, Effort::High);
    assert!(!result.xhigh_trigger_met);
    assert!(result.obligations.is_empty());
    assert!(result.open[0].participant.starts_with("xhigh-reasoning:"));
    assert!(result.open[0]
        .reasons
        .iter()
        .any(|r| r.contains("trigger not met")));
}
#[test]
fn every_resolved_launch_is_pinned_not_alias() {
    let c = catalog();
    for duty in c.duties.keys() {
        for chosen in c
            .resolve(&request(duty, &BTreeMap::new()))
            .unwrap()
            .participants
        {
            assert!(chosen.pinned_id.ends_with("-pin"));
            assert!(!chosen.pinned_id.ends_with("-alias"));
        }
    }
}
#[test]
fn drifted_seat_falls_back_but_design_stays_open() {
    let observed = BTreeMap::from([("orchid-one-pin".into(), "orchid-new-pin".into())]);
    let c = catalog();
    let orchestration = c.resolve(&request("orchestrate", &observed)).unwrap();
    assert_eq!(orchestration.participants[0].version, "orchid-two");
    assert!(orchestration.participants[0].reduced_assurance);
    let design = c.resolve(&request("design", &observed)).unwrap();
    assert!(design.participants.is_empty());
    assert!(design.is_open());
    let both = BTreeMap::from([
        ("orchid-one-pin".into(), "orchid-new-pin".into()),
        ("orchid-two-pin".into(), "orchid-newer-pin".into()),
    ]);
    assert!(c.resolve(&request("orchestrate", &both)).unwrap().is_open());
}
#[test]
fn worker_drift_tries_alternatives_before_candidate_only_use() {
    let c = catalog();
    let observed = BTreeMap::from([("quartz-two-pin".into(), "quartz-new-pin".into())]);
    let bounded = c.resolve(&request("bounded-execution", &observed)).unwrap();
    assert_eq!(bounded.participants[0].version, "orchid-one");
    let light = c.resolve(&request("light-execution", &observed)).unwrap();
    assert_eq!(light.participants[0].pinned_id, "quartz-new-pin");
    assert_eq!(light.participants[0].eligibility, Eligibility::Candidate);
    assert!(light.participants[0]
        .limitations
        .iter()
        .any(|r| r.contains("evidence discarded")));
}
#[test]
fn design_first_line_and_reduced_assurance_fallbacks() {
    assert_eq!(resolved("design").participants[0].version, "orchid-one");
    let observed = BTreeMap::new();
    let exclusions = [exclude("orchid-one")];
    for duty in ["design", "orchestrate", "independent-plan"] {
        let mut req = request(duty, &observed);
        req.exclusions = &exclusions;
        let result = catalog().resolve(&req).unwrap();
        if duty == "design" {
            assert!(result.is_open());
        } else {
            let fallback = result
                .participants
                .iter()
                .find(|p| p.lineage == "claude")
                .unwrap();
            assert_eq!(fallback.version, "orchid-two");
            assert!(fallback.reduced_assurance);
        }
    }
}
fn override_record() -> OperatorOverride {
    OperatorOverride {
        task: "TSK-900".into(),
        duty: "design".into(),
        route: Alternative {
            target: Target::Line("orchid-support".into()),
            harness: Some("claude-code".into()),
            effort: Effort::High,
        },
        plan_version: "Plan v3.4".into(),
        instruction_record: "approved-plan.md#operator-instruction".into(),
    }
}
fn override_result(
    record: Option<&OperatorOverride>,
    route: &Alternative,
    c: &Catalog,
    excluded: &[Exclusion],
    observed: &BTreeMap<String, String>,
) -> Resolution {
    let mut req = request("design", observed);
    req.exclusions = excluded;
    req.requested_override = Some(route);
    req.operator_override = record;
    c.resolve(&req).unwrap()
}
#[test]
fn exact_override_allows_second_seat_line_for_named_task_only() {
    let record = override_record();
    let excluded = [exclude("orchid-one")];
    let result = override_result(
        Some(&record),
        &record.route,
        &catalog(),
        &excluded,
        &BTreeMap::new(),
    );
    assert!(!result.is_open());
    assert_eq!(result.participants[0].pinned_id, "orchid-two-pin");
    assert_eq!(result.participants[0].effort, Effort::High);
    assert_eq!(
        result.participants[0]
            .operator_override
            .as_ref()
            .unwrap()
            .task,
        "TSK-900"
    );
}
#[test]
fn override_missing_record_is_open() {
    let record = override_record();
    assert!(
        override_result(None, &record.route, &catalog(), &[], &BTreeMap::new()).open[0].reasons[0]
            .contains("missing OPERATOR_OVERRIDE")
    );
}
#[test]
fn override_task_mismatch_is_open() {
    let mut record = override_record();
    record.task = "TSK-901".into();
    assert!(override_result(
        Some(&record),
        &record.route,
        &catalog(),
        &[],
        &BTreeMap::new()
    )
    .open[0]
        .reasons[0]
        .contains("task mismatch"));
}
#[test]
fn override_duty_mismatch_is_open() {
    let mut record = override_record();
    record.duty = "orchestrate".into();
    assert!(override_result(
        Some(&record),
        &record.route,
        &catalog(),
        &[],
        &BTreeMap::new()
    )
    .open[0]
        .reasons[0]
        .contains("duty mismatch"));
}
#[test]
fn override_route_mismatch_is_open() {
    let record = override_record();
    let route = Alternative {
        target: Target::Line("orchid-main".into()),
        ..record.route.clone()
    };
    assert!(
        override_result(Some(&record), &route, &catalog(), &[], &BTreeMap::new()).open[0].reasons
            [0]
        .contains("route mismatch")
    );
}
#[test]
fn override_effort_mismatch_is_open() {
    let record = override_record();
    let route = Alternative {
        effort: Effort::Xhigh,
        ..record.route.clone()
    };
    assert!(
        override_result(Some(&record), &route, &catalog(), &[], &BTreeMap::new()).open[0].reasons
            [0]
        .contains("effort mismatch")
    );
}
#[test]
fn override_on_non_design_duties_is_rejected() {
    let observed = BTreeMap::new();
    for duty in ["orchestrate", "unit-review"] {
        for record_duty in ["design", duty] {
            let mut record = override_record();
            record.duty = record_duty.into();
            let mut req = request(duty, &observed);
            req.requested_override = Some(&record.route);
            req.operator_override = Some(&record);
            assert_eq!(
                catalog().resolve(&req).unwrap_err(),
                "OPERATOR_OVERRIDE is only valid for design"
            );
        }
    }
}
#[test]
fn override_exclusion_and_identity_drift_remain_open() {
    let record = override_record();
    assert!(override_result(
        Some(&record),
        &record.route,
        &catalog(),
        &[exclude("orchid-two")],
        &BTreeMap::new()
    )
    .is_open());
    let observed = BTreeMap::from([("orchid-two-pin".into(), "unapproved-pin".into())]);
    assert!(override_result(Some(&record), &record.route, &catalog(), &[], &observed).is_open());
}
#[test]
fn override_unsupported_and_below_floor_efforts_remain_open() {
    let mut c = catalog();
    c.lines[1].versions[0].efforts = vec![Effort::High];
    let mut record = override_record();
    record.route.effort = Effort::Xhigh;
    assert!(override_result(Some(&record), &record.route, &c, &[], &BTreeMap::new()).is_open());
    record.route.effort = Effort::Medium;
    assert!(override_result(
        Some(&record),
        &record.route,
        &catalog(),
        &[],
        &BTreeMap::new()
    )
    .is_open());
}
#[test]
fn override_missing_provenance_and_worker_line_are_refused() {
    let mut record = override_record();
    record.instruction_record.clear();
    assert!(override_result(
        Some(&record),
        &record.route,
        &catalog(),
        &[],
        &BTreeMap::new()
    )
    .is_open());
    record = override_record();
    record.route.target = Target::Line("quartz-worker".into());
    record.route.harness = Some("codex-cli".into());
    assert!(override_result(
        Some(&record),
        &record.route,
        &catalog(),
        &[],
        &BTreeMap::new()
    )
    .is_open());
}
#[test]
fn general_review_second_opinion_does_not_fill_independent_gap() {
    let observed = BTreeMap::new();
    let excluded = [exclude("cinder-one")];
    let mut req = request("general-review", &observed);
    req.exclusions = &excluded;
    let result = catalog().resolve(&req).unwrap();
    assert_eq!(
        result.participants[0].label,
        ParticipantLabel::SecondOpinion
    );
    assert_eq!(result.open[0].participant, "independent");
}
#[test]
fn reasoning_support_excludes_current_seat_version() {
    let result = resolved("reasoning-support");
    assert_eq!(result.participants[0].version, "orchid-two");
}
#[test]
fn computer_use_qa_uses_independent_capable_harness() {
    for (author, expected) in [
        ("claude", "codex-app"),
        ("codex", "claude-code"),
        ("grok", "codex-app"),
    ] {
        let observed = BTreeMap::new();
        let mut req = request("computer-use-qa", &observed);
        req.author_lineage = Some(author);
        assert_eq!(
            catalog().resolve(&req).unwrap().participants[0].harness,
            expected
        );
    }
}
#[test]
fn test_authoring_belongs_to_the_executor() {
    let result = resolved("test-authoring");
    assert!(result.participants.is_empty());
    assert!(result.open[0].reasons[0].contains("executing participant"));
}
#[test]
fn worker_adoption_selects_latest_eligible_but_manual_pins() {
    let mut c = catalog();
    c.lines[3].versions.push(
        serde_json::from_value(version("quartz-new", &["codex-cli", "codex-app"], None)).unwrap(),
    );
    let observed = BTreeMap::new();
    let req = request("light-execution", &observed);
    assert_eq!(
        c.resolve(&req).unwrap().participants[0].version,
        "quartz-new"
    );
    c.lines[3].adoption = Adoption::Manual;
    assert_eq!(
        c.resolve(&req).unwrap().participants[0].version,
        "quartz-two"
    );
}
#[test]
fn usage_bucket_exclusion_removes_every_family_line_unknown_does_not() {
    let observed = BTreeMap::new();
    let mut excluded = [Exclusion {
        scope: ExclusionScope::UsageBucket("quartz-bucket".into()),
        reason: "exhausted".into(),
        fresh_native: true,
    }];
    let mut req = request("engineering-implementation", &observed);
    req.exclusions = &excluded;
    assert_eq!(
        catalog().resolve(&req).unwrap().participants[0].version,
        "orchid-one"
    );
    excluded[0].fresh_native = false;
    let mut req = request("engineering-implementation", &observed);
    req.exclusions = &excluded;
    assert_eq!(
        catalog().resolve(&req).unwrap().participants[0].version,
        "quartz-one"
    );
}
#[test]
fn fallback_only_is_limited_to_listed_seat_fallback() {
    let mut c = catalog();
    c.lines[1].versions[0].lifecycle = Lifecycle::FallbackOnly;
    let observed = BTreeMap::new();
    let excluded = [exclude("orchid-one")];
    let mut req = request("orchestrate", &observed);
    req.exclusions = &excluded;
    assert_eq!(
        c.resolve(&req).unwrap().participants[0].version,
        "orchid-two"
    );
    assert!(c
        .resolve(&request("consultation", &observed))
        .unwrap()
        .participants[0]
        .seat
        .is_some());
    c.lines[0].versions[0].lifecycle = Lifecycle::FallbackOnly;
    assert!(c.validate().is_err());
}
#[test]
fn overlay_candidate_never_fills_seat_and_apply_is_atomic() {
    let c = catalog();
    let overlay = json!({"schema_version":1,"additions":[{"line":"orchid-main","id":"orchid-new","alias":"orchid-latest","pinned_id":"orchid-new-pin","selectors":{"claude-code":"orchid-new-pin"},"efforts":["high"]}],"exclusions":["orchid-one"]});
    let (copy, excluded) = PersonalOverlay::parse(&serde_json::to_vec(&overlay).unwrap())
        .unwrap()
        .apply(&c)
        .unwrap();
    assert!(copy.lines[0].versions[1].designations.is_empty());
    let observed = BTreeMap::new();
    let mut req = request("design", &observed);
    req.exclusions = &excluded;
    assert!(copy.resolve(&req).unwrap().is_open());
    assert_eq!(c.lines[0].versions.len(), 1);
    for field in ["designations", "qualification", "family", "probe"] {
        let mut bad = overlay.clone();
        bad["additions"][0][field] = json!([]);
        assert!(PersonalOverlay::parse(&serde_json::to_vec(&bad).unwrap()).is_err());
    }
}

fn binding() -> crate::model_qualification::QualifiedBinding {
    let digest = format!("sha256:{}", "a".repeat(64));
    serde_json::from_value(json!({
        "schema_version":1,"binding_id":"orchid-approved","provider":"anthropic","lineage":"claude",
        "eligible_roles":["claude-judgment-primary"],"qualified_at":"2026-09-23T10:00:00Z",
        "requested":{"model":"orchid-one-pin","effort":"high","harness":"claude-code","harness_version":"1.0","settings_digest":digest},
        "observed":{"model":"orchid-one-pin","effort":"high","evidence":[{"kind":"session","digest":digest}]},
        "qualification":{"run_id":"fictional-run","suite":"full","suite_digest":digest,"result_digest":digest,"codeflow_revision":"abc123"},
        "settings_sources":[],"approval":{"reviewer":"operator","reviewed_at":"2026-09-23T10:00:00Z"}
    })).unwrap()
}
#[test]
fn full_suite_binding_qualifies_only_exact_role_and_tuple() {
    let mut c = catalog();
    c.lines[0].versions[0].designations.clear();
    c.bindings = vec![binding()];
    c.validate().unwrap();
    let observed = BTreeMap::new();
    let req = EligibilityRequest {
        duty: "orchestrate",
        line: "orchid-main",
        version: "orchid-one",
        seat: Some("orchid-seat"),
        harness: "claude-code",
        effort: Effort::High,
        exclusions: &[],
        observed_ids: &observed,
    };
    assert_eq!(c.eligible(&req).unwrap(), Eligibility::Qualified);
    assert!(c
        .eligible(&EligibilityRequest {
            effort: Effort::Xhigh,
            ..req.clone()
        })
        .is_err());
    assert!(c
        .eligible(&EligibilityRequest {
            harness: "codex-cli",
            ..req.clone()
        })
        .is_err());
    c.bindings[0].eligible_roles = vec!["reviewer".into()];
    assert!(c.eligible(&req).is_err());
    assert!(c.validate().is_err());
}
#[test]
fn project_schema_one_binding_is_exact_in_role_harness_and_effort() {
    let selection=ProjectSelection::parse(br#"{"schema_version":1,"bindings":[{"role":"claude-judgment-primary","binding_id":"orchid-approved"}]}"#).unwrap();
    let records = [binding()];
    let c = catalog();
    let chosen = selection
        .resolve(
            &c,
            &records,
            "claude-judgment-primary",
            "claude-code",
            Effort::High,
        )
        .unwrap()
        .unwrap();
    assert_eq!(chosen.model, "orchid-one-pin");
    assert!(selection
        .resolve(
            &c,
            &records,
            "codex-engineering-primary",
            "codex-cli",
            Effort::High
        )
        .unwrap()
        .is_none());
    assert!(selection
        .resolve(
            &c,
            &records,
            "claude-judgment-primary",
            "codex-cli",
            Effort::High
        )
        .is_err());
    assert!(selection
        .resolve(
            &c,
            &records,
            "claude-judgment-primary",
            "claude-code",
            Effort::Xhigh
        )
        .is_err());
}
#[test]
fn project_selection_rejects_raw_selectors_missing_records_and_partial_application() {
    for bytes in [
        br#"{"schema_version":2,"bindings":[]}"#.as_slice(),
        br#"{"schema_version":1,"bindings":[{"role":"claude-judgment-primary","binding_id":"orchid-approved","selector":"raw"}]}"#,
        br#"{"schema_version":1,"schema_version":1,"bindings":[]}"#,
    ] { assert!(ProjectSelection::parse(bytes).is_err()); }
    let selection=ProjectSelection::parse(br#"{"schema_version":1,"bindings":[{"role":"claude-judgment-primary","binding_id":"orchid-approved"},{"role":"codex-engineering-primary","binding_id":"missing"}]}"#).unwrap();
    assert!(selection
        .resolve(
            &catalog(),
            &[binding()],
            "claude-judgment-primary",
            "claude-code",
            Effort::High
        )
        .is_err());
}
#[test]
fn full_suite_binding_rejects_claimed_qualification_without_approval() {
    let mut c = catalog();
    c.bindings = vec![binding()];
    c.bindings[0].approval.reviewer.clear();
    assert!(c.validate().unwrap_err().contains("approval.reviewer"));
    c.bindings = vec![binding()];
    c.bindings[0].qualification.suite = "focused".into();
    assert!(c.validate().unwrap_err().contains("full-suite"));
}
#[test]
fn native_identity_drift_discards_full_suite_and_scoped_evidence() {
    let mut c = catalog();
    c.lines[0].versions[0].designations.clear();
    c.bindings = vec![binding()];
    c.lines[1].versions[0].qualification =
        vec![serde_json::from_value(scoped("design-implementation", "medium")).unwrap()];
    let observed = BTreeMap::from([
        ("orchid-one-pin".into(), "new-pin".into()),
        ("orchid-two-pin".into(), "other-pin".into()),
    ]);
    for (duty, line, version, seat, effort) in [
        (
            "design",
            "orchid-main",
            "orchid-one",
            Some("orchid-seat"),
            Effort::High,
        ),
        (
            "design-implementation",
            "orchid-support",
            "orchid-two",
            None,
            Effort::Medium,
        ),
    ] {
        let req = EligibilityRequest {
            duty,
            line,
            version,
            seat,
            harness: "claude-code",
            effort,
            exclusions: &[],
            observed_ids: &observed,
        };
        assert!(c.eligible(&req).unwrap_err().contains("identity drift"));
    }
}
#[test]
fn retired_override_version_cannot_fill_design() {
    let mut c = catalog();
    c.lines[1].versions[0].lifecycle = Lifecycle::Retired;
    c.lines[1].versions[0].designations.clear();
    // Keep this a valid seat line, but exclude its active replacement.
    c.lines[1].versions.push(
        serde_json::from_value(version(
            "orchid-replacement",
            &["claude-code"],
            Some("orchid-seat"),
        ))
        .unwrap(),
    );
    c.validate().unwrap();
    let record = override_record();
    let result = override_result(
        Some(&record),
        &record.route,
        &c,
        &[exclude("orchid-replacement")],
        &BTreeMap::new(),
    );
    assert!(result.is_open());
    assert!(result.open[0].reasons[0].contains("retired version"));
}
#[test]
fn same_override_record_on_another_task_grants_nothing() {
    let record = override_record();
    let observed = BTreeMap::new();
    let mut req = request("design", &observed);
    req.task = "TSK-901";
    req.operator_override = Some(&record);
    req.requested_override = Some(&record.route);
    assert!(catalog().resolve(&req).unwrap().is_open());
}
#[test]
fn explicit_override_can_name_another_familys_seat_line() {
    let mut record = override_record();
    record.route.target = Target::Line("quartz-main".into());
    record.route.harness = Some("codex-cli".into());
    let result = override_result(
        Some(&record),
        &record.route,
        &catalog(),
        &[exclude("orchid-one")],
        &BTreeMap::new(),
    );
    assert!(!result.is_open());
    assert_eq!(result.participants[0].seat.as_deref(), Some("quartz-seat"));
    assert_eq!(result.participants[0].pinned_id, "quartz-one-pin");
}
#[test]
fn remaining_alternatives_include_eligible_seat_fallbacks() {
    let result = resolved("orchestrate");
    assert_eq!(
        result.participants[0].remaining_alternatives[0].pinned_id,
        "orchid-two-pin"
    );
    assert_eq!(
        result.participants[0].remaining_alternatives[0]
            .seat
            .as_deref(),
        Some("orchid-seat")
    );
}
#[test]
fn high_trigger_escalates_worker_without_raising_seat() {
    let observed = BTreeMap::new();
    let facts = vec!["material".into()];
    for duty in ["light-execution", "orchestrate"] {
        let mut req = request(duty, &observed);
        req.trigger_facts = &facts;
        let result = catalog().resolve(&req).unwrap();
        assert_eq!(result.participants[0].effort, Effort::High);
        assert!(result.obligations.is_empty());
    }
}
#[test]
fn native_harness_exclusion_can_use_a_listed_transport_alternative() {
    let mut c = catalog();
    let participant = &mut c.duties.get_mut("unit-review").unwrap().required[0];
    participant.alternatives.insert(
        0,
        Alternative {
            target: Target::Seat("quartz-seat".into()),
            harness: Some("codex-app".into()),
            effort: Effort::High,
        },
    );
    let exclusions = [Exclusion {
        scope: ExclusionScope::Harness("codex-app".into()),
        reason: "stale plugin login".into(),
        fresh_native: true,
    }];
    let observed = BTreeMap::new();
    let mut req = request("unit-review", &observed);
    req.exclusions = &exclusions;
    let result = c.resolve(&req).unwrap();
    assert_eq!(result.participants[0].harness, "codex-cli");
    assert_eq!(result.participants[0].seat.as_deref(), Some("quartz-seat"));
}
#[test]
fn overlay_rejects_unknown_lines_versions_harnesses_and_duplicate_ids_atomically() {
    let c = catalog();
    let base = json!({"schema_version":1,"additions":[{"line":"quartz-worker","id":"quartz-new","alias":"quartz-latest","pinned_id":"quartz-new-pin","selectors":{"codex-cli":"quartz-new-pin"},"efforts":["medium"]}],"exclusions":[]});
    for (path, value) in [
        ("line", json!("unlisted")),
        ("id", json!("quartz-one")),
        ("selectors", json!({"shell":"quartz-new-pin"})),
    ] {
        let mut bad = base.clone();
        bad["additions"][0][path] = value;
        assert!(PersonalOverlay::parse(&serde_json::to_vec(&bad).unwrap())
            .unwrap()
            .apply(&c)
            .is_err());
        assert_eq!(c.lines[3].versions.len(), 1);
    }
    let mut bad = base;
    bad["exclusions"] = json!(["unknown"]);
    assert!(PersonalOverlay::parse(&serde_json::to_vec(&bad).unwrap())
        .unwrap()
        .apply(&c)
        .is_err());
}
#[test]
fn newer_undesignated_version_never_displaces_designated_seat() {
    let mut c = catalog();
    c.lines[0].adoption = Adoption::Workers;
    c.lines[0]
        .versions
        .push(serde_json::from_value(version("orchid-later", &["claude-code"], None)).unwrap());
    assert_eq!(
        c.resolve(&request("design", &BTreeMap::new()))
            .unwrap()
            .participants[0]
            .version,
        "orchid-one"
    );
}
#[test]
fn latest_eligible_skips_newest_unsupported_effort() {
    let mut c = catalog();
    let mut newest = version("quartz-later", &["codex-cli"], None);
    newest["efforts"] = json!(["high"]);
    c.lines[3]
        .versions
        .push(serde_json::from_value(newest).unwrap());
    assert_eq!(
        c.resolve(&request("light-execution", &BTreeMap::new()))
            .unwrap()
            .participants[0]
            .version,
        "quartz-two"
    );
}
#[test]
fn exact_tuple_matrix_never_reuses_design_evidence() {
    let mut c = catalog();
    c.lines[1].versions[0].qualification =
        vec![serde_json::from_value(scoped("design-implementation", "medium")).unwrap()];
    let observed = BTreeMap::new();
    for duty in ["design-implementation", "fidelity-approval", "design"] {
        for harness in ["claude-code", "codex-cli", "codex-app", "grok-cli"] {
            for effort in [Effort::Low, Effort::Medium, Effort::High, Effort::Xhigh] {
                let req = EligibilityRequest {
                    duty,
                    line: "orchid-support",
                    version: "orchid-two",
                    seat: None,
                    harness,
                    effort,
                    exclusions: &[],
                    observed_ids: &observed,
                };
                assert_eq!(
                    c.eligible(&req).is_ok(),
                    duty == "design-implementation"
                        && harness == "claude-code"
                        && effort == Effort::Medium
                );
            }
        }
    }
}
#[test]
fn schema_four_transition_reader_accepts_fictional_models() {
    let binding = |role: &str, seat: &str, lineage: &str, provider: &str, harness: &str| {
        json!({
            "role":role,"seat":seat,"lineage":lineage,"provider":provider,"model_class":"fictional-latest",
            "native_selectors":{harness:"fictional-pin"},"default_effort":"high","escalation_effort":"xhigh",
            "responsibilities":["planning"],"internal_routes":[]
        })
    };
    let value = json!({
        "schema_version":4,"policy_id":"claude-codex-duo",
        "standing_roles":["claude-judgment-primary","codex-engineering-primary"],
        "design_execution_owner":"claude-judgment-primary",
        "bindings":[binding("claude-judgment-primary","orchid-seat","claude","anthropic","claude-code"),binding("codex-engineering-primary","quartz-seat","codex","openai","codex-cli")],
        "high_triggers":["material"],"xhigh_triggers":["deep"],"rules":["Use pinned identities."]
    });
    assert!(matches!(
        CatalogDocument::parse(&serde_json::to_vec(&value).unwrap()).unwrap(),
        CatalogDocument::Legacy(_)
    ));
    assert!(matches!(
        CatalogDocument::parse(&serde_json::to_vec(&fixture()).unwrap()).unwrap(),
        CatalogDocument::Current(_)
    ));
}

#[test]
fn designated_latest_seat_version_can_supply_xhigh_with_manual_adoption() {
    let mut c = catalog();
    c.lines[0].versions[0].efforts = vec![Effort::High];
    c.lines[1].versions[0].efforts = vec![Effort::High];
    c.lines[0].versions.push(
        serde_json::from_value(version(
            "orchid-three",
            &["claude-code"],
            Some("orchid-seat"),
        ))
        .unwrap(),
    );
    let observed = BTreeMap::new();
    let facts = vec!["deep".into()];
    let mut req = request("orchestrate", &observed);
    req.trigger_facts = &facts;
    let result = c.resolve(&req).unwrap();
    assert_eq!(result.participants[0].version, "orchid-three");
    assert_eq!(result.obligations[0].version, "orchid-three");
    assert_eq!(result.obligations[0].effort, Effort::Xhigh);
    assert!(result.xhigh_trigger_met);
}

#[test]
fn overlay_cannot_gain_seat_authority_from_a_preexisting_binding() {
    let mut c = catalog();
    let mut record = binding();
    record.requested.model = "orchid-new-pin".into();
    record.observed.model = "orchid-new-pin".into();
    c.bindings.push(record);
    let overlay = json!({"schema_version":1,"additions":[{"line":"orchid-main","id":"orchid-new","alias":"orchid-latest","pinned_id":"orchid-new-pin","selectors":{"claude-code":"orchid-new-pin"},"efforts":["high"]}],"exclusions":["orchid-one"]});
    let (copy, excluded) = PersonalOverlay::parse(&serde_json::to_vec(&overlay).unwrap())
        .unwrap()
        .apply(&c)
        .unwrap();
    let observed = BTreeMap::new();
    let mut req = request("design", &observed);
    req.exclusions = &excluded;
    let result = copy.resolve(&req).unwrap();
    assert!(result.is_open());
    assert!(result.open[0]
        .reasons
        .iter()
        .any(|r| r.contains("overlay candidates never fill seats")));
    let mut forged = fixture();
    forged["lines"][0]["versions"][0]["personal_candidate"] = json!(false);
    assert!(parse(&forged).is_err());
}

#[test]
fn overlay_worker_adoption_selects_addition_but_manual_keeps_it_inert() {
    for (adoption, expected) in [
        (Adoption::Workers, "quartz-new"),
        (Adoption::Manual, "quartz-two"),
    ] {
        let mut c = catalog();
        c.lines[3].adoption = adoption;
        let overlay = json!({"schema_version":1,"additions":[{"line":"quartz-worker","id":"quartz-new","alias":"quartz-latest","pinned_id":"quartz-new-pin","selectors":{"codex-cli":"quartz-new-pin"},"efforts":["medium"]}],"exclusions":[]});
        let (copy, exclusions) = PersonalOverlay::parse(&serde_json::to_vec(&overlay).unwrap())
            .unwrap()
            .apply(&c)
            .unwrap();
        assert!(exclusions.is_empty());
        let result = copy
            .resolve(&request("light-execution", &BTreeMap::new()))
            .unwrap();
        assert!(!result.is_open());
        assert_eq!(result.participants[0].version, expected);
        assert_eq!(result.participants[0].pinned_id, format!("{expected}-pin"));
        assert_eq!(result.participants[0].eligibility, Eligibility::Candidate);
        assert!(result.participants[0].seat.is_none());
    }
}

#[test]
fn route_less_override_leaves_eligible_design_open() {
    let c = catalog();
    let observed = BTreeMap::new();
    let record = override_record();
    let mut req = request("design", &observed);
    assert!(!c.resolve(&req).unwrap().is_open());
    req.operator_override = Some(&record);
    let result = c.resolve(&req).unwrap();
    assert!(result.is_open());
    assert!(result.participants.is_empty());
    assert_eq!(
        result.open[0].reasons,
        ["missing OPERATOR_OVERRIDE invocation route"]
    );
}

#[test]
fn override_floor_is_explicitly_the_design_duty_floor() {
    let mut record = override_record();
    record.route.effort = Effort::Medium;
    let result = override_result(
        Some(&record),
        &record.route,
        &catalog(),
        &[],
        &BTreeMap::new(),
    );
    assert!(result.is_open());
    assert_eq!(
        result.open[0].reasons,
        ["OPERATOR_OVERRIDE effort below design duty floor"]
    );
}

#[test]
fn missing_optional_second_opinion_does_not_open_filled_independent_review() {
    let c = catalog();
    let observed = BTreeMap::new();
    let mut req = request("general-review", &observed);
    req.author_lineage = Some("codex");
    let result = c.resolve(&req).unwrap();
    assert!(!result.is_open());
    assert_eq!(result.participants[0].participant, "independent");
    assert_eq!(result.open.len(), 1);
    assert_eq!(result.open[0].participant, "advisory");
    assert_eq!(result.open[0].label, ParticipantLabel::SecondOpinion);

    let excluded = [
        exclude("cinder-one"),
        exclude("orchid-one"),
        exclude("orchid-two"),
    ];
    req.exclusions = &excluded;
    let result = c.resolve(&req).unwrap();
    assert!(result.is_open());
    assert!(result
        .open
        .iter()
        .any(|gap| gap.label == ParticipantLabel::Required));
}

#[test]
fn resolution_revalidates_public_catalog_mutations() {
    let mut c = catalog();
    let observed = BTreeMap::new();
    let req = request("orchestrate", &observed);
    assert!(!c.resolve(&req).unwrap().is_open());
    c.duties.get_mut("orchestrate").unwrap().required[0].alternatives[0].effort = Effort::Medium;
    assert!(c
        .resolve(&req)
        .unwrap_err()
        .contains("below its effort floor"));
}

#[test]
fn native_routing_policy_trigger_is_shared_with_catalog_validation() {
    let mut c = catalog();
    c.duties.get_mut("unit-review").unwrap().triggered[0].trigger =
        "security-sensitive change".into();
    let observed = BTreeMap::new();
    let facts = vec!["security-sensitive change".into()];
    let mut req = request("unit-review", &observed);
    req.trigger_facts = &facts;
    let result = c.resolve(&req).unwrap();
    assert!(!result.is_open());
    assert_eq!(result.participants.len(), 2);
    assert_eq!(result.participants[1].participant, "extra");
}
