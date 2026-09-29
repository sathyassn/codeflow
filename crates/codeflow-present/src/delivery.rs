//! Delivery of stored events to the agent (SPC-014 B8, I4).
//!
//! Reviews live in `events.jsonl` and answers in `responses.jsonl`. This
//! module reads both under one session lock and gives each event its v2
//! feedback line and its status: pending until a `feedback` read delivers it,
//! then delivered, then acknowledged once the agent runs `present ack`. A
//! review's delivery stays in `events.jsonl`, so the v1 and v2 streams never
//! deliver the same review twice; the delivery of an answer and every
//! acknowledgment are lines of `responses.jsonl`. Listing never changes a
//! state.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::{
    error::{PresentError, Result},
    form::{Outcome, QuestionSnapshot},
    responses::{AnswerRecord, Ledger, ResponseEvent, StateRecord},
    state::{
        note_anchor, now_unix, FeedbackAnchor, FeedbackEnvelope, FeedbackEvent, FeedbackNote,
        FeedbackVerdict, SessionStore, SourceRevisions,
    },
};

/// Where an event is on its way to the agent (B8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    Pending,
    Delivered,
    Acknowledged,
}

impl DeliveryStatus {
    /// The word the page uses for an answer in this state: a pending answer
    /// is stored, waiting for the agent.
    #[must_use]
    pub const fn page_state(self) -> &'static str {
        match self {
            Self::Pending => "stored",
            Self::Delivered => "delivered",
            Self::Acknowledged => "acknowledged",
        }
    }
}

/// The kinds of v2 event the store holds today (I4). Reopen and tombstone
/// events come with threads (TSK-121).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Review,
    Answer,
    Amendment,
}

/// One v2 feedback line (I4), as `feedback --format v2` and `responses
/// list` print it. Every line is marked untrusted: an answer is evidence of
/// the operator's choice, never authority (B7).
#[derive(Debug, Clone, Serialize)]
pub struct FeedbackLine {
    pub format: u8,
    pub kind: EventKind,
    pub event_id: Uuid,
    pub session_id: Uuid,
    pub revision: u64,
    pub sequence: u64,
    pub status: DeliveryStatus,
    pub created_at_unix: u64,
    pub untrusted: bool,
    #[serde(flatten)]
    pub fields: LineFields,
}

/// The fields of a line's kind (I4).
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum LineFields {
    Review {
        verdict: FeedbackVerdict,
        #[serde(skip_serializing_if = "Option::is_none")]
        instruction: Option<String>,
        notes: Vec<ReviewNoteLine>,
    },
    Answer {
        form_id: String,
        outcome: Outcome,
        values: Map<String, Value>,
        rationales: BTreeMap<String, String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        question: QuestionSnapshot,
        #[serde(skip_serializing_if = "Option::is_none")]
        amends: Option<Uuid>,
    },
}

/// A review note as stored, with its entity selector and any `crop_check`,
/// and its anchor on the current revision (B1, I2).
#[derive(Debug, Clone, Serialize)]
pub struct ReviewNoteLine {
    #[serde(flatten)]
    pub note: FeedbackNote,
    pub anchor: FeedbackAnchor,
}

/// The `responses list` filters (B8); they combine with AND.
#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    pub revision: Option<u64>,
    pub form: Option<String>,
    pub status: Option<DeliveryStatus>,
    pub kind: Option<EventKind>,
}

impl EventFilter {
    /// Only the events the v1 stream carries: reviews.
    #[must_use]
    pub fn reviews() -> Self {
        Self {
            kind: Some(EventKind::Review),
            ..Self::default()
        }
    }

    fn admits(
        &self,
        kind: EventKind,
        revision: u64,
        form: Option<&str>,
        status: DeliveryStatus,
    ) -> bool {
        self.kind.is_none_or(|wanted| wanted == kind)
            && self.revision.is_none_or(|wanted| wanted == revision)
            && self
                .form
                .as_deref()
                .is_none_or(|wanted| form == Some(wanted))
            && self.status.is_none_or(|wanted| wanted == status)
    }
}

/// The state of one answer, as the page's poll reports it (I4 `answer_state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct AnswerState {
    pub answer_id: Uuid,
    pub status: DeliveryStatus,
}

/// The answer states the ledger lines after a cursor changed, or at a
/// closure each form's latest answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnswerStates {
    /// The last line these states cover; the next cursor.
    pub through: u64,
    pub states: Vec<AnswerState>,
    /// At a closure: each form's answer as a reload shows it.
    pub forms: Vec<FormAnswerState>,
}

/// A form's answer as a reload renders it (I4 `session_closed`): the form
/// and digest it answers, the original a correction names, the latest
/// answer or correction and that one's state in the page's words. The page
/// binds each form to it, so a closure ends every form where a reload would.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FormAnswerState {
    pub form_id: String,
    pub form_digest: String,
    pub answer_id: Uuid,
    pub latest_answer_id: Uuid,
    pub state: &'static str,
}

/// The latest answer to a form, as the page shows it when it loads: the
/// states after "stored" survive a reload (B6, B8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormAnswer {
    /// The latest answer or amendment to the form.
    pub latest: Uuid,
    /// The original answer a correction names (the latest when it is one).
    pub original: Uuid,
    pub status: DeliveryStatus,
}

/// Each form's latest answer, keyed by form id and the form digest the
/// answer was given against, so an answer to a question that has since
/// changed is not shown on the new question.
pub type FormAnswers = HashMap<(String, String), FormAnswer>;

/// Each form's latest answer in a ledger and its state.
pub(crate) fn form_answers_of(ledger: &[ResponseEvent]) -> FormAnswers {
    let states = States::of(&[], ledger);
    let mut answers = FormAnswers::new();
    for record in ledger.iter().filter_map(ResponseEvent::answer) {
        answers.insert(
            (record.form_id.clone(), record.form_digest.clone()),
            FormAnswer {
                latest: record.answer_id,
                original: record.amends.unwrap_or(record.answer_id),
                status: states.status(record.answer_id),
            },
        );
    }
    answers
}

/// Delivery and acknowledgment, replayed from both ledgers.
struct States {
    delivered: HashSet<Uuid>,
    acknowledged: HashSet<Uuid>,
}

impl States {
    fn of(events: &[FeedbackEvent], ledger: &[ResponseEvent]) -> Self {
        let mut delivered = HashSet::new();
        let mut acknowledged = HashSet::new();
        for event in events {
            if let FeedbackEvent::Delivered { event_id, .. } = event {
                delivered.insert(*event_id);
            }
        }
        for event in ledger {
            match event {
                ResponseEvent::Delivered(state) => {
                    delivered.insert(state.target);
                }
                ResponseEvent::Acknowledged(state) => {
                    acknowledged.insert(state.target);
                }
                ResponseEvent::Answer(_) | ResponseEvent::Amendment(_) => {}
            }
        }
        Self {
            delivered,
            acknowledged,
        }
    }

    fn status(&self, event_id: Uuid) -> DeliveryStatus {
        if self.acknowledged.contains(&event_id) {
            DeliveryStatus::Acknowledged
        } else if self.delivered.contains(&event_id) {
            DeliveryStatus::Delivered
        } else {
            DeliveryStatus::Pending
        }
    }
}

/// What `feedback_lines` read under the lock.
struct Snapshot {
    events: Vec<FeedbackEvent>,
    ledger: Vec<ResponseEvent>,
    states: States,
    current_revision: u64,
    current: Option<crate::state::RevisionContent>,
    sources: SourceRevisions,
}

impl SessionStore {
    /// The session's events as v2 feedback lines that the filter admits, in
    /// the order they were stored: by time, a review before any other kind
    /// of the same second, then by sequence. Reading changes no state.
    pub fn feedback_lines(&self, id: Uuid, filter: &EventFilter) -> Result<Vec<FeedbackLine>> {
        let snapshot = {
            let _lock = self.lock_session(id)?;
            let session = self.load(id)?;
            let events = self.read_events_unlocked(id)?;
            let ledger = Ledger::open(self.responses_path(id)?, id)?.events;
            // Anchors need the revisions; only a read that lists a review
            // loads them, so a wait on answers reads no revision.
            let states = States::of(&events, &ledger);
            let reviews = events.iter().any(|event| match event {
                FeedbackEvent::Received { envelope, .. } => filter.admits(
                    EventKind::Review,
                    envelope.revision,
                    None,
                    states.status(envelope.event_id),
                ),
                _ => false,
            });
            let (current, sources) = if reviews {
                let revision = self.revision(id, session.current_revision)?;
                let sources = self.source_revisions(id, &events, &revision)?;
                (Some(revision.content), sources)
            } else {
                (None, SourceRevisions::new())
            };
            Snapshot {
                events,
                ledger,
                states,
                current_revision: session.current_revision,
                current,
                sources,
            }
        };
        let replayed = &snapshot.states;
        let mut lines = Vec::new();
        for event in &snapshot.events {
            let FeedbackEvent::Received { sequence, envelope } = event else {
                continue;
            };
            let status = replayed.status(envelope.event_id);
            if filter.admits(EventKind::Review, envelope.revision, None, status) {
                lines.extend(snapshot.review_line(id, *sequence, envelope, status));
            }
        }
        for event in &snapshot.ledger {
            let (kind, record) = match event {
                ResponseEvent::Answer(record) => (EventKind::Answer, record),
                ResponseEvent::Amendment(record) => (EventKind::Amendment, record),
                ResponseEvent::Delivered(_) | ResponseEvent::Acknowledged(_) => continue,
            };
            let status = replayed.status(record.answer_id);
            if filter.admits(kind, record.revision, Some(&record.form_id), status) {
                lines.push(answer_line(kind, record, status));
            }
        }
        lines.sort_by_key(|line| {
            (
                line.created_at_unix,
                line.kind != EventKind::Review,
                line.sequence,
            )
        });
        Ok(lines)
    }

    /// Marks each pending event delivered: a review in `events.jsonl`, an
    /// answer or amendment by a `delivered` line of `responses.jsonl`. An
    /// event already delivered is left as it is.
    pub fn deliver(&self, id: Uuid, event_ids: &[Uuid]) -> Result<()> {
        let _project_lease = self.prepare_control_mutation()?;
        let _lock = self.lock_session(id)?;
        self.load(id)?;
        let mut ledger = Ledger::open(self.responses_path(id)?, id)?;
        let states = States::of(&[], &ledger.events);
        let answers = answer_ids(&ledger.events);
        let mut reviews = Vec::new();
        let mut seen = HashSet::new();
        for event_id in event_ids {
            if !seen.insert(*event_id) {
                continue;
            }
            if !answers.contains(event_id) {
                reviews.push(*event_id);
            } else if states.status(*event_id) == DeliveryStatus::Pending {
                let record = StateRecord {
                    sequence: ledger.next_sequence(),
                    target: *event_id,
                    at_unix: now_unix()?,
                };
                ledger.append(self, ResponseEvent::Delivered(record))?;
            }
        }
        self.mark_delivered_unlocked(id, &reviews)
    }

    /// Records the agent's acknowledgment of a delivered event (B8). Returns
    /// false when it was already acknowledged, which changes nothing. An
    /// unknown event, or one not yet delivered, is refused.
    pub fn acknowledge(&self, id: Uuid, event_id: Uuid) -> Result<bool> {
        let _project_lease = self.prepare_control_mutation()?;
        let _lock = self.lock_session(id)?;
        self.load(id)?;
        let events = self.read_events_unlocked(id)?;
        let mut ledger = Ledger::open(self.responses_path(id)?, id)?;
        let review = events.iter().any(|event| match event {
            FeedbackEvent::Received { envelope, .. } => envelope.event_id == event_id,
            _ => false,
        });
        if !review && !answer_ids(&ledger.events).contains(&event_id) {
            return Err(PresentError::InvalidRequest(format!(
                "event {event_id} does not belong to session {id}"
            )));
        }
        match States::of(&events, &ledger.events).status(event_id) {
            DeliveryStatus::Acknowledged => Ok(false),
            DeliveryStatus::Pending => Err(PresentError::InvalidRequest(format!(
                "event {event_id} is not delivered yet; acknowledge only what feedback printed"
            ))),
            DeliveryStatus::Delivered => {
                let record = StateRecord {
                    sequence: ledger.next_sequence(),
                    target: event_id,
                    at_unix: now_unix()?,
                };
                ledger.append(self, ResponseEvent::Acknowledged(record))?;
                Ok(true)
            }
        }
    }

    /// Each form's latest answer and its state, for the page as it loads.
    pub fn form_answers(&self, id: Uuid) -> Result<FormAnswers> {
        let ledger = {
            let _lock = self.lock_session(id)?;
            self.load(id)?;
            Ledger::open(self.responses_path(id)?, id)?.events
        };
        Ok(form_answers_of(&ledger))
    }

    /// How many pending events only the v2 stream carries (answers and
    /// amendments), for the v1 stream's notice on stderr (B8).
    pub fn pending_v2_only(&self, id: Uuid) -> Result<usize> {
        let _lock = self.lock_session(id)?;
        self.load(id)?;
        let ledger = Ledger::open(self.responses_path(id)?, id)?.events;
        let states = States::of(&[], &ledger);
        Ok(answer_ids(&ledger)
            .into_iter()
            .filter(|answer| states.status(*answer) == DeliveryStatus::Pending)
            .count())
    }

    /// The number of lines in the session's `responses.jsonl`: the page's
    /// cursor into it.
    pub fn latest_response_sequence(&self, id: Uuid) -> Result<u64> {
        let _lock = self.lock_session(id)?;
        self.load(id)?;
        Ok(Ledger::open(self.responses_path(id)?, id)?.events.len() as u64)
    }

    /// The answers whose state the ledger lines after `after` changed, with
    /// their state as of the last line read, reading at most `limit` lines,
    /// for the page's poll (I4 `answer_state`).
    pub fn answer_states_since(&self, id: Uuid, after: u64, limit: usize) -> Result<AnswerStates> {
        let ledger = {
            let _lock = self.lock_session(id)?;
            self.load(id)?;
            Ledger::open(self.responses_path(id)?, id)?.events
        };
        let after = usize::try_from(after).map_or(ledger.len(), |after| after.min(ledger.len()));
        let through = after.saturating_add(limit).min(ledger.len());
        let read = &ledger[..through];
        let states = States::of(&[], read);
        let answers = answer_ids(read);
        let mut changed = Vec::new();
        for event in &read[after..] {
            let target = match event {
                ResponseEvent::Answer(record) | ResponseEvent::Amendment(record) => {
                    record.answer_id
                }
                ResponseEvent::Delivered(state) | ResponseEvent::Acknowledged(state) => {
                    state.target
                }
            };
            if answers.contains(&target) && !changed.contains(&target) {
                changed.push(target);
            }
        }
        Ok(AnswerStates {
            forms: Vec::new(),
            through: through as u64,
            states: changed
                .into_iter()
                .map(|answer_id| AnswerState {
                    answer_id,
                    status: states.status(answer_id),
                })
                .collect(),
        })
    }
}

impl SessionStore {
    /// What a closure carries (Grok 2, C120-R2-1): the poll stops at a
    /// closure, so rather than the next batch of ledger lines it carries
    /// each form's answer as a reload shows it, whatever the backlog and
    /// whichever answer a page followed. The current revision's forms come
    /// first, then the latest answers to older questions, newest first, up
    /// to `limit`.
    pub fn closing_states(&self, id: Uuid, limit: usize) -> Result<AnswerStates> {
        let (ledger, current) = {
            let _lock = self.lock_session(id)?;
            let session = self.load(id)?;
            let current = match self.revision(id, session.current_revision)?.content {
                crate::state::RevisionContent::Supported { document } => document
                    .walk()
                    .into_iter()
                    .filter_map(crate::form::FormView::of)
                    .map(|form| (form.id.to_string(), form.digest()))
                    .collect::<HashSet<_>>(),
                _ => HashSet::new(),
            };
            (Ledger::open(self.responses_path(id)?, id)?.events, current)
        };
        let sequences: HashMap<Uuid, u64> = ledger
            .iter()
            .filter_map(ResponseEvent::answer)
            .map(|record| (record.answer_id, record.sequence))
            .collect();
        let mut forms: Vec<_> = form_answers_of(&ledger).into_iter().collect();
        forms.sort_by_key(|(key, answer)| {
            (
                !current.contains(key),
                std::cmp::Reverse(sequences.get(&answer.latest).copied()),
            )
        });
        forms.truncate(limit);
        Ok(AnswerStates {
            through: ledger.len() as u64,
            states: Vec::new(),
            forms: forms
                .into_iter()
                .map(|((form_id, form_digest), answer)| FormAnswerState {
                    form_id,
                    form_digest,
                    answer_id: answer.original,
                    latest_answer_id: answer.latest,
                    state: answer.status.page_state(),
                })
                .collect(),
        })
    }
}

impl Snapshot {
    /// A review's line, each note anchored on the current revision; none when
    /// the read loaded no revision, which it does only when it lists reviews.
    fn review_line(
        &self,
        session_id: Uuid,
        sequence: u64,
        envelope: &FeedbackEnvelope,
        status: DeliveryStatus,
    ) -> Option<FeedbackLine> {
        let current = self.current.as_ref()?;
        let notes = envelope
            .notes
            .iter()
            .map(|note| ReviewNoteLine {
                note: note.clone(),
                anchor: note_anchor(
                    session_id,
                    envelope,
                    note,
                    current,
                    self.current_revision,
                    &self.sources,
                ),
            })
            .collect();
        Some(FeedbackLine {
            format: 2,
            kind: EventKind::Review,
            event_id: envelope.event_id,
            session_id: envelope.session_id,
            revision: envelope.revision,
            sequence,
            status,
            created_at_unix: envelope.created_at_unix,
            untrusted: true,
            fields: LineFields::Review {
                verdict: envelope.verdict.clone(),
                instruction: envelope.instruction.clone(),
                notes,
            },
        })
    }
}

/// An answer's or amendment's line, with the question as it was shown.
fn answer_line(kind: EventKind, record: &AnswerRecord, status: DeliveryStatus) -> FeedbackLine {
    FeedbackLine {
        format: 2,
        kind,
        event_id: record.answer_id,
        session_id: record.session_id,
        revision: record.revision,
        sequence: record.sequence,
        status,
        created_at_unix: record.created_at_unix,
        untrusted: true,
        fields: LineFields::Answer {
            form_id: record.form_id.clone(),
            outcome: record.outcome,
            values: record.values.clone(),
            rationales: record.rationales.clone(),
            reason: record.reason.clone(),
            question: record.question.clone(),
            amends: record.amends,
        },
    }
}

fn answer_ids(ledger: &[ResponseEvent]) -> HashSet<Uuid> {
    ledger
        .iter()
        .filter_map(ResponseEvent::answer)
        .map(|record| record.answer_id)
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{
        answer_contract_tests::{FormsSession, FIXTURE_SESSION},
        contract_tests::{fixture_bytes, fixture_json},
        state::{FeedbackEnvelope, FeedbackLifecycle, FeedbackResolution},
    };

    fn current_revision(session: &FormsSession) -> u64 {
        session.store.load(session.id).unwrap().current_revision
    }

    /// A review with no notes, on the current revision.
    fn try_review(session: &FormsSession) -> Result<Uuid> {
        let event_id = Uuid::new_v4();
        session.store.append_feedback(FeedbackEnvelope {
            event_id,
            session_id: session.id,
            revision: current_revision(session),
            actor: "operator".to_string(),
            verdict: FeedbackVerdict::Approve,
            instruction: None,
            notes: Vec::new(),
            created_at_unix: 0,
        })?;
        Ok(event_id)
    }

    fn review(session: &FormsSession) -> Uuid {
        try_review(session).unwrap()
    }

    /// The valid submit fixture as a new request on the current revision.
    /// With `amends`, a correction of that answer.
    fn try_answer(
        session: &FormsSession,
        amends: Option<Uuid>,
    ) -> Result<crate::responses::AnswerReceipt> {
        let mut request = fixture_json("answers/submit-valid.json");
        request["request_id"] = json!(Uuid::new_v4());
        request["revision"] = json!(current_revision(session));
        if let Some(amends) = amends {
            request["amends"] = json!(amends);
        }
        session.submit(&session.body(&request, &[]))
    }

    fn answer(session: &FormsSession) -> Uuid {
        try_answer(session, None).unwrap().answer_id
    }

    /// R120-1 and C120-2: an answer is admitted only with room for its
    /// delivered and acknowledged lines under both bounds, and a review only
    /// with room for its acknowledgment. At a bound, and with the bound kept,
    /// the ledger still delivers and acknowledges the answer it holds and
    /// acknowledges the delivered review; a correction sent while the answer
    /// is pending, delivered or acknowledged, and a new review, are refused
    /// as the store at its capacity. Neither kind takes the other's room.
    #[test]
    fn a_full_ledger_still_delivers_and_acknowledges_its_answers() {
        use crate::responses::{fault::lower_bounds, STATE_LINE_BYTES};

        for bound in ["lines", "bytes"] {
            let session = FormsSession::open();
            let reviewed = review(&session);
            session.store.deliver(session.id, &[reviewed]).unwrap();
            let first = answer(&session);
            // Full: the answer line, plus room for its two state lines and
            // the review's acknowledgment.
            let line = session.ledger_bytes().unwrap().len() as u64;
            if bound == "lines" {
                lower_bounds(Some(4), None);
            } else {
                lower_bounds(None, Some(line + 3 * STATE_LINE_BYTES));
            }
            let full = |outcome: Result<()>, what: &str| match outcome {
                Err(PresentError::ServiceUnavailable(message)) => {
                    assert!(message.contains("room kept"), "{bound}, {what}: {message}");
                }
                other => panic!("{bound}, {what}: not refused: {other:?}"),
            };
            let refused = |when: &str| {
                let before = session.ledger_bytes();
                full(try_answer(&session, Some(first)).map(|_| ()), when);
                full(
                    try_review(&session).map(|_| ()),
                    &format!("a review, {when}"),
                );
                assert_eq!(
                    session.ledger_bytes(),
                    before,
                    "{bound}, {when}: the ledger changed"
                );
            };
            refused("pending");
            session.store.deliver(session.id, &[first]).unwrap();
            refused("delivered");
            assert!(
                session.store.acknowledge(session.id, first).unwrap(),
                "{bound}"
            );
            refused("acknowledged");
            // The review's own room is still there after the answer's.
            assert!(
                session.store.acknowledge(session.id, reviewed).unwrap(),
                "{bound}"
            );
            refused("the review acknowledged");
            assert_eq!(
                statuses(&session.store, session.id),
                vec![
                    (reviewed, DeliveryStatus::Acknowledged),
                    (first, DeliveryStatus::Acknowledged)
                ],
                "{bound}"
            );
            lower_bounds(None, None);
        }
    }

    /// PR 713 (B6): a second original answer to an answered form is
    /// refused with the stored answer and the state a reload shows, and
    /// nothing is appended; a correction of it is still stored.
    #[test]
    fn a_second_original_answer_names_the_stored_one_and_its_state() {
        let session = FormsSession::open();
        let first = answer(&session);
        // What a reload shows: the original, the latest answer or
        // correction, and the latest record's state (C120-1).
        let exists = |latest: Uuid, state: &str| {
            let before = session.ledger_bytes();
            match try_answer(&session, None) {
                Err(PresentError::Review {
                    code: "answer_exists",
                    details,
                    ..
                }) => assert_eq!(
                    details,
                    json!({ "answer_id": first, "latest_answer_id": latest, "state": state })
                ),
                other => panic!("{state}: {other:?}"),
            }
            assert_eq!(
                session.ledger_bytes(),
                before,
                "{state}: the ledger changed"
            );
        };
        exists(first, "stored");
        session.store.deliver(session.id, &[first]).unwrap();
        exists(first, "delivered");
        assert!(session.store.acknowledge(session.id, first).unwrap());
        exists(first, "acknowledged");
        // A pending correction of an acknowledged original: the refusal
        // names the correction and its state, never the original's.
        let correction = try_answer(&session, Some(first)).unwrap().answer_id;
        exists(correction, "stored");
        session.store.deliver(session.id, &[correction]).unwrap();
        exists(correction, "delivered");
        assert!(session.store.acknowledge(session.id, correction).unwrap());
        exists(correction, "acknowledged");
    }

    fn pending() -> EventFilter {
        EventFilter {
            status: Some(DeliveryStatus::Pending),
            ..EventFilter::default()
        }
    }

    fn statuses(store: &SessionStore, id: Uuid) -> Vec<(Uuid, DeliveryStatus)> {
        store
            .feedback_lines(id, &EventFilter::default())
            .unwrap()
            .iter()
            .map(|line| (line.event_id, line.status))
            .collect()
    }

    fn private_write(path: &std::path::Path, bytes: &[u8]) {
        std::fs::write(path, bytes).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
    }

    /// AC-3 on the TSK-117 query fixture (`ledger/queries.jsonl`): answers
    /// across two revisions, two forms and every status. Each case lists
    /// exactly its ids in order, alone and combined, however often it runs;
    /// listing leaves the ledger byte for byte as it was, and the delivery
    /// after all of them still gets every pending event.
    #[test]
    fn the_query_fixture_lists_exactly_its_ids_and_listing_consumes_nothing() {
        let session = FormsSession::open();
        let ledger = String::from_utf8(fixture_bytes("ledger/queries.jsonl"))
            .unwrap()
            .replace(FIXTURE_SESSION, &session.id.to_string());
        private_write(&session.ledger_path(), ledger.as_bytes());
        let ids = |lines: &[FeedbackLine]| {
            lines
                .iter()
                .map(|line| line.event_id.to_string())
                .collect::<Vec<_>>()
        };
        let expected = |case: &Value| {
            case["expect_event_ids"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| id.as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        };
        let cases = fixture_json("ledger/queries.json")["cases"].clone();
        let mut then = None;
        for case in cases.as_array().unwrap() {
            let Some(args) = case["args"].as_array() else {
                then = Some(case.clone());
                continue;
            };
            let mut filter = EventFilter::default();
            for pair in args.chunks(2) {
                let value = pair[1].as_str().unwrap();
                match pair[0].as_str().unwrap() {
                    "--revision" => filter.revision = Some(value.parse().unwrap()),
                    "--form" => filter.form = Some(value.to_string()),
                    "--status" => {
                        filter.status = Some(serde_json::from_value(json!(value)).unwrap());
                    }
                    "--kind" => filter.kind = Some(serde_json::from_value(json!(value)).unwrap()),
                    other => panic!("unknown query flag {other}"),
                }
            }
            for _ in 0..3 {
                let lines = session.store.feedback_lines(session.id, &filter).unwrap();
                assert_eq!(ids(&lines), expected(case), "{args:?}");
            }
        }
        assert_eq!(
            session.ledger_bytes().unwrap(),
            ledger.as_bytes(),
            "listing changed the ledger"
        );
        let then = then.expect("the fixture ends with a delivery case");
        let delivered = session
            .store
            .feedback_lines(session.id, &pending())
            .unwrap();
        assert_eq!(ids(&delivered), expected(&then), "{}", then["why"]);
        let event_ids = delivered
            .iter()
            .map(|line| line.event_id)
            .collect::<Vec<_>>();
        session.store.deliver(session.id, &event_ids).unwrap();
        assert!(session
            .store
            .feedback_lines(session.id, &pending())
            .unwrap()
            .is_empty());
    }

    /// AC-4: acknowledgment is recorded apart from delivery. An event is
    /// acknowledged only once it was delivered; acknowledging again changes
    /// nothing; an unknown event is refused. The rail shows a review's
    /// delivery and acknowledgment side by side, and `resolve` stays its own
    /// state.
    #[test]
    fn delivered_and_acknowledged_are_separate_states() {
        let session = FormsSession::open();
        let (store, id) = (&session.store, session.id);
        let review = review(&session);
        let answer = answer(&session);
        let refused = |event: Uuid| match store.acknowledge(id, event) {
            Err(PresentError::InvalidRequest(message)) => message,
            other => panic!("acknowledging {event}: {other:?}"),
        };
        for event in [review, answer] {
            assert!(refused(event).contains("not delivered"));
        }
        assert!(refused(Uuid::new_v4()).contains("does not belong"));
        assert_eq!(
            statuses(store, id),
            [
                (review, DeliveryStatus::Pending),
                (answer, DeliveryStatus::Pending)
            ]
        );

        store.deliver(id, &[review, answer]).unwrap();
        assert_eq!(
            statuses(store, id),
            [
                (review, DeliveryStatus::Delivered),
                (answer, DeliveryStatus::Delivered)
            ]
        );
        let rail = store.feedback_snapshot(id).unwrap().items[0].clone();
        assert_eq!(
            (rail.lifecycle, rail.acknowledged),
            (FeedbackLifecycle::Delivered, false)
        );

        assert!(store.acknowledge(id, review).unwrap());
        assert!(store.acknowledge(id, answer).unwrap());
        let ledger = session.ledger_bytes().unwrap();
        assert!(
            !store.acknowledge(id, review).unwrap(),
            "a second ack is a no-op"
        );
        assert!(
            !store.acknowledge(id, answer).unwrap(),
            "a second ack is a no-op"
        );
        assert_eq!(session.ledger_bytes().unwrap(), ledger);
        assert_eq!(
            statuses(store, id),
            [
                (review, DeliveryStatus::Acknowledged),
                (answer, DeliveryStatus::Acknowledged)
            ]
        );
        let rail = store.feedback_snapshot(id).unwrap().items[0].clone();
        assert_eq!(
            (rail.lifecycle, rail.acknowledged),
            (FeedbackLifecycle::Delivered, true)
        );
        store
            .resolve_feedback(
                id,
                review,
                rail.event_version,
                FeedbackResolution::Addressed,
            )
            .unwrap();
        let rail = store.feedback_snapshot(id).unwrap().items[0].clone();
        assert_eq!(
            (rail.lifecycle, rail.acknowledged),
            (FeedbackLifecycle::Addressed, true)
        );

        // The ledger: the answer, its delivery, then both acknowledgments.
        let kinds = crate::responses::Ledger::open(store.responses_path(id).unwrap(), id)
            .unwrap()
            .events
            .iter()
            .map(|event| serde_json::to_value(event).unwrap()["event"].clone())
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            ["answer", "delivered", "acknowledged", "acknowledged"]
        );
    }

    /// The page's poll reads answer states after its cursor, a bounded
    /// number of lines at a time, each with its state as of the last line
    /// read; a review's acknowledgment moves the cursor and names no answer.
    #[test]
    fn answer_states_follow_the_ledger_a_bounded_step_at_a_time() {
        let session = FormsSession::open();
        let (store, id) = (&session.store, session.id);
        let review = review(&session);
        let answer = answer(&session);
        store.deliver(id, &[review, answer]).unwrap();
        store.acknowledge(id, answer).unwrap();
        store.acknowledge(id, review).unwrap();
        assert_eq!(store.latest_response_sequence(id).unwrap(), 4);
        let state = |status| {
            vec![AnswerState {
                answer_id: answer,
                status,
            }]
        };
        let step = store.answer_states_since(id, 0, 1).unwrap();
        assert_eq!(
            (step.through, step.states),
            (1, state(DeliveryStatus::Pending))
        );
        let step = store.answer_states_since(id, 1, 1).unwrap();
        assert_eq!(
            (step.through, step.states),
            (2, state(DeliveryStatus::Delivered))
        );
        let step = store.answer_states_since(id, 0, 100).unwrap();
        assert_eq!(
            (step.through, step.states),
            (4, state(DeliveryStatus::Acknowledged))
        );
        let step = store.answer_states_since(id, 3, 100).unwrap();
        assert_eq!((step.through, step.states), (4, Vec::new()));
        let step = store.answer_states_since(id, 9, 100).unwrap();
        assert_eq!((step.through, step.states), (4, Vec::new()));
    }

    /// AC-6: with no listener the events wait in the store. A new store over
    /// the same state (a restarted service, or the next CLI process) finds
    /// them pending; a delivery cut off mid-line leaves its answer pending;
    /// each event is delivered once and a later read finds nothing.
    #[test]
    fn pending_events_survive_a_restart_and_are_delivered_once() {
        let session = FormsSession::open();
        let id = session.id;
        let review = review(&session);
        let answer = answer(&session);
        let reopen = || {
            SessionStore::at_root(session.store.root().to_path_buf(), "key".to_string()).unwrap()
        };

        let restarted = reopen();
        let waiting = restarted.feedback_lines(id, &pending()).unwrap();
        assert_eq!(
            waiting
                .iter()
                .map(|line| (line.kind, line.event_id))
                .collect::<Vec<_>>(),
            [(EventKind::Review, review), (EventKind::Answer, answer)]
        );
        assert_eq!(restarted.pending_v2_only(id).unwrap(), 1);

        // A crash in the middle of writing the answer's delivered line.
        let whole = session.ledger_bytes().unwrap();
        let mut torn = whole.clone();
        torn.extend_from_slice(b"{\"event\":\"delivered\",\"sequence\":2,\"tar");
        private_write(&session.ledger_path(), &torn);
        let restarted = reopen();
        assert_eq!(restarted.feedback_lines(id, &pending()).unwrap().len(), 2);
        assert_eq!(
            session.ledger_bytes().unwrap(),
            whole,
            "the torn line is cut"
        );

        restarted.deliver(id, &[review, answer]).unwrap();
        restarted.deliver(id, &[review, answer]).unwrap();
        let restarted = reopen();
        assert!(restarted.feedback_lines(id, &pending()).unwrap().is_empty());
        assert_eq!(restarted.pending_v2_only(id).unwrap(), 0);
        assert_eq!(
            statuses(&restarted, id),
            [
                (review, DeliveryStatus::Delivered),
                (answer, DeliveryStatus::Delivered)
            ]
        );
        let reviews_delivered = restarted
            .events(id)
            .unwrap()
            .iter()
            .filter(|event| matches!(event, FeedbackEvent::Delivered { .. }))
            .count();
        let answers_delivered =
            crate::responses::Ledger::open(restarted.responses_path(id).unwrap(), id)
                .unwrap()
                .events
                .iter()
                .filter(|event| matches!(event, ResponseEvent::Delivered(_)))
                .count();
        assert_eq!((reviews_delivered, answers_delivered), (1, 1));
    }

    /// B7, I4: every v2 line is untrusted and carries its kind's fields; a
    /// review line keeps the note's entity selector (with any crop check)
    /// that the v1 view drops, and an answer line the question as shown.
    #[test]
    fn v2_lines_carry_their_kind_fields_and_are_untrusted() {
        let session = FormsSession::open();
        review(&session);
        answer(&session);
        let lines = session
            .store
            .feedback_lines(session.id, &EventFilter::default())
            .unwrap();
        let values = lines
            .iter()
            .map(|line| serde_json::to_value(line).unwrap())
            .collect::<Vec<_>>();
        for value in &values {
            assert_eq!(value["format"], 2);
            assert_eq!(value["untrusted"], true);
            assert_eq!(value["status"], "pending");
        }
        assert_eq!(values[0]["kind"], "review");
        assert_eq!(values[0]["verdict"], "approve");
        assert!(values[0]["notes"].as_array().unwrap().is_empty());
        assert_eq!(values[1]["kind"], "answer");
        assert_eq!(values[1]["form_id"], "store-choice");
        assert_eq!(values[1]["values"]["home"], "local");
        assert_eq!(values[1]["question"]["fields"][0]["id"], "home");
        assert!(values[1].get("amends").is_none() && values[1].get("reason").is_none());
    }
}
