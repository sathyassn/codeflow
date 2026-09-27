//! Dependency edges of a task (SPC-013 R-26, R-112).
//!
//! A `depends_on` entry is a bare task id, which is a code dependency, or an
//! object `{id: TSK-NNN, kind: research | decision, pin: <commit>}`. A code
//! dependency is met when the predecessor's accepted change is in the
//! execution base; a research or decision dependency is met at its pinned
//! commit. The kind is never inferred from the predecessor's `work_type`.

use serde_yaml::Value;

use super::is_valid_task_format_id;

/// What a dependency edge needs from its predecessor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyKind {
    /// The predecessor's change must be in this task's execution base.
    Code,
    /// The predecessor's findings, read at a pinned commit.
    Research,
    /// The predecessor's decision, read at a pinned commit.
    Decision,
}

impl DependencyKind {
    /// The word used in records and messages.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::Research => "research",
            Self::Decision => "decision",
        }
    }
}

/// One `depends_on` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    /// Predecessor task id.
    pub id: String,
    /// What the edge needs.
    pub kind: DependencyKind,
    /// The commit a research or decision dependency is read at; `None` for a
    /// code dependency, and for a pinned kind whose pin is not known yet
    /// (which leaves the edge unmet).
    pub pin: Option<String>,
}

/// Parse one `depends_on` entry.
///
/// # Errors
///
/// Returns why the entry is neither a task id nor a well-formed
/// `{id, kind, pin}` object.
pub fn parse_dependency(value: &Value) -> Result<Dependency, String> {
    if let Some(id) = value.as_str() {
        return if is_valid_task_format_id(id) {
            Ok(Dependency {
                id: id.to_string(),
                kind: DependencyKind::Code,
                pin: None,
            })
        } else {
            Err(format!("dependency '{id}' is not a task id"))
        };
    }
    let Some(map) = value.as_mapping() else {
        return Err("a dependency is a task id or {id, kind, pin}".to_string());
    };
    let field = |name: &str| map.get(Value::String(name.to_string()));
    for key in map.keys() {
        match key.as_str() {
            Some("id" | "kind" | "pin") => {}
            other => {
                return Err(format!(
                    "dependency has unknown key '{}'",
                    other.unwrap_or("<non-string>")
                ))
            }
        }
    }
    let id = field("id")
        .and_then(Value::as_str)
        .filter(|id| is_valid_task_format_id(id))
        .ok_or("dependency object needs `id: TSK-NNN`")?;
    let kind = match field("kind").and_then(Value::as_str) {
        Some("research") => DependencyKind::Research,
        Some("decision") => DependencyKind::Decision,
        Some(other) => {
            return Err(format!(
                "dependency {id} has kind '{other}'; an object entry is research or decision, a code dependency is a bare id"
            ))
        }
        None => return Err(format!("dependency {id} needs `kind: research | decision`")),
    };
    let pin = match field("pin") {
        None | Some(Value::Null) => None,
        Some(Value::String(pin)) if is_commit_id(pin) => Some(pin.clone()),
        Some(_) => return Err(format!("dependency {id} pin must be a commit id")),
    };
    Ok(Dependency {
        id: id.to_string(),
        kind,
        pin,
    })
}

/// Parse a whole `depends_on` value (absent is empty).
///
/// # Errors
///
/// Returns the first malformed entry, or that the value is not a list.
pub fn parse_dependencies(value: Option<&Value>) -> Result<Vec<Dependency>, String> {
    match value {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Sequence(items)) => items.iter().map(parse_dependency).collect(),
        Some(_) => Err("depends_on must be a YAML list".to_string()),
    }
}

fn is_commit_id(value: &str) -> bool {
    (7..=64).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yaml(text: &str) -> Value {
        serde_yaml::from_str(text).unwrap()
    }

    #[test]
    fn a_bare_id_is_a_code_dependency_and_objects_carry_kind_and_pin() {
        let deps = parse_dependencies(Some(&yaml(
            "[TSK-001, {id: TSK-002, kind: research, pin: 0123abcd}, {id: TSK-003, kind: decision}]",
        )))
        .unwrap();
        assert_eq!(deps[0].kind, DependencyKind::Code);
        assert_eq!(deps[1].kind, DependencyKind::Research);
        assert_eq!(deps[1].pin.as_deref(), Some("0123abcd"));
        assert_eq!(deps[2].kind, DependencyKind::Decision);
        assert_eq!(deps[2].pin, None);
    }

    #[test]
    fn malformed_entries_are_refused_and_kind_is_never_inferred() {
        for (text, needle) in [
            ("[{id: TSK-001, kind: code}]", "bare id"),
            ("[{id: TSK-001}]", "needs `kind"),
            ("[{id: TSK-001, kind: research, pin: main}]", "commit id"),
            ("[{id: nope, kind: research}]", "needs `id"),
            ("[{id: TSK-001, kind: research, extra: 1}]", "unknown key"),
            ("[not-a-task]", "not a task id"),
            ("TSK-001", "YAML list"),
        ] {
            let error = parse_dependencies(Some(&yaml(text))).unwrap_err();
            assert!(error.contains(needle), "{text}: {error}");
        }
    }
}
