use crate::{
    answer_contract_tests::FormsSession,
    contract_tests::fixture_json,
    conversation::ThreadRequest,
    delivery::{EventFilter, EventKind},
    document::{parse_document, ParsedDocument},
    responses::ResponseEvent,
    state::{
        FeedbackEnvelope, FeedbackExcerpt, FeedbackKind, FeedbackNote, FeedbackResolution,
        FeedbackVerdict,
    },
};
use serde_json::json;
use uuid::Uuid;

fn review(session: &FormsSession) -> (Uuid, Uuid) {
    let event = Uuid::new_v4();
    let note = Uuid::new_v4();
    session
        .store
        .append_feedback(FeedbackEnvelope {
            event_id: event,
            session_id: session.id,
            revision: 1,
            actor: "operator".into(),
            verdict: FeedbackVerdict::ApproveWithNotes,
            instruction: None,
            created_at_unix: 0,
            notes: vec![FeedbackNote {
                id: note,
                block_id: "store-choice".into(),
                block_label: s_label(&session.document),
                kind: FeedbackKind::Comment,
                body: "private deleted body".into(),
                selector: None,
                element_selector: None,
                region_selector: None,
                entity_selector: None,
                excerpt: Some(FeedbackExcerpt {
                    text: Some("private deleted quote".into()),
                    image: None,
                }),
            }],
        })
        .unwrap();
    (event, note)
}
fn request(session: &FormsSession, target: Uuid, note: Option<Uuid>) -> ThreadRequest {
    ThreadRequest {
        session_id: session.id,
        target,
        note_id: note,
        revision: Some(1),
    }
}

#[test]
fn conversation_tombstone_redacts_every_projection_but_not_the_private_ledger() {
    let s = FormsSession::open();
    let (event, note) = review(&s);
    let reply = s
        .store
        .reply(s.id, event, Some(note), "private deleted reply")
        .unwrap();
    assert!(!s
        .store
        .feedback_lines(s.id, &EventFilter::default())
        .unwrap()
        .iter()
        .any(|l| l.event_id == reply));
    let deleted = s
        .store
        .change_thread(s.id, &request(&s, event, Some(note)), true)
        .unwrap();
    assert_eq!(
        s.store
            .change_thread(s.id, &request(&s, event, Some(note)), true)
            .unwrap(),
        deleted
    );
    let projections = [
        serde_json::to_string(&s.store.history(s.id).unwrap()).unwrap(),
        serde_json::to_string(
            &s.store
                .feedback_lines(s.id, &EventFilter::default())
                .unwrap(),
        )
        .unwrap(),
        serde_json::to_string(&s.store.pending_feedback(s.id).unwrap()).unwrap(),
        serde_json::to_string(&s.store.feedback_snapshot(s.id).unwrap()).unwrap(),
        serde_json::to_string(&s.store.conversation(s.id).unwrap()).unwrap(),
        serde_json::to_string(&s.store.responses(s.id).unwrap()).unwrap(),
        serde_json::to_string(&s.store.diff(s.id, 1, 1).unwrap()).unwrap(),
    ];
    for output in projections {
        assert!(!output.contains("private deleted"), "{output}");
    }
    let path = s.store.root().join("export.html");
    crate::export::export_session_with_notes(
        &s.store,
        s.id,
        &path,
        crate::export::ExportTheme::Slate,
        crate::export::ExportMode::Light,
    )
    .unwrap();
    let exported = std::fs::read_to_string(path).unwrap();
    assert!(exported.contains("Deleted by the reviewer"));
    assert!(!exported.contains("private deleted"));
    assert!(String::from_utf8(s.ledger_bytes().unwrap())
        .unwrap()
        .contains("private deleted reply"));
    assert!(std::fs::read_to_string(
        s.store
            .root()
            .join("sessions")
            .join(s.id.to_string())
            .join("events.jsonl")
    )
    .unwrap()
    .contains("private deleted body"));
    assert!(s.store.reply(s.id, event, Some(note), "later").is_err());
    assert!(s
        .store
        .change_thread(s.id, &request(&s, event, Some(note)), false)
        .is_err());
}

#[test]
fn conversation_reopen_delivers_once_and_keeps_revision_and_target() {
    let s = FormsSession::open();
    let (event, note) = review(&s);
    assert!(s
        .store
        .change_thread(s.id, &request(&s, event, Some(note)), false)
        .is_err());
    s.store.deliver(s.id, &[event]).unwrap();
    s.store
        .resolve_feedback(s.id, event, 2, FeedbackResolution::Addressed)
        .unwrap();
    let reopened = s
        .store
        .change_thread(s.id, &request(&s, event, Some(note)), false)
        .unwrap();
    assert_eq!(
        s.store
            .change_thread(s.id, &request(&s, event, Some(note)), false)
            .unwrap(),
        reopened
    );
    assert!(s.store.acknowledge(s.id, reopened).is_err());
    let pending = s
        .store
        .feedback_lines(
            s.id,
            &EventFilter {
                kind: Some(EventKind::Reopen),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].revision, 1);
    s.store.deliver(s.id, &[reopened, reopened]).unwrap();
    assert!(s.store.acknowledge(s.id, reopened).unwrap());
    assert!(!s.store.acknowledge(s.id, reopened).unwrap());
    let records = s.store.responses(s.id).unwrap();
    assert_eq!(
        records
            .iter()
            .filter(|r| matches!(r, ResponseEvent::Reopen(_)))
            .count(),
        1
    );
    // An acknowledged reopen may be reopened again, preserving the old line.
    assert_ne!(
        s.store
            .change_thread(s.id, &request(&s, event, Some(note)), false)
            .unwrap(),
        reopened
    );
}

#[test]
fn conversation_answer_reply_reopen_and_refusals() {
    let s = FormsSession::open();
    let body = s.body(&fixture_json("answers/submit-valid.json"), &[]);
    let answer = s.submit(&body).unwrap().answer_id;
    s.store.reply(s.id, answer, None, "Answer noted").unwrap();
    assert!(s
        .store
        .reply(s.id, answer, Some(Uuid::new_v4()), "unknown note")
        .is_err());
    assert!(s
        .store
        .reply(s.id, Uuid::new_v4(), None, "foreign")
        .is_err());
    assert!(s.store.reply(s.id, answer, None, "").is_err());
    assert!(s
        .store
        .reply(s.id, answer, None, &"x".repeat(16385))
        .is_err());
    s.store.deliver(s.id, &[answer]).unwrap();
    s.store.acknowledge(s.id, answer).unwrap();
    s.store
        .change_thread(s.id, &request(&s, answer, None), false)
        .unwrap();
    assert!(s
        .store
        .change_thread(s.id, &request(&s, answer, None), true)
        .is_err());
    s.store
        .update_document(s.id, ParsedDocument::Supported(s.document.clone()))
        .unwrap();
    assert!(s
        .store
        .change_thread(s.id, &request(&s, answer, None), false)
        .is_err());
    s.store
        .reply(s.id, answer, None, "Revision two reply")
        .unwrap();
    assert!(s
        .store
        .responses(s.id)
        .unwrap()
        .iter()
        .any(|e| matches!(e,ResponseEvent::Reply(r) if r.transition.revision == 2)));
    s.store.close(s.id).unwrap();
    assert!(s.store.reply(s.id, answer, None, "closed").is_err());
}

#[test]
fn conversation_diff_carries_notes_and_answers_and_matches_golden() {
    let s = FormsSession::open();
    let (_, _) = review(&s);
    s.submit(&s.body(&fixture_json("answers/submit-valid.json"), &[]))
        .unwrap();
    let mut document = serde_json::to_value(&s.document).unwrap();
    document["blocks"]
        .as_array_mut()
        .unwrap()
        .retain(|b| b["id"] != "d-scope");
    document["blocks"][0]["title"] = json!("Changed question");
    document["blocks"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"narrative","id":"new","markdown":"New block"}));
    s.store
        .update_document(
            s.id,
            parse_document(&serde_json::to_vec(&document).unwrap()).unwrap(),
        )
        .unwrap();
    let diff = s.store.diff(s.id, 1, 2).unwrap();
    let summary: Vec<_> = diff["blocks"].as_array().unwrap().iter().map(|b| json!({"id":b["id"],"status":b["status"],
        "notes":b["notes"].as_array().unwrap().iter().map(|n| json!({"carried":n["carried"],"source_revision":n["source_revision"],"body":n["note"]["body"]})).collect::<Vec<_>>(),
        "answers":b["answers"].as_array().unwrap().iter().map(|a| json!({"carried":a["carried"],"revision":a["answer"]["revision"],"form":a["answer"]["form_id"]})).collect::<Vec<_>>() })).collect();
    assert_eq!(json!(summary), fixture_json("revisions/diff-golden.json"));
    assert!(s.store.diff(s.id, 1, 99).is_err());
}

fn s_label(document: &crate::document::PresentationDocument) -> String {
    document
        .walk()
        .into_iter()
        .find(|b| b.id() == "store-choice")
        .unwrap()
        .review_label()
}
