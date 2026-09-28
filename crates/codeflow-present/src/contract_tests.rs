//! The SPC-014 contract fixtures whose first passing task is TSK-118:
//! documents, entity anchors and re-anchoring. Fixtures live under
//! `tests/fixtures/contract-v2/` and are indexed by its `index.json`. The
//! TSK-119 fixtures (forms and answers) run in `answer_contract_tests.rs`
//! with the helpers below.

use std::path::PathBuf;

use serde_json::Value;
use uuid::Uuid;

use crate::{
    document::{parse_document, Block, ParsedDocument, PresentationDocument},
    render::{render_document, RenderOptions},
    state::{
        block_digest, CropCheck, FeedbackAnchor, FeedbackEnvelope, FeedbackKind, FeedbackNote,
        FeedbackVerdict, SessionStore, TextSelector,
    },
    PresentError,
};

pub(crate) fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/contract-v2")
}

pub(crate) fn fixture_bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture_root().join(name)).unwrap_or_else(|error| panic!("{name}: {error}"))
}

pub(crate) fn fixture_json(name: &str) -> Value {
    serde_json::from_slice(&fixture_bytes(name)).unwrap()
}

pub(crate) fn index_entries(task: &str) -> Vec<Value> {
    let index = fixture_json("index.json");
    index["fixtures"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["first_passing_task"] == task)
        .cloned()
        .collect()
}

pub(crate) fn supported(name: &str) -> PresentationDocument {
    match parse_document(&fixture_bytes(name)).unwrap() {
        ParsedDocument::Supported(document) => document,
        ParsedDocument::Unsupported { .. } => panic!("{name} parsed as unsupported"),
    }
}

pub(crate) fn find<'a>(document: &'a PresentationDocument, id: &str) -> &'a Block {
    document
        .walk()
        .into_iter()
        .find(|block| block.id() == id)
        .unwrap_or_else(|| panic!("no block {id}"))
}

pub(crate) fn render(document: &PresentationDocument, interactive: bool) -> String {
    render_document(
        document,
        &RenderOptions {
            session_id: "00000000-0000-4000-8000-000000000000",
            revision: 1,
            event_sequence: 0,
            response_sequence: 0,
            script_path: None,
            style_path: None,
            prepaint_source: None,
            utility_style: None,
            identity: None,
            feedback: None,
            read_only_warning: None,
            interactive,
        },
    )
}

#[test]
fn every_tsk118_document_fixture_is_accepted_or_refused_as_indexed() {
    let mut seen = 0;
    for entry in index_entries("TSK-118") {
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
        } else {
            let error = parsed.expect_err(file).to_string();
            let expect = entry["expect"].as_str().unwrap();
            // The expectation names the offending block or attribute; the
            // error must name it too.
            let key = expect
                .split_whitespace()
                .find(|word| {
                    word.contains('-')
                        || word.starts_with("data-cf")
                        || word.starts_with('[')
                        || *word == "Service"
                })
                .unwrap_or("")
                .trim_end_matches([':', ',']);
            assert!(
                error.contains(key.trim_matches(|c| c == '[' || c == ']')) || key.is_empty(),
                "{file}: {error} does not name {key}"
            );
        }
    }
    assert!(seen >= 12, "only {seen} document fixtures ran");
}

#[test]
fn a_v2_document_numbers_figures_and_tables_and_resolves_references() {
    let document = supported("documents/v2-framed.json");
    for interactive in [true, false] {
        let html = render(&document, interactive);
        assert!(
            html.contains("data-cf-figure-number=\"1\""),
            "figure block number"
        );
        for expected in [
            "<span class=\"cf-frame-number\">Figure 2</span> · <span class=\"cf-frame-name\">How an answer reaches the agent</span>",
            "<span class=\"cf-frame-number\">Figure 3</span> · <span class=\"cf-frame-name\">A nested stage</span>",
            "<span class=\"cf-frame-number\">Table 1</span> · <span class=\"cf-frame-name\">Answer limits</span>",
            "<figcaption class=\"cf-frame-caption\">An answer goes from the page",
            "<summary>Details</summary><p class=\"cf-frame-description\">Three boxes",
            "data-cf-entity=\"legend-1\"",
            "<a class=\"cf-ref\" href=\"#landing\">Figure 1</a>",
            "<a class=\"cf-ref\" href=\"#answer-flow\">Figure 2</a>",
            "<a class=\"cf-ref\" href=\"#limits\">Table 1</a>",
        ] {
            assert!(html.contains(expected), "interactive {interactive}: missing {expected}");
        }
        assert!(!html.contains("[fig:"), "a reference stayed literal");
    }
    // The rail and the note label name a framed table by its title, never
    // its id (AC-13 design review F1); a v1 table keeps the id.
    assert_eq!(find(&document, "limits").review_label(), "Answer limits");
    assert!(render(&document, true).contains("data-cf-block-label=\"Answer limits\""));
    let framing = crate::document::Framing::of(&document);
    assert_eq!(
        find(&document, "intro").canonical_review_text(&framing),
        "The paths are in Figure 1 and the stage in Figure 2; the limits are in Table 1."
    );
}

#[test]
fn a_v2_stage_carries_server_labelled_entities_in_the_live_page() {
    let document = supported("documents/v2-framed.json");
    let html = render(&document, true);
    for expected in [
        "data-cf-entity=\"browser\" data-cf-entity-label=\"Browser page\"",
        "data-cf-entity=\"submit-edge\" data-cf-entity-label=\"submit\"",
        "data-cf-entity=\"service\" data-cf-entity-label=\"Service\"",
        "data-cf-entity=\"ledger\" data-cf-entity-label=\"Ledger\"",
        "data-cf-entity-none",
    ] {
        assert!(html.contains(expected), "missing {expected}");
    }
}

#[test]
fn a_v1_html_title_is_visible_and_v1_references_stay_literal() {
    let document = supported("documents/v1-html-title.json");
    let html = render(&document, true);
    assert!(html.contains(
        "<p class=\"cf-frame-title\"><span class=\"cf-frame-name\">A stage title</span></p>"
    ));
    assert!(
        !html.contains("cf-frame-number"),
        "a v1 document is not numbered"
    );
    assert!(html.contains("See [fig:stage] literally."));
}

pub(crate) fn store() -> (tempfile::TempDir, SessionStore) {
    let temp = tempfile::tempdir().unwrap();
    let store = SessionStore::at_root(temp.path().join("project"), "key".to_string()).unwrap();
    (temp, store)
}

/// Build the envelope a fixture describes, against a live session: the
/// placeholders become the session id, the current revision and the block
/// digests the store computes.
fn envelope_from(
    fixture: &Value,
    session: Uuid,
    revision: u64,
    document: &PresentationDocument,
) -> FeedbackEnvelope {
    let mut text = serde_json::to_string(fixture).unwrap();
    for block in document.walk() {
        text = text.replace(
            &format!("{{{{digest:{}}}}}", block.id()),
            &block_digest(block),
        );
    }
    let mut value: Value = serde_json::from_str(&text).unwrap();
    value["session_id"] = Value::String(session.to_string());
    value["revision"] = Value::from(revision);
    value["actor"] = Value::String("operator".to_string());
    value["created_at_unix"] = Value::from(0);
    for note in value["notes"].as_array_mut().unwrap() {
        let block_id = note["block_id"].as_str().unwrap().to_string();
        note["block_label"] = Value::String(find(document, &block_id).review_label());
    }
    serde_json::from_value(value).unwrap()
}

#[test]
fn entity_anchor_fixtures_are_accepted_or_refused_with_their_codes() {
    let document = supported("documents/v2-framed.json");
    let mut seen = 0;
    for entry in index_entries("TSK-118") {
        let file = entry["file"].as_str().unwrap();
        if !file.starts_with("anchors/") {
            continue;
        }
        seen += 1;
        let (_temp, store) = store();
        let session = store
            .create(ParsedDocument::Supported(document.clone()))
            .unwrap();
        let envelope = envelope_from(&fixture_json(file), session.id, 1, &document);
        let result = store.append_feedback(envelope.clone());
        if entry["valid"] == true {
            result.unwrap_or_else(|error| panic!("{file}: {error}"));
            let stored = store.pending_feedback(session.id).unwrap();
            let selector = stored[0].notes[0].entity_selector.clone().unwrap();
            let unverified = entry["expect"].as_str().unwrap().contains("unverified");
            assert_eq!(
                selector.crop_check,
                unverified.then_some(CropCheck::Unverified),
                "{file}"
            );
            // The v1 stream never carries the entity selector (C1).
            let v1 = serde_json::to_string(&stored[0].v1_view()).unwrap();
            assert!(
                !v1.contains("entity_selector") && !v1.contains("crop_check"),
                "{file}"
            );
            assert!(v1.contains("element_selector"), "{file}");
            // An identical retry is the same receipt, not a conflict, even
            // though the server added crop_check to the stored note.
            assert!(!store.append_feedback(envelope).unwrap().created, "{file}");
        } else {
            let code = entry["expect"].as_str().unwrap();
            match result {
                Err(PresentError::Review { code: found, .. }) => assert_eq!(found, code, "{file}"),
                other => panic!("{file}: expected {code}, got {other:?}"),
            }
        }
    }
    assert_eq!(seen, 9);
}

#[test]
fn a_stale_review_is_refused_with_the_current_revision() {
    let document = supported("documents/v2-framed.json");
    let (_temp, store) = store();
    let session = store
        .create(ParsedDocument::Supported(document.clone()))
        .unwrap();
    let envelope = envelope_from(
        &fixture_json("anchors/entity-valid-stage.json"),
        session.id,
        7,
        &document,
    );
    match store.append_feedback(envelope) {
        Err(PresentError::Review { code, details, .. }) => {
            assert_eq!(code, "stale_revision");
            assert_eq!(details["current_revision"], 1);
        }
        other => panic!("{other:?}"),
    }
}

fn note(block: &str, label: &str) -> FeedbackNote {
    FeedbackNote {
        id: Uuid::new_v4(),
        block_id: block.to_string(),
        block_label: label.to_string(),
        kind: FeedbackKind::Comment,
        body: format!("A note on {block}."),
        selector: None,
        element_selector: None,
        region_selector: None,
        entity_selector: None,
        excerpt: None,
    }
}

fn entity_note(
    document: &PresentationDocument,
    block: &str,
    entity: &str,
    label: &str,
    path: &str,
    excerpt: Option<&str>,
) -> FeedbackNote {
    let target = find(document, block);
    let digest = block_digest(target);
    let mut note = note(block, &target.review_label());
    note.element_selector = Some(crate::state::ElementSelector {
        element_path: path.to_string(),
        tag_name: "g".to_string(),
        label: label.to_string(),
        block_digest: digest.clone(),
    });
    note.entity_selector = Some(crate::state::EntitySelector {
        entity_id: entity.to_string(),
        label: label.to_string(),
        block_digest: digest,
        variant: None,
        crop_box: None,
        crop_check: None,
    });
    note.excerpt = excerpt.map(|text| crate::state::FeedbackExcerpt {
        text: Some(text.to_string()),
        image: None,
    });
    note
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one session walks every re-anchor case in order"
)]
fn reanchor_cases_reach_their_pinned_states() {
    let cases = fixture_json("reanchor/cases.json");
    let first = supported("documents/v2-framed.json");
    let second = supported("reanchor/revision-2.json");
    let (_temp, store) = store();
    let session = store
        .create(ParsedDocument::Supported(first.clone()))
        .unwrap();
    let case = |name: &str| {
        cases["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["name"] == name)
            .unwrap_or_else(|| panic!("no case {name}"))
            .clone()
    };
    let text_case = case("text edited, fuzzy match");
    let text_selector: TextSelector =
        serde_json::from_value(text_case["note"]["selector"].clone()).unwrap();
    let below_case = case("text rewritten, fuzzy below threshold");
    let below_selector: TextSelector =
        serde_json::from_value(below_case["note"]["selector"].clone()).unwrap();
    let mut text_note = note("intro", &find(&first, "intro").review_label());
    text_note.selector = Some(text_selector);
    let mut below_note = note("outro", &find(&first, "outro").review_label());
    below_note.selector = Some(below_selector);
    let path = "svg:nth-of-type(1) > g:nth-of-type(2)";
    let notes = vec![
        entity_note(&first, "answer-flow", "service", "Service", path, None),
        entity_note(
            &first,
            "answer-flow",
            "submit-edge",
            "submit",
            path,
            Some("Ledger"),
        ),
        entity_note(&first, "answer-flow", "submit-edge", "submit", path, None),
        text_note,
        below_note,
        note("snippet", &find(&first, "snippet").review_label()),
    ];
    let envelope = FeedbackEnvelope {
        event_id: Uuid::new_v4(),
        session_id: session.id,
        revision: 1,
        actor: "operator".to_string(),
        verdict: FeedbackVerdict::ApproveWithNotes,
        instruction: None,
        notes,
        created_at_unix: 0,
    };
    store.append_feedback(envelope).unwrap();

    // Same revision: every note keeps its original target.
    let before = store.feedback_snapshot(session.id).unwrap();
    let anchors = before.items[0]
        .notes
        .iter()
        .map(|note| note.anchor.clone())
        .collect::<Vec<_>>();
    assert!(
        matches!(&anchors[0], FeedbackAnchor::EntityAnchored { entity_id } if entity_id == "service")
    );
    assert!(matches!(anchors[3], FeedbackAnchor::Anchored { .. }));
    assert!(matches!(anchors[5], FeedbackAnchor::Block { .. }));

    store
        .update_document(session.id, ParsedDocument::Supported(second))
        .unwrap();
    let after = store.feedback_snapshot(session.id).unwrap();
    let anchors = after.items[0]
        .notes
        .iter()
        .map(|note| note.anchor.clone())
        .collect::<Vec<_>>();
    assert_eq!(anchors.len(), 6, "no note is dropped");
    assert_eq!(
        anchors[0],
        FeedbackAnchor::EntityReanchored {
            entity_id: "service".to_string(),
            label_changed: true
        }
    );
    assert!(
        matches!(anchors[1], FeedbackAnchor::Reanchored { changed: true, .. }),
        "{:?}",
        anchors[1]
    );
    assert!(
        matches!(anchors[2], FeedbackAnchor::BlockFallback { .. }),
        "{:?}",
        anchors[2]
    );
    let expect = &text_case["expect"];
    assert_eq!(
        anchors[3],
        FeedbackAnchor::Reanchored {
            start_utf16: u32::try_from(expect["start_utf16"].as_u64().unwrap()).unwrap(),
            end_utf16: u32::try_from(expect["end_utf16"].as_u64().unwrap()).unwrap(),
            changed: true
        }
    );
    assert!(
        matches!(anchors[4], FeedbackAnchor::BlockFallback { .. }),
        "{:?}",
        anchors[4]
    );
    assert!(
        matches!(anchors[5], FeedbackAnchor::Orphaned { .. }),
        "{:?}",
        anchors[5]
    );
}

/// Property: entity ids and labels come from the declaration's content and
/// role, never from draw order (SPC-014 B2), over every rotation and the
/// reversal of each composition's draw list.
#[test]
fn figure_entity_ids_do_not_depend_on_draw_order() {
    let document = supported("documents/v2-framed.json");
    let Block::Figure { id, declaration } = find(&document, "landing").clone() else {
        panic!("landing is a figure block");
    };
    let table = |declaration: &Value| {
        let block = Block::Figure {
            id: id.clone(),
            declaration: declaration.clone(),
        };
        let mut entities = crate::entity::block_entities(&block)
            .unwrap()
            .into_iter()
            .map(|entity| (entity.id, entity.variant, entity.label))
            .collect::<Vec<_>>();
        entities.sort_by(|left, right| format!("{left:?}").cmp(&format!("{right:?}")));
        entities
    };
    let baseline = table(&declaration);
    assert!(baseline.len() > 30, "{} entities", baseline.len());
    for composition in ["wide", "narrow"] {
        let draw = declaration["figure"][composition]["draw"]
            .as_array()
            .unwrap()
            .clone();
        let mut orders = (0..draw.len())
            .map(|shift| {
                let mut rotated = draw.clone();
                rotated.rotate_left(shift);
                rotated
            })
            .collect::<Vec<_>>();
        orders.push(draw.iter().rev().cloned().collect());
        for order in orders {
            let mut permuted = declaration.clone();
            permuted["figure"][composition]["draw"] = Value::Array(order);
            assert_eq!(table(&permuted), baseline, "{composition}");
        }
    }
}

/// Property: re-anchoring notes to a revision whose blocks are unchanged
/// keeps every note on its original target, never marked changed.
#[test]
fn reanchoring_to_an_unchanged_revision_keeps_every_note() {
    let document = supported("documents/v2-framed.json");
    let (_temp, store) = store();
    let session = store
        .create(ParsedDocument::Supported(document.clone()))
        .unwrap();
    let mut notes = Vec::new();
    for file in [
        "anchors/entity-valid-figure.json",
        "anchors/entity-valid-stage.json",
        "anchors/entity-transformed-unverified.json",
        "anchors/entity-label-from-for.json",
    ] {
        let mut envelope = envelope_from(&fixture_json(file), session.id, 1, &document);
        let mut note = envelope.notes.remove(0);
        note.id = Uuid::new_v4();
        notes.push(note);
    }
    let intro = find(&document, "intro");
    let mut text = note("intro", &intro.review_label());
    let review = intro.canonical_review_text(&crate::document::Framing::of(&document));
    assert!(
        review.starts_with("The paths are in Figure 1 and"),
        "{review}"
    );
    let units = review.encode_utf16().collect::<Vec<_>>();
    text.selector = Some(TextSelector {
        exact: "paths".to_string(),
        prefix: "The ".to_string(),
        suffix: String::from_utf16(&units[9..41]).unwrap(),
        start_utf16: 4,
        end_utf16: 9,
    });
    notes.push(text);
    notes.push(note("outro", &find(&document, "outro").review_label()));
    let count = notes.len();
    store
        .append_feedback(FeedbackEnvelope {
            event_id: Uuid::new_v4(),
            session_id: session.id,
            revision: 1,
            actor: "operator".to_string(),
            verdict: FeedbackVerdict::ApproveWithNotes,
            instruction: None,
            notes,
            created_at_unix: 0,
        })
        .unwrap();
    store
        .update_document(session.id, ParsedDocument::Supported(document))
        .unwrap();
    let anchors = store.feedback_snapshot(session.id).unwrap().items[0]
        .notes
        .iter()
        .map(|note| note.anchor.clone())
        .collect::<Vec<_>>();
    assert_eq!(anchors.len(), count);
    for anchor in &anchors[..4] {
        assert!(
            matches!(anchor, FeedbackAnchor::EntityAnchored { .. }),
            "{anchor:?}"
        );
    }
    assert_eq!(
        anchors[4],
        FeedbackAnchor::Reanchored {
            start_utf16: 4,
            end_utf16: 9,
            changed: false
        }
    );
    assert!(matches!(anchors[5], FeedbackAnchor::Block { .. }));
}

/// An entity note may carry a PNG crop; the v1 view drops it, since v1
/// carries JPEG crops only, and keeps the excerpt text.
#[test]
fn an_entity_note_accepts_a_png_crop_that_the_v1_view_drops() {
    use base64::Engine as _;
    let document = supported("documents/v2-framed.json");
    let (_temp, store) = store();
    let session = store
        .create(ParsedDocument::Supported(document.clone()))
        .unwrap();
    let mut envelope = envelope_from(
        &fixture_json("anchors/entity-valid-figure.json"),
        session.id,
        1,
        &document,
    );
    let png = base64::engine::general_purpose::STANDARD.encode(crate::media::test_png(40, 20));
    envelope.notes[0].excerpt = Some(crate::state::FeedbackExcerpt {
        text: Some("Proposal".to_string()),
        image: Some(crate::state::FeedbackImage {
            media_type: "image/png".to_string(),
            data_base64: png.clone(),
        }),
    });
    store.append_feedback(envelope).unwrap();
    let stored = store.pending_feedback(session.id).unwrap();
    let full = serde_json::to_string(&stored[0]).unwrap();
    assert!(full.contains("image/png") && full.contains(&png));
    let v1 = stored[0].v1_view();
    let excerpt = v1.notes[0].excerpt.as_ref().unwrap();
    assert!(excerpt.image.is_none());
    assert_eq!(excerpt.text.as_deref(), Some("Proposal"));
}

/// The entity table of the framed figure, as `entities/landing.json` pins
/// it. The page's grammar module draws the same ids and labels (the web
/// check reads this file too), so the label rule of B2, implemented in Rust
/// and in the grammar, cannot drift apart.
#[test]
fn the_figure_entity_table_matches_the_shared_golden() {
    let document = supported("documents/v2-framed.json");
    let mut rows = crate::entity::block_entities(find(&document, "landing"))
        .unwrap()
        .into_iter()
        .map(|entity| {
            serde_json::json!({
                "id": entity.id,
                "variant": entity.variant.map(crate::entity::Variant::as_str),
                "label": entity.label,
            })
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| format!("{}|{}", row["variant"], row["id"]));
    let computed = Value::Array(rows);
    let golden = fixture_json("entities/landing.json");
    assert_eq!(
        golden,
        computed,
        "entities/landing.json is stale; computed:\n{}",
        serde_json::to_string_pretty(&computed).unwrap()
    );
}

/// The operator's delivery document at `schema_version` 2: the migrated flow
/// figure draws its marks as entities, and stage-registry names its groups,
/// arrows and file rectangles, each labelled by the B2 order.
#[test]
fn the_delivery_document_names_the_review_entities_it_was_built_with() {
    let document = supported("documents/delivery-v2.json");
    let entities = |id: &str| {
        crate::entity::block_entities(find(&document, id))
            .unwrap()
            .into_iter()
            .map(|entity| (entity.id, entity.label, entity.bounds.is_some()))
            .collect::<Vec<_>>()
    };
    let registry = entities("stage-registry");
    let expect = |id: &str, label: &str, bounded: bool| {
        assert!(
            registry
                .iter()
                .any(|(found, text, has)| found == id && text == label && *has == bounded),
            "{id}: {registry:?}"
        );
    };
    // A group holding text cannot be bounded by the server (B4).
    expect("agent-a", "Agent A one laptop", false);
    expect("registry", "codeflow/registry branch on GitHub", false);
    expect("read-a", "highest number ever issued: 99", true);
    expect(
        "reject-b",
        "rejected: someone pushed since you looked",
        true,
    );
    expect("push-b-again", "add ids/TSK/101, push", true);
    expect("file-a", "writes TSK-100.md", true);
    assert_eq!(registry.len(), 12, "{registry:?}");
    let flow = entities("stage-flow");
    assert!(
        flow.iter()
            .any(|(id, label, _)| id == "e3" && label == "n : n"),
        "{flow:?}"
    );
    assert!(
        flow.iter()
            .any(|(id, label, _)| id == "legend-missing"
                && label == "A link that does not exist today"),
        "{flow:?}"
    );
}

/// `crop_check` is the service's finding; a page that sets it is refused.
#[test]
fn a_page_cannot_set_crop_check() {
    let document = supported("documents/v2-framed.json");
    let (_temp, store) = store();
    let session = store
        .create(ParsedDocument::Supported(document.clone()))
        .unwrap();
    let mut envelope = envelope_from(
        &fixture_json("anchors/entity-valid-figure.json"),
        session.id,
        1,
        &document,
    );
    envelope.notes[0]
        .entity_selector
        .as_mut()
        .unwrap()
        .crop_check = Some(CropCheck::Unverified);
    assert!(store.append_feedback(envelope).is_err());
}

/// Path data the server cannot read, non-ASCII included, leaves the part
/// unverified: validation, the entity table, rendering and a submitted note
/// all complete without a panic (T118-1).
#[test]
fn unreadable_path_data_leaves_the_part_unverified() {
    let mut value: Value = fixture_json("documents/v2-framed.json");
    let stage = value["blocks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|block| block["id"] == "answer-flow")
        .unwrap();
    let html = stage["html"].as_str().unwrap().to_string();
    stage["html"] = Value::String(html.replace(
        "</svg>",
        "<path data-cf-target='odd-path' data-cf-label='Odd path' d='M0 0 \u{2603} L\u{e9}1 2'/></svg>",
    ));
    let document = match parse_document(&serde_json::to_vec(&value).unwrap()).unwrap() {
        ParsedDocument::Supported(document) => document,
        ParsedDocument::Unsupported { .. } => panic!("unsupported"),
    };
    let block = find(&document, "answer-flow");
    let entity = crate::entity::block_entities(block)
        .unwrap()
        .into_iter()
        .find(|entity| entity.id == "odd-path")
        .expect("the odd path is an entity");
    assert!(entity.bounds.is_none());
    assert!(render(&document, true).contains("data-cf-entity=\"odd-path\""));
    let (_temp, store) = store();
    let session = store
        .create(ParsedDocument::Supported(document.clone()))
        .unwrap();
    let mut envelope = envelope_from(
        &fixture_json("anchors/entity-valid-stage.json"),
        session.id,
        1,
        &document,
    );
    let selector = envelope.notes[0].entity_selector.as_mut().unwrap();
    selector.entity_id = "odd-path".to_string();
    selector.label = "Odd path".to_string();
    store.append_feedback(envelope).unwrap();
    let stored = store.pending_feedback(session.id).unwrap();
    assert_eq!(
        stored[0].notes[0]
            .entity_selector
            .as_ref()
            .unwrap()
            .crop_check,
        Some(CropCheck::Unverified)
    );
}

/// A PNG crop is checked whole on an entity note: a signature alone, a
/// truncated file and a file declaring a huge raster are refused (T118-4).
#[test]
fn an_entity_note_refuses_a_broken_or_oversized_png_crop() {
    use base64::Engine as _;
    let document = supported("documents/v2-framed.json");
    let (_temp, store) = store();
    let session = store
        .create(ParsedDocument::Supported(document.clone()))
        .unwrap();
    let valid = crate::media::test_png(40, 20);
    let mut huge = crate::media::test_png(1, 1);
    huge[16..20].copy_from_slice(&8192_u32.to_be_bytes());
    huge[20..24].copy_from_slice(&8192_u32.to_be_bytes());
    let crc = crate::media::crc32(&huge[12..29]);
    huge[29..33].copy_from_slice(&crc.to_be_bytes());
    assert_eq!(
        crate::media::crop_png_refusal(&huge),
        Some("is larger than a crop may be")
    );
    for bytes in [
        b"\x89PNG\r\n\x1a\n".to_vec(),
        valid[..valid.len() - 5].to_vec(),
        huge,
        crate::media::test_png(481, 10),
        // Round 2: a CRC-correct grey PNG declaring bit depth 3.
        crate::media::test_png_with_header_byte(24, 3),
    ] {
        let mut envelope = envelope_from(
            &fixture_json("anchors/entity-valid-figure.json"),
            session.id,
            1,
            &document,
        );
        envelope.notes[0].excerpt = Some(crate::state::FeedbackExcerpt {
            text: None,
            image: Some(crate::state::FeedbackImage {
                media_type: "image/png".to_string(),
                data_base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
            }),
        });
        assert!(
            store.append_feedback(envelope).is_err(),
            "{} bytes",
            bytes.len()
        );
    }
    assert!(store.pending_feedback(session.id).unwrap().is_empty());
}

/// The worst case at the accepted fuzzy bounds reaches the block step with a
/// notice at once, instead of a long search (T118-5), and a second snapshot
/// of the same revision reuses the result.
#[test]
fn a_quote_search_past_its_budget_falls_to_the_block_with_a_notice() {
    let narrative = |markdown: String| {
        serde_json::json!({
            "schema_version": 1,
            "title": "Budget",
            "blocks": [{"type": "narrative", "id": "long", "markdown": markdown}]
        })
    };
    let parse = |value: Value| match parse_document(&serde_json::to_vec(&value).unwrap()).unwrap() {
        ParsedDocument::Supported(document) => document,
        ParsedDocument::Unsupported { .. } => panic!("unsupported"),
    };
    let quote = "a".repeat(crate::limits::MAX_FUZZY_QUOTE_UTF16);
    let first = parse(narrative(format!("{quote} end")));
    let (_temp, store) = store();
    let session = store
        .create(ParsedDocument::Supported(first.clone()))
        .unwrap();
    let mut long = note("long", &find(&first, "long").review_label());
    long.selector = Some(TextSelector {
        exact: quote.clone(),
        prefix: String::new(),
        suffix: " end".to_string(),
        start_utf16: 0,
        end_utf16: u32::try_from(quote.len()).unwrap(),
    });
    store
        .append_feedback(FeedbackEnvelope {
            event_id: Uuid::new_v4(),
            session_id: session.id,
            revision: 1,
            actor: "operator".to_string(),
            verdict: FeedbackVerdict::ApproveWithNotes,
            instruction: None,
            notes: vec![long],
            created_at_unix: 0,
        })
        .unwrap();
    let edited = "a ".repeat(crate::limits::MAX_FUZZY_TEXT_UTF16 / 2 - 4);
    store
        .update_document(
            session.id,
            ParsedDocument::Supported(parse(narrative(edited))),
        )
        .unwrap();
    let started = std::time::Instant::now();
    let anchor = store.feedback_snapshot(session.id).unwrap().items[0].notes[0]
        .anchor
        .clone();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    match &anchor {
        FeedbackAnchor::BlockFallback { block_id, reason } => {
            assert_eq!(block_id, "long");
            assert!(reason.contains("too large to search"), "{reason}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        store.feedback_snapshot(session.id).unwrap().items[0].notes[0].anchor,
        anchor
    );
}

/// QA defect 8: a whole-document note stays positioned across revisions; a
/// part of the document stays pinned to its revision with its reason.
#[test]
fn a_whole_document_note_survives_a_revision() {
    let first = supported("documents/v2-framed.json");
    // The revision also drops `intro`, the first block, which carried the
    // note's digest: the document is still there, so the note holds.
    let mut second = supported("reanchor/revision-2.json");
    second.blocks.retain(|block| block.id() != "intro");
    let (_temp, store) = store();
    let session = store
        .create(ParsedDocument::Supported(first.clone()))
        .unwrap();
    let region = |width_ppm: u32| {
        let mut note = note("intro", &find(&first, "intro").review_label());
        note.region_selector = Some(crate::state::RegionSelector {
            scope: crate::state::RegionScope::Document,
            anchor_id: "document".to_string(),
            block_digest: crate::state::block_digest(find(&first, "intro")),
            x_ppm: 0,
            y_ppm: 0,
            width_ppm,
            height_ppm: crate::limits::REGION_COORDINATE_SCALE,
            capture_width_px: 800,
            capture_height_px: 4000,
        });
        note
    };
    let envelope = FeedbackEnvelope {
        event_id: Uuid::new_v4(),
        session_id: session.id,
        revision: 1,
        actor: "operator".to_string(),
        verdict: FeedbackVerdict::ApproveWithNotes,
        instruction: None,
        notes: vec![
            region(crate::limits::REGION_COORDINATE_SCALE),
            region(500_000),
        ],
        created_at_unix: 0,
    };
    store.append_feedback(envelope).unwrap();
    store
        .update_document(session.id, ParsedDocument::Supported(second))
        .unwrap();
    let after = store.feedback_snapshot(session.id).unwrap();
    let anchors: Vec<_> = after.items[0]
        .notes
        .iter()
        .map(|note| note.anchor.clone())
        .collect();
    assert!(
        matches!(anchors[0], FeedbackAnchor::RegionReanchored { .. }),
        "{:?}",
        anchors[0]
    );
    assert!(
        matches!(anchors[1], FeedbackAnchor::Orphaned { .. }),
        "{:?}",
        anchors[1]
    );
}

/// SPC-014 B1 and C1 across the TSK-071 separator change: a review stored
/// before the parts of a block were joined with a line break carries offsets
/// into the old review text (`FirstSecond`). Reopening that same revision
/// never trusts them blindly: a quote inside one part is found at its new
/// place, and a quote across two parts is found near it or falls back to its
/// block, never at stale offsets that select other text.
#[test]
fn a_stored_quote_from_before_the_separator_change_reanchors() {
    const DOCUMENT: &str = include_str!("../tests/fixtures/separator-migration/document.json");
    // One received review, its offsets taken in the pre-change review text.
    const EVENTS: &str = include_str!("../tests/fixtures/separator-migration/events.jsonl");
    const CAPTURED_SESSION: &str = "5e9a7c1e-0d2b-4f5a-9c3e-7b1d2f4a6c80";
    let document = match parse_document(DOCUMENT.as_bytes()).unwrap() {
        ParsedDocument::Supported(document) => document,
        ParsedDocument::Unsupported { .. } => panic!("the fixture parsed as unsupported"),
    };
    let framing = crate::document::Framing::default();
    let current = |id: &str| find(&document, id).canonical_review_text(&framing);
    assert_eq!(current("intro"), "First\nSecond");
    assert_eq!(
        current("checks"),
        "Linux Chrome run\none crop was a sliver\nmacOS run"
    );
    let (_temp, store) = store();
    let session = store
        .create(ParsedDocument::Supported(document.clone()))
        .unwrap();
    let events = store
        .root()
        .join("sessions")
        .join(session.id.to_string())
        .join("events.jsonl");
    std::fs::write(
        &events,
        EVENTS.replace(CAPTURED_SESSION, &session.id.to_string()),
    )
    .unwrap();

    let snapshot = store.feedback_snapshot(session.id).unwrap();
    let notes = &snapshot.items[0].notes;
    let selected = |block: &str, anchor: &FeedbackAnchor| match anchor {
        FeedbackAnchor::Anchored {
            start_utf16,
            end_utf16,
        }
        | FeedbackAnchor::Reanchored {
            start_utf16,
            end_utf16,
            ..
        } => {
            let units: Vec<u16> = current(block).encode_utf16().collect();
            Some(String::from_utf16(&units[*start_utf16 as usize..*end_utf16 as usize]).unwrap())
        }
        _ => None,
    };
    // Inside one part: the old offsets now select "\nSecon" and "r\nmacOS r".
    for (note, block, quote) in [
        (&notes[0], "intro", "Second"),
        (&notes[1], "checks", "macOS run"),
    ] {
        assert_eq!(
            note.quote.as_deref(),
            Some(quote),
            "the stored quote survives"
        );
        assert_eq!(
            selected(block, &note.anchor).as_deref(),
            Some(quote),
            "{quote}: {:?}",
            note.anchor
        );
    }
    // Across two parts: the old quote no longer occurs as it was, so the
    // fuzzy step finds it with its new separator and marks it changed.
    let across = &notes[2];
    assert_eq!(across.quote.as_deref(), Some("runone crop"));
    assert!(
        matches!(
            across.anchor,
            FeedbackAnchor::Reanchored { changed: true, .. }
        ),
        "{:?}",
        across.anchor
    );
    assert_eq!(
        selected("checks", &across.anchor).as_deref(),
        Some("run\none crop")
    );
    // A diff quote stored with its line's screen-reader label and marker
    // ("Added: +"), which the review text no longer holds (round 3): it is
    // read without them and found at the same line.
    assert_eq!(
        current("change"),
        "One change.\n--- a/x.rs\n+++ b/x.rs\n fn f() {\n    let n = 1;\n    let n = 2;\n }\n"
    );
    let diff = &notes[3];
    assert_eq!(diff.quote.as_deref(), Some("Added: +    let n = 2;"));
    assert!(
        matches!(diff.anchor, FeedbackAnchor::Reanchored { .. }),
        "{:?}",
        diff.anchor
    );
    assert_eq!(
        selected("change", &diff.anchor).as_deref(),
        Some("    let n = 2;")
    );
}

/// A text selector over `start..end` of `text`, with the 32-unit context the
/// page stores around it.
fn stored_selector(text: &str, start: usize, end: usize) -> TextSelector {
    let units: Vec<u16> = text.encode_utf16().collect();
    let part = |from: usize, to: usize| String::from_utf16(&units[from..to]).unwrap();
    TextSelector {
        exact: part(start, end),
        prefix: part(start.saturating_sub(32), start),
        suffix: part(end, (end + 32).min(units.len())),
        start_utf16: u32::try_from(start).unwrap(),
        end_utf16: u32::try_from(end).unwrap(),
    }
}

/// Stores one review of `notes` on revision 1, then rewrites the selectors
/// named in `stored` as an earlier runtime wrote them, which the store would
/// no longer accept from a page.
fn store_review(
    store: &SessionStore,
    session: Uuid,
    notes: Vec<FeedbackNote>,
    stored: &[(usize, TextSelector)],
) {
    store
        .append_feedback(FeedbackEnvelope {
            event_id: Uuid::new_v4(),
            session_id: session,
            revision: 1,
            actor: "operator".to_string(),
            verdict: FeedbackVerdict::ApproveWithNotes,
            instruction: None,
            notes,
            created_at_unix: 0,
        })
        .unwrap();
    let events = store
        .root()
        .join("sessions")
        .join(session.to_string())
        .join("events.jsonl");
    let mut received: Value =
        serde_json::from_str(&std::fs::read_to_string(&events).unwrap()).unwrap();
    for (index, selector) in stored {
        received["envelope"]["notes"][*index]["selector"] = serde_json::to_value(selector).unwrap();
    }
    std::fs::write(
        &events,
        format!("{}\n", serde_json::to_string(&received).unwrap()),
    )
    .unwrap();
}

/// The text of `block` in `document` that a text anchor selects.
fn anchored_text(
    document: &PresentationDocument,
    block: &str,
    anchor: &FeedbackAnchor,
) -> Option<String> {
    let (FeedbackAnchor::Anchored {
        start_utf16,
        end_utf16,
    }
    | FeedbackAnchor::Reanchored {
        start_utf16,
        end_utf16,
        ..
    }) = anchor
    else {
        return None;
    };
    let units: Vec<u16> = find(document, block)
        .canonical_review_text(&crate::document::Framing::default())
        .encode_utf16()
        .collect();
    Some(String::from_utf16(&units[*start_utf16 as usize..*end_utf16 as usize]).unwrap())
}

/// SPC-014 B1 across the TSK-071 diff change (review C071-R3-1): a quote
/// stored while a changed line began with "Added: +" or "Removed: -" is
/// found in the diff's text as it was then and carried over, so only the
/// generated label and marker drop out. A line whose own words read like a
/// label keeps them, wherever the label falls between quote and context; a
/// note taken after the change anchors as it was taken; a quote of nothing
/// but a label falls back to its block.
#[test]
fn a_stored_diff_quote_keeps_line_text_that_reads_like_a_label() {
    let document: PresentationDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "title": "Literal diff content",
        "blocks": [{"type": "diff", "id": "change", "diff": "+Added: +foo\n-bar\n baz"}]
    }))
    .unwrap();
    let canonical =
        find(&document, "change").canonical_review_text(&crate::document::Framing::default());
    assert_eq!(canonical, "Added: +foo\nbar\n baz\n");
    // The diff's review text in 3.0.x, and each note's range in the text it
    // was taken in.
    let legacy = "Added: +Added: +foo\nRemoved: -bar\n baz\n";
    let cases = [
        stored_selector(legacy, 0, 19),
        stored_selector(legacy, 7, 19),
        stored_selector(legacy, 16, 25),
        stored_selector(&canonical, 0, 11),
        stored_selector(legacy, 20, 30),
    ];
    let expected = [
        Some("Added: +foo"),
        Some("Added: +foo"),
        Some("foo\n"),
        Some("Added: +foo"),
        None,
    ];
    let (_temp, store) = store();
    let session = store
        .create(ParsedDocument::Supported(document.clone()))
        .unwrap();
    let placeholder = || {
        let mut placed = note("change", &find(&document, "change").review_label());
        placed.selector = Some(stored_selector(&canonical, 8, 11));
        placed
    };
    let stored: Vec<_> = cases.iter().cloned().enumerate().collect();
    store_review(
        &store,
        session.id,
        cases.iter().map(|_| placeholder()).collect(),
        &stored,
    );

    let snapshot = store.feedback_snapshot(session.id).unwrap();
    let notes = &snapshot.items[0].notes;
    for ((note, selector), expected) in notes.iter().zip(&cases).zip(expected) {
        let exact = selector.exact.as_str();
        assert_eq!(
            note.quote.as_deref(),
            Some(exact),
            "the stored quote survives"
        );
        assert!(
            matches!(
                note.anchor,
                FeedbackAnchor::Anchored { .. }
                    | FeedbackAnchor::Reanchored { changed: false, .. }
                    | FeedbackAnchor::BlockFallback { .. }
            ),
            "{exact:?}: {:?}",
            note.anchor
        );
        assert_eq!(
            anchored_text(&document, "change", &note.anchor).as_deref(),
            expected,
            "{exact:?}: {:?}",
            note.anchor
        );
    }
    // A note taken after the change is read at its own offsets.
    assert!(
        matches!(notes[3].anchor, FeedbackAnchor::Anchored { .. }),
        "{:?}",
        notes[3].anchor
    );
}

/// SPC-014 B1 for diff quotes across an update (review C071-R4-1): a quote is
/// read in the old labelled text only when the revision it was taken in held
/// it there. A quote of literal "Added: +" words taken after TSK-071 is read
/// as the text is now: it holds while its words do and falls back to its
/// block when they are gone; a quote taken in the old text is carried over.
#[test]
fn a_diff_quote_is_read_in_the_text_of_its_own_revision() {
    let revision = |kept: &str, gone: &str| -> PresentationDocument {
        serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "title": "Diff quotes across an update",
            "blocks": [
                {"type": "diff", "id": "kept", "diff": kept},
                {"type": "diff", "id": "gone", "diff": gone}
            ]
        }))
        .unwrap()
    };
    let first = revision("+Added: +foo", "+Added: +foo");
    let second = revision("+Added: +foo\n+bar", "+foo");
    let now = find(&first, "kept").canonical_review_text(&crate::document::Framing::default());
    assert_eq!(now, "Added: +foo\n");
    let taken = |block: &str| {
        let mut taken = note(block, &find(&first, block).review_label());
        taken.selector = Some(stored_selector(&now, 0, 11));
        taken
    };
    let (_temp, store) = store();
    let session = store
        .create(ParsedDocument::Supported(first.clone()))
        .unwrap();
    // The third note as 3.0.x stored it, in the old text.
    store_review(
        &store,
        session.id,
        vec![taken("gone"), taken("kept"), taken("kept")],
        &[(2, stored_selector("Added: +Added: +foo\n", 0, 19))],
    );
    store
        .update_document(session.id, ParsedDocument::Supported(second.clone()))
        .unwrap();

    let snapshot = store.feedback_snapshot(session.id).unwrap();
    let notes = &snapshot.items[0].notes;
    assert!(
        matches!(notes[0].anchor, FeedbackAnchor::BlockFallback { .. }),
        "the literal words are gone: {:?}",
        notes[0].anchor
    );
    for (index, quote) in [(1, "Added: +foo"), (2, "Added: +Added: +foo")] {
        assert_eq!(notes[index].quote.as_deref(), Some(quote));
        assert_eq!(
            anchored_text(&second, "kept", &notes[index].anchor).as_deref(),
            Some("Added: +foo"),
            "{quote}: {:?}",
            notes[index].anchor
        );
    }
}

/// SPC-014 B1 for a diff note on a revision that also held a removed diagram
/// (review C071-R5-1): the retired revision still gives the diff its 3.0.x
/// text, so the note re-anchors after conversion, while the note on the
/// diagram orphans with its named reason even though its id is reused.
#[test]
fn a_diff_note_beside_a_retired_diagram_still_reanchors() {
    let diff = serde_json::json!({"type": "diff", "id": "change", "diff": "+foo\n bar"});
    let mut revision: Value =
        serde_json::from_str(crate::state::retired_fixture::REVISION).unwrap();
    revision["content"]["document"]["blocks"]
        .as_array_mut()
        .unwrap()
        .push(diff.clone());
    let (_temp, store) = store();
    let id =
        crate::state::retired_fixture::install(&store, &serde_json::to_string(&revision).unwrap());
    assert!(matches!(
        store.current_revision(id).unwrap().content,
        crate::state::RevisionContent::Retired { .. }
    ));
    let events = store
        .root()
        .join("sessions")
        .join(id.to_string())
        .join("events.jsonl");
    let mut received: Value =
        serde_json::from_str(&std::fs::read_to_string(&events).unwrap()).unwrap();
    let block: Block = serde_json::from_value(diff.clone()).unwrap();
    let mut on_diff = note("change", &block.review_label());
    on_diff.selector = Some(stored_selector("Added: +foo\n bar\n", 0, 11));
    received["envelope"]["notes"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::to_value(on_diff).unwrap());
    std::fs::write(
        &events,
        format!("{}\n", serde_json::to_string(&received).unwrap()),
    )
    .unwrap();
    let converted: PresentationDocument = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "title": "Converted document",
        "blocks": [{"type": "narrative", "id": "flow", "markdown": "The flow, converted."}, diff]
    }))
    .unwrap();
    store
        .update_document(id, ParsedDocument::Supported(converted.clone()))
        .unwrap();

    let snapshot = store.feedback_snapshot(id).unwrap();
    let notes = &snapshot.items[0].notes;
    assert!(
        matches!(&notes[0].anchor, FeedbackAnchor::Orphaned { reason } if reason.contains("diagram block flow")),
        "{:?}",
        notes[0].anchor
    );
    assert!(
        matches!(
            notes[1].anchor,
            FeedbackAnchor::Reanchored { changed: false, .. }
        ),
        "{:?}",
        notes[1].anchor
    );
    assert_eq!(
        anchored_text(&converted, "change", &notes[1].anchor).as_deref(),
        Some("foo")
    );
}
