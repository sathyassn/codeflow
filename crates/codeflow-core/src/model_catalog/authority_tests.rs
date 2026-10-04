use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::*;

fn authority_json() -> Value {
    json!({
        "schema_version": 2,
        "bindings": [],
        "design_authority": {
            "seat": "orchid-seat",
            "designated": {"date": "2026-10-04", "record": "owner instruction fixture"},
            "lines": {
                "orchid-main": {"role": "owner"},
                "orchid-support": {
                    "role": "co-owner",
                    "approves": ["phase-exit", "owner-gate"],
                    "consulted_before": ["structure"]
                }
            }
        },
        "standing_reviews": [
            {"area": "design", "seat": "cinder-seat", "mode": "read-only", "per": "phase",
             "record": "owner instruction fixture"}
        ]
    })
}

fn selection(value: &Value) -> Result<ProjectSelection, String> {
    let selection = ProjectSelection::parse(&serde_json::to_vec(value).unwrap())?;
    selection.validate_blocks(&catalog())?;
    Ok(selection)
}

/// A catalog carrying `value` as the working-tree selection and, when
/// `committed`, its design authority as committed on the integration target.
fn authority_catalog(value: &Value, committed: bool) -> Catalog {
    let mut c = catalog();
    let selection = selection(value).unwrap();
    if committed {
        c.authority = selection
            .design_authority
            .clone()
            .map(|authority| AnchoredAuthority {
                authority,
                source: "committed on integration/fixture at 0123abcd".into(),
            });
    }
    c.selection = Some(selection);
    c
}

fn with_exclusions<'a>(
    duty: &'a str,
    observed: &'a BTreeMap<String, String>,
    exclusions: &'a [Exclusion],
) -> ResolveRequest<'a> {
    ResolveRequest {
        exclusions,
        ..request(duty, observed)
    }
}

#[test]
fn committed_co_owner_designs_after_the_owner_without_override_or_reduced_assurance() {
    let observed = BTreeMap::new();
    let exclusions = [exclude("orchid-one")];
    let c = authority_catalog(&authority_json(), true);
    // Owner eligible: the owner designs and the co-owner is its alternative.
    let owner = c.resolve(&request("design", &observed)).unwrap();
    assert_eq!(owner.participants[0].line, "orchid-main");
    assert_eq!(
        owner.participants[0].remaining_alternatives[0].line,
        "orchid-support"
    );
    // Owner excluded: the co-owner designs, not as a fallback.
    let result = c
        .resolve(&with_exclusions("design", &observed, &exclusions))
        .unwrap();
    let chosen = &result.participants[0];
    assert_eq!(chosen.line, "orchid-support");
    assert!(chosen.operator_override.is_none());
    assert!(!chosen.reduced_assurance);
    assert!(!chosen
        .limitations
        .iter()
        .any(|l| l.contains("reduced assurance")));
    assert!(chosen.limitations.iter().any(|l| l
        == "design co-owner by project designation (.codeflow/model-selection.json, owner \
            instruction fixture; committed on integration/fixture at 0123abcd)"));
    // The designation is for design authority only: elsewhere the second line
    // is still a seat fallback with reduced assurance.
    let plan = c
        .resolve(&with_exclusions("independent-plan", &observed, &exclusions))
        .unwrap();
    let second = plan
        .participants
        .iter()
        .find(|p| p.line == "orchid-support")
        .unwrap();
    assert!(second.reduced_assurance);
}

#[test]
fn a_working_tree_block_alone_confers_no_design_authority() {
    let observed = BTreeMap::new();
    let exclusions = [exclude("orchid-one")];
    let mut c = authority_catalog(&authority_json(), false);
    c.authority_note = Some("not committed fixture note".into());
    let result = c
        .resolve(&with_exclusions("design", &observed, &exclusions))
        .unwrap();
    assert!(result.participants.is_empty());
    assert!(result.is_open());
    assert!(result.open[0]
        .reasons
        .iter()
        .any(|r| r == "not committed fixture note"));
    let approval = c.resolve(&request("design-approval", &observed)).unwrap();
    assert!(approval.participants.is_empty());
    assert!(!approval.is_open());
}

#[test]
fn a_consultant_never_designs() {
    let observed = BTreeMap::new();
    let exclusions = [exclude("orchid-one")];
    let mut value = authority_json();
    value["design_authority"]["lines"]["orchid-support"]["role"] = json!("consultant");
    let c = authority_catalog(&value, true);
    let result = c
        .resolve(&with_exclusions("design", &observed, &exclusions))
        .unwrap();
    assert!(result.participants.is_empty() && result.is_open());
    let approval = c.resolve(&request("design-approval", &observed)).unwrap();
    assert_eq!(
        approval.participants[0].participant,
        "consultant:orchid-support"
    );
    assert_eq!(
        approval.participants[0].label,
        ParticipantLabel::SecondOpinion
    );
    assert!(!approval.is_open());
}

#[test]
fn design_approval_is_a_separate_same_family_duty_never_the_independent_review() {
    let observed = BTreeMap::new();
    // Unconfigured: an optional open participant; the resolution stays filled.
    let unconfigured = resolved("design-approval");
    assert!(unconfigured.participants.is_empty());
    assert_eq!(unconfigured.open.len(), 1);
    assert_eq!(unconfigured.open[0].participant, "design-approval");
    assert_eq!(unconfigured.open[0].label, ParticipantLabel::SecondOpinion);
    assert!(!unconfigured.is_open());
    // Configured: the committed co-owner is the required approver.
    let c = authority_catalog(&authority_json(), true);
    let approval = c.resolve(&request("design-approval", &observed)).unwrap();
    let approver = &approval.participants[0];
    assert_eq!(approver.participant, "approver:orchid-support");
    assert_eq!(approver.label, ParticipantLabel::Required);
    assert_eq!(approver.seat.as_deref(), Some("orchid-seat"));
    assert!(!approver.reduced_assurance);
    for text in [
        "same-family design approval; not the independent review",
        "approves at: phase-exit, owner-gate",
        "consulted before: structure",
    ] {
        assert!(approver.limitations.iter().any(|l| l == text), "{text}");
    }
    // The independent review is unchanged: an author of the design owner's
    // lineage still gets the other lineage, and the approver never fills it.
    let review = c.resolve(&request("unit-review", &observed)).unwrap();
    assert_eq!(review.participants[0].seat.as_deref(), Some("quartz-seat"));
    assert!(review.participants.iter().all(|p| p.lineage == "codex"));
    // An excluded approver leaves the required approval open.
    let exclusions = [exclude("orchid-two")];
    let open = c
        .resolve(&with_exclusions("design-approval", &observed, &exclusions))
        .unwrap();
    assert!(open.is_open());
    // An override never applies to design approval.
    let route = Alternative {
        target: Target::Line("orchid-support".into()),
        harness: Some("claude-code".into()),
        effort: Effort::High,
    };
    assert!(c
        .resolve(&ResolveRequest {
            requested_override: Some(&route),
            ..request("design-approval", &observed)
        })
        .is_err());
}

#[test]
fn a_standing_review_adds_the_catalog_participant_only_for_its_area() {
    let observed = BTreeMap::new();
    let c = authority_catalog(&authority_json(), true);
    let baseline = serde_json::to_string(
        &catalog()
            .resolve(&request("unit-review", &observed))
            .unwrap(),
    )
    .unwrap();
    for area in [None, Some("billing")] {
        let result = c
            .resolve(&ResolveRequest {
                area,
                ..request("unit-review", &observed)
            })
            .unwrap();
        assert_eq!(serde_json::to_string(&result).unwrap(), baseline);
    }
    let result = c
        .resolve(&ResolveRequest {
            area: Some("design"),
            ..request("unit-review", &observed)
        })
        .unwrap();
    let extra = result
        .participants
        .iter()
        .find(|p| p.participant == "extra")
        .unwrap();
    assert_eq!(extra.seat.as_deref(), Some("cinder-seat"));
    assert_eq!(extra.label, ParticipantLabel::Required);
    assert!(extra.limitations.iter().any(|l| l
        == "standing assignment (area design, seat cinder-seat, read-only, per phase; owner \
            instruction fixture)"));
    // The extra family's own author still never reviews itself.
    let own = c
        .resolve(&ResolveRequest {
            area: Some("design"),
            author_lineage: Some("grok"),
            ..request("unit-review", &observed)
        })
        .unwrap();
    assert!(own.participants.iter().all(|p| p.participant != "extra"));
    // The area adds nothing outside the review duties.
    let design = c
        .resolve(&ResolveRequest {
            area: Some("design"),
            ..request("design", &observed)
        })
        .unwrap();
    assert!(design.participants.iter().all(|p| p.participant != "extra"));
}

type Change = Box<dyn Fn(&mut Value)>;

#[test]
#[allow(clippy::too_many_lines)] // one table row per rejected shape
fn each_invalid_block_rejects_the_whole_file() {
    let cases: Vec<(Change, &str)> = vec![
        (
            Box::new(|v| v["design_authority"]["seat"] = json!("quartz-seat")),
            "design owner seat",
        ),
        (
            Box::new(|v| {
                v["design_authority"]["lines"]["quartz-main"] = json!({"role": "co-owner"});
            }),
            "is another family",
        ),
        (
            Box::new(|v| v["design_authority"]["lines"]["nowhere"] = json!({"role": "co-owner"})),
            "unknown line",
        ),
        (
            Box::new(|v| {
                v["design_authority"]["lines"]["orchid-main"]["role"] = json!("co-owner");
                v["design_authority"]["lines"]["orchid-support"] = json!({"role": "owner"});
            }),
            "must be seat orchid-seat's first line",
        ),
        (
            Box::new(|v| {
                v["design_authority"]["lines"]["orchid-support"] = json!({"role": "owner"});
            }),
            "exactly one owner",
        ),
        (
            Box::new(|v| {
                v["design_authority"]["lines"]
                    .as_object_mut()
                    .unwrap()
                    .remove("orchid-support");
            }),
            "no co-owner or consultant",
        ),
        (
            Box::new(|v| v["design_authority"]["lines"]["orchid-support"]["role"] = json!("vote")),
            "unknown variant",
        ),
        (
            Box::new(|v| {
                v["design_authority"]["lines"]["orchid-support"]["selector"] = json!("x");
            }),
            "unknown field",
        ),
        (
            Box::new(|v| v["design_authority"]["family"] = json!("quartz")),
            "unknown field",
        ),
        (
            Box::new(|v| v["design_authority"]["designated"]["date"] = json!("Oct 4")),
            "YYYY-MM-DD",
        ),
        (
            Box::new(|v| v["design_authority"]["designated"]["record"] = json!(" ")),
            "design authority record must not be empty",
        ),
        (
            Box::new(|v| {
                v["design_authority"]["lines"]["orchid-support"]["approves"] = json!([]);
            }),
            "approves must not be empty",
        ),
        (
            Box::new(|v| {
                v["design_authority"]["lines"]["orchid-main"]["approves"] = json!(["x"]);
            }),
            "takes no approves",
        ),
        (
            Box::new(|v| v["standing_reviews"][0]["seat"] = json!("quartz-seat")),
            "standing-pair",
        ),
        (
            Box::new(|v| v["standing_reviews"][0]["seat"] = json!("nobody")),
            "unknown seat",
        ),
        (
            Box::new(|v| v["standing_reviews"][0]["mode"] = json!("write")),
            "unknown variant",
        ),
        (
            Box::new(|v| v["standing_reviews"][0]["per"] = json!("epic")),
            "unknown variant",
        ),
        (
            Box::new(|v| v["standing_reviews"][0]["record"] = json!("")),
            "must not be empty",
        ),
        (
            Box::new(|v| v["standing_reviews"] = json!([])),
            "must not be empty",
        ),
        (
            Box::new(|v| {
                let entry = v["standing_reviews"][0].clone();
                v["standing_reviews"].as_array_mut().unwrap().push(entry);
            }),
            "duplicate standing review",
        ),
        (
            Box::new(|v| v["schema_version"] = json!(1)),
            "need project model-selection schema 2",
        ),
        (Box::new(|v| v["schema_version"] = json!(3)), "unsupported"),
    ];
    for (change, reason) in cases {
        let mut value = authority_json();
        change(&mut value);
        let error = selection(&value).unwrap_err();
        assert!(error.contains(reason), "{error}, expected {reason}");
    }
    let duplicate = serde_json::to_string(&authority_json()).unwrap().replace(
        r#""orchid-main":{"role":"owner"}"#,
        r#""orchid-main":{"role":"owner"},"orchid-main":{"role":"owner"}"#,
    );
    assert!(ProjectSelection::parse(duplicate.as_bytes())
        .unwrap_err()
        .contains("duplicate JSON key"));
    assert!(
        ProjectSelection::parse(br#"{"schema_version":2,"bindings":[]}"#)
            .unwrap_err()
            .contains("needs design_authority or standing_reviews")
    );
}

#[test]
fn a_line_without_an_active_designated_version_cannot_hold_design_authority() {
    // The second line keeps a designated fallback-only version, which keeps
    // the catalog valid, but its active version is undesignated and its
    // oldest version is retired.
    let mut value = fixture();
    let mut fallback = version("orchid-two", &["claude-code"], Some("orchid-seat"));
    fallback["lifecycle"] = json!("fallback-only");
    let mut retired = version("orchid-zero", &["claude-code"], None);
    retired["lifecycle"] = json!("retired");
    let active = version("orchid-three", &["claude-code"], None);
    value["lines"][1]["versions"] = json!([retired, fallback, active]);
    value["lines"][1]["adopted_version"] = json!("orchid-three");
    let c = parse(&value).unwrap();
    let error = ProjectSelection::parse(&serde_json::to_vec(&authority_json()).unwrap())
        .unwrap()
        .validate_blocks(&c)
        .unwrap_err();
    assert!(error.contains("no active version designated"), "{error}");
}

#[test]
fn the_catalog_cannot_define_the_engine_owned_design_approval_duty() {
    assert_invalid(
        |v| v["duties"]["design-approval"] = v["duties"]["design"].clone(),
        "engine-owned",
    );
}
