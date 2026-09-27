//! The SPC-014 contract fixtures whose first passing task is TSK-118:
//! documents, entity anchors and re-anchoring. Fixtures live under
//! `tests/fixtures/contract-v2/` and are indexed by its `index.json`.

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

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/contract-v2")
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture_root().join(name)).unwrap_or_else(|error| panic!("{name}: {error}"))
}

fn fixture_json(name: &str) -> Value {
    serde_json::from_slice(&fixture_bytes(name)).unwrap()
}

fn index_entries(task: &str) -> Vec<Value> {
    let index = fixture_json("index.json");
    index["fixtures"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["first_passing_task"] == task)
        .cloned()
        .collect()
}

fn supported(name: &str) -> PresentationDocument {
    match parse_document(&fixture_bytes(name)).unwrap() {
        ParsedDocument::Supported(document) => document,
        ParsedDocument::Unsupported { .. } => panic!("{name} parsed as unsupported"),
    }
}

fn find<'a>(document: &'a PresentationDocument, id: &str) -> &'a Block {
    document
        .walk()
        .into_iter()
        .find(|block| block.id() == id)
        .unwrap_or_else(|| panic!("no block {id}"))
}

fn render(document: &PresentationDocument, interactive: bool) -> String {
    render_document(
        document,
        &RenderOptions {
            session_id: "00000000-0000-4000-8000-000000000000",
            revision: 1,
            event_sequence: 0,
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

fn store() -> (tempfile::TempDir, SessionStore) {
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
