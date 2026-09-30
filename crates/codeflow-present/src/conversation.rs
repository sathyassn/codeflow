//! Conversation transitions share the answer ledger and its durability bounds.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    responses::{
        Actor, AnswerRecord, Ledger, ReopenRecord, ReplyRecord, ResponseEvent, ThreadTransition,
        TombstoneRecord,
    },
    state::{now_unix, FeedbackEvent, FeedbackSnapshot, SessionStatus, SessionStore},
    PresentError, Result,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThreadRequest {
    pub session_id: Uuid,
    pub target: Uuid,
    pub note_id: Option<Uuid>,
    pub revision: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct ConversationSnapshot {
    pub feedback: FeedbackSnapshot,
    pub answers: Vec<AnswerRecord>,
    pub replies: Vec<ReplyRecord>,
    pub reopens: Vec<ReopenRecord>,
    pub tombstones: Vec<TombstoneRecord>,
    pub acknowledged: Vec<Uuid>,
    pub omitted_older: usize,
}

/// Redact only the public projection, never the private append-only source.
pub(crate) fn redact(events: &mut [FeedbackEvent], ledger: &[ResponseEvent]) {
    for event in events {
        let FeedbackEvent::Received { envelope, .. } = event else {
            continue;
        };
        for note in &mut envelope.notes {
            if deleted(ledger, envelope.event_id, Some(note.id)) {
                note.body = "Deleted by the reviewer".into();
                note.block_label = "Deleted note".into();
                note.selector = None;
                note.element_selector = None;
                note.region_selector = None;
                note.entity_selector = None;
                note.excerpt = None;
            }
        }
    }
}

pub(crate) fn deleted(ledger: &[ResponseEvent], target: Uuid, note: Option<Uuid>) -> bool {
    ledger.iter().any(|event| {
        matches!(event, ResponseEvent::Tombstone(r)
        if r.transition.target == target && r.transition.note_id == note)
    })
}

pub(crate) fn public_responses(ledger: &[ResponseEvent]) -> Vec<ResponseEvent> {
    ledger
        .iter()
        .filter(|event| {
            !matches!(event, ResponseEvent::Reply(r)
        if deleted(ledger, r.transition.target, r.transition.note_id))
        })
        .cloned()
        .collect()
}

impl SessionStore {
    pub fn reply(&self, id: Uuid, target: Uuid, note_id: Option<Uuid>, text: &str) -> Result<Uuid> {
        if text.trim().is_empty() || text.len() > crate::limits::MAX_REPLY_TEXT_BYTES {
            return Err(PresentError::InvalidRequest(format!(
                "reply must be 1 to {} bytes",
                crate::limits::MAX_REPLY_TEXT_BYTES
            )));
        }
        let _lease = self.prepare_control_mutation()?;
        let _lock = self.lock_session(id)?;
        let session = self.load(id)?;
        if session.status != SessionStatus::Active {
            return Err(PresentError::SessionClosed(id.to_string()));
        }
        let mut ledger = Ledger::open(self.responses_path(id)?, id)?;
        self.check_thread(id, target, note_id, &ledger.events)?;
        let reply_id = Uuid::new_v4();
        ledger.append(
            self,
            ResponseEvent::Reply(ReplyRecord {
                reply_id,
                text: text.into(),
                transition: ThreadTransition {
                    sequence: ledger.next_sequence(),
                    target,
                    note_id,
                    revision: session.current_revision,
                    actor: Actor::Agent,
                    at_unix: now_unix()?,
                },
            }),
        )?;
        Ok(reply_id)
    }

    /// A reviewer can reopen an acknowledged or resolved thread, or delete a note.
    /// Retrying an already open/deleted transition returns the durable original.
    pub fn change_thread(
        &self,
        id: Uuid,
        request: &ThreadRequest,
        tombstone: bool,
    ) -> Result<Uuid> {
        if request.session_id != id {
            return Err(PresentError::InvalidRequest("wrong session_id".into()));
        }
        let _lease = self.prepare_control_mutation()?;
        let _lock = self.lock_session(id)?;
        let session = self.load(id)?;
        if session.status != SessionStatus::Active {
            return Err(PresentError::SessionClosed(id.to_string()));
        }
        if let Some(expected) = request.revision {
            if expected != session.current_revision {
                return Err(PresentError::RevisionConflict {
                    expected,
                    current: session.current_revision,
                });
            }
        }
        let mut ledger = Ledger::open(self.responses_path(id)?, id)?;
        let matches =
            |r: &ThreadTransition| r.target == request.target && r.note_id == request.note_id;
        if tombstone {
            if let Some(id) = ledger.events.iter().find_map(|event| match event {
                ResponseEvent::Tombstone(r) if matches(&r.transition) => Some(r.tombstone_id),
                _ => None,
            }) {
                ledger.sync()?;
                return Ok(id);
            }
        }
        self.check_thread(id, request.target, request.note_id, &ledger.events)?;
        if tombstone && request.note_id.is_none() {
            return Err(PresentError::InvalidRequest(
                "delete names a note in a review".into(),
            ));
        }
        if !tombstone {
            if let Some(reopen) = ledger.events.iter().rev().find_map(|event| match event {
                ResponseEvent::Reopen(r) if matches(&r.transition) => Some(r),
                _ => None,
            }) {
                if !ledger.events.iter().any(|event| matches!(event, ResponseEvent::Acknowledged(s) if s.target == reopen.reopen_id)) {
                    let id = reopen.reopen_id; ledger.sync()?; return Ok(id);
                }
            }
            let acknowledged = ledger.events.iter().any(|event| {
                matches!(event,
                ResponseEvent::Acknowledged(s) if s.target == request.target)
            });
            let resolved = self.read_events_unlocked(id)?.iter().any(|event| matches!(event,
                FeedbackEvent::Addressed {event_id,..} | FeedbackEvent::Dismissed {event_id,..} if *event_id == request.target));
            if !acknowledged && !resolved {
                return Err(PresentError::InvalidRequest(
                    "only an acknowledged or resolved thread can reopen".into(),
                ));
            }
        }
        let event_id = Uuid::new_v4();
        let transition = ThreadTransition {
            sequence: ledger.next_sequence(),
            target: request.target,
            note_id: request.note_id,
            revision: session.current_revision,
            actor: Actor::Operator,
            at_unix: now_unix()?,
        };
        ledger.append(
            self,
            if tombstone {
                ResponseEvent::Tombstone(TombstoneRecord {
                    tombstone_id: event_id,
                    transition,
                })
            } else {
                ResponseEvent::Reopen(ReopenRecord {
                    reopen_id: event_id,
                    transition,
                })
            },
        )?;
        Ok(event_id)
    }

    fn check_thread(
        &self,
        id: Uuid,
        target: Uuid,
        note: Option<Uuid>,
        ledger: &[ResponseEvent],
    ) -> Result<()> {
        let known = self.read_events_unlocked(id)?.iter().any(|event| matches!(event,
            FeedbackEvent::Received {envelope,..} if envelope.event_id == target && note.is_none_or(|n| envelope.notes.iter().any(|v| v.id == n))))
            || (note.is_none() && ledger.iter().filter_map(ResponseEvent::answer).any(|a| a.answer_id == target));
        if !known || deleted(ledger, target, note) {
            return Err(PresentError::InvalidRequest(
                "unknown or deleted thread/note".into(),
            ));
        }
        Ok(())
    }

    pub fn conversation(&self, id: Uuid) -> Result<ConversationSnapshot> {
        let feedback = self.feedback_snapshot(id)?;
        let ledger = self.responses(id)?;
        let mut answers = Vec::new();
        let mut replies = Vec::new();
        let mut reopens = Vec::new();
        let mut count = 0;
        let tombstones = ledger
            .iter()
            .filter_map(|e| match e {
                ResponseEvent::Tombstone(r)
                    if feedback
                        .items
                        .iter()
                        .any(|f| f.event_id == r.transition.target) =>
                {
                    Some(r.clone())
                }
                _ => None,
            })
            .collect();
        let acknowledged = ledger
            .iter()
            .filter_map(|e| match e {
                ResponseEvent::Acknowledged(s) => Some(s.target),
                _ => None,
            })
            .collect();
        // Bound rail data independently of the on-disk ledger. Full history stays in the CLI.
        for event in ledger.iter().rev() {
            if event.answer().is_none() && event.transition().is_none() {
                continue;
            }
            count += 1;
            if count > crate::limits::MAX_VISIBLE_FEEDBACK {
                continue;
            }
            match event {
                ResponseEvent::Answer(a) | ResponseEvent::Amendment(a) => answers.push(a.clone()),
                ResponseEvent::Reply(r) => replies.push(r.clone()),
                ResponseEvent::Reopen(r) => reopens.push(r.clone()),
                _ => {}
            }
        }
        answers.reverse();
        replies.reverse();
        reopens.reverse();
        Ok(ConversationSnapshot {
            feedback,
            answers,
            replies,
            reopens,
            tombstones,
            acknowledged,
            omitted_older: count.saturating_sub(crate::limits::MAX_VISIBLE_FEEDBACK),
        })
    }
}
