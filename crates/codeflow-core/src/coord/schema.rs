//! The event envelope and the per-event data, validated the same way when an
//! event is written and when the log is replayed.
//!
//! One line of `events.jsonl` is one envelope:
//!
//! ```text
//! {"schema_version":1,"event":"item_added","subject_id":"FU-001","sequence":7,
//!  "recorded_at":"2026-10-23T09:00:00Z","recorded_by":{...},"data":{...}}
//! ```
//!
//! An envelope whose `schema_version` is newer than this build knows makes
//! the ledger read-only; every event changes state, so none can be skipped.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::actor::{is_follow_up_id, is_lane_id, is_stream_id, RecordedBy, INTAKE};
use super::reference::Link;
use super::text::{check, check_required, LONG, MEDIUM, SHORT};

/// The schema this build writes and reads.
pub const SCHEMA_VERSION: u32 = 1;

/// Every event kind, with its log subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    ItemAdded,
    ItemUpdated,
    ItemChecked,
    ItemHandedOver,
    ItemClosed,
    ItemDropped,
    ItemReopened,
    ItemPurged,
    StreamOpened,
    StreamScopeChanged,
    StreamClosed,
    LaneOpened,
    LaneScopeChanged,
    SeatAttached,
    SeatDetached,
    LaneReported,
    LaneAdopted,
    LaneClosed,
    LaneHandedOver,
    CoordinatorClaimed,
    CoordinatorAdopted,
    CoordinatorReleased,
}

impl EventKind {
    /// Every kind, in documentation order.
    pub const ALL: [Self; 22] = [
        Self::ItemAdded,
        Self::ItemUpdated,
        Self::ItemChecked,
        Self::ItemHandedOver,
        Self::ItemClosed,
        Self::ItemDropped,
        Self::ItemReopened,
        Self::ItemPurged,
        Self::StreamOpened,
        Self::StreamScopeChanged,
        Self::StreamClosed,
        Self::LaneOpened,
        Self::LaneScopeChanged,
        Self::SeatAttached,
        Self::SeatDetached,
        Self::LaneReported,
        Self::LaneAdopted,
        Self::LaneClosed,
        Self::LaneHandedOver,
        Self::CoordinatorClaimed,
        Self::CoordinatorAdopted,
        Self::CoordinatorReleased,
    ];

    /// The kind as written.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ItemAdded => "item_added",
            Self::ItemUpdated => "item_updated",
            Self::ItemChecked => "item_checked",
            Self::ItemHandedOver => "item_handed_over",
            Self::ItemClosed => "item_closed",
            Self::ItemDropped => "item_dropped",
            Self::ItemReopened => "item_reopened",
            Self::ItemPurged => "item_purged",
            Self::StreamOpened => "stream_opened",
            Self::StreamScopeChanged => "stream_scope_changed",
            Self::StreamClosed => "stream_closed",
            Self::LaneOpened => "lane_opened",
            Self::LaneScopeChanged => "lane_scope_changed",
            Self::SeatAttached => "seat_attached",
            Self::SeatDetached => "seat_detached",
            Self::LaneReported => "lane_reported",
            Self::LaneAdopted => "lane_adopted",
            Self::LaneClosed => "lane_closed",
            Self::LaneHandedOver => "lane_handed_over",
            Self::CoordinatorClaimed => "coordinator_claimed",
            Self::CoordinatorAdopted => "coordinator_adopted",
            Self::CoordinatorReleased => "coordinator_released",
        }
    }

    /// What the event's `subject_id` names.
    #[must_use]
    pub fn subject(self) -> Subject {
        match self {
            Self::ItemAdded
            | Self::ItemUpdated
            | Self::ItemChecked
            | Self::ItemHandedOver
            | Self::ItemClosed
            | Self::ItemDropped
            | Self::ItemReopened
            | Self::ItemPurged => Subject::FollowUp,
            Self::StreamOpened
            | Self::StreamScopeChanged
            | Self::StreamClosed
            | Self::CoordinatorClaimed
            | Self::CoordinatorAdopted
            | Self::CoordinatorReleased => Subject::Stream,
            Self::LaneOpened
            | Self::LaneScopeChanged
            | Self::SeatAttached
            | Self::SeatDetached
            | Self::LaneReported
            | Self::LaneAdopted
            | Self::LaneClosed
            | Self::LaneHandedOver => Subject::Lane,
        }
    }
}

/// The kind of id an event's subject is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subject {
    FollowUp,
    Lane,
    Stream,
}

/// One line of the log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub schema_version: u32,
    pub event: EventKind,
    pub subject_id: String,
    pub sequence: u64,
    /// UTC, for people; order comes from `sequence` alone.
    pub recorded_at: String,
    pub recorded_by: RecordedBy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// A retried operation with the same key is recorded once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    /// The subject's revision the writer read; a stale one is refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
    pub data: Value,
}

/// What reading one line found.
#[derive(Debug)]
pub enum Line {
    /// A valid envelope with its typed data.
    Event(Box<Envelope>, Box<Data>),
    /// A schema this build does not know.
    NewerSchema(u32),
    /// A line that is not a valid event.
    Malformed(String),
}

/// Parse and validate one line. Validation is the same on input and on
/// replay: the envelope's fields, the actor, and the typed data.
#[must_use]
pub fn parse_line(text: &str) -> Line {
    let value: Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(error) => return Line::Malformed(format!("not JSON: {error}")),
    };
    let version = value
        .get("schema_version")
        .and_then(Value::as_u64)
        .and_then(|version| u32::try_from(version).ok());
    match version {
        Some(version) if version > SCHEMA_VERSION => return Line::NewerSchema(version),
        Some(0) | None => return Line::Malformed("no valid schema_version".into()),
        Some(_) => {}
    }
    let envelope: Envelope = match serde_json::from_value(value) {
        Ok(envelope) => envelope,
        Err(error) => return Line::Malformed(format!("envelope: {error}")),
    };
    match validate(&envelope) {
        Ok(data) => Line::Event(Box::new(envelope), Box::new(data)),
        Err(error) => Line::Malformed(error),
    }
}

/// Validate an envelope and return its typed data.
///
/// # Errors
///
/// The reason the envelope or its data is invalid.
pub fn validate(envelope: &Envelope) -> Result<Data, String> {
    if envelope.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "schema_version {} is not {SCHEMA_VERSION}",
            envelope.schema_version
        ));
    }
    if envelope.sequence == 0 {
        return Err("sequence starts at 1".into());
    }
    super::parse_time(&envelope.recorded_at)?;
    envelope.recorded_by.validate()?;
    if let Some(comment) = &envelope.comment {
        check("comment", comment, MEDIUM, true)?;
    }
    if let Some(key) = &envelope.idempotency_key {
        check_required("idempotency_key", key, SHORT, false)?;
    }
    let subject_ok = match envelope.event.subject() {
        Subject::FollowUp => is_follow_up_id(&envelope.subject_id),
        Subject::Lane => is_lane_id(&envelope.subject_id),
        Subject::Stream => is_stream_id(&envelope.subject_id),
    };
    if !subject_ok {
        return Err(format!(
            "subject `{}` does not fit {}",
            super::text::shown(&envelope.subject_id),
            envelope.event.as_str()
        ));
    }
    let data = Data::from_value(envelope.event, &envelope.data)?;
    data.validate()?;
    Ok(data)
}

/// Where a follow-up comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Operator,
    Coordinator,
    Lane,
    Review,
    Issue,
    Outside,
}

impl Source {
    /// Parse a written source.
    ///
    /// # Errors
    ///
    /// The reason the text is no source.
    pub fn parse(text: &str) -> Result<Self, String> {
        serde_json::from_value(Value::String(text.into())).map_err(|_| {
            format!(
                "`{}` is not a source: use operator, coordinator, lane, review, issue or outside",
                super::text::shown(text)
            )
        })
    }

    /// The source as written.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Operator => "operator",
            Self::Coordinator => "coordinator",
            Self::Lane => "lane",
            Self::Review => "review",
            Self::Issue => "issue",
            Self::Outside => "outside",
        }
    }
}

/// A suggestion the adder considered and rejected, with its reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rejected {
    pub reference: String,
    pub reason: String,
}

/// An observed state of one link, recorded so a later change makes the
/// follow-up due.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    /// The link as written.
    pub link: String,
    /// The state observed: `claimed`, `complete`, `merged`, `unknown` ...
    pub state: String,
    /// The version observed (a tip commit, a sequence), when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// The full follow-up record, as `item_added` carries it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FollowUp {
    pub title: String,
    pub description: String,
    pub done_when: String,
    pub source: Source,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_reference: Option<String>,
    /// A stream id, or `intake`.
    pub stream: String,
    /// `coordinator` or a lane id.
    pub holder: String,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_link_reason: Option<String>,
    /// The effective next check, UTC.
    pub next_check: String,
    /// Other follow-ups or lanes, possibly in other streams, this waits on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waits_on: Vec<String>,
    /// A condition the operator must settle; never a holder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub awaiting_operator: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejected_suggestions: Vec<Rejected>,
    /// The links' states when the follow-up was added.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observed: Vec<Observation>,
}

/// An `item_updated` change; `source` is never updatable.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemUpdate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_when: Option<String>,
    /// The new links, replacing the old.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<Link>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_link_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_check: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waits_on: Option<Vec<String>>,
    /// An empty string clears the condition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub awaiting_operator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed: Option<Vec<Observation>>,
}

/// An `item_checked` observation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemCheck {
    /// What was observed, in words.
    pub observed: String,
    pub next_check: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observations: Vec<Observation>,
    /// Where an answer came from when the holder records someone else's
    /// (the operator's answer is recorded with its source, never as the
    /// operator).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer_source: Option<String>,
}

/// An `item_handed_over` change of holder or stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandOver {
    pub to_holder: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_stream: Option<String>,
}

/// How strongly closure evidence was checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    /// The closer says so; the reference can be opened to check.
    Asserted,
    /// `CodeFlow` observed it on the target.
    MachineVerified,
}

/// The proof an `item_closed` carries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub kind: EvidenceKind,
    /// A reference a fresh session can open: a link as written, a URL.
    pub reference: String,
    /// What made the observation (`records on origin/main`, `gh`, ...).
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    pub observed_at: String,
}

/// An `item_dropped` reason and the disposition of what it leaves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Drop {
    pub reason: String,
    pub disposition: String,
}

/// A reason, for `item_reopened`, `stream_closed`, `seat_detached`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reason {
    pub reason: String,
}

/// A descriptive scope: links only (`stream_opened`, `stream_scope_changed`,
/// `lane_scope_changed`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    pub scope: Vec<String>,
}

/// `lane_opened`: the lane's stream and scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaneOpen {
    pub stream: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    pub scope: Vec<String>,
}

/// How a seat lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeatKind {
    External,
    Resumable,
    InProcess,
}

/// `seat_attached`: the harness session attached to a lane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seat {
    pub seat_instance: String,
    pub kind: SeatKind,
    pub harness: String,
}

/// `seat_detached`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeatDetach {
    pub seat_instance: String,
    pub reason: String,
}

/// `lane_reported`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub summary: String,
}

/// `lane_adopted` and `coordinator_adopted`: the compare-and-swap of a claim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Adopt {
    pub expected_holder: String,
    pub expected_generation: u64,
    pub new_holder: String,
    pub generation: u64,
}

/// How a lane ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaneOutcome {
    Done,
    Abandoned,
    Superseded,
}

/// `lane_closed`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaneClose {
    pub outcome: LaneOutcome,
    /// Done: the evidence. Abandoned: the reason.
    pub detail: String,
    /// What happens to the lane's open follow-ups.
    pub disposition: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub successor: Option<String>,
}

/// `lane_handed_over`: a lane moved between streams by both coordinators.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaneMove {
    pub from_stream: String,
    pub to_stream: String,
    /// The receiving coordinator's instance, who agreed to the move.
    pub accepted_by: String,
}

/// `coordinator_claimed`: the first claim of a stream's lead, or a claim
/// after a release.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    pub generation: u64,
}

/// `item_purged`: who purged an item's text and when come from the
/// envelope; the data holds only how many events were rewritten.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Purge {
    pub events: u64,
}

/// `coordinator_released` and other events with no data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

/// The typed data of an event.
#[derive(Debug, Clone, PartialEq)]
pub enum Data {
    ItemAdded(FollowUp),
    ItemUpdated(ItemUpdate),
    ItemChecked(ItemCheck),
    ItemHandedOver(HandOver),
    ItemClosed(Evidence),
    ItemDropped(Drop),
    ItemReopened(Reason),
    ItemPurged(Purge),
    StreamOpened(Scope),
    StreamScopeChanged(Scope),
    StreamClosed(Reason),
    LaneOpened(LaneOpen),
    LaneScopeChanged(Scope),
    SeatAttached(Seat),
    SeatDetached(SeatDetach),
    LaneReported(Report),
    LaneAdopted(Adopt),
    LaneClosed(LaneClose),
    LaneHandedOver(LaneMove),
    CoordinatorClaimed(Claim),
    CoordinatorAdopted(Adopt),
    CoordinatorReleased(Empty),
}

impl Data {
    /// Read the typed data of `kind`.
    ///
    /// # Errors
    ///
    /// The reason the data does not fit the kind.
    pub fn from_value(kind: EventKind, value: &Value) -> Result<Self, String> {
        fn typed<T: serde::de::DeserializeOwned>(
            kind: EventKind,
            value: &Value,
        ) -> Result<T, String> {
            serde_json::from_value(value.clone())
                .map_err(|error| format!("{}: {error}", kind.as_str()))
        }
        Ok(match kind {
            EventKind::ItemAdded => Self::ItemAdded(typed(kind, value)?),
            EventKind::ItemUpdated => Self::ItemUpdated(typed(kind, value)?),
            EventKind::ItemChecked => Self::ItemChecked(typed(kind, value)?),
            EventKind::ItemHandedOver => Self::ItemHandedOver(typed(kind, value)?),
            EventKind::ItemClosed => Self::ItemClosed(typed(kind, value)?),
            EventKind::ItemDropped => Self::ItemDropped(typed(kind, value)?),
            EventKind::ItemReopened => Self::ItemReopened(typed(kind, value)?),
            EventKind::ItemPurged => Self::ItemPurged(typed(kind, value)?),
            EventKind::StreamOpened => Self::StreamOpened(typed(kind, value)?),
            EventKind::StreamScopeChanged => Self::StreamScopeChanged(typed(kind, value)?),
            EventKind::StreamClosed => Self::StreamClosed(typed(kind, value)?),
            EventKind::LaneOpened => Self::LaneOpened(typed(kind, value)?),
            EventKind::LaneScopeChanged => Self::LaneScopeChanged(typed(kind, value)?),
            EventKind::SeatAttached => Self::SeatAttached(typed(kind, value)?),
            EventKind::SeatDetached => Self::SeatDetached(typed(kind, value)?),
            EventKind::LaneReported => Self::LaneReported(typed(kind, value)?),
            EventKind::LaneAdopted => Self::LaneAdopted(typed(kind, value)?),
            EventKind::LaneClosed => Self::LaneClosed(typed(kind, value)?),
            EventKind::LaneHandedOver => Self::LaneHandedOver(typed(kind, value)?),
            EventKind::CoordinatorClaimed => Self::CoordinatorClaimed(typed(kind, value)?),
            EventKind::CoordinatorAdopted => Self::CoordinatorAdopted(typed(kind, value)?),
            EventKind::CoordinatorReleased => Self::CoordinatorReleased(typed(kind, value)?),
        })
    }

    /// The data as JSON.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let value = match self {
            Self::ItemAdded(data) => serde_json::to_value(data),
            Self::ItemUpdated(data) => serde_json::to_value(data),
            Self::ItemChecked(data) => serde_json::to_value(data),
            Self::ItemHandedOver(data) => serde_json::to_value(data),
            Self::ItemClosed(data) => serde_json::to_value(data),
            Self::ItemDropped(data) => serde_json::to_value(data),
            Self::ItemPurged(data) => serde_json::to_value(data),
            Self::ItemReopened(data) | Self::StreamClosed(data) => serde_json::to_value(data),
            Self::StreamOpened(data)
            | Self::StreamScopeChanged(data)
            | Self::LaneScopeChanged(data) => serde_json::to_value(data),
            Self::LaneOpened(data) => serde_json::to_value(data),
            Self::SeatAttached(data) => serde_json::to_value(data),
            Self::SeatDetached(data) => serde_json::to_value(data),
            Self::LaneReported(data) => serde_json::to_value(data),
            Self::LaneAdopted(data) | Self::CoordinatorAdopted(data) => serde_json::to_value(data),
            Self::LaneClosed(data) => serde_json::to_value(data),
            Self::LaneHandedOver(data) => serde_json::to_value(data),
            Self::CoordinatorClaimed(data) => serde_json::to_value(data),
            Self::CoordinatorReleased(data) => serde_json::to_value(data),
        };
        value.unwrap_or(Value::Null)
    }

    /// The kind of this data.
    #[must_use]
    pub fn kind(&self) -> EventKind {
        match self {
            Self::ItemAdded(_) => EventKind::ItemAdded,
            Self::ItemUpdated(_) => EventKind::ItemUpdated,
            Self::ItemChecked(_) => EventKind::ItemChecked,
            Self::ItemHandedOver(_) => EventKind::ItemHandedOver,
            Self::ItemClosed(_) => EventKind::ItemClosed,
            Self::ItemDropped(_) => EventKind::ItemDropped,
            Self::ItemReopened(_) => EventKind::ItemReopened,
            Self::ItemPurged(_) => EventKind::ItemPurged,
            Self::StreamOpened(_) => EventKind::StreamOpened,
            Self::StreamScopeChanged(_) => EventKind::StreamScopeChanged,
            Self::StreamClosed(_) => EventKind::StreamClosed,
            Self::LaneOpened(_) => EventKind::LaneOpened,
            Self::LaneScopeChanged(_) => EventKind::LaneScopeChanged,
            Self::SeatAttached(_) => EventKind::SeatAttached,
            Self::SeatDetached(_) => EventKind::SeatDetached,
            Self::LaneReported(_) => EventKind::LaneReported,
            Self::LaneAdopted(_) => EventKind::LaneAdopted,
            Self::LaneClosed(_) => EventKind::LaneClosed,
            Self::LaneHandedOver(_) => EventKind::LaneHandedOver,
            Self::CoordinatorClaimed(_) => EventKind::CoordinatorClaimed,
            Self::CoordinatorAdopted(_) => EventKind::CoordinatorAdopted,
            Self::CoordinatorReleased(_) => EventKind::CoordinatorReleased,
        }
    }

    /// Check every field of the data: required text, bounds and the secret
    /// scan, ids, times and links.
    ///
    /// # Errors
    ///
    /// The reason the data is invalid.
    #[allow(clippy::too_many_lines)]
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::ItemAdded(item) => validate_follow_up(item),
            Self::ItemUpdated(update) => {
                if let Some(title) = &update.title {
                    check_required("title", title, SHORT, false)?;
                }
                if let Some(description) = &update.description {
                    check_required("description", description, LONG, true)?;
                }
                if let Some(done_when) = &update.done_when {
                    check_required("done_when", done_when, MEDIUM, true)?;
                }
                if let Some(links) = &update.links {
                    validate_links(links)?;
                }
                if let Some(reason) = &update.no_link_reason {
                    check_required("no_link_reason", reason, MEDIUM, true)?;
                }
                if let Some(next) = &update.next_check {
                    super::parse_time(next)?;
                }
                if let Some(waits) = &update.waits_on {
                    validate_waits(waits)?;
                }
                if let Some(condition) = &update.awaiting_operator {
                    check("awaiting_operator", condition, MEDIUM, true)?;
                }
                if let Some(observed) = &update.observed {
                    validate_observations(observed)?;
                }
                if update == &ItemUpdate::default() {
                    return Err("item_updated changes nothing".into());
                }
                Ok(())
            }
            Self::ItemChecked(check_data) => {
                check_required("observed", &check_data.observed, MEDIUM, true)?;
                super::parse_time(&check_data.next_check)?;
                validate_observations(&check_data.observations)?;
                if let Some(source) = &check_data.answer_source {
                    check_required("answer_source", source, SHORT, false)?;
                }
                Ok(())
            }
            Self::ItemHandedOver(hand) => {
                validate_holder(&hand.to_holder)?;
                if let Some(stream) = &hand.to_stream {
                    validate_stream(stream)?;
                }
                Ok(())
            }
            Self::ItemClosed(evidence) => {
                check_required("evidence.reference", &evidence.reference, MEDIUM, false)?;
                check_required("evidence.source", &evidence.source, SHORT, false)?;
                for (field, value) in [
                    ("evidence.repository", &evidence.repository),
                    ("evidence.target", &evidence.target),
                    ("evidence.revision", &evidence.revision),
                ] {
                    if let Some(value) = value {
                        check_required(field, value, SHORT, false)?;
                    }
                }
                super::parse_time(&evidence.observed_at)?;
                if evidence.kind == EvidenceKind::MachineVerified
                    && (evidence.target.is_none() || evidence.revision.is_none())
                {
                    return Err("machine-verified evidence names its target and revision".into());
                }
                Ok(())
            }
            Self::ItemDropped(drop) => {
                check_required("reason", &drop.reason, MEDIUM, true)?;
                check_required("disposition", &drop.disposition, MEDIUM, true)
            }
            Self::ItemPurged(purge) => {
                if purge.events == 0 {
                    return Err("item_purged rewrites at least one event".into());
                }
                Ok(())
            }
            Self::ItemReopened(reason) | Self::StreamClosed(reason) => {
                check_required("reason", &reason.reason, MEDIUM, true)
            }
            Self::StreamOpened(scope)
            | Self::StreamScopeChanged(scope)
            | Self::LaneScopeChanged(scope) => validate_scope(scope.slug.as_deref(), &scope.scope),
            Self::LaneOpened(open) => {
                if !is_stream_id(&open.stream) {
                    return Err("a lane belongs to a stream WS-NNN".into());
                }
                validate_scope(open.slug.as_deref(), &open.scope)
            }
            Self::SeatAttached(seat) => {
                check_required("seat_instance", &seat.seat_instance, 120, false)?;
                check_required("harness", &seat.harness, 120, false)
            }
            Self::SeatDetached(detach) => {
                check_required("seat_instance", &detach.seat_instance, 120, false)?;
                check_required("reason", &detach.reason, MEDIUM, true)
            }
            Self::LaneReported(report) => check_required("summary", &report.summary, LONG, true),
            Self::LaneAdopted(adopt) | Self::CoordinatorAdopted(adopt) => {
                check_required("expected_holder", &adopt.expected_holder, 120, false)?;
                check_required("new_holder", &adopt.new_holder, 120, false)?;
                if adopt.generation != adopt.expected_generation + 1 {
                    return Err("an adoption raises the generation by one".into());
                }
                Ok(())
            }
            Self::LaneClosed(close) => {
                check_required("detail", &close.detail, MEDIUM, true)?;
                check_required("disposition", &close.disposition, MEDIUM, true)?;
                match (close.outcome, &close.successor) {
                    (LaneOutcome::Superseded, Some(successor)) if is_lane_id(successor) => Ok(()),
                    (LaneOutcome::Superseded, _) => {
                        Err("a superseded lane names its successor lane".into())
                    }
                    (_, Some(_)) => Err("only a superseded lane names a successor".into()),
                    (_, None) => Ok(()),
                }
            }
            Self::LaneHandedOver(moved) => {
                if !is_stream_id(&moved.from_stream) || !is_stream_id(&moved.to_stream) {
                    return Err("a lane moves between two streams WS-NNN".into());
                }
                if moved.from_stream == moved.to_stream {
                    return Err("a lane hand-over names another stream".into());
                }
                check_required("accepted_by", &moved.accepted_by, 120, false)
            }
            Self::CoordinatorClaimed(claim) => {
                if claim.generation == 0 {
                    return Err("a claim generation starts at 1".into());
                }
                Ok(())
            }
            Self::CoordinatorReleased(_) => Ok(()),
        }
    }
}

fn validate_follow_up(item: &FollowUp) -> Result<(), String> {
    check_required("title", &item.title, SHORT, false)?;
    check_required("description", &item.description, LONG, true)?;
    check_required("done_when", &item.done_when, MEDIUM, true)?;
    if let Some(reference) = &item.source_reference {
        check_required("source_reference", reference, SHORT, false)?;
    }
    validate_stream(&item.stream)?;
    validate_holder(&item.holder)?;
    validate_links(&item.links)?;
    match (&item.links.is_empty(), &item.no_link_reason) {
        (true, None) => {
            return Err("a follow-up carries a typed link or a no_link_reason".into());
        }
        (false, Some(_)) => {
            return Err("a follow-up with links carries no no_link_reason".into());
        }
        (true, Some(reason)) => check_required("no_link_reason", reason, MEDIUM, true)?,
        (false, None) => {}
    }
    super::parse_time(&item.next_check)?;
    validate_waits(&item.waits_on)?;
    if let Some(condition) = &item.awaiting_operator {
        check_required("awaiting_operator", condition, MEDIUM, true)?;
    }
    for rejected in &item.rejected_suggestions {
        check_required(
            "rejected_suggestions.reference",
            &rejected.reference,
            SHORT,
            false,
        )?;
        check_required(
            "rejected_suggestions.reason",
            &rejected.reason,
            MEDIUM,
            true,
        )?;
    }
    validate_observations(&item.observed)
}

fn validate_links(links: &[Link]) -> Result<(), String> {
    if links.len() > 32 {
        return Err("a follow-up carries at most 32 links".into());
    }
    for link in links {
        link.validate()?;
    }
    let mut seen = std::collections::BTreeSet::new();
    for link in links {
        if !seen.insert(link.display()) {
            return Err(format!("the link `{}` is given twice", link.display()));
        }
    }
    Ok(())
}

fn validate_waits(waits: &[String]) -> Result<(), String> {
    if waits.len() > 32 {
        return Err("a follow-up waits on at most 32 items".into());
    }
    for wait in waits {
        if !is_follow_up_id(wait) && !is_lane_id(wait) {
            return Err(format!(
                "`{}` in waits_on is neither a follow-up nor a lane id",
                super::text::shown(wait)
            ));
        }
    }
    Ok(())
}

fn validate_observations(observations: &[Observation]) -> Result<(), String> {
    if observations.len() > 64 {
        return Err("at most 64 observations".into());
    }
    for observation in observations {
        check_required("observation.link", &observation.link, SHORT, false)?;
        check_required("observation.state", &observation.state, SHORT, false)?;
        if let Some(version) = &observation.version {
            check_required("observation.version", version, SHORT, false)?;
        }
    }
    Ok(())
}

fn validate_stream(stream: &str) -> Result<(), String> {
    if stream == INTAKE || is_stream_id(stream) {
        Ok(())
    } else {
        Err(format!(
            "`{}` is not a stream: use WS-NNN or {INTAKE}",
            super::text::shown(stream)
        ))
    }
}

fn validate_holder(holder: &str) -> Result<(), String> {
    if holder == "coordinator" || is_lane_id(holder) {
        Ok(())
    } else {
        Err(format!(
            "`{}` is not a holder: use coordinator or a lane id LN-NNN",
            super::text::shown(holder)
        ))
    }
}

fn validate_scope(slug: Option<&str>, scope: &[String]) -> Result<(), String> {
    if let Some(slug) = slug {
        if slug.is_empty()
            || slug.len() > 60
            || !slug
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err("a slug is lowercase letters, digits and hyphens".into());
        }
    }
    if scope.len() > 32 {
        return Err("a scope names at most 32 links".into());
    }
    for entry in scope {
        check_required("scope", entry, SHORT, false)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_round_trips_its_name() {
        for kind in EventKind::ALL {
            let text = serde_json::to_string(&kind).unwrap();
            assert_eq!(text, format!("\"{}\"", kind.as_str()));
        }
    }

    #[test]
    fn a_newer_schema_is_reported_not_parsed() {
        let line = r#"{"schema_version":2,"event":"whatever"}"#;
        assert!(matches!(parse_line(line), Line::NewerSchema(2)));
        assert!(matches!(parse_line("{"), Line::Malformed(_)));
        assert!(matches!(
            parse_line(r#"{"schema_version":1,"event":"item_added"}"#),
            Line::Malformed(_)
        ));
    }
}
