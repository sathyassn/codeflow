//! The fold of the log into state, the transition rules and the permission
//! matrix. [`Ledger::apply`] is the one judge: the store calls it before it
//! appends an event, and the fold calls it again for every line it replays,
//! so an event that breaks a rule can neither be written nor read as valid.
//!
//! Permission matrix, one row per action:
//!
//! ```text
//! action      who may act
//! add         any bound actor
//! update      the holder, the operator
//! check       the holder, the operator
//! hand-over   the holder, the operator; any coordinator takes an intake item
//! close       the holder, the operator
//! drop        the holder, the stream's coordinator (a reason always), the operator
//! reopen      the holder, the stream's coordinator, the operator
//! purge       the holder, the operator
//! ```
//!
//! The holder is `coordinator` (the bound coordinator of the item's stream;
//! any coordinator for an intake item) or a lane id (the lane's bound seat).
//! A coordinator binds by its stream's lease, instance and generation; a lane
//! by its seat instance and generation. A stale writer is refused.

use std::collections::{BTreeMap, BTreeSet};

use super::actor::{is_follow_up_id, is_lane_id, RecordedBy, Role, INTAKE};
use super::reference::{Link, Target};
use super::schema::{Data, Envelope, Evidence, FollowUp, ItemCheck, LaneOutcome, Line};

/// What state a follow-up is in.
#[derive(Debug, Clone, PartialEq)]
pub enum ItemState {
    Open,
    Closed(Evidence),
    Dropped { reason: String, disposition: String },
}

impl ItemState {
    /// The state as written.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed(_) => "closed",
            Self::Dropped { .. } => "dropped",
        }
    }
}

/// One follow-up, folded.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: String,
    /// The record with every update applied.
    pub record: FollowUp,
    pub state: ItemState,
    /// Events applied to this item; `expected_revision` is compared to it.
    pub revision: u64,
    pub added_at: String,
    pub added_by: RecordedBy,
    /// The last check: when, and what it recorded.
    pub last_check: Option<(String, ItemCheck)>,
    /// Every closure proof, kept after a reopen.
    pub proofs: Vec<Evidence>,
    /// Every reopen reason, in order.
    pub reopened: Vec<String>,
    /// Each purge of the item's text: when and by which role.
    pub purged: Vec<(String, String)>,
    /// The links' states last recorded (at add, update or check): a change
    /// from these makes the item due.
    pub baseline: Vec<super::schema::Observation>,
    /// The sequence of the last event on this item.
    pub last_sequence: u64,
}

impl Item {
    /// The links' states the item last recorded: at the latest of its add,
    /// a dependency update and its last check.
    #[must_use]
    pub fn baseline(&self) -> &[super::schema::Observation] {
        &self.baseline
    }
}

/// A coordinator's lease on a stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lease {
    pub instance: String,
    pub generation: u64,
}

/// One work stream, folded.
#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    pub id: String,
    pub slug: Option<String>,
    pub scope: Vec<String>,
    pub closed: bool,
    pub lease: Option<Lease>,
    /// The highest generation ever claimed; a new claim is one above it.
    pub generation: u64,
}

/// One lane, folded.
#[derive(Debug, Clone, PartialEq)]
pub struct Lane {
    pub id: String,
    pub stream: String,
    pub slug: Option<String>,
    pub scope: Vec<String>,
    pub closed: Option<LaneOutcome>,
    pub generation: u64,
    pub seat: Option<String>,
    /// When the lane last reported.
    pub reported_at: Option<String>,
    pub last_sequence: u64,
}

/// The whole folded ledger.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ledger {
    pub last_sequence: u64,
    pub items: BTreeMap<String, Item>,
    pub streams: BTreeMap<String, Stream>,
    pub lanes: BTreeMap<String, Lane>,
    /// Idempotency key to the sequence that recorded it and the digest of
    /// the operation it recorded.
    pub keys: BTreeMap<String, (u64, String)>,
    /// The highest follow-up and lane numbers seen in the log.
    pub max_follow_up: u64,
    pub max_lane: u64,
}

/// An actor bound to what it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bound {
    Operator,
    Coordinator { stream: String },
    Lane { id: String, stream: String },
}

/// The result of folding a log.
#[derive(Debug, Clone, Default)]
pub struct Folded {
    /// The state of the valid prefix.
    pub ledger: Ledger,
    /// Why the ledger is read-only, when it is: interior damage, a bad
    /// sequence, a broken transition or a newer schema.
    pub damage: Option<String>,
    /// Bytes after the last line break: a write in progress or a torn tail.
    pub tail: usize,
    /// Byte length of the committed prefix that folded cleanly.
    pub valid_bytes: usize,
    /// Complete lines read.
    pub lines: usize,
}

/// Fold the committed prefix of a log: every complete line, in order.
/// Folding stops at the first line that is not a valid event or breaks a
/// rule; the state is then that of the valid prefix and `damage` says why.
#[must_use]
pub fn fold(bytes: &[u8]) -> Folded {
    let committed = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    let mut folded = Folded {
        tail: bytes.len() - committed,
        ..Folded::default()
    };
    let mut offset = 0;
    for (index, raw) in bytes[..committed].split(|byte| *byte == b'\n').enumerate() {
        if offset >= committed {
            break;
        }
        let number = index + 1;
        let line_len = raw.len() + 1;
        let Ok(text) = std::str::from_utf8(raw) else {
            folded.damage = Some(format!("line {number} is not UTF-8"));
            return folded;
        };
        let result = match super::schema::parse_line(text) {
            Line::Event(envelope, data) => {
                if envelope.sequence == folded.ledger.last_sequence + 1 {
                    folded.ledger.apply(&envelope, &data)
                } else {
                    Err(format!(
                        "sequence {} follows {}; each event is one above the last",
                        envelope.sequence, folded.ledger.last_sequence
                    ))
                }
            }
            Line::NewerSchema(version) => Err(format!(
                "schema_version {version} is newer than this build reads ({}); upgrade codeflow",
                super::schema::SCHEMA_VERSION
            )),
            Line::Malformed(reason) => Err(reason),
        };
        if let Err(reason) = result {
            folded.damage = Some(format!("line {number}: {reason}"));
            return folded;
        }
        folded.lines = number;
        offset += line_len;
        folded.valid_bytes = offset;
    }
    folded
}

impl Ledger {
    /// Bind a recorded actor to what it holds, against the folded claims.
    ///
    /// # Errors
    ///
    /// The reason the actor is not bound: no claim, a stale generation or a
    /// replaced instance.
    pub fn bind(&self, by: &RecordedBy) -> Result<Bound, String> {
        match by.role()? {
            Role::Operator => Ok(Bound::Operator),
            Role::Coordinator => {
                let stream_id = by.stream.clone().unwrap_or_default();
                let stream = self
                    .streams
                    .get(&stream_id)
                    .filter(|stream| !stream.closed)
                    .ok_or_else(|| format!("stream {stream_id} is not open in this ledger"))?;
                match &stream.lease {
                    Some(lease)
                        if lease.instance == by.actor_instance
                            && Some(lease.generation) == by.generation =>
                    {
                        Ok(Bound::Coordinator { stream: stream_id })
                    }
                    Some(lease) => Err(format!(
                        "this coordinator is stale: {stream_id} is held at generation {} by another instance or generation",
                        lease.generation
                    )),
                    None => Err(format!("{stream_id} has no coordinator claim")),
                }
            }
            Role::Lane(id) => {
                let lane = self
                    .lanes
                    .get(&id)
                    .filter(|lane| lane.closed.is_none())
                    .ok_or_else(|| format!("lane {id} is not open in this ledger"))?;
                if lane.seat.as_deref() == Some(by.actor_instance.as_str())
                    && Some(lane.generation) == by.generation
                {
                    Ok(Bound::Lane {
                        id,
                        stream: lane.stream.clone(),
                    })
                } else {
                    Err(format!(
                        "this seat is stale: {id} is at generation {} with another seat",
                        lane.generation
                    ))
                }
            }
        }
    }

    /// Apply one event, judging every rule first. On an error nothing has
    /// changed.
    ///
    /// # Errors
    ///
    /// The rule the event breaks.
    #[allow(clippy::too_many_lines)]
    pub fn apply(&mut self, envelope: &Envelope, data: &Data) -> Result<(), String> {
        if data.kind() != envelope.event {
            return Err("the data does not fit the event".into());
        }
        if let Some(key) = &envelope.idempotency_key {
            if let Some((sequence, _)) = self.keys.get(key) {
                return Err(format!(
                    "the idempotency key was already recorded at sequence {sequence}"
                ));
            }
        }
        if let Some(expected) = envelope.expected_revision {
            let current = self.revision(&envelope.subject_id);
            if current != expected {
                return Err(format!(
                    "{} changed since it was read (revision {current}, expected {expected}); read it again",
                    envelope.subject_id
                ));
            }
        }
        let subject = envelope.subject_id.as_str();
        let by = &envelope.recorded_by;
        match data {
            Data::ItemAdded(record) => self.add(envelope, record)?,
            Data::ItemUpdated(update) => {
                let bound = self.bind(by)?;
                let item = self.open_item(subject)?;
                require(
                    holds(&bound, item),
                    "only the holder or the operator updates a follow-up",
                )?;
                let mut record = item.record.clone();
                if let Some(title) = &update.title {
                    record.title.clone_from(title);
                }
                if let Some(description) = &update.description {
                    record.description.clone_from(description);
                }
                if let Some(done_when) = &update.done_when {
                    record.done_when.clone_from(done_when);
                }
                if let Some(links) = &update.links {
                    record.links.clone_from(links);
                    record.no_link_reason = None;
                }
                if let Some(reason) = &update.no_link_reason {
                    record.no_link_reason = Some(reason.clone());
                    if update.links.is_none() {
                        record.links.clear();
                    }
                }
                if record.links.is_empty() == record.no_link_reason.is_none() {
                    return Err("a follow-up carries a typed link or a no_link_reason".into());
                }
                if let Some(next) = &update.next_check {
                    record.next_check.clone_from(next);
                }
                if let Some(waits) = &update.waits_on {
                    record.waits_on.clone_from(waits);
                }
                if let Some(condition) = &update.awaiting_operator {
                    record.awaiting_operator =
                        Some(condition.clone()).filter(|condition| !condition.is_empty());
                }
                let dependencies_changed = update.links.is_some()
                    || update.no_link_reason.is_some()
                    || update.waits_on.is_some();
                if let Some(observed) = &update.observed {
                    record.observed.clone_from(observed);
                } else if dependencies_changed {
                    // New dependencies with no recorded states: nothing old
                    // may stand in as their baseline.
                    record.observed.clear();
                }
                self.check_references(subject, &record)?;
                let item = self.items.get_mut(subject).ok_or("vanished")?;
                if update.observed.is_some() || dependencies_changed {
                    item.baseline.clone_from(&record.observed);
                }
                item.record = record;
            }
            Data::ItemChecked(check) => {
                let bound = self.bind(by)?;
                let item = self.open_item(subject)?;
                require(
                    holds(&bound, item),
                    "only the holder or the operator checks a follow-up",
                )?;
                let item = self.items.get_mut(subject).ok_or("vanished")?;
                item.record.next_check.clone_from(&check.next_check);
                if !check.observations.is_empty() {
                    item.baseline.clone_from(&check.observations);
                }
                item.last_check = Some((envelope.recorded_at.clone(), check.clone()));
            }
            Data::ItemHandedOver(hand) => {
                let bound = self.bind(by)?;
                let item = self.open_item(subject)?;
                let taking_intake =
                    item.record.stream == INTAKE && matches!(bound, Bound::Coordinator { .. });
                require(
                    holds(&bound, item) || taking_intake,
                    "only the holder or the operator hands a follow-up over",
                )?;
                let stream = hand
                    .to_stream
                    .clone()
                    .unwrap_or_else(|| item.record.stream.clone());
                self.check_holder(&stream, &hand.to_holder)?;
                let item = self.items.get_mut(subject).ok_or("vanished")?;
                item.record.stream = stream;
                item.record.holder.clone_from(&hand.to_holder);
            }
            Data::ItemClosed(evidence) => {
                let bound = self.bind(by)?;
                let item = self.open_item(subject)?;
                require(
                    holds(&bound, item),
                    "only the holder or the operator closes a follow-up",
                )?;
                let item = self.items.get_mut(subject).ok_or("vanished")?;
                item.state = ItemState::Closed(evidence.clone());
                item.proofs.push(evidence.clone());
            }
            Data::ItemDropped(drop) => {
                let bound = self.bind(by)?;
                let item = self.open_item(subject)?;
                require(
                    holds(&bound, item) || coordinates(&bound, item),
                    "only the holder, the stream's coordinator or the operator drops a follow-up",
                )?;
                let item = self.items.get_mut(subject).ok_or("vanished")?;
                item.state = ItemState::Dropped {
                    reason: drop.reason.clone(),
                    disposition: drop.disposition.clone(),
                };
            }
            Data::ItemPurged(_) => {
                let bound = self.bind(by)?;
                let item = self
                    .items
                    .get(subject)
                    .ok_or_else(|| format!("{subject} is not in this ledger"))?;
                require(
                    holds(&bound, item),
                    "only the holder or the operator purges a follow-up",
                )?;
                let item = self.items.get_mut(subject).ok_or("vanished")?;
                item.purged
                    .push((envelope.recorded_at.clone(), by.role.clone()));
            }
            Data::ItemReopened(reason) => {
                let bound = self.bind(by)?;
                let item = self
                    .items
                    .get(subject)
                    .ok_or_else(|| format!("{subject} is not in this ledger"))?;
                require(
                    item.state != ItemState::Open,
                    "an open follow-up cannot be reopened",
                )?;
                require(
                    holds(&bound, item) || coordinates(&bound, item),
                    "only the holder, the stream's coordinator or the operator reopens a follow-up",
                )?;
                let item = self.items.get_mut(subject).ok_or("vanished")?;
                item.state = ItemState::Open;
                item.reopened.push(reason.reason.clone());
            }
            _ => self.apply_coordination(envelope, data)?,
        }
        if let Some(item) = self.items.get_mut(subject) {
            item.revision += 1;
            item.last_sequence = envelope.sequence;
        }
        if let Some(lane) = self.lanes.get_mut(subject) {
            lane.last_sequence = envelope.sequence;
        }
        if let Some(key) = &envelope.idempotency_key {
            let subject = Some(envelope.subject_id.as_str());
            let digest = operation_digest(
                envelope.event,
                subject,
                &envelope.recorded_by.actor_instance,
                &envelope.data,
            );
            self.keys.insert(key.clone(), (envelope.sequence, digest));
        }
        self.last_sequence = envelope.sequence;
        Ok(())
    }

    /// The revision of a subject: events applied to a follow-up, or the last
    /// sequence of a lane or stream event.
    #[must_use]
    pub fn revision(&self, subject: &str) -> u64 {
        self.items
            .get(subject)
            .map(|item| item.revision)
            .or_else(|| self.lanes.get(subject).map(|lane| lane.last_sequence))
            .unwrap_or(0)
    }

    fn open_item(&self, id: &str) -> Result<&Item, String> {
        let item = self
            .items
            .get(id)
            .ok_or_else(|| format!("{id} is not in this ledger"))?;
        if item.state == ItemState::Open {
            Ok(item)
        } else {
            Err(format!("{id} is {}; reopen it first", item.state.as_str()))
        }
    }

    fn add(&mut self, envelope: &Envelope, record: &FollowUp) -> Result<(), String> {
        let subject = &envelope.subject_id;
        self.bind(&envelope.recorded_by)?;
        if self.items.contains_key(subject) {
            return Err(format!("{subject} already exists"));
        }
        self.check_holder(&record.stream, &record.holder)?;
        if let Some(duplicate) = self.exact_duplicate(record) {
            return Err(format!(
                "an exact duplicate of {duplicate} (same source and source reference)"
            ));
        }
        self.check_references(subject, record)?;
        let number = super::actor::id_number(subject).unwrap_or(0);
        self.max_follow_up = self.max_follow_up.max(number);
        self.items.insert(
            subject.clone(),
            Item {
                id: subject.clone(),
                record: record.clone(),
                state: ItemState::Open,
                revision: 0,
                added_at: envelope.recorded_at.clone(),
                added_by: envelope.recorded_by.clone(),
                last_check: None,
                proofs: Vec::new(),
                reopened: Vec::new(),
                purged: Vec::new(),
                baseline: record.observed.clone(),
                last_sequence: envelope.sequence,
            },
        );
        Ok(())
    }

    /// The open follow-up with the same source identity, if any. Only an
    /// exact duplicate is refused; a similar one is a suggestion.
    #[must_use]
    pub fn exact_duplicate(&self, record: &FollowUp) -> Option<String> {
        let reference = record.source_reference.as_ref()?;
        // A purged item keeps its identity as a digest: the same source is
        // still a duplicate of it.
        let purged = purged_reference(reference);
        self.items
            .values()
            .find(|item| {
                item.state == ItemState::Open
                    && item.record.source == record.source
                    && item
                        .record
                        .source_reference
                        .as_ref()
                        .is_some_and(|existing| existing == reference || *existing == purged)
            })
            .map(|item| item.id.clone())
    }

    fn check_holder(&self, stream: &str, holder: &str) -> Result<(), String> {
        if stream != INTAKE {
            let open = self
                .streams
                .get(stream)
                .is_some_and(|stream| !stream.closed);
            require(open, &format!("stream {stream} is not open in this ledger"))?;
        }
        if is_lane_id(holder) {
            let lane = self
                .lanes
                .get(holder)
                .filter(|lane| lane.closed.is_none())
                .ok_or_else(|| format!("lane {holder} is not open in this ledger"))?;
            require(
                lane.stream == stream,
                &format!("lane {holder} belongs to {}, not {stream}", lane.stream),
            )?;
        }
        Ok(())
    }

    /// Every follow-up and lane a record names exists, and the follow-ups
    /// it waits on (its prerequisites and `waits_on`) form no cycle.
    fn check_references(&self, subject: &str, record: &FollowUp) -> Result<(), String> {
        let mut named = Vec::new();
        for link in &record.links {
            match &link.target {
                Target::FollowUp { id } | Target::Lane { id } => named.push(id.clone()),
                _ => {}
            }
        }
        named.extend(record.waits_on.iter().cloned());
        for id in &named {
            let known = if is_follow_up_id(id) {
                self.items.contains_key(id) || id == subject
            } else {
                self.lanes.contains_key(id)
            };
            require(known, &format!("{id} is not in this ledger"))?;
            require(id != subject, &format!("{subject} cannot wait on itself"))?;
        }
        let waits = |id: &str, record: &FollowUp| -> Vec<String> {
            let _ = id;
            record
                .links
                .iter()
                .filter_map(|link: &Link| match (&link.kind, &link.target) {
                    (super::reference::LinkKind::Prerequisite, Target::FollowUp { id }) => {
                        Some(id.clone())
                    }
                    _ => None,
                })
                .chain(
                    record
                        .waits_on
                        .iter()
                        .filter(|id| is_follow_up_id(id))
                        .cloned(),
                )
                .collect()
        };
        let mut stack = waits(subject, record);
        let mut seen = BTreeSet::new();
        while let Some(next) = stack.pop() {
            if next == subject {
                return Err(format!(
                    "{subject} would wait on itself through its prerequisites; break the cycle"
                ));
            }
            if !seen.insert(next.clone()) {
                continue;
            }
            if let Some(item) = self.items.get(&next) {
                stack.extend(waits(&next, &item.record));
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn apply_coordination(&mut self, envelope: &Envelope, data: &Data) -> Result<(), String> {
        let subject = envelope.subject_id.as_str();
        let by = &envelope.recorded_by;
        let role = by.role()?;
        match data {
            Data::StreamOpened(scope) => {
                require(
                    !self.streams.contains_key(subject),
                    &format!("{subject} already exists"),
                )?;
                let allowed = role == Role::Operator
                    || (role == Role::Coordinator && by.stream.as_deref() == Some(subject));
                require(
                    allowed,
                    "a stream is opened by the operator or by its own coordinator",
                )?;
                self.streams.insert(
                    subject.into(),
                    Stream {
                        id: subject.into(),
                        slug: scope.slug.clone(),
                        scope: scope.scope.clone(),
                        closed: false,
                        lease: None,
                        generation: 0,
                    },
                );
            }
            Data::StreamScopeChanged(scope) => {
                self.lead_of(by, subject)?;
                let stream = self.streams.get_mut(subject).ok_or("vanished")?;
                stream.scope.clone_from(&scope.scope);
                if scope.slug.is_some() {
                    stream.slug.clone_from(&scope.slug);
                }
            }
            Data::StreamClosed(_) => {
                self.lead_of(by, subject)?;
                let stream = self.streams.get_mut(subject).ok_or("vanished")?;
                stream.closed = true;
                stream.lease = None;
            }
            Data::CoordinatorClaimed(claim) => {
                let stream = self
                    .streams
                    .get(subject)
                    .filter(|stream| !stream.closed)
                    .ok_or_else(|| format!("stream {subject} is not open"))?;
                require(
                    stream.lease.is_none(),
                    &format!("{subject} already has a coordinator; adopt it instead"),
                )?;
                require(
                    role == Role::Coordinator && by.stream.as_deref() == Some(subject),
                    "a claim is recorded by the claiming coordinator",
                )?;
                require(
                    claim.generation == stream.generation + 1
                        && by.generation == Some(claim.generation),
                    "a claim takes the next generation",
                )?;
                let stream = self.streams.get_mut(subject).ok_or("vanished")?;
                stream.lease = Some(Lease {
                    instance: by.actor_instance.clone(),
                    generation: claim.generation,
                });
                stream.generation = claim.generation;
            }
            Data::CoordinatorAdopted(adopt) => {
                let stream = self
                    .streams
                    .get(subject)
                    .filter(|stream| !stream.closed)
                    .ok_or_else(|| format!("stream {subject} is not open"))?;
                let lease = stream
                    .lease
                    .clone()
                    .ok_or("there is no coordinator to adopt")?;
                require(
                    lease.instance == adopt.expected_holder
                        && lease.generation == adopt.expected_generation,
                    "the coordinator changed since it was observed; observe it again",
                )?;
                let by_new = role == Role::Coordinator
                    && by.stream.as_deref() == Some(subject)
                    && by.actor_instance == adopt.new_holder
                    && by.generation == Some(adopt.generation);
                require(
                    by_new || role == Role::Operator,
                    "an adoption is recorded by the adopter or the operator",
                )?;
                let stream = self.streams.get_mut(subject).ok_or("vanished")?;
                stream.lease = Some(Lease {
                    instance: adopt.new_holder.clone(),
                    generation: adopt.generation,
                });
                stream.generation = adopt.generation;
            }
            Data::CoordinatorReleased(_) => {
                let bound = self.bind(by)?;
                require(
                    bound
                        == Bound::Coordinator {
                            stream: subject.into(),
                        }
                        || bound == Bound::Operator,
                    "a claim is released by its holder or the operator",
                )?;
                let stream = self.streams.get_mut(subject).ok_or("vanished")?;
                stream.lease = None;
            }
            Data::LaneOpened(open) => {
                require(
                    !self.lanes.contains_key(subject),
                    &format!("{subject} already exists"),
                )?;
                self.lead_of(by, &open.stream)?;
                self.max_lane = self
                    .max_lane
                    .max(super::actor::id_number(subject).unwrap_or(0));
                self.lanes.insert(
                    subject.into(),
                    Lane {
                        id: subject.into(),
                        stream: open.stream.clone(),
                        slug: open.slug.clone(),
                        scope: open.scope.clone(),
                        closed: None,
                        generation: 1,
                        seat: None,
                        reported_at: None,
                        last_sequence: envelope.sequence,
                    },
                );
            }
            Data::LaneScopeChanged(scope) => {
                let stream = self.open_lane(subject)?.stream.clone();
                self.lead_of(by, &stream)?;
                let lane = self.lanes.get_mut(subject).ok_or("vanished")?;
                lane.scope.clone_from(&scope.scope);
                if scope.slug.is_some() {
                    lane.slug.clone_from(&scope.slug);
                }
            }
            Data::SeatAttached(seat) => {
                let lane = self.open_lane(subject)?;
                require(lane.seat.is_none(), "the lane has a seat; detach it first")?;
                let stream = lane.stream.clone();
                self.lead_of(by, &stream)?;
                let lane = self.lanes.get_mut(subject).ok_or("vanished")?;
                lane.seat = Some(seat.seat_instance.clone());
            }
            Data::SeatDetached(detach) => {
                let lane = self.open_lane(subject)?;
                require(
                    lane.seat.as_deref() == Some(detach.seat_instance.as_str()),
                    "that seat is not attached to the lane",
                )?;
                let stream = lane.stream.clone();
                let own = self.bind(by)
                    == Ok(Bound::Lane {
                        id: subject.into(),
                        stream: stream.clone(),
                    });
                if !own {
                    self.lead_of(by, &stream)?;
                }
                let lane = self.lanes.get_mut(subject).ok_or("vanished")?;
                lane.seat = None;
            }
            Data::LaneReported(_) => {
                let stream = self.open_lane(subject)?.stream.clone();
                let own = self.bind(by)
                    == Ok(Bound::Lane {
                        id: subject.into(),
                        stream: stream.clone(),
                    });
                if !own {
                    self.lead_of(by, &stream)?;
                }
                let lane = self.lanes.get_mut(subject).ok_or("vanished")?;
                lane.reported_at = Some(envelope.recorded_at.clone());
            }
            Data::LaneAdopted(adopt) => {
                let lane = self.open_lane(subject)?;
                require(
                    lane.seat.as_deref().unwrap_or("none") == adopt.expected_holder
                        && lane.generation == adopt.expected_generation,
                    "the lane changed since it was observed; observe it again",
                )?;
                let stream = lane.stream.clone();
                self.lead_of(by, &stream)?;
                let lane = self.lanes.get_mut(subject).ok_or("vanished")?;
                lane.seat = Some(adopt.new_holder.clone());
                lane.generation = adopt.generation;
            }
            Data::LaneClosed(close) => {
                let stream = self.open_lane(subject)?.stream.clone();
                self.lead_of(by, &stream)?;
                if let Some(successor) = &close.successor {
                    require(
                        self.lanes.contains_key(successor),
                        &format!("{successor} is not in this ledger"),
                    )?;
                }
                let lane = self.lanes.get_mut(subject).ok_or("vanished")?;
                lane.closed = Some(close.outcome);
                lane.seat = None;
            }
            Data::LaneHandedOver(moved) => {
                let lane = self.open_lane(subject)?;
                require(
                    lane.stream == moved.from_stream,
                    "the lane is not in that stream",
                )?;
                self.lead_of(by, &moved.from_stream)?;
                let receiving = self
                    .streams
                    .get(&moved.to_stream)
                    .filter(|stream| !stream.closed)
                    .and_then(|stream| stream.lease.clone())
                    .ok_or_else(|| {
                        format!("{} has no coordinator to accept the lane", moved.to_stream)
                    })?;
                require(
                    receiving.instance == moved.accepted_by,
                    "the receiving coordinator did not accept the lane",
                )?;
                let lane = self.lanes.get_mut(subject).ok_or("vanished")?;
                lane.stream.clone_from(&moved.to_stream);
                for item in self.items.values_mut() {
                    if item.record.holder == subject && item.state == ItemState::Open {
                        item.record.stream.clone_from(&moved.to_stream);
                    }
                }
            }
            _ => return Err("not a coordination event".into()),
        }
        Ok(())
    }

    fn open_lane(&self, id: &str) -> Result<&Lane, String> {
        self.lanes
            .get(id)
            .filter(|lane| lane.closed.is_none())
            .ok_or_else(|| format!("lane {id} is not open in this ledger"))
    }

    /// The actor leads `stream` (its bound coordinator) or is the operator.
    fn lead_of(&self, by: &RecordedBy, stream: &str) -> Result<(), String> {
        match self.bind(by)? {
            Bound::Operator => Ok(()),
            Bound::Coordinator { stream: own } if own == stream => Ok(()),
            _ => Err(format!(
                "only {stream}'s coordinator or the operator does this"
            )),
        }
    }
}

/// The prefix of an idempotency key kept, as a digest, after its item was
/// purged.
pub const PURGED_KEY: &str = "purged:";
/// The prefix of a source reference kept, as a digest, after its item was
/// purged.
pub const PURGED_REFERENCE: &str = "[purged ";

fn digest_of(text: &str) -> String {
    crate::scaffold::sha256_hex(text.as_bytes())[..32].to_string()
}

/// The opaque key a purged event keeps: a retry of the original operation
/// maps to it, its text does not survive.
#[must_use]
pub fn purged_key(key: &str) -> String {
    format!("{PURGED_KEY}{}", digest_of(key))
}

/// The opaque source reference a purged item keeps.
#[must_use]
pub fn purged_reference(reference: &str) -> String {
    format!("{PURGED_REFERENCE}{}]", digest_of(reference))
}

/// The digest an idempotency key is bound to: the event kind, the subject
/// (except for an event that reserves its subject), the actor instance and
/// the data, without only the fields a retry recomputes for that kind: an
/// add's default schedule and observed link states, a check's observed link
/// states, an update's observed link states and a closure's observation
/// time. What the user wrote (a check's `observed` text, an explicit
/// schedule) is always part of the content.
#[must_use]
pub fn operation_digest(
    kind: super::schema::EventKind,
    subject: Option<&str>,
    actor_instance: &str,
    data: &serde_json::Value,
) -> String {
    use super::schema::EventKind;
    let volatile: &[&str] = match kind {
        EventKind::ItemAdded => &["next_check", "observed"],
        EventKind::ItemChecked => &["observations"],
        EventKind::ItemUpdated => &["observed"],
        EventKind::ItemClosed => &["observed_at"],
        _ => &[],
    };
    let mut data = data.clone();
    if let Some(object) = data.as_object_mut() {
        for key in volatile {
            object.remove(*key);
        }
    }
    let subject = match kind {
        EventKind::ItemAdded | EventKind::LaneOpened => None,
        _ => subject,
    };
    let canonical = serde_json::json!({
        "event": kind.as_str(),
        "subject": subject,
        "actor": actor_instance,
        "data": data,
    });
    crate::scaffold::sha256_hex(canonical.to_string().as_bytes())
}

fn require(condition: bool, message: &str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.to_string())
    }
}

/// Whether the bound actor holds the item (or is the operator).
#[must_use]
pub fn holds(bound: &Bound, item: &Item) -> bool {
    match bound {
        Bound::Operator => true,
        Bound::Coordinator { stream } => {
            item.record.holder == "coordinator"
                && (item.record.stream == *stream || item.record.stream == INTAKE)
        }
        Bound::Lane { id, .. } => item.record.holder == *id,
    }
}

/// Whether the bound actor coordinates the item's stream.
#[must_use]
pub fn coordinates(bound: &Bound, item: &Item) -> bool {
    matches!(bound, Bound::Coordinator { stream } if item.record.stream == *stream || item.record.stream == INTAKE)
}
