//! Actors: who records an event, and how an actor is bound to its role.
//!
//! A role is `coordinator` (the lead of one work stream), a lane id
//! (`LN-NNN`) or `operator`. Every actor carries an instance id and, for a
//! coordinator or a lane, the generation of its claim; the ledger checks
//! both against the folded claims and seats before it accepts a mutation,
//! so a stale writer is refused. Harness, model and session are recorded
//! when known and written as `unknown` otherwise.
//!
//! The role commands that claim, release and adopt are not part of this
//! module: an actor names itself through the `CODEFLOW_COORD_*`
//! environment, and the binding is proven against the log.

use serde::{Deserialize, Serialize};

use crate::root_checkout::{EnvLookup, AGENT_MARKERS};

/// The role environment variable: `coordinator`, `LN-NNN` or `operator`.
pub const ROLE_ENV: &str = "CODEFLOW_COORD_ROLE";
/// The actor instance id.
pub const INSTANCE_ENV: &str = "CODEFLOW_COORD_INSTANCE";
/// The claim generation the actor holds.
pub const GENERATION_ENV: &str = "CODEFLOW_COORD_GENERATION";
/// The stream a coordinator leads.
pub const STREAM_ENV: &str = "CODEFLOW_COORD_STREAM";
/// The harness, when known.
pub const HARNESS_ENV: &str = "CODEFLOW_COORD_HARNESS";
/// The model, when known.
pub const MODEL_ENV: &str = "CODEFLOW_COORD_MODEL";
/// The harness session id, when known.
pub const SESSION_ENV: &str = "CODEFLOW_COORD_SESSION";

/// The written value of a field that is not known.
pub const UNKNOWN: &str = "unknown";

/// The project intake list: the stream of a follow-up no coordinator has
/// taken yet.
pub const INTAKE: &str = "intake";

/// A role as the envelope records it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    /// The lead of one work stream.
    Coordinator,
    /// A lane, by its id.
    Lane(String),
    /// The person, running a command by hand.
    Operator,
}

impl Role {
    /// Parse `coordinator`, `operator` or a lane id.
    ///
    /// # Errors
    ///
    /// The reason the text is no role.
    pub fn parse(text: &str) -> Result<Self, String> {
        match text {
            "coordinator" => Ok(Self::Coordinator),
            "operator" => Ok(Self::Operator),
            lane if is_lane_id(lane) => Ok(Self::Lane(lane.to_string())),
            other => Err(format!(
                "`{}` is not a role: use coordinator, operator or a lane id LN-NNN",
                crate::git::display_message(other)
            )),
        }
    }

    /// The role as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Coordinator => "coordinator",
            Self::Lane(id) => id,
            Self::Operator => "operator",
        }
    }
}

/// Whether `text` is a follow-up id, `FU-` and three or more digits.
#[must_use]
pub fn is_follow_up_id(text: &str) -> bool {
    numbered(text, "FU-")
}

/// Whether `text` is a lane id, `LN-` and three or more digits.
#[must_use]
pub fn is_lane_id(text: &str) -> bool {
    numbered(text, "LN-")
}

/// Whether `text` is a work stream id, `WS-` and three or more digits.
#[must_use]
pub fn is_stream_id(text: &str) -> bool {
    numbered(text, "WS-")
}

fn numbered(text: &str, prefix: &str) -> bool {
    text.strip_prefix(prefix).is_some_and(|digits| {
        digits.len() >= 3 && digits.len() <= 9 && digits.bytes().all(|byte| byte.is_ascii_digit())
    })
}

/// The number of a numbered id.
#[must_use]
pub fn id_number(text: &str) -> Option<u64> {
    text.split_once('-')
        .and_then(|(_, digits)| digits.parse().ok())
}

/// Format a numbered id: `FU-007`.
#[must_use]
pub fn format_id(prefix: &str, number: u64) -> String {
    format!("{prefix}-{number:03}")
}

/// Who recorded an event, as the envelope's `recorded_by`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedBy {
    /// `coordinator`, a lane id or `operator`.
    pub role: String,
    /// The actor instance id, always present.
    pub actor_instance: String,
    /// The claim generation, for a coordinator or a lane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<u64>,
    /// The stream a coordinator leads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<String>,
    /// The harness, or `unknown`.
    pub harness: String,
    /// The model, or `unknown`.
    pub model: String,
    /// The harness session id, or `unknown`.
    pub session: String,
}

impl RecordedBy {
    /// The role this records.
    ///
    /// # Errors
    ///
    /// The reason the role is malformed.
    pub fn role(&self) -> Result<Role, String> {
        Role::parse(&self.role)
    }

    /// Check the fields a reader relies on.
    ///
    /// # Errors
    ///
    /// The reason the actor is malformed.
    pub fn validate(&self) -> Result<(), String> {
        let role = self.role()?;
        for (field, value) in [
            ("actor_instance", &self.actor_instance),
            ("harness", &self.harness),
            ("model", &self.model),
            ("session", &self.session),
        ] {
            super::text::check_required(&format!("recorded_by.{field}"), value, 120, false)?;
        }
        match role {
            Role::Coordinator => {
                if self.generation.is_none() {
                    return Err("a coordinator records its claim generation".into());
                }
                match &self.stream {
                    Some(stream) if is_stream_id(stream) => {}
                    _ => return Err("a coordinator records the stream it leads".into()),
                }
            }
            Role::Lane(_) => {
                if self.generation.is_none() {
                    return Err("a lane records its generation".into());
                }
                if self.stream.is_some() {
                    return Err("a lane's stream comes from the lane, not the actor".into());
                }
            }
            Role::Operator => {
                if self.generation.is_some() || self.stream.is_some() {
                    return Err("the operator holds no claim generation or stream".into());
                }
            }
        }
        Ok(())
    }
}

/// An actor as it asks to act: its role and the claim it says it holds.
/// The ledger binds it against the log before it acts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub role: Role,
    pub recorded_by: RecordedBy,
}

/// Why no actor could be formed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unbound {
    /// No role is set: the caller is an observer and may only read.
    Observer,
    /// The role is set but malformed or forbidden.
    Refused(String),
}

impl Actor {
    /// Form an actor from a recorded actor, as replay does.
    ///
    /// # Errors
    ///
    /// The reason the recorded actor is malformed.
    pub fn from_recorded(recorded_by: RecordedBy) -> Result<Self, String> {
        recorded_by.validate()?;
        Ok(Self {
            role: recorded_by.role()?,
            recorded_by,
        })
    }

    /// Form the actor from the environment.
    ///
    /// An agent's shell (any harness marker set) can never act as the
    /// operator: an operator's answer is recorded by the agent, with its
    /// source, never as the operator.
    ///
    /// # Errors
    ///
    /// [`Unbound::Observer`] with no role set, or the reason the role is
    /// refused.
    pub fn from_env(env: EnvLookup<'_>) -> Result<Self, Unbound> {
        let read = |name: &str| {
            env(name)
                .and_then(|value| value.into_string().ok())
                .filter(|value| !value.is_empty())
        };
        let Some(role_text) = read(ROLE_ENV) else {
            return Err(Unbound::Observer);
        };
        let role = Role::parse(&role_text).map_err(Unbound::Refused)?;
        let agent = AGENT_MARKERS
            .iter()
            .find(|name| env(name).is_some_and(|value| !value.is_empty()));
        if role == Role::Operator {
            if let Some(marker) = agent {
                return Err(Unbound::Refused(format!(
                    "an agent cannot record as the operator ({marker} is set); record the operator's answer under your own role with --answer-source operator"
                )));
            }
        }
        let instance = match (&role, read(INSTANCE_ENV)) {
            (_, Some(instance)) => instance,
            (Role::Operator, None) => "operator".to_string(),
            (_, None) => {
                return Err(Unbound::Refused(format!(
                "{ROLE_ENV} is set but {INSTANCE_ENV} is not: every actor carries its instance id"
            )))
            }
        };
        let generation = match role {
            Role::Operator => None,
            _ => Some(
                read(GENERATION_ENV)
                    .and_then(|text| text.parse::<u64>().ok())
                    .ok_or_else(|| {
                        Unbound::Refused(format!(
                            "{GENERATION_ENV} must name the claim generation this actor holds"
                        ))
                    })?,
            ),
        };
        let stream = match role {
            Role::Coordinator => Some(read(STREAM_ENV).ok_or_else(|| {
                Unbound::Refused(format!("a coordinator names its stream in {STREAM_ENV}"))
            })?),
            _ => None,
        };
        let recorded_by = RecordedBy {
            role: role.as_str().to_string(),
            actor_instance: instance,
            generation,
            stream,
            harness: read(HARNESS_ENV).unwrap_or_else(|| UNKNOWN.into()),
            model: read(MODEL_ENV).unwrap_or_else(|| UNKNOWN.into()),
            session: read(SESSION_ENV).unwrap_or_else(|| UNKNOWN.into()),
        };
        Self::from_recorded(recorded_by).map_err(Unbound::Refused)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::ffi::OsString;

    fn env_of(pairs: &[(&str, &str)]) -> BTreeMap<String, OsString> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), OsString::from(*v)))
            .collect()
    }

    #[test]
    fn no_role_is_an_observer() {
        let env = env_of(&[]);
        assert_eq!(
            Actor::from_env(&|name| env.get(name).cloned()),
            Err(Unbound::Observer)
        );
    }

    #[test]
    fn an_agent_cannot_act_as_the_operator() {
        let env = env_of(&[(ROLE_ENV, "operator"), ("CLAUDECODE", "1")]);
        let Err(Unbound::Refused(reason)) = Actor::from_env(&|name| env.get(name).cloned()) else {
            panic!("an agent acted as the operator");
        };
        assert!(reason.contains("cannot record as the operator"), "{reason}");
        let env = env_of(&[(ROLE_ENV, "operator")]);
        assert!(Actor::from_env(&|name| env.get(name).cloned()).is_ok());
    }

    #[test]
    fn a_lane_needs_instance_and_generation() {
        let env = env_of(&[(ROLE_ENV, "LN-001"), (INSTANCE_ENV, "seat-a")]);
        assert!(matches!(
            Actor::from_env(&|name| env.get(name).cloned()),
            Err(Unbound::Refused(_))
        ));
        let env = env_of(&[
            (ROLE_ENV, "LN-001"),
            (INSTANCE_ENV, "seat-a"),
            (GENERATION_ENV, "1"),
        ]);
        let actor = Actor::from_env(&|name| env.get(name).cloned()).unwrap();
        assert_eq!(actor.role, Role::Lane("LN-001".into()));
        assert_eq!(actor.recorded_by.harness, UNKNOWN);
    }

    #[test]
    fn ids_are_recognised() {
        assert!(is_follow_up_id("FU-001"));
        assert!(!is_follow_up_id("FU-01"));
        assert!(!is_follow_up_id("FU-NNN"));
        assert!(is_lane_id("LN-1234"));
        assert!(is_stream_id("WS-002"));
        assert_eq!(format_id("FU", 7), "FU-007");
        assert_eq!(id_number("LN-012"), Some(12));
    }
}
