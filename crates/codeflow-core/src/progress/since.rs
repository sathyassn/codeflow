//! The `--since <previous facts file>` delta: what changed since an earlier
//! snapshot, and what the earlier snapshot held that this one has no
//! counterpart for, as `superseded` by an evidenced event or `not observed`
//! when its source was not checked this time. No earlier fact is dropped
//! silently, and lost coverage never reads as a replacement.

use std::collections::BTreeMap;

use serde_json::{json, Value};

/// The most bytes a previous facts file may hold.
const MAX_PREVIOUS_BYTES: u64 = 16 * 1024 * 1024;

/// Read a previous facts file.
///
/// # Errors
///
/// The reason when it cannot be read or is not JSON.
pub fn read(path: &std::path::Path) -> Result<Value, String> {
    let bytes = crate::bounded_file::read_bounded_regular(path, MAX_PREVIOUS_BYTES)
        .map_err(|error| format!("--since {}: cannot be read: {error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("--since {}: not a facts file: {error}", path.display()))
}

fn text<'a>(value: &'a Value, pointer: &str) -> &'a str {
    value.pointer(pointer).and_then(Value::as_str).unwrap_or("")
}

/// Refuse a previous snapshot of another schema version, kind, repository
/// or scope, naming the field.
fn check_envelope(previous: &Value, current: &Value) -> Result<(), String> {
    for (pointer, name) in [
        ("/schema_version", "schema_version"),
        ("/kind", "kind"),
        ("/repository", "repository"),
        ("/scope/kind", "scope kind"),
        ("/scope/names", "scope names"),
        ("/scope/target", "scope target"),
    ] {
        let (before, now) = (previous.pointer(pointer), current.pointer(pointer));
        if before != now {
            return Err(format!(
                "--since: the previous snapshot has another {name} ({} against {}); compare snapshots of one repository and scope",
                before.map_or_else(|| "none".into(), Value::to_string),
                now.map_or_else(|| "none".into(), Value::to_string),
            ));
        }
    }
    Ok(())
}

/// The sources this snapshot checked.
fn checked_sources(facts: &Value) -> BTreeMap<String, bool> {
    let mut out = BTreeMap::new();
    if let Some(entries) = facts.pointer("/evidence").and_then(Value::as_array) {
        for entry in entries {
            let checked = entry
                .get("checked")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let source = text(entry, "/source").to_string();
            let slot = out.entry(source).or_insert(true);
            *slot = *slot && checked;
        }
    }
    if let Some(sources) = facts
        .pointer("/issues/operator/sources")
        .and_then(Value::as_array)
    {
        for entry in sources {
            let checked = entry
                .get("checked")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let slot = out
                .entry(text(entry, "/source").to_string())
                .or_insert(true);
            *slot = *slot && checked;
        }
    }
    out
}

/// Each keyed item of one family: issues by subject and kind, operator
/// items by source and id, tasks by id.
fn keyed(facts: &Value) -> BTreeMap<(String, String), (String, Value)> {
    let mut out = BTreeMap::new();
    for list in ["/issues/tasks", "/issues/lanes"] {
        for issue in facts
            .pointer(list)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let key = format!("{} {}", text(issue, "/kind"), text(issue, "/subject"));
            out.insert(
                ("issue".into(), key),
                (text(issue, "/source").to_string(), issue.clone()),
            );
        }
    }
    for item in facts
        .pointer("/issues/operator/items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let key = format!("{} {}", text(item, "/source"), text(item, "/id"));
        out.insert(
            ("operator item".into(), key),
            (text(item, "/source").to_string(), item.clone()),
        );
    }
    for task in facts
        .pointer("/tasks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        out.insert(
            ("task".into(), text(task, "/id").to_string()),
            ("records".into(), task.clone()),
        );
    }
    out
}

/// The event that replaced an earlier item no longer present.
fn replaced_by(family: &str, earlier: &Value, current: &Value) -> String {
    match family {
        "issue" => {
            let subject = text(earlier, "/subject");
            let landed = current
                .pointer("/tasks")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .any(|task| text(task, "/id") == subject && text(task, "/stage") == "landed");
            if landed {
                format!("{subject} landed")
            } else {
                format!(
                    "cleared: {} no longer read for {subject}",
                    text(earlier, "/kind")
                )
            }
        }
        "operator item" => "resolved: its source no longer holds it".into(),
        _ => "no longer in the scope the records give".into(),
    }
}

/// The source family of an item, as the evidence entries name it.
fn family_source(source: &str) -> &str {
    match source {
        "records" => "records",
        "forge" => "forge",
        "ledger" => "ledger",
        "session" => "session file",
        other => other,
    }
}

/// A word that says the current value was not observed.
fn unobserved(value: &Value) -> bool {
    matches!(value.as_str(), Some("unknown" | "unverified"))
        || value.get("state").and_then(Value::as_str) == Some("unverified")
}

/// The fields of a retained item that were observed before and are not
/// now: each kept with its earlier value, never dropped.
fn lost_fields(
    family: &str,
    key: &str,
    earlier: &Value,
    current: &Value,
    earlier_at: &str,
) -> Vec<Value> {
    let (Some(before), Some(now)) = (earlier.as_object(), current.as_object()) else {
        return Vec::new();
    };
    before
        .iter()
        .filter(|(field, value)| {
            now.get(*field)
                .is_some_and(|now| unobserved(now) && !unobserved(value))
        })
        .map(|(field, value)| {
            json!({
                "family": family,
                "subject": key,
                "field": field,
                "earlier": value,
                "earlier_observed_at": earlier_at,
                "current": "unknown",
            })
        })
        .collect()
}

/// The delta between `previous` and `current` facts.
///
/// # Errors
///
/// The reason, naming the field, when the two snapshots are not of one
/// schema version, repository and scope.
pub fn delta(previous: &Value, current: &Value) -> Result<Value, String> {
    check_envelope(previous, current)?;
    let at = text(current, "/snapshot/at").to_string();
    let earlier_at = text(previous, "/snapshot/at").to_string();
    let checked = checked_sources(current);
    let before = keyed(previous);
    let now = keyed(current);
    let mut changes = Vec::new();
    let mut superseded = Vec::new();
    let mut not_observed = Vec::new();
    for ((family, key), (_, item)) in &now {
        let Some((_, earlier)) = before.get(&(family.clone(), key.clone())) else {
            let what = match family.as_str() {
                "issue" if text(item, "/kind").starts_with("required check") => {
                    "check turned not green"
                }
                "issue" if text(item, "/kind") == "blocked" => "blocker opened",
                "issue" => "issue opened",
                "operator item" if text(item, "/source") == "session" => "decision recorded",
                "operator item" => "operator item added",
                _ => "task added",
            };
            changes.push(json!({"change": what, "subject": key, "at": at}));
            continue;
        };
        not_observed.extend(lost_fields(family, key, earlier, item, &earlier_at));
        if family == "task" {
            let (was, is) = (text(earlier, "/stage"), text(item, "/stage"));
            if was != "landed" && is == "landed" {
                changes.push(json!({"change": "landed", "subject": key, "at": at}));
            }
            if text(earlier, "/status") != "cancelled" && text(item, "/status") == "cancelled" {
                changes.push(json!({"change": "task cancelled", "subject": key, "at": at}));
            }
        }
    }
    for ((family, key), (source, earlier)) in &before {
        if now.contains_key(&(family.clone(), key.clone())) {
            continue;
        }
        let source_checked = checked.get(family_source(source)).copied().unwrap_or(false);
        if source_checked {
            let event = replaced_by(family, earlier, current);
            let what = match family.as_str() {
                "issue" if text(earlier, "/kind") == "blocked" => "blocker cleared",
                "issue" if text(earlier, "/kind").starts_with("required check") => "check cleared",
                "operator item" => "operator item resolved",
                _ => "removed",
            };
            changes.push(json!({"change": what, "subject": key, "at": at}));
            superseded.push(json!({
                "family": family,
                "subject": key,
                "event": event,
                "at": at,
                "earlier": earlier,
            }));
        } else {
            not_observed.push(json!({
                "family": family,
                "subject": key,
                "source": source,
                "earlier": earlier,
                "earlier_observed_at": earlier_at,
                "current": "unknown",
            }));
        }
    }
    Ok(json!({
        "previous_at": earlier_at,
        "changes": changes,
        "superseded": superseded,
        "not_observed": not_observed,
    }))
}
