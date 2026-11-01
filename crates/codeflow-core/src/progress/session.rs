//! The `--session <file>` input: facts only a coordinating session knows
//! (which seat builds or reviews a lane, a verdict in flight, a blocker it
//! saw), each with the time it was observed. Validated against
//! `session-v1.schema.json`; merged into the lanes view labelled `session`
//! and never changing a percent or a stage.

use serde::Deserialize;

/// The most bytes a session file may hold.
const MAX_SESSION_BYTES: u64 = 1024 * 1024;

/// A recommended action as a session recorded it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recommendation {
    pub action: String,
    pub why: String,
    #[serde(default)]
    pub alternatives: Vec<String>,
}

/// A decision put to the operator, as a session recorded it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionDecision {
    pub context: String,
    pub options: Vec<String>,
    pub recommendation: String,
    pub reasoning: String,
}

/// One session fact.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionFact {
    pub observed_at: String,
    #[serde(default)]
    pub lane: Option<String>,
    #[serde(default)]
    pub task: Option<String>,
    #[serde(default)]
    pub builder: Option<String>,
    #[serde(default)]
    pub reviewer: Option<String>,
    #[serde(default)]
    pub seat: Option<String>,
    #[serde(default)]
    pub verdict_in_flight: Option<String>,
    #[serde(default)]
    pub blocker: Option<String>,
    /// The seat has made no progress since the last tick.
    #[serde(default)]
    pub no_progress: bool,
    #[serde(default)]
    pub recommendation: Option<Recommendation>,
    #[serde(default)]
    pub decision: Option<SessionDecision>,
}

/// A validated session file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionFile {
    pub schema_version: u32,
    pub facts: Vec<SessionFact>,
}

/// Read and validate a session file.
///
/// # Errors
///
/// The reason, naming the field, when the file cannot be read, is not
/// JSON, carries a field the schema does not name, has another
/// `schema_version` or an `observed_at` that is not a UTC time.
pub fn read(path: &std::path::Path) -> Result<SessionFile, String> {
    let bytes = crate::bounded_file::read_bounded_regular(path, MAX_SESSION_BYTES)
        .map_err(|error| format!("cannot read it as a regular file of at most 1 MiB: {error}"))?;
    parse(&bytes)
}

/// Validate session file bytes.
///
/// # Errors
///
/// As [`read`].
pub fn parse(bytes: &[u8]) -> Result<SessionFile, String> {
    let file: SessionFile =
        serde_json::from_slice(bytes).map_err(|error| format!("not a session-v1 file: {error}"))?;
    if file.schema_version != 1 {
        return Err(format!("schema_version {} is not 1", file.schema_version));
    }
    for (index, fact) in file.facts.iter().enumerate() {
        crate::coord::parse_time(&fact.observed_at)
            .map_err(|error| format!("facts[{index}].observed_at: {error}"))?;
        if fact.lane.is_none() && fact.task.is_none() && fact.decision.is_none() {
            return Err(format!("facts[{index}] names no lane, task or decision"));
        }
    }
    Ok(file)
}
