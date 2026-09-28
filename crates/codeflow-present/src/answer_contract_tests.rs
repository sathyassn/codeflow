//! The SPC-014 contract fixtures whose first passing task is TSK-119: the
//! `form` block, the v2 `decision` as a form, and the answers stored against
//! the revision shown (B6, B7, I1, I3, I4).

use scraper::{ElementRef, Html, Node, Selector};

use crate::{
    contract_tests::{find, fixture_bytes, index_entries, render, supported},
    document::{parse_document, Block, Framing, ParsedDocument},
    state::block_digest,
};

/// What each refused document fixture's error must name.
const REFUSALS: &[(&str, &[&str])] = &[
    (
        "documents/v2-form-required-unknown.json",
        &["required", "\"missing\""],
    ),
    (
        "documents/v2-form-two-recommended.json",
        &["\"home\"", "recommended"],
    ),
    ("documents/v2-form-default-value.json", &["default"]),
    (
        "documents/v2-decision-with-status.json",
        &["d-scope", "no status"],
    ),
];

#[test]
fn every_tsk119_document_fixture_is_accepted_or_refused_as_indexed() {
    let mut seen = 0;
    for entry in index_entries("TSK-119") {
        let file = entry["file"].as_str().unwrap();
        if !file.starts_with("documents/") {
            continue;
        }
        seen += 1;
        let parsed = parse_document(&fixture_bytes(file));
        if entry["valid"] == true {
            assert!(
                matches!(parsed, Ok(ParsedDocument::Supported(_))),
                "{file}: {parsed:?}"
            );
            continue;
        }
        let error = parsed.expect_err(file).to_string();
        let (_, names) = REFUSALS
            .iter()
            .find(|(name, _)| *name == file)
            .unwrap_or_else(|| panic!("{file}: no expected refusal listed"));
        for name in *names {
            assert!(error.contains(name), "{file}: {error} does not name {name}");
        }
    }
    assert_eq!(seen, 5);
}

fn select<'a>(root: &'a Html, css: &str) -> Vec<ElementRef<'a>> {
    root.select(&Selector::parse(css).unwrap()).collect()
}

/// The text a selection in the block's review root can reach: every text
/// node under the root except those the runtime marks `data-cf-review-skip`
/// (what `reviewText` in `selection.ts` reads), without whitespace.
fn review_text(root: ElementRef<'_>) -> String {
    let mut text = String::new();
    for node in root.descendants() {
        let Node::Text(part) = node.value() else {
            continue;
        };
        let skipped = node.ancestors().any(|ancestor| {
            ancestor
                .value()
                .as_element()
                .is_some_and(|element| element.attr("data-cf-review-skip").is_some())
        });
        if !skipped {
            text.push_str(part);
        }
    }
    text.split_whitespace().collect()
}

#[test]
fn a_form_renders_its_fields_in_order_with_nothing_preselected() {
    let document = supported("documents/v2-forms.json");
    for interactive in [true, false] {
        let html = Html::parse_document(&render(&document, interactive));
        let form = select(&html, "[data-cf-form='store-choice']");
        assert_eq!(form.len(), 1);
        let fields = select(&html, "[data-cf-form='store-choice'] [data-cf-field]")
            .into_iter()
            .map(|field| field.attr("data-cf-field").unwrap().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            fields,
            ["home", "keep-days", "share", "channels", "contact", "notes"]
        );
        assert_eq!(
            form[0].attr("data-cf-form-digest"),
            Some(block_digest(find(&document, "store-choice")).as_str()),
            "the server renders the form digest"
        );
        // Recommended is a label on its option, never a preselection; no
        // field has a value.
        let recommended = select(&html, "[data-cf-field='home'] .cf-option")
            .into_iter()
            .filter(|option| option.text().any(|text| text == "Recommended"))
            .map(|option| {
                select_in(option, "input")
                    .attr("value")
                    .unwrap()
                    .to_string()
            })
            .collect::<Vec<_>>();
        assert_eq!(recommended, ["local"]);
        assert!(select(&html, "[checked], [selected]").is_empty());
        for control in select(&html, "[data-cf-form] input") {
            let kind = control.attr("type").unwrap();
            assert!(
                matches!(kind, "radio" | "checkbox") || control.attr("value").is_none(),
                "a {kind} input has a value"
            );
            assert_eq!(
                control.attr("disabled").is_some(),
                !interactive,
                "an export shows the question read only"
            );
        }
        assert!(select(&html, "textarea")
            .iter()
            .all(|area| area.text().next().is_none()));
        assert!(
            select(&html, "form").is_empty(),
            "no form element: the page script submits"
        );
        assert_eq!(
            select(&html, "[data-cf-form-action]").is_empty(),
            !interactive,
            "an export has no actions"
        );
        // The decision is a form with one choice field `choice` and an
        // optional rationale.
        let decision = select(&html, "[data-cf-form='d-scope']");
        assert_eq!(decision.len(), 1);
        assert_eq!(decision[0].attr("data-cf-form-kind"), Some("decision"));
        let choice = select(&html, "[data-cf-form='d-scope'] [data-cf-field]");
        assert_eq!(choice.len(), 1);
        assert_eq!(choice[0].attr("data-cf-field"), Some("choice"));
        assert_eq!(choice[0].attr("data-cf-field-kind"), Some("choice"));
        assert_eq!(choice[0].attr("data-cf-rationale"), Some("optional"));
        assert!(choice[0].attr("data-cf-required").is_some());
    }
}

fn select_in<'a>(element: ElementRef<'a>, css: &str) -> ElementRef<'a> {
    element
        .select(&Selector::parse(css).unwrap())
        .next()
        .unwrap_or_else(|| panic!("no {css}"))
}

#[test]
fn a_form_review_text_is_what_the_page_lets_a_reviewer_select() {
    let document = supported("documents/v2-forms.json");
    let framing = Framing::of(&document);
    let html = Html::parse_document(&render(&document, true));
    for id in ["store-choice", "d-scope"] {
        let root = select_in(
            select(&html, &format!("[data-cf-block-id='{id}']"))[0],
            "[data-cf-review-text-root]",
        );
        let canonical = find(&document, id).canonical_review_text(&framing);
        assert_eq!(
            root.attr("data-cf-canonical-text"),
            Some(canonical.as_str())
        );
        let compact: String = canonical.split_whitespace().collect();
        assert_eq!(review_text(root), compact, "{id}");
    }
    let canonical = find(&document, "store-choice").canonical_review_text(&framing);
    assert!(canonical.starts_with("Where should answers live?\nPick one store."));
    assert!(!canonical.contains("Recommended") && !canonical.contains("required"));
}

#[test]
fn a_v1_decision_with_a_status_renders_as_before() {
    let document: crate::PresentationDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "title": "Legacy",
        "blocks": [{ "type": "decision", "id": "d", "title": "Ship it", "status": "accepted", "markdown": "We ship." }]
    }))
    .unwrap();
    document.validate().unwrap();
    let block = find(&document, "d");
    assert_eq!(
        serde_json::to_string(block).unwrap(),
        r#"{"type":"decision","id":"d","title":"Ship it","status":"accepted","markdown":"We ship."}"#,
        "the v1 block serializes, and so digests, as before"
    );
    assert_eq!(
        block.canonical_review_text(&Framing::of(&document)),
        "Ship it\nAccepted\nWe ship."
    );
    assert!(render(&document, true).contains(
        "<article class=\"decision\"><header><h2>Ship it</h2><span class=\"decision__status\">Accepted</span></header><p>We ship.</p>\n</article>"
    ));
    assert!(
        crate::form::FormView::of(block).is_none(),
        "a v1 decision takes no answer"
    );
    // A v1 decision needs its status, and the v2 members need schema 2.
    for (block, expect) in [
        (
            serde_json::json!({ "type": "decision", "id": "d", "title": "T", "markdown": "m" }),
            "needs a status",
        ),
        (
            serde_json::json!({ "type": "decision", "id": "d", "title": "T", "status": "open", "markdown": "m",
                                "options": [{ "value": "a", "label": "A" }, { "value": "b", "label": "B" }] }),
            "schema_version 2",
        ),
        (
            serde_json::json!({ "type": "form", "id": "f", "title": "T",
                                "fields": [{ "id": "a", "label": "A", "kind": "boolean" }] }),
            "schema_version 2",
        ),
    ] {
        let bytes = serde_json::to_vec(
            &serde_json::json!({ "schema_version": 1, "title": "T", "blocks": [block] }),
        )
        .unwrap();
        let error = parse_document(&bytes).unwrap_err().to_string();
        assert!(error.contains(expect), "{error}");
    }
}

#[test]
fn form_definitions_refuse_misplaced_and_out_of_range_constraints() {
    let parse = |field: serde_json::Value| {
        let bytes = serde_json::to_vec(&serde_json::json!({
            "schema_version": 2, "title": "T",
            "blocks": [{ "type": "form", "id": "f", "title": "T", "fields": [field] }]
        }))
        .unwrap();
        parse_document(&bytes)
    };
    assert!(parse(serde_json::json!({ "id": "a", "label": "A", "kind": "boolean" })).is_ok());
    for bad in [
        serde_json::json!({ "id": "a", "label": "A", "kind": "boolean", "options": [] }),
        serde_json::json!({ "id": "a", "label": "A", "kind": "text", "max_length": 16385 }),
        serde_json::json!({ "id": "a", "label": "A", "kind": "text", "min_length": 5, "max_length": 4 }),
        serde_json::json!({ "id": "a", "label": "A", "kind": "text", "minimum": 1 }),
        serde_json::json!({ "id": "a", "label": "A", "kind": "integer", "minimum": 0.5 }),
        serde_json::json!({ "id": "a", "label": "A", "kind": "integer", "maximum": 9_007_199_254_740_992_u64 }),
        serde_json::json!({ "id": "a", "label": "A", "kind": "number", "minimum": 3, "maximum": 2 }),
        serde_json::json!({ "id": "a", "label": "A", "kind": "integer", "minimum": 3, "maximum": 2 }),
        serde_json::json!({ "id": "a", "label": "A", "kind": "choice" }),
        serde_json::json!({ "id": "a", "label": "A", "kind": "choice", "options": [{ "value": "x", "label": "X" }] }),
        serde_json::json!({ "id": "a", "label": "A", "kind": "choice", "options": [
            { "value": "x", "label": "X" }, { "value": "x", "label": "Y" }] }),
        serde_json::json!({ "id": "a", "label": "A", "kind": "choice", "min_items": 1, "options": [
            { "value": "x", "label": "X" }, { "value": "y", "label": "Y" }] }),
        serde_json::json!({ "id": "a", "label": "A", "kind": "choices", "max_items": 3, "options": [
            { "value": "x", "label": "X" }, { "value": "y", "label": "Y" }] }),
        serde_json::json!({ "id": "a", "label": "A", "kind": "choices", "min_items": 2, "max_items": 1, "options": [
            { "value": "x", "label": "X" }, { "value": "y", "label": "Y" }] }),
        serde_json::json!({ "id": "A", "label": "A", "kind": "boolean" }),
        serde_json::json!({ "id": "a", "label": "", "kind": "boolean" }),
        serde_json::json!({ "id": "a", "label": "x".repeat(201), "kind": "boolean" }),
        serde_json::json!({ "id": "a", "label": "A\u{7}", "kind": "boolean" }),
        serde_json::json!({ "id": "a", "label": "A", "kind": "boolean", "description": " " }),
        serde_json::json!({ "id": "a", "label": "A", "kind": "boolean", "default": true }),
        serde_json::json!({ "id": "a", "label": "A", "kind": "date" }),
        serde_json::json!({ "id": "a", "label": "A", "kind": "text", "format": "phone" }),
    ] {
        assert!(parse(bad.clone()).is_err(), "accepted {bad}");
    }
    let document = |blocks: serde_json::Value| {
        parse_document(
            &serde_json::to_vec(
                &serde_json::json!({ "schema_version": 2, "title": "T", "blocks": blocks }),
            )
            .unwrap(),
        )
    };
    let field = |index: usize| serde_json::json!({ "id": format!("f{index}"), "label": "L", "kind": "boolean" });
    let form = |fields: Vec<serde_json::Value>| serde_json::json!({ "type": "form", "id": "f", "title": "T", "fields": fields });
    assert!(document(serde_json::json!([form((0..32).map(field).collect())])).is_ok());
    assert!(document(serde_json::json!([form((0..33).map(field).collect())])).is_err());
    assert!(document(serde_json::json!([form(Vec::new())])).is_err());
    assert!(document(serde_json::json!([form(vec![field(0), field(0)])])).is_err());
    let options = |count: usize| {
        (0..count)
            .map(|index| serde_json::json!({ "value": format!("v{index}"), "label": "L" }))
            .collect::<Vec<_>>()
    };
    let choice = |count: usize| {
        form(vec![
            serde_json::json!({ "id": "c", "label": "C", "kind": "choice", "options": options(count) }),
        ])
    };
    assert!(document(serde_json::json!([choice(24)])).is_ok());
    assert!(document(serde_json::json!([choice(25)])).is_err());
    let decision = |count: usize| serde_json::json!({ "type": "decision", "id": "d", "title": "T", "markdown": "m", "options": options(count) });
    assert!(document(serde_json::json!([decision(8)])).is_ok());
    assert!(document(serde_json::json!([decision(9)])).is_err());
    assert!(document(serde_json::json!([decision(1)])).is_err());
    let no_options =
        serde_json::json!({ "type": "decision", "id": "d", "title": "T", "markdown": "m" });
    assert!(document(serde_json::json!([no_options]))
        .unwrap_err()
        .to_string()
        .contains("needs options"));
    // At most 32 forms and v2 decisions in one document, nested ones too.
    let many = |count: usize| {
        (0..count)
            .map(|index| serde_json::json!({ "type": "decision", "id": format!("d{index}"), "title": "T", "markdown": "m", "options": options(2) }))
            .collect::<Vec<_>>()
    };
    assert!(document(serde_json::json!(many(32))).is_ok());
    let nested = serde_json::json!([{ "type": "disclosure", "id": "more", "summary": "More", "blocks": many(33) }]);
    assert!(document(nested)
        .unwrap_err()
        .to_string()
        .contains("at most 32"));
}

#[test]
fn a_decision_defaults_to_an_optional_rationale_and_a_form_to_none() {
    let document = supported("documents/v2-forms.json");
    let decision = crate::form::FormView::of(find(&document, "d-scope")).unwrap();
    assert_eq!(decision.fields.len(), 1);
    assert_eq!(decision.fields[0].id, "choice");
    assert_eq!(decision.fields[0].label, "Review scope");
    assert_eq!(
        decision.fields[0].rationale_mode(),
        crate::form::RationaleMode::Optional
    );
    assert!(decision.is_required("choice"));
    let Block::Decision { .. } = find(&document, "d-scope") else {
        panic!("d-scope is a decision");
    };
    let form = crate::form::FormView::of(find(&document, "store-choice")).unwrap();
    assert_eq!(
        form.fields[2].rationale_mode(),
        crate::form::RationaleMode::None
    );
    assert!(
        form.is_required("home") && form.is_required("keep-days") && !form.is_required("share")
    );
}
