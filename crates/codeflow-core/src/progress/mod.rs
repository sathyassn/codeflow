//! Progress facts for `codeflow status`: percent complete, location, order,
//! lanes, windows, issues and pace for a task, one or more epics, a line or
//! the default scope (TSK-1095).
//!
//! The work splits into a read and a computation. [`gather`] reads records,
//! git refs, the forge, the coordination ledger, an adopted forecast, an
//! optional session file and the owed-item sources once, into
//! [`inputs::Inputs`]; [`compute::facts`] turns those inputs into the
//! versioned [`model::Facts`] without further reads. Nothing here writes:
//! no ref, record, ledger, cache or forge state changes.

mod checks;
pub mod compute;
pub mod epics;
pub mod gather;
pub mod inputs;
pub mod issues;
pub mod model;
pub mod owed;
pub mod plan;
pub mod session;
pub mod since;
pub mod text;

#[cfg(test)]
mod tests;

pub use compute::facts;
pub use gather::{collect, Options};
pub use inputs::ScopeSpec;
pub use model::{Facts, SCHEMA_VERSION};
pub use owed::{OwedItem, OwedKind, OwedRead, OwedSource};

/// The facts as JSON, with the `--since` delta when a previous snapshot is
/// given: `since` carries the changes, and each scenario's
/// `intake_since_previous` names the tasks added since that snapshot.
///
/// # Errors
///
/// The reason, naming the field, when the previous snapshot is of another
/// schema version, repository or scope.
pub fn to_json(
    facts: &Facts,
    previous: Option<&serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let mut value = serde_json::to_value(facts).map_err(|error| error.to_string())?;
    let Some(previous) = previous else {
        return Ok(value);
    };
    let delta = since::delta(previous, &value)?;
    let added: Vec<serde_json::Value> = delta
        .get("changes")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|change| {
            change.get("change").and_then(serde_json::Value::as_str) == Some("task added")
        })
        .filter_map(|change| change.get("subject").cloned())
        .collect();
    if let Some(scenarios) = value
        .pointer_mut("/windows/scenarios")
        .and_then(serde_json::Value::as_array_mut)
    {
        for scenario in scenarios {
            scenario["intake_since_previous"] = serde_json::Value::Array(added.clone());
        }
    }
    value["since"] = delta;
    Ok(value)
}
