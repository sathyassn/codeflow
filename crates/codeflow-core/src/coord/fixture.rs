//! Role binding for tests and journeys, until the role commands ship
//! (`coord` stream, claim and lane commands are a later task). Each helper
//! writes the same events those commands will write, through the same
//! store and rules, so a fixture-bound actor is bound exactly as a claimed
//! one will be.

use super::actor::{RecordedBy, UNKNOWN};
use super::schema::{Claim, Data, LaneOpen, Scope, Seat, SeatKind};
use super::store::{commit, Draft, Paths, SubjectId, COMMAND_WAIT};
use super::CoordError;

/// The operator, as the fixture records it.
#[must_use]
pub fn operator() -> RecordedBy {
    RecordedBy {
        role: "operator".into(),
        actor_instance: "operator".into(),
        generation: None,
        stream: None,
        harness: UNKNOWN.into(),
        model: UNKNOWN.into(),
        session: UNKNOWN.into(),
    }
}

/// A coordinator of `stream` with `instance` at `generation`.
#[must_use]
pub fn coordinator(stream: &str, instance: &str, generation: u64) -> RecordedBy {
    RecordedBy {
        role: "coordinator".into(),
        actor_instance: instance.into(),
        generation: Some(generation),
        stream: Some(stream.into()),
        harness: UNKNOWN.into(),
        model: UNKNOWN.into(),
        session: UNKNOWN.into(),
    }
}

/// A lane's seat `instance` at `generation`.
#[must_use]
pub fn lane(lane: &str, instance: &str, generation: u64) -> RecordedBy {
    RecordedBy {
        role: lane.into(),
        actor_instance: instance.into(),
        generation: Some(generation),
        stream: None,
        harness: UNKNOWN.into(),
        model: UNKNOWN.into(),
        session: UNKNOWN.into(),
    }
}

/// Open `stream` as the operator and claim it for `instance` at
/// generation 1.
///
/// # Errors
///
/// As [`commit`].
pub fn open_stream(paths: &Paths, stream: &str, instance: &str) -> Result<(), CoordError> {
    commit(paths, &operator(), COMMAND_WAIT, |_| {
        Ok(vec![Draft::new(
            SubjectId::Given(stream.into()),
            Data::StreamOpened(Scope {
                slug: None,
                scope: vec!["fixture".into()],
            }),
        )])
    })?;
    commit(
        paths,
        &coordinator(stream, instance, 1),
        COMMAND_WAIT,
        |_| {
            Ok(vec![Draft::new(
                SubjectId::Given(stream.into()),
                Data::CoordinatorClaimed(Claim { generation: 1 }),
            )])
        },
    )?;
    Ok(())
}

/// Open the next lane in `stream`, as its coordinator `coordinator_instance`
/// at generation 1, and attach the seat `seat_instance`. Returns the lane id.
///
/// # Errors
///
/// As [`commit`].
pub fn open_lane(
    paths: &Paths,
    stream: &str,
    coordinator_instance: &str,
    seat_instance: &str,
) -> Result<String, CoordError> {
    let lead = coordinator(stream, coordinator_instance, 1);
    let opened = commit(paths, &lead, COMMAND_WAIT, |_| {
        Ok(vec![Draft::new(
            SubjectId::NextLane,
            Data::LaneOpened(LaneOpen {
                stream: stream.into(),
                slug: None,
                scope: vec!["fixture".into()],
            }),
        )])
    })?;
    let id = opened
        .envelopes
        .first()
        .map(|envelope| envelope.subject_id.clone())
        .ok_or_else(|| CoordError::Refused("no lane was opened".into()))?;
    commit(paths, &lead, COMMAND_WAIT, |_| {
        Ok(vec![Draft::new(
            SubjectId::Given(id.clone()),
            Data::SeatAttached(Seat {
                seat_instance: seat_instance.into(),
                kind: SeatKind::External,
                harness: UNKNOWN.into(),
            }),
        )])
    })?;
    Ok(id)
}
