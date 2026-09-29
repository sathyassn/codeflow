//! Dependency edges of a task (SPC-013 R-26, R-112).
//!
//! A `depends_on` entry is a bare task id, which is a code dependency, or an
//! object `{id: TSK-NNN, kind: research | decision, pin: "<commit>"}`. A code
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
        // No `pin` key: the pin is not known yet, and the edge is unmet.
        None => None,
        // `pin: null` or `pin: ~` was written, but YAML kept no text.
        Some(Value::Null) => {
            return Err(format!(
                "dependency {id} pin reads as YAML null, not a commit id; quote it: pin: \"<commit sha>\", or leave pin out until it is known"
            ))
        }
        Some(Value::String(pin)) if is_commit_id(pin) => Some(pin.clone()),
        // YAML may have dropped the text (`949894e0` is 949894.0, a 40-digit
        // id is a rounded float, `+1234567` loses its sign), so a number is
        // never read back (R-112). The text may well be a commit id, so the
        // remedy is the quoting, never a verdict on the id (TSK-142 AC-4).
        Some(Value::Number(_)) => {
            return Err(format!(
                "dependency {id} pin reads as a YAML number, which loses the text as written; the pin must be quoted: pin: \"<commit sha>\""
            ))
        }
        Some(_) => {
            return Err(format!(
                "dependency {id} pin must be a commit id, written quoted: pin: \"<commit sha>\""
            ))
        }
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

    /// R-112: a pin that YAML reads as a number (digits, the shape of an
    /// exponent, or a full id of digits) is refused with the quote remedy;
    /// quoted, the same text is a commit id, and a pin YAML keeps as text
    /// (a letter, or a leading zero) reads either way.
    #[test]
    fn a_number_shaped_pin_is_refused_unquoted_and_read_quoted() {
        let full_digits = "1234567890123456789012345678901234567890";
        // TSK-142 AC-4: an all-digit and an exponent-form prefix are refused
        // as unquoted, never as "not a commit id". An exponent too large for
        // a float (`12e45678`) stays text, so it is read as written below.
        for pin in ["70283613", "12345678", "949894e0", full_digits] {
            let unquoted = format!("[{{id: TSK-001, kind: research, pin: {pin}}}]");
            let error = parse_dependencies(Some(&yaml(&unquoted))).unwrap_err();
            assert!(
                error.contains("TSK-001 pin reads as a YAML number")
                    && error.contains("the pin must be quoted: pin: \"<commit sha>\""),
                "{pin}: {error}"
            );
            assert!(!error.contains("not a commit id"), "{pin}: {error}");
            let quoted = format!("[{{id: TSK-001, kind: research, pin: \"{pin}\"}}]");
            let deps = parse_dependencies(Some(&yaml(&quoted))).unwrap();
            assert_eq!(deps[0].pin.as_deref(), Some(pin), "{pin}");
        }
        for pin in [
            "0123abcd",
            "\"0123abcd\"",
            "07028361",
            "12e45678",
            "\"12e45678\"",
        ] {
            let text = format!("[{{id: TSK-001, kind: research, pin: {pin}}}]");
            let deps = parse_dependencies(Some(&yaml(&text))).unwrap();
            assert_eq!(
                deps[0].pin.as_deref(),
                Some(pin.trim_matches('"')),
                "{text}"
            );
        }
        let error = parse_dependencies(Some(&yaml("[{id: TSK-001, kind: research, pin: main}]")))
            .unwrap_err();
        assert!(error.contains("written quoted"), "{error}");
    }

    /// Review T156-1: a written `pin: null` or `pin: ~` keeps no text, so it
    /// is refused with the quote remedy; only a missing `pin` key is the
    /// not-yet-known state, which parses as an unmet edge.
    #[test]
    fn an_explicit_null_pin_is_refused_and_a_missing_pin_is_unmet() {
        for pin in ["null", "~", "Null", "NULL"] {
            let text = format!("[{{id: TSK-001, kind: research, pin: {pin}}}]");
            let error = parse_dependencies(Some(&yaml(&text))).unwrap_err();
            assert!(
                error.contains("TSK-001 pin reads as YAML null")
                    && error.contains("quote it: pin: \"<commit sha>\""),
                "{pin}: {error}"
            );
        }
        let deps = parse_dependencies(Some(&yaml("[{id: TSK-001, kind: decision}]"))).unwrap();
        assert_eq!(deps[0].pin, None);
    }
}
