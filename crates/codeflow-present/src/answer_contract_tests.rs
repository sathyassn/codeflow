//! The SPC-014 contract fixtures whose first passing task is TSK-119: the
//! `form` block, the v2 `decision` as a form, and the answers stored against
//! the revision shown (B6, B7, I1, I3, I4).

use scraper::{ElementRef, Html, Node, Selector};

use serde_json::Value;
use uuid::Uuid;

use crate::{
    contract_tests::{find, fixture_bytes, fixture_json, index_entries, render, supported},
    document::{parse_document, Block, Framing, ParsedDocument, PresentationDocument},
    responses::{Actor, AnswerReceipt, ResponseEvent, RESPONSES_FILE},
    state::{block_digest, SessionStore},
    PresentError,
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
        // A count hint reads as a count once, like the page's errors.
        assert_eq!(
            select_in(
                select(&html, "[data-cf-field='channels']")[0],
                ".cf-field__hint"
            )
            .text()
            .collect::<String>(),
            "Choose 1 or 2."
        );
        // The error line comes before the rationale, which names it too.
        let home = select(&html, "[data-cf-field='home']")[0];
        let parts = home
            .children()
            .filter_map(ElementRef::wrap)
            .map(|part| part.value().classes().next().unwrap_or("").to_string())
            .filter(|class| class == "cf-field__error" || class == "cf-field__rationale")
            .collect::<Vec<_>>();
        assert_eq!(parts, ["cf-field__error", "cf-field__rationale"]);
        assert_eq!(
            select_in(home, "[data-cf-rationale-input]").attr("aria-describedby"),
            Some("cf-form-store-choice-home-error")
        );
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

// The answer fixtures, against a live session of `documents/v2-forms.json`.

/// The session id the answer fixtures name; the harness puts the live
/// session's id in its place.
const FIXTURE_SESSION: &str = "7c1e2d3a-0000-4000-8000-000000000001";

pub(crate) struct FormsSession {
    pub(crate) _temp: tempfile::TempDir,
    pub(crate) store: SessionStore,
    pub(crate) id: Uuid,
    pub(crate) document: PresentationDocument,
}

impl FormsSession {
    pub(crate) fn open() -> Self {
        let (temp, store) = crate::contract_tests::store();
        let document = supported("documents/v2-forms.json");
        let id = store
            .create(ParsedDocument::Supported(document.clone()))
            .unwrap()
            .id;
        Self {
            _temp: temp,
            store,
            id,
            document,
        }
    }

    pub(crate) fn ledger_path(&self) -> std::path::PathBuf {
        self.store.session_dir(self.id).join(RESPONSES_FILE)
    }

    /// The ledger bytes, or `None` while no answer was stored.
    pub(crate) fn ledger_bytes(&self) -> Option<Vec<u8>> {
        std::fs::read(self.ledger_path()).ok()
    }

    /// One request of a fixture as the page would send it: the session id,
    /// the form digests and earlier answer ids filled in.
    pub(crate) fn body(&self, request: &Value, answers: &[Uuid]) -> Vec<u8> {
        let mut text = serde_json::to_string(request).unwrap();
        text = text.replace(FIXTURE_SESSION, &self.id.to_string());
        for block in self.document.walk() {
            text = text.replace(
                &format!("{{{{form_digest:{}}}}}", block.id()),
                &block_digest(block),
            );
        }
        for (index, answer) in answers.iter().enumerate() {
            text = text.replace(&format!("{{{{answer_id:{index}}}}}"), &answer.to_string());
        }
        assert!(!text.contains("{{"), "unfilled placeholder in {text:.200}");
        text.into_bytes()
    }

    pub(crate) fn submit(&self, body: &[u8]) -> crate::Result<AnswerReceipt> {
        self.store.submit_answer(self.id, body)
    }

    pub(crate) fn digest(&self, form: &str) -> String {
        block_digest(find(&self.document, form))
    }
}

/// A refusal's code and details.
fn refusal(result: crate::Result<AnswerReceipt>) -> (&'static str, Value) {
    match result {
        Err(PresentError::Review { code, details, .. }) => (code, details),
        other => panic!("expected a typed refusal, got {other:?}"),
    }
}

fn field_errors(details: &Value) -> Vec<(String, String)> {
    let mut fields: Vec<(String, String)> = details["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["field"].as_str().unwrap().to_string(),
                entry["code"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    fields.sort();
    fields
}

fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = expected
        .iter()
        .map(|(field, code)| ((*field).to_string(), (*code).to_string()))
        .collect();
    pairs.sort();
    pairs
}

/// Each answer fixture stores or refuses as `index.json` says, and a
/// refusal leaves the ledger exactly as it was.
#[test]
#[allow(clippy::too_many_lines)]
fn every_tsk119_answer_fixture_is_stored_or_refused_as_indexed() {
    let mut seen = 0;
    for entry in index_entries("TSK-119") {
        let file = entry["file"].as_str().unwrap();
        if !file.starts_with("answers/") {
            continue;
        }
        seen += 1;
        let session = FormsSession::open();
        let fixture = fixture_json(file);
        let requests: Vec<Value> = fixture.get("requests").map_or_else(
            || vec![fixture.clone()],
            |list| list.as_array().unwrap().clone(),
        );
        let mut answers = Vec::new();
        let mut results = Vec::new();
        let mut before_last = None;
        for (index, request) in requests.iter().enumerate() {
            if index + 1 == requests.len() {
                before_last = Some(session.ledger_bytes());
            }
            let body = session.body(request, &answers);
            let result = session.submit(&body);
            if let Ok(receipt) = &result {
                answers.push(receipt.answer_id);
            }
            results.push((body, result));
        }
        let before_last = before_last.unwrap();
        let events = session.store.responses(session.id).unwrap();
        let name = file
            .trim_start_matches("answers/")
            .trim_end_matches(".json");
        let last = results.pop().unwrap().1;
        let refused_unchanged = |last: crate::Result<AnswerReceipt>| {
            let refused = refusal(last);
            assert_eq!(
                session.ledger_bytes(),
                before_last,
                "{file}: ledger changed"
            );
            refused
        };
        match name {
            "submit-valid" | "decline" | "cancel" => {
                let receipt = last.unwrap();
                assert_eq!(
                    (receipt.state.as_str(), receipt.replayed, receipt.sequence),
                    ("stored", false, 1),
                    "{file}"
                );
                assert_eq!(events.len(), 1, "{file}");
                let ResponseEvent::Answer(record) = &events[0] else {
                    panic!("{file}: not an answer line")
                };
                let body = session.body(&requests[0], &[]);
                assert_eq!(record.payload_digest, crate::form::sha256_hex(&body));
                assert_eq!(record.form_digest, session.digest("store-choice"));
                assert_eq!(record.actor, Actor::Operator);
                assert_eq!(record.session_id, session.id);
                assert_eq!(record.answer_id, receipt.answer_id);
                assert_eq!(record.created_at_unix, receipt.stored_at_unix);
                assert_eq!(record.question.title, "Where should answers live?");
                assert_eq!(record.question.fields.len(), 6);
                assert_eq!(Value::Object(record.values.clone()), requests[0]["values"]);
                assert_eq!(
                    record.reason.as_deref(),
                    requests[0]["reason"].as_str(),
                    "{file}"
                );
                if name != "submit-valid" {
                    assert!(record.values.is_empty(), "{file}");
                }
            }
            "missing-required" => {
                let (code, details) = refused_unchanged(last);
                assert_eq!(code, "invalid_answer");
                assert_eq!(field_errors(&details), pairs(&[("keep-days", "required")]));
            }
            "malformed-values" => {
                let (code, details) = refused_unchanged(last);
                assert_eq!(code, "invalid_answer");
                assert_eq!(
                    field_errors(&details),
                    pairs(&[
                        ("home", "unknown_option"),
                        ("keep-days", "not_integer"),
                        ("share", "wrong_kind"),
                        ("channels", "too_many"),
                        ("contact", "format"),
                        ("extra", "unknown_field"),
                    ])
                );
            }
            "rationale-not-allowed" => {
                let (code, details) = refused_unchanged(last);
                assert_eq!(code, "invalid_answer");
                assert_eq!(
                    field_errors(&details),
                    pairs(&[("notes", "rationale_not_allowed")])
                );
            }
            "stale-revision" => {
                let (code, details) = refused_unchanged(last);
                assert_eq!(code, "stale_revision");
                assert_eq!(
                    details,
                    serde_json::json!({
                        "current_revision": 1,
                        "form_present": true,
                        "current_form_digest": session.digest("store-choice"),
                    })
                );
            }
            "form-digest-mismatch" => {
                assert_eq!(refused_unchanged(last).0, "form_digest_mismatch");
            }
            "oversized" => {
                let (code, details) = refused_unchanged(last);
                assert_eq!(code, "answer_too_large");
                assert_eq!(details["limit_bytes"], 65_536);
            }
            "retry-identical" => {
                assert_eq!(fixture["drop_first_response"], true);
                // The first response is dropped: only the retry's receipt
                // reaches the page, and it is the original one.
                let receipt = last.unwrap();
                assert!(receipt.replayed);
                assert_eq!(receipt.sequence, 1);
                assert_eq!(receipt.answer_id, answers[0]);
                assert_eq!(receipt.stored_at_unix, events[0].record().created_at_unix);
                assert_eq!(events.len(), 1);
            }
            "request-id-conflict" => {
                let (code, details) = refused_unchanged(last);
                assert_eq!(code, "request_id_conflict");
                assert_eq!(details["request_id"], requests[1]["request_id"]);
                assert_eq!(events.len(), 1);
            }
            "amendment" => {
                let receipt = last.unwrap();
                assert_eq!((receipt.sequence, receipt.replayed), (2, false));
                assert_eq!(events.len(), 2);
                assert!(
                    matches!(&events[0], ResponseEvent::Answer(record) if record.amends.is_none())
                );
                assert!(matches!(&events[1], ResponseEvent::Amendment(record)
                    if record.amends == Some(answers[0]) && record.values["home"] == "repo"));
            }
            "amend-an-amendment" => {
                let (code, details) = refused_unchanged(last);
                assert_eq!(code, "invalid_amendment");
                assert_eq!(details["amends"], answers[1].to_string());
                assert_eq!(events.len(), 2);
            }
            other => panic!("{file}: no expectation for {other}"),
        }
    }
    assert_eq!(seen, 13);
}

/// I4: a torn final line is cut when the ledger is opened; the complete
/// line before it stays and the next append takes the next sequence.
#[test]
fn a_torn_ledger_tail_is_truncated_on_open() {
    let session = FormsSession::open();
    let fixture = String::from_utf8(fixture_bytes("ledger/torn-tail.jsonl"))
        .unwrap()
        .replace(FIXTURE_SESSION, &session.id.to_string());
    let whole_line = fixture.find('\n').unwrap() + 1;
    assert!(!fixture.ends_with('\n'), "the fixture's last line is torn");
    std::fs::write(session.ledger_path(), &fixture).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(
            session.ledger_path(),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
    }
    let events = session.store.responses(session.id).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].record().outcome, crate::form::Outcome::Cancel);
    assert_eq!(
        session.ledger_bytes().unwrap(),
        fixture.as_bytes()[..whole_line]
    );
    let mut request = fixture_json("answers/submit-valid.json");
    request["request_id"] = Value::String("3f2a0c11-0000-4000-8000-0000000000c9".into());
    let receipt = session.submit(&session.body(&request, &[])).unwrap();
    assert_eq!((receipt.sequence, receipt.replayed), (2, false));
    let events = session.store.responses(session.id).unwrap();
    assert_eq!(events.len(), 2);
    assert!(session.ledger_bytes().unwrap().ends_with(b"}\n"));
}

/// B6: a request for a closed session, for another session, for a form the
/// revision lacks, or amending an answer of another form is refused, and
/// the ledger stays as it was; the ledger file is owner-only.
#[test]
fn answers_are_refused_for_closed_sessions_unknown_forms_and_foreign_amendments() {
    let session = FormsSession::open();
    let valid = fixture_json("answers/submit-valid.json");
    let receipt = session.submit(&session.body(&valid, &[])).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(session.ledger_path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "the ledger is owner-only");
    }
    let before = session.ledger_bytes();
    let with = |field: &str, value: Value| {
        let mut request = valid.clone();
        request["request_id"] = Value::String(Uuid::new_v4().to_string());
        request[field] = value;
        session.body(&request, &[])
    };
    // Another session's id: refused before the store is touched.
    let foreign = String::from_utf8(with("revision", 1.into()))
        .unwrap()
        .replace(&session.id.to_string(), &Uuid::new_v4().to_string());
    assert_eq!(
        refusal(session.submit(foreign.as_bytes())).0,
        "invalid_answer"
    );
    let unknown = with("form_id", "no-such-form".into());
    let (code, details) = refusal(session.submit(&unknown));
    assert_eq!(
        (code, details["form_id"].as_str()),
        ("unknown_form", Some("no-such-form"))
    );
    // A v2 decision is a form too; amending an answer of `store-choice`
    // from `d-scope` names an answer of another form.
    let mut decision = serde_json::json!({
        "request_id": Uuid::new_v4(), "session_id": FIXTURE_SESSION, "revision": 1,
        "form_id": "d-scope", "form_digest": "{{form_digest:d-scope}}", "outcome": "submit",
        "values": { "choice": "a" }, "rationales": {}, "amends": receipt.answer_id,
    });
    let (code, _) = refusal(session.submit(&session.body(&decision, &[])));
    assert_eq!(code, "invalid_amendment");
    decision["amends"] = Value::String(Uuid::new_v4().to_string());
    assert_eq!(
        refusal(session.submit(&session.body(&decision, &[]))).0,
        "invalid_amendment"
    );
    let malformed = b"{\"request_id\": 7}";
    assert_eq!(refusal(session.submit(malformed)).0, "invalid_answer");
    assert_eq!(session.ledger_bytes(), before);
    session.store.close(session.id).unwrap();
    let late = with("revision", 1.into());
    assert!(matches!(
        session.submit(&late),
        Err(PresentError::SessionClosed(_))
    ));
    // A retry of the stored request still gets its receipt after close.
    let retry = session.submit(&session.body(&valid, &[])).unwrap();
    assert!(retry.replayed);
    assert_eq!(session.ledger_bytes(), before);
}

/// B6: the page and the server check answers alike. The vectors in
/// `tests/fixtures/form-rules/parity.json` are also run by the page's
/// `form-rules.ts` in `web/scripts/check.mjs`; both must give exactly these
/// errors in this order.
#[test]
fn the_server_answer_checks_match_the_shared_parity_vectors() {
    use crate::form::{is_date_time, is_email, is_full_date, is_uri, AnswerRequest, FormView};

    let parity: Value = serde_json::from_slice(
        &std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/form-rules/parity.json"),
        )
        .unwrap(),
    )
    .unwrap();
    for vector in parity["formats"].as_array().unwrap() {
        let value = vector["value"].as_str().unwrap();
        let valid = match vector["format"].as_str().unwrap() {
            "email" => is_email(value),
            "uri" => is_uri(value),
            "date" => is_full_date(value),
            "date-time" => is_date_time(value),
            other => panic!("unknown format {other}"),
        };
        assert_eq!(
            valid,
            vector["valid"].as_bool().unwrap(),
            "{}: {value:?}",
            vector["format"]
        );
    }
    let ParsedDocument::Supported(document) =
        parse_document(&serde_json::to_vec(&parity["document"]).unwrap()).unwrap()
    else {
        panic!("the parity document is a v2 document")
    };
    let mut seen = 0;
    for case in parity["answers"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let form = FormView::of(find(&document, case["form"].as_str().unwrap())).unwrap();
        let mut request = serde_json::json!({
            "request_id": Uuid::nil(), "session_id": Uuid::nil(), "revision": 1,
            "form_id": case["form"], "form_digest": form.digest(), "outcome": case["outcome"],
            "values": case["values"], "rationales": case["rationales"],
        });
        if let Some(reason) = case.get("reason") {
            request["reason"] = reason.clone();
        }
        let request: AnswerRequest = serde_json::from_value(request).unwrap();
        let errors = match form.validate_answer(&request) {
            Ok(()) => Vec::new(),
            Err(PresentError::Review { code, details, .. }) => {
                assert_eq!(code, "invalid_answer", "{name}");
                details["fields"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|entry| serde_json::json!([entry["field"], entry["code"]]))
                    .collect()
            }
            Err(other) => panic!("{name}: {other}"),
        };
        assert_eq!(Value::Array(errors), case["errors"], "{name}");
        seen += 1;
    }
    assert!(seen >= 30, "{seen} parity cases");
}

/// I3: the 64 KiB bound is checked before anything else in the body, at
/// the store as at the route: exactly 64 KiB is read, one byte more is
/// `answer_too_large` whatever the body holds. A revision newer than the
/// current one is stale too.
#[test]
fn the_answer_bound_is_exact_and_a_future_revision_is_stale() {
    let session = FormsSession::open();
    let valid = fixture_json("answers/submit-valid.json");
    let mut at_limit = session.body(&valid, &[]);
    at_limit.resize(crate::limits::MAX_ANSWER_REQUEST_BYTES, b' ');
    let receipt = session.submit(&at_limit).unwrap();
    assert_eq!((receipt.sequence, receipt.replayed), (1, false));
    let stored = session.ledger_bytes();
    let mut over = at_limit.clone();
    over.push(b' ');
    let (code, details) = refusal(session.submit(&over));
    assert_eq!(
        (code, details["limit_bytes"].as_u64()),
        ("answer_too_large", Some(65_536))
    );
    let mut garbage = vec![b'x'; crate::limits::MAX_ANSWER_REQUEST_BYTES + 1];
    garbage[0] = b'{';
    assert_eq!(refusal(session.submit(&garbage)).0, "answer_too_large");
    let mut future = valid.clone();
    future["request_id"] = Value::String(Uuid::new_v4().to_string());
    future["revision"] = 2.into();
    let (code, details) = refusal(session.submit(&session.body(&future, &[])));
    assert_eq!(code, "stale_revision");
    assert_eq!(details["current_revision"], 1);
    assert_eq!(session.ledger_bytes(), stored);
}
