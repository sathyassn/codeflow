//! The answer ledger `responses.jsonl` (SPC-014 B6, B7, I4).
//!
//! Answers and amendments are appended to `responses.jsonl` in the session
//! directory, beside the v1 `events.jsonl`, which keeps its exact shape. The
//! ledger lives in the private local session store, owner-only like the rest
//! of it, and is never written into the repository (B7). A session with no
//! answer has no ledger file.
//!
//! Append rule (I4): each event is one line, written with its newline in one
//! write and synced before the receipt is returned, under the session lock.
//! A failed write is cut back to the previous length. When the ledger is
//! opened, a final line with no newline, or one that does not parse, is a
//! torn append from a crash: it is truncated under the lock. Its request
//! never got a receipt, so the page's retry appends it again. Any other line
//! that does not parse, or a ledger whose sequence, ids or amendments do not
//! hold together, is corrupt state and is never rewritten.

use std::{
    collections::{BTreeMap, HashSet},
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::{
    error::{PresentError, Result},
    form::{AnswerRequest, AnswerSubmission, FormView, Outcome, QuestionSnapshot},
    limits,
    state::{
        now_unix, open_private_append, open_private_rw, RevisionContent, SessionStatus,
        SessionStore,
    },
};

/// The ledger's file name in the session directory.
pub const RESPONSES_FILE: &str = "responses.jsonl";

/// Who wrote an answer: always the reviewer, derived by the server (B6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    Operator,
}

/// An `answer` or `amendment` line of I4, without its `event` tag.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnswerRecord {
    pub sequence: u64,
    pub answer_id: Uuid,
    pub request_id: Uuid,
    pub payload_digest: String,
    pub session_id: Uuid,
    pub revision: u64,
    pub form_id: String,
    pub form_digest: String,
    pub outcome: Outcome,
    pub values: Map<String, Value>,
    pub rationales: BTreeMap<String, String>,
    /// A decline's reason, when the reviewer gave one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub question: QuestionSnapshot,
    pub actor: Actor,
    pub created_at_unix: u64,
    /// An amendment's original answer; absent on an answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amends: Option<Uuid>,
}

/// One line of `responses.jsonl` (I4). This task writes answers and
/// amendments; later event kinds extend the enum additively.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ResponseEvent {
    Answer(AnswerRecord),
    Amendment(AnswerRecord),
}

impl ResponseEvent {
    #[must_use]
    pub const fn record(&self) -> &AnswerRecord {
        match self {
            Self::Answer(record) | Self::Amendment(record) => record,
        }
    }
}

/// The receipt `POST /app/api/answers` returns (I4, HTTP 200).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnswerReceipt {
    pub answer_id: Uuid,
    pub request_id: Uuid,
    pub sequence: u64,
    pub revision: u64,
    pub form_id: String,
    /// Always `stored`: the answer is durable in the ledger.
    pub state: String,
    /// True when the receipt is the original one, returned again for a
    /// retry of the same request and payload.
    pub replayed: bool,
    pub stored_at_unix: u64,
}

impl AnswerReceipt {
    fn of(record: &AnswerRecord, replayed: bool) -> Self {
        Self {
            answer_id: record.answer_id,
            request_id: record.request_id,
            sequence: record.sequence,
            revision: record.revision,
            form_id: record.form_id.clone(),
            state: "stored".to_string(),
            replayed,
            stored_at_unix: record.created_at_unix,
        }
    }
}

/// The ledger of one session, read under the caller's session lock.
struct Ledger {
    path: PathBuf,
    events: Vec<ResponseEvent>,
    length: u64,
}

impl Ledger {
    /// Reads the ledger, truncating a torn final line. The caller holds the
    /// session lock. An absent file is an empty ledger.
    fn open(path: PathBuf, session_id: Uuid) -> Result<Self> {
        let mut file = match std::fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    path,
                    events: Vec::new(),
                    length: 0,
                });
            }
            Err(error) => return Err(PresentError::io(&path, error)),
            Ok(_) => open_private_rw(&path)?,
        };
        let length = file
            .metadata()
            .map_err(|error| PresentError::io(&path, error))?
            .len();
        if length > limits::MAX_RESPONSE_LOG_BYTES {
            return Err(PresentError::CorruptState(format!(
                "{} exceeds the {} byte ledger bound",
                path.display(),
                limits::MAX_RESPONSE_LOG_BYTES
            )));
        }
        let mut bytes = Vec::new();
        std::io::Read::by_ref(&mut file)
            .take(limits::MAX_RESPONSE_LOG_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| PresentError::io(&path, error))?;
        if bytes.len() as u64 != length {
            return Err(PresentError::CorruptState(format!(
                "{} changed while it was read",
                path.display()
            )));
        }
        let (events, keep) = parse_ledger(&path, &bytes)?;
        let keep = keep as u64;
        if keep < length {
            file.set_len(keep)
                .and_then(|()| file.sync_data())
                .map_err(|error| PresentError::io(&path, error))?;
        }
        validate_ledger(&path, &events, session_id)?;
        Ok(Self {
            path,
            events,
            length: keep,
        })
    }

    fn next_sequence(&self) -> u64 {
        self.events.len() as u64 + 1
    }

    fn by_request(&self, request_id: Uuid) -> Option<&AnswerRecord> {
        self.events
            .iter()
            .map(ResponseEvent::record)
            .find(|record| record.request_id == request_id)
    }

    /// An amendment names an original answer of the same form (B6).
    fn check_amendment(&self, amends: Uuid, form_id: &str) -> Result<()> {
        let refuse = |message: &str| {
            PresentError::review(
                "invalid_amendment",
                message,
                serde_json::json!({ "amends": amends }),
            )
        };
        match self
            .events
            .iter()
            .find(|event| event.record().answer_id == amends)
        {
            Some(ResponseEvent::Answer(original)) if original.form_id == form_id => Ok(()),
            Some(ResponseEvent::Answer(_)) => Err(refuse("amends names an answer to another form")),
            Some(ResponseEvent::Amendment(_)) => Err(refuse(
                "amends names an amendment; an amendment names the original answer",
            )),
            None => Err(refuse("amends names no answer in this session")),
        }
    }

    /// Writes one event with its newline in one write at the end of the
    /// ledger and syncs; on a failure the ledger is cut back.
    fn append(&mut self, store: &SessionStore, event: ResponseEvent) -> Result<()> {
        if self.events.len() >= limits::MAX_RESPONSE_EVENTS {
            return Err(PresentError::InvalidDocument(format!(
                "the answer ledger holds at most {} lines",
                limits::MAX_RESPONSE_EVENTS
            )));
        }
        let mut line = serde_json::to_vec(&event)?;
        if line.len() as u64 > limits::MAX_RESPONSE_RECORD_BYTES {
            return Err(PresentError::InvalidDocument(format!(
                "an answer record exceeds {} bytes",
                limits::MAX_RESPONSE_RECORD_BYTES
            )));
        }
        line.push(b'\n');
        let grown = self.length + line.len() as u64;
        if grown > limits::MAX_RESPONSE_LOG_BYTES {
            return Err(PresentError::InvalidDocument(format!(
                "the answer ledger reached its {} byte bound",
                limits::MAX_RESPONSE_LOG_BYTES
            )));
        }
        store.enforce_retention_unlocked()?;
        store.ensure_project_capacity_unlocked(line.len() as u64, true)?;
        let mut file = open_private_append(&self.path)?;
        #[cfg(test)]
        crash::interrupt(&mut file, &line);
        let written = file.write_all(&line).and_then(|()| file.sync_data());
        if let Err(error) = written {
            let _ = file.set_len(self.length).and_then(|()| file.sync_data());
            return Err(PresentError::io(&self.path, error));
        }
        self.length = grown;
        self.events.push(event);
        Ok(())
    }
}

/// Splits the ledger into its events and the byte length to keep. A final
/// line with no newline, or a final line that does not parse, is torn and
/// left out; any other line that does not parse is corruption.
fn parse_ledger(path: &Path, bytes: &[u8]) -> Result<(Vec<ResponseEvent>, usize)> {
    let mut events = Vec::new();
    let mut start = 0;
    while start < bytes.len() {
        let Some(offset) = bytes[start..].iter().position(|byte| *byte == b'\n') else {
            return Ok((events, start));
        };
        let end = start + offset;
        let last = end + 1 == bytes.len();
        let line = &bytes[start..end];
        let parsed = if line.len() as u64 > limits::MAX_RESPONSE_RECORD_BYTES {
            None
        } else {
            serde_json::from_slice::<ResponseEvent>(line).ok()
        };
        match parsed {
            Some(event) => events.push(event),
            None if last => return Ok((events, start)),
            None => {
                return Err(PresentError::CorruptState(format!(
                    "{} line {} is not an answer record",
                    path.display(),
                    events.len() + 1
                )));
            }
        }
        start = end + 1;
    }
    Ok((events, start))
}

/// The ledger holds together: sequences count from 1, each answer and
/// request id is used once, and each amendment names an earlier original
/// answer of its form.
fn validate_ledger(path: &Path, events: &[ResponseEvent], session_id: Uuid) -> Result<()> {
    let corrupt = |message: String| {
        Err(PresentError::CorruptState(format!(
            "{}: {message}",
            path.display()
        )))
    };
    if events.len() > limits::MAX_RESPONSE_EVENTS {
        return corrupt(format!("more than {} lines", limits::MAX_RESPONSE_EVENTS));
    }
    let mut answers = HashSet::new();
    let mut requests = HashSet::new();
    for (index, event) in events.iter().enumerate() {
        let record = event.record();
        let line = index + 1;
        if record.sequence != line as u64 {
            return corrupt(format!("line {line} has sequence {}", record.sequence));
        }
        if record.session_id != session_id {
            return corrupt(format!("line {line} names another session"));
        }
        if !answers.insert(record.answer_id) || !requests.insert(record.request_id) {
            return corrupt(format!("line {line} reuses an answer or request id"));
        }
        let original_of_form = |amends: Uuid| {
            events[..index].iter().any(|earlier| {
                matches!(earlier, ResponseEvent::Answer(original)
                    if original.answer_id == amends && original.form_id == record.form_id)
            })
        };
        match (event, record.amends) {
            (ResponseEvent::Answer(_), None) => {}
            (ResponseEvent::Amendment(_), Some(amends)) if original_of_form(amends) => {}
            _ => {
                return corrupt(format!(
                    "line {line} is an answer that amends, or an amendment of no original answer of its form"
                ));
            }
        }
    }
    Ok(())
}

impl SessionStore {
    /// Stores an answer or an amendment from the body of `POST
    /// /app/api/answers` (B6), or returns the original receipt for a retry
    /// of the same request id and payload. The 64 KiB bound is checked
    /// before anything else; the actor and the time are the server's.
    ///
    /// Order: body, session, request id (so a retry whose first response
    /// was lost replays even after a newer revision), session state,
    /// revision, form, form digest, amendment, values.
    pub fn submit_answer(&self, session_id: Uuid, body: &[u8]) -> Result<AnswerReceipt> {
        let submission = AnswerSubmission::parse(body)?;
        let request = &submission.request;
        if request.session_id != session_id {
            return Err(crate::form::invalid_answer(
                "session_id does not name this session",
                &[],
            ));
        }
        let _project_lease = self.prepare_control_mutation()?;
        let _lock = self.lock_session(session_id)?;
        let session = self.load(session_id)?;
        let mut ledger = Ledger::open(self.responses_path(session_id)?, session_id)?;
        if let Some(existing) = ledger.by_request(request.request_id) {
            if existing.payload_digest == submission.payload_digest {
                return Ok(AnswerReceipt::of(existing, true));
            }
            return Err(PresentError::review(
                "request_id_conflict",
                format!(
                    "request {} was already used for a different answer",
                    request.request_id
                ),
                serde_json::json!({ "request_id": request.request_id }),
            ));
        }
        if session.status != SessionStatus::Active {
            return Err(PresentError::SessionClosed(session_id.to_string()));
        }
        let current = self.revision(session_id, session.current_revision)?;
        let form = current_form(&current.content, session.current_revision, request)?;
        let form_digest = form.digest();
        if let Some(amends) = request.amends {
            ledger.check_amendment(amends, &request.form_id)?;
        }
        form.validate_answer(request)?;
        let record = AnswerRecord {
            sequence: ledger.next_sequence(),
            answer_id: Uuid::new_v4(),
            request_id: request.request_id,
            payload_digest: submission.payload_digest.clone(),
            session_id,
            revision: request.revision,
            form_id: request.form_id.clone(),
            form_digest,
            outcome: request.outcome,
            values: request.values.clone(),
            rationales: request.rationales.clone(),
            reason: request.reason.clone(),
            question: form.snapshot(),
            actor: Actor::Operator,
            created_at_unix: now_unix()?,
            amends: request.amends,
        };
        let receipt = AnswerReceipt::of(&record, false);
        ledger.append(
            self,
            if record.amends.is_some() {
                ResponseEvent::Amendment(record)
            } else {
                ResponseEvent::Answer(record)
            },
        )?;
        Ok(receipt)
    }

    /// Every answer and amendment of a session, in ledger order. Reading
    /// repairs a torn final line under the session lock, as every open does.
    pub fn responses(&self, session_id: Uuid) -> Result<Vec<ResponseEvent>> {
        let _lock = self.lock_session(session_id)?;
        self.load(session_id)?;
        Ok(Ledger::open(self.responses_path(session_id)?, session_id)?.events)
    }

    fn responses_path(&self, session_id: Uuid) -> Result<PathBuf> {
        self.ensure_session_layout(session_id)?;
        Ok(self.session_dir(session_id).join(RESPONSES_FILE))
    }
}

/// The form the request names in the current revision: a request for an
/// older revision is stale (with what the page needs to confirm it), then
/// the form must exist and its digest match.
fn current_form<'a>(
    content: &'a RevisionContent,
    current_revision: u64,
    request: &AnswerRequest,
) -> Result<FormView<'a>> {
    let document = match content {
        RevisionContent::Supported { document } => Some(document),
        RevisionContent::Unsupported { .. } | RevisionContent::Retired { .. } => None,
    };
    let form = document.and_then(|document| {
        document
            .walk()
            .into_iter()
            .find(|block| block.id() == request.form_id)
            .and_then(FormView::of)
    });
    if request.revision != current_revision {
        let mut details = serde_json::json!({
            "current_revision": current_revision,
            "form_present": form.is_some(),
        });
        if let Some(form) = &form {
            details["current_form_digest"] = Value::String(form.digest());
        }
        return Err(PresentError::review(
            "stale_revision",
            format!(
                "the answer names revision {} but revision {current_revision} is current; confirm it against the current revision",
                request.revision
            ),
            details,
        ));
    }
    let Some(form) = form else {
        return Err(PresentError::review(
            "unknown_form",
            format!(
                "revision {} has no form {:?}",
                request.revision, request.form_id
            ),
            serde_json::json!({ "form_id": request.form_id }),
        ));
    };
    if request.form_digest != form.digest() {
        return Err(PresentError::review(
            "form_digest_mismatch",
            "the form digest is not the digest of this form at this revision",
            serde_json::json!({}),
        ));
    }
    Ok(form)
}

/// A test-only interruption of an append: with `CF_PRESENT_TEST_CRASH_AT`
/// set to a byte count, the append writes that many bytes of its line,
/// syncs them and aborts the process, as a crash mid-write would.
#[cfg(test)]
mod crash {
    pub(super) const CRASH_AT: &str = "CF_PRESENT_TEST_CRASH_AT";

    pub(super) fn interrupt(file: &mut std::fs::File, line: &[u8]) {
        use std::io::Write as _;
        let Some(cut) = std::env::var(CRASH_AT)
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
        else {
            return;
        };
        let _ = file.write_all(&line[..cut.min(line.len())]);
        let _ = file.sync_data();
        std::process::abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_torn_final_line_is_cut_and_a_middle_one_is_corruption() {
        let path = Path::new("responses.jsonl");
        let line = |sequence: u64| {
            let record = serde_json::json!({
                "event": "answer", "sequence": sequence, "answer_id": Uuid::from_u128(u128::from(sequence)),
                "request_id": Uuid::from_u128(100 + u128::from(sequence)), "payload_digest": "a".repeat(64),
                "session_id": Uuid::nil(), "revision": 1, "form_id": "f", "form_digest": "b".repeat(64),
                "outcome": "cancel", "values": {}, "rationales": {},
                "question": { "title": "T", "fields": [] }, "actor": "operator", "created_at_unix": 1
            });
            format!("{record}\n")
        };
        let whole = format!("{}{}", line(1), line(2));
        let (events, keep) = parse_ledger(path, whole.as_bytes()).unwrap();
        assert_eq!((events.len(), keep), (2, whole.len()));
        // No newline: torn, even though the bytes parse.
        let unterminated = whole.trim_end_matches('\n');
        let (events, keep) = parse_ledger(path, unterminated.as_bytes()).unwrap();
        assert_eq!((events.len(), keep), (1, line(1).len()));
        // A final line with its newline that does not parse: torn.
        let broken = format!("{}{{\"event\":\"answ\n", line(1));
        let (events, keep) = parse_ledger(path, broken.as_bytes()).unwrap();
        assert_eq!((events.len(), keep), (1, line(1).len()));
        // A line that does not parse before the last: corruption.
        let middle = format!("{{\"event\":\"nonsense\"}}\n{}", line(1));
        assert!(matches!(
            parse_ledger(path, middle.as_bytes()),
            Err(PresentError::CorruptState(_))
        ));
        // Sequence, ids and amendments must hold together.
        let events = parse_ledger(path, whole.as_bytes()).unwrap().0;
        assert!(validate_ledger(path, &events, Uuid::nil()).is_ok());
        assert!(validate_ledger(path, &events, Uuid::from_u128(9)).is_err());
        assert!(validate_ledger(path, &events[1..], Uuid::nil()).is_err());
        let mut repeated = events.clone();
        if let ResponseEvent::Answer(record) = &mut repeated[1] {
            record.request_id = events[0].record().request_id;
        }
        assert!(validate_ledger(path, &repeated, Uuid::nil()).is_err());
        let mut amending = events.clone();
        if let ResponseEvent::Answer(record) = &mut amending[1] {
            record.amends = Some(events[0].record().answer_id);
        }
        assert!(
            validate_ledger(path, &amending, Uuid::nil()).is_err(),
            "an answer line cannot amend"
        );
        let ResponseEvent::Answer(record) = amending[1].clone() else {
            unreachable!()
        };
        amending[1] = ResponseEvent::Amendment(record);
        assert!(validate_ledger(path, &amending, Uuid::nil()).is_ok());
        if let ResponseEvent::Amendment(record) = &mut amending[1] {
            record.form_id = "g".to_string();
        }
        assert!(
            validate_ledger(path, &amending, Uuid::nil()).is_err(),
            "an amendment names an answer of its own form"
        );
    }

    /// AC-5: an append interrupted by a crash leaves the ledger whole or
    /// without the line. The test re-runs its own binary as a child that
    /// writes part of the line through the test-only hook, syncs it and
    /// aborts; the parent then reopens the store.
    #[test]
    fn a_crash_mid_append_leaves_the_ledger_whole_or_without_the_line() {
        use crate::answer_contract_tests::FormsSession;

        if std::env::var_os(CHILD_ROOT).is_some() {
            return;
        }
        let session = FormsSession::open();
        let first = crate::contract_tests::fixture_json("answers/submit-valid.json");
        let stored = session.submit(&session.body(&first, &[])).unwrap();
        let mut second = first.clone();
        second["request_id"] = serde_json::json!(Uuid::from_u128(0x00c0_ffee));
        second["values"]["keep-days"] = serde_json::json!(90);
        let body = session.body(&second, &[]);
        let body_path = session.store.root().with_file_name("crash-body.json");
        std::fs::write(&body_path, &body).unwrap();
        let before = session.ledger_bytes().unwrap();
        let line_len = {
            // The line the child writes has the length of an equal record.
            let probe = FormsSession::open();
            probe.submit(&probe.body(&first, &[])).unwrap();
            probe.submit(&probe.body(&second, &[])).unwrap();
            let bytes = probe.ledger_bytes().unwrap();
            bytes.len() - (bytes.iter().position(|byte| *byte == b'\n').unwrap() + 1)
        };
        for cut in [1, line_len / 2, line_len - 1, line_len] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "responses::tests::crash_child",
                    "--test-threads=1",
                    "--nocapture",
                ])
                .env(crash::CRASH_AT, cut.to_string())
                .env(CHILD_ROOT, session.store.root())
                .env(CHILD_SESSION, session.id.to_string())
                .env(CHILD_BODY, &body_path)
                .output()
                .unwrap();
            assert!(
                !output.status.success(),
                "cut {cut}: the child must abort: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let events = session.store.responses(session.id).unwrap();
            let after = session.ledger_bytes().unwrap();
            if cut == line_len {
                // The whole line and its newline reached the disk before
                // the crash: it stays, and the retry gets its receipt.
                assert_eq!(events.len(), 2, "cut {cut}");
                assert_eq!(after.len(), before.len() + line_len);
                let retry = session.submit(&body).unwrap();
                assert!(retry.replayed && retry.sequence == 2, "cut {cut}");
            } else {
                assert_eq!(events.len(), 1, "cut {cut}: the torn line is cut");
                assert_eq!(after, before, "cut {cut}");
                assert_eq!(events[0].record().answer_id, stored.answer_id);
            }
        }
        // The session still works: the interrupted request stores on retry
        // when it was cut, and the next answer takes the next sequence.
        let mut third = first.clone();
        third["request_id"] = serde_json::json!(Uuid::from_u128(0x00c0_ffef));
        let receipt = session.submit(&session.body(&third, &[])).unwrap();
        assert_eq!((receipt.sequence, receipt.replayed), (3, false));
        assert_eq!(session.store.responses(session.id).unwrap().len(), 3);
    }

    const CHILD_ROOT: &str = "CF_PRESENT_TEST_CRASH_ROOT";
    const CHILD_SESSION: &str = "CF_PRESENT_TEST_CRASH_SESSION";
    const CHILD_BODY: &str = "CF_PRESENT_TEST_CRASH_BODY";

    /// The crash child: does nothing in a normal run.
    #[test]
    fn crash_child() {
        let (Some(root), Some(session), Some(body)) = (
            std::env::var_os(CHILD_ROOT),
            std::env::var(CHILD_SESSION).ok(),
            std::env::var_os(CHILD_BODY),
        ) else {
            return;
        };
        let store = SessionStore::at_root(PathBuf::from(root), "key".to_string()).unwrap();
        let body = std::fs::read(body).unwrap();
        let result = store.submit_answer(session.parse().unwrap(), &body);
        // The hook aborts before this; reaching it fails the parent's check.
        eprintln!("crash child returned: {result:?}");
        std::process::exit(0);
    }

    /// AC-3: a deterministic generator drives sequences of new answers,
    /// retries, reused request ids, amendments, invalid bodies and document
    /// updates against one session, and checks the ledger and every receipt
    /// against a model after each step.
    #[test]
    #[allow(clippy::too_many_lines)]
    fn generated_submission_sequences_keep_the_ledger_consistent() {
        use crate::answer_contract_tests::FormsSession;
        use crate::document::{parse_document, ParsedDocument};

        struct Sent {
            body: Vec<u8>,
            receipt: AnswerReceipt,
            amendment: bool,
            form: &'static str,
        }
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = move |bound: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % bound
        };
        let forms_json = crate::contract_tests::fixture_bytes("documents/v2-forms.json");
        for run in 0_u8..24 {
            let session = FormsSession::open();
            let mut revision = 1_u64;
            let mut sent: Vec<Sent> = Vec::new();
            let mut request_counter = 0_u128;
            for step in 0..40 {
                let context = format!("run {run} step {step}");
                let before = session.ledger_bytes();
                let digest_of = |form: &str| {
                    let crate::state::RevisionContent::Supported { document } =
                        session.store.current_revision(session.id).unwrap().content
                    else {
                        unreachable!()
                    };
                    crate::state::block_digest(
                        document
                            .walk()
                            .into_iter()
                            .find(|b| b.id() == form)
                            .unwrap(),
                    )
                };
                let mut fresh = || {
                    request_counter += 1;
                    Uuid::from_u128((u128::from(run) << 64) | request_counter)
                };
                let valid =
                    |form: &'static str, request_id: Uuid, amends: Option<Uuid>, pick: u64| {
                        let index = usize::try_from(pick % 2).unwrap();
                        let (home, choice) = (["local", "repo"][index], ["a", "b"][index]);
                        let mut request = if form == "store-choice" {
                            serde_json::json!({
                                "values": {
                                    "home": home,
                                    "keep-days": 1 + pick % 365,
                                },
                                "rationales": { "home": format!("because {pick}") },
                            })
                        } else {
                            serde_json::json!({
                                "values": { "choice": choice },
                                "rationales": {},
                            })
                        };
                        request["request_id"] = serde_json::json!(request_id);
                        request["session_id"] = serde_json::json!(session.id);
                        request["revision"] = serde_json::json!(revision);
                        request["form_id"] = serde_json::json!(form);
                        request["form_digest"] = serde_json::json!(digest_of(form));
                        request["outcome"] = serde_json::json!("submit");
                        if let Some(amends) = amends {
                            request["amends"] = serde_json::json!(amends);
                        }
                        serde_json::to_vec(&request).unwrap()
                    };
                let form: &'static str = if next(2) == 0 {
                    "store-choice"
                } else {
                    "d-scope"
                };
                match next(7) {
                    // A new answer.
                    0 | 1 => {
                        let body = valid(form, fresh(), None, next(1000));
                        let receipt = session.submit(&body).unwrap();
                        assert_eq!(receipt.sequence, sent.len() as u64 + 1, "{context}");
                        assert!(!receipt.replayed, "{context}");
                        sent.push(Sent {
                            body,
                            receipt,
                            amendment: false,
                            form,
                        });
                    }
                    // A retry of an earlier request, even after an update.
                    2 if !sent.is_empty() => {
                        let earlier = &sent[usize::try_from(next(sent.len() as u64)).unwrap()];
                        let receipt = session.submit(&earlier.body).unwrap();
                        assert!(receipt.replayed, "{context}");
                        assert_eq!(
                            AnswerReceipt {
                                replayed: false,
                                ..receipt
                            },
                            earlier.receipt,
                            "{context}"
                        );
                        assert_eq!(session.ledger_bytes(), before, "{context}");
                    }
                    // An earlier request id with a different payload.
                    3 if !sent.is_empty() => {
                        let earlier = &sent[usize::try_from(next(sent.len() as u64)).unwrap()];
                        let mut changed: Value = serde_json::from_slice(&earlier.body).unwrap();
                        changed["rationales"] = serde_json::json!({});
                        changed["values"] = serde_json::json!({});
                        changed["outcome"] = serde_json::json!("cancel");
                        let body = serde_json::to_vec(&changed).unwrap();
                        let result = session.store.submit_answer(session.id, &body);
                        assert!(
                            matches!(
                                result,
                                Err(PresentError::Review {
                                    code: "request_id_conflict",
                                    ..
                                })
                            ),
                            "{context}: {result:?}"
                        );
                        assert_eq!(session.ledger_bytes(), before, "{context}");
                    }
                    // An amendment: of an original answer of the same form it
                    // stores; of an amendment or another form's answer it is
                    // refused.
                    4 if !sent.is_empty() => {
                        let target = &sent[usize::try_from(next(sent.len() as u64)).unwrap()];
                        let (target_id, target_form, target_is_amendment) =
                            (target.receipt.answer_id, target.form, target.amendment);
                        let body = valid(form, fresh(), Some(target_id), next(1000));
                        let result = session.submit(&body);
                        if target_is_amendment || target_form != form {
                            assert!(
                                matches!(
                                    result,
                                    Err(PresentError::Review {
                                        code: "invalid_amendment",
                                        ..
                                    })
                                ),
                                "{context}: {result:?}"
                            );
                            assert_eq!(session.ledger_bytes(), before, "{context}");
                        } else {
                            let receipt = result.unwrap();
                            assert_eq!(receipt.sequence, sent.len() as u64 + 1, "{context}");
                            sent.push(Sent {
                                body,
                                receipt,
                                amendment: true,
                                form,
                            });
                        }
                    }
                    // An invalid body: a value out of range, or the old
                    // revision after an update.
                    5 => {
                        let mut body: Value =
                            serde_json::from_slice(&valid(form, fresh(), None, next(1000)))
                                .unwrap();
                        let stale = revision > 1 && next(2) == 0;
                        if stale {
                            body["revision"] = serde_json::json!(revision - 1);
                        } else if form == "store-choice" {
                            body["values"]["keep-days"] = serde_json::json!(366 + next(10));
                        } else {
                            body["values"]["choice"] = serde_json::json!("z");
                        }
                        let result = session.submit(&serde_json::to_vec(&body).unwrap());
                        let expected = if stale {
                            "stale_revision"
                        } else {
                            "invalid_answer"
                        };
                        assert!(
                            matches!(&result, Err(PresentError::Review { code, .. }) if *code == expected),
                            "{context}: {result:?}"
                        );
                        assert_eq!(session.ledger_bytes(), before, "{context}");
                    }
                    // A new revision of the same document.
                    _ => {
                        let ParsedDocument::Supported(document) =
                            parse_document(&forms_json).unwrap()
                        else {
                            unreachable!()
                        };
                        revision = session
                            .store
                            .update_document(session.id, ParsedDocument::Supported(document))
                            .unwrap();
                    }
                }
                // The ledger matches the model after every step.
                let events = session.store.responses(session.id).unwrap();
                assert_eq!(events.len(), sent.len(), "{context}");
                for (event, model) in events.iter().zip(&sent) {
                    let record = event.record();
                    assert_eq!(record.answer_id, model.receipt.answer_id, "{context}");
                    assert_eq!(record.sequence, model.receipt.sequence, "{context}");
                    assert_eq!(
                        record.payload_digest,
                        crate::form::sha256_hex(&model.body),
                        "{context}"
                    );
                    assert_eq!(
                        matches!(event, ResponseEvent::Amendment(_)),
                        model.amendment
                    );
                }
                if let Some(bytes) = session.ledger_bytes() {
                    assert!(bytes.ends_with(b"\n"), "{context}");
                }
            }
        }
    }
}
