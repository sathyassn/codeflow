//! Registry ids, record paths and the registry file format (SPC-013
//! Interfaces): one TOML file per issued id at `ids/<KIND>/<N>.toml`.

use std::fmt;

/// The record kinds the registry numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Epc,
    Spc,
    Tsk,
    Adr,
}

impl Kind {
    /// Every kind, in registry order.
    pub const ALL: [Kind; 4] = [Kind::Epc, Kind::Spc, Kind::Tsk, Kind::Adr];

    /// The id prefix and registry directory name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Epc => "EPC",
            Kind::Spc => "SPC",
            Kind::Tsk => "TSK",
            Kind::Adr => "ADR",
        }
    }

    /// Parse a kind prefix.
    #[must_use]
    pub fn parse(value: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|kind| kind.as_str() == value)
    }

    /// Zero-padded width of a canonical number.
    #[must_use]
    pub fn width(self) -> usize {
        if self == Kind::Adr {
            4
        } else {
            3
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A record id: its kind and the number as written (`100`, `0072`, or a
/// legacy `002-001`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RegId {
    kind: Kind,
    number: String,
}

impl RegId {
    /// A canonical id of `kind` with sequence `seq`.
    #[must_use]
    pub fn canonical(kind: Kind, seq: u64) -> RegId {
        RegId {
            kind,
            number: format!("{seq:0width$}", width = kind.width()),
        }
    }

    /// Parse `TSK-100`, `ADR-0072` or the legacy `TSK-002-001`.
    #[must_use]
    pub fn parse(id: &str) -> Option<RegId> {
        let (prefix, number) = id.split_once('-')?;
        let kind = Kind::parse(prefix)?;
        let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
        let valid = match number.split_once('-') {
            None => digits(number) && number.len() >= kind.width(),
            Some((epic, task)) => kind == Kind::Tsk && digits(epic) && digits(task),
        };
        valid.then(|| RegId {
            kind,
            number: number.to_string(),
        })
    }

    /// Parse a registry path `ids/<KIND>/<N>.toml`.
    #[must_use]
    pub fn from_registry_path(path: &str) -> Option<RegId> {
        let rest = path.strip_prefix("ids/")?;
        let (kind, file) = rest.split_once('/')?;
        let number = file.strip_suffix(".toml")?;
        let id = RegId::parse(&format!("{kind}-{number}"))?;
        (id.registry_path() == path).then_some(id)
    }

    /// The record kind.
    #[must_use]
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// The number as written.
    #[must_use]
    pub fn number(&self) -> &str {
        &self.number
    }

    /// A legacy `TSK-NNN-MMM` id, which never raises the high-water mark.
    #[must_use]
    pub fn is_legacy(&self) -> bool {
        self.number.contains('-')
    }

    /// The canonical sequence, `None` for a legacy id.
    #[must_use]
    pub fn seq(&self) -> Option<u64> {
        if self.is_legacy() {
            None
        } else {
            self.number.parse().ok()
        }
    }

    /// The registry file path.
    #[must_use]
    pub fn registry_path(&self) -> String {
        format!("ids/{}/{}.toml", self.kind, self.number)
    }
}

impl fmt::Display for RegId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.kind, self.number)
    }
}

/// The record id a tracked path holds, for the record layouts `CodeFlow`
/// reads: flat tasks, specs and epics, the nested epic layout, and ADRs.
#[must_use]
pub fn record_id_from_path(path: &str) -> Option<RegId> {
    if let Some(name) = path.strip_prefix("docs/decisions/") {
        if name.contains('/') {
            return None;
        }
        let stem = name.strip_suffix(".md")?;
        let rest = stem.strip_prefix("ADR-")?;
        let number: String = rest.chars().take_while(char::is_ascii_digit).collect();
        let tail = &rest[number.len()..];
        if number.len() != 4 || !(tail.is_empty() || tail.starts_with('-')) {
            return None;
        }
        return RegId::parse(&format!("ADR-{number}"));
    }
    let rest = path.strip_prefix("project-management/")?;
    let parts: Vec<&str> = rest.split('/').collect();
    let (stem, expected) = match parts.as_slice() {
        ["tasks", file] | ["epics", _, "tasks", file] => (file.strip_suffix(".md")?, Kind::Tsk),
        ["specs", file] => (file.strip_suffix(".md")?, Kind::Spc),
        ["epics", file] => (file.strip_suffix(".md")?, Kind::Epc),
        ["epics", epic, file] if file.strip_suffix(".md") == Some(epic) => (*epic, Kind::Epc),
        _ => return None,
    };
    RegId::parse(stem).filter(|id| id.kind() == expected)
}

/// The path prefixes that hold records, for tree listings.
pub const RECORD_ROOTS: [&str; 2] = ["project-management", "docs/decisions"];

/// A fresh lower-case `UUIDv4`.
#[must_use]
pub fn new_uid() -> String {
    uuid::Uuid::new_v4().hyphenated().to_string()
}

/// Whether `value` is a lower-case hyphenated `UUIDv4`.
#[must_use]
pub fn is_uid(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (index, byte) in bytes.iter().enumerate() {
        let ok = match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            14 => *byte == b'4',
            19 => matches!(byte, b'8' | b'9' | b'a' | b'b'),
            _ => byte.is_ascii_digit() || (b'a'..=b'f').contains(byte),
        };
        if !ok {
            return false;
        }
    }
    true
}

/// Read the `id` and `uid` frontmatter values of a record, without parsing
/// the whole document. Values are unquoted and comments stripped.
#[must_use]
pub fn frontmatter_value(text: &str, key: &str) -> Option<String> {
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))?;
    for line in rest.lines() {
        if line.trim_end_matches([' ', '\t']) == "---" {
            return None;
        }
        let Some(value) = line
            .strip_prefix(key)
            .and_then(|tail| tail.strip_prefix(':'))
        else {
            continue;
        };
        let value = value
            .split(" #")
            .next()
            .unwrap_or_default()
            .trim_matches([' ', '\t']);
        let value = if let Some(quoted) = value.strip_prefix('"') {
            quoted.strip_suffix('"')?
        } else if let Some(quoted) = value.strip_prefix('\'') {
            quoted.strip_suffix('\'')?
        } else {
            value
        };
        return (!value.is_empty() && value != "null").then(|| value.to_string());
    }
    None
}

/// One registry file (SPC-013 Interfaces). Every field is required.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: RegId,
    pub uid: String,
    pub title: String,
    pub issuer: String,
    pub created: String,
    pub target: String,
    pub introduced: String,
    pub landed: String,
    pub mapped: Vec<String>,
    pub mapped_by: String,
}

impl Entry {
    /// An issued entry: provenance fields are `none`.
    #[must_use]
    pub fn issued(id: RegId, uid: String, title: &str, issuer: &str, target: &str) -> Entry {
        Entry {
            id,
            uid,
            title: title.to_string(),
            issuer: issuer.to_string(),
            created: crate::workgraph::now_rfc3339(),
            target: target.to_string(),
            introduced: "none".to_string(),
            landed: "none".to_string(),
            mapped: Vec::new(),
            mapped_by: "none".to_string(),
        }
    }

    /// The file bytes, in the fixed field order of the format.
    #[must_use]
    pub fn render(&self) -> String {
        let quote = |value: &str| toml::Value::String(value.to_string()).to_string();
        let mapped = self
            .mapped
            .iter()
            .map(|sha| quote(sha))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "id = {}\nuid = {}\nkind = {}\ntitle = {}\nissuer = {}\ncreated = {}\ntarget = {}\nintroduced = {}\nlanded = {}\nmapped = [{mapped}]\nmapped_by = {}\n",
            quote(&self.id.to_string()),
            quote(&self.uid),
            quote(self.id.kind().as_str()),
            quote(&self.title),
            quote(&self.issuer),
            quote(&self.created),
            quote(&self.target),
            quote(&self.introduced),
            quote(&self.landed),
            quote(&self.mapped_by),
        )
    }

    /// Parse and validate a registry file found at `path`.
    ///
    /// # Errors
    ///
    /// Returns every problem found: invalid TOML, a missing field, an id or
    /// kind that disagrees with the path, or a malformed `uid`.
    pub fn parse(path: &str, text: &str) -> Result<Entry, Vec<String>> {
        let Some(expected) = RegId::from_registry_path(path) else {
            return Err(vec![format!("{path} is not an ids/<KIND>/<N>.toml path")]);
        };
        let table: toml::Table = text
            .parse()
            .map_err(|error| vec![format!("{path} is not valid TOML: {error}")])?;
        let mut problems = Vec::new();
        let string = |key: &str, problems: &mut Vec<String>| -> String {
            match table.get(key) {
                Some(toml::Value::String(value)) => value.clone(),
                Some(_) => {
                    problems.push(format!("{path}: field {key} must be a string"));
                    String::new()
                }
                None => {
                    problems.push(format!("{path} lacks field {key}"));
                    String::new()
                }
            }
        };
        let id = string("id", &mut problems);
        let uid = string("uid", &mut problems);
        let kind = string("kind", &mut problems);
        let title = string("title", &mut problems);
        let issuer = string("issuer", &mut problems);
        let created = string("created", &mut problems);
        let target = string("target", &mut problems);
        let introduced = string("introduced", &mut problems);
        let landed = string("landed", &mut problems);
        let mapped_by = string("mapped_by", &mut problems);
        let mapped = match table.get("mapped") {
            Some(toml::Value::Array(values)) => values
                .iter()
                .filter_map(|value| {
                    let text = value.as_str().map(str::to_string);
                    if text.is_none() {
                        problems.push(format!("{path}: mapped must hold strings"));
                    }
                    text
                })
                .collect(),
            Some(_) => {
                problems.push(format!("{path}: field mapped must be a list"));
                Vec::new()
            }
            None => {
                problems.push(format!("{path} lacks field mapped"));
                Vec::new()
            }
        };
        if !id.is_empty() && id != expected.to_string() {
            problems.push(format!("{path} has id {id:?}, expected {expected}"));
        }
        if !kind.is_empty() && kind != expected.kind().as_str() {
            problems.push(format!(
                "{path} has kind {kind:?}, expected {}",
                expected.kind()
            ));
        }
        if !uid.is_empty() && !is_uid(&uid) {
            problems.push(format!("{path} has a uid that is not a lower-case UUIDv4"));
        }
        if problems.is_empty() {
            Ok(Entry {
                id: expected,
                uid,
                title,
                issuer,
                created,
                target,
                introduced,
                landed,
                mapped,
                mapped_by,
            })
        } else {
            Err(problems)
        }
    }

    /// The introducing commit sha, `None` when `none`.
    #[must_use]
    pub fn introduced_sha(&self) -> Option<&str> {
        let sha = self.introduced.split('@').next().unwrap_or_default();
        (!sha.is_empty() && sha != "none").then_some(sha)
    }

    /// The landing commit sha, `None` when `none`.
    #[must_use]
    pub fn landed_sha(&self) -> Option<&str> {
        (!self.landed.is_empty() && self.landed != "none").then_some(self.landed.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_parse_canonical_legacy_and_adr() {
        let task = RegId::parse("TSK-100").unwrap();
        assert_eq!(task.registry_path(), "ids/TSK/100.toml");
        assert_eq!(task.seq(), Some(100));
        let legacy = RegId::parse("TSK-002-001").unwrap();
        assert!(legacy.is_legacy());
        assert_eq!(legacy.seq(), None);
        assert_eq!(legacy.registry_path(), "ids/TSK/002-001.toml");
        assert_eq!(
            RegId::parse("ADR-0072").unwrap().registry_path(),
            "ids/ADR/0072.toml"
        );
        assert!(RegId::parse("EPC-001-002").is_none());
        assert!(RegId::parse("TSK-1").is_none());
        assert!(RegId::parse("ADR-072").is_none());
        assert!(RegId::parse("FOO-100").is_none());
        assert_eq!(RegId::canonical(Kind::Adr, 73).to_string(), "ADR-0073");
        assert_eq!(
            RegId::from_registry_path("ids/TSK/002-001.toml"),
            Some(legacy)
        );
        assert_eq!(RegId::from_registry_path("ids/SPC/013.toml.bak"), None);
        assert_eq!(RegId::from_registry_path("ids/TSK/100.toml/x"), None);
    }

    #[test]
    fn record_paths_cover_every_layout() {
        let id = |path: &str| record_id_from_path(path).map(|id| id.to_string());
        assert_eq!(
            id("project-management/tasks/TSK-101.md").as_deref(),
            Some("TSK-101")
        );
        assert_eq!(
            id("project-management/specs/SPC-013.md").as_deref(),
            Some("SPC-013")
        );
        assert_eq!(
            id("project-management/epics/EPC-020.md").as_deref(),
            Some("EPC-020")
        );
        assert_eq!(
            id("project-management/epics/EPC-002/EPC-002.md").as_deref(),
            Some("EPC-002")
        );
        assert_eq!(
            id("project-management/epics/EPC-002/tasks/TSK-002-001.md").as_deref(),
            Some("TSK-002-001")
        );
        assert_eq!(
            id("docs/decisions/ADR-0072-shared-id-registry.md").as_deref(),
            Some("ADR-0072")
        );
        assert_eq!(id("project-management/tasks/SPC-001.md"), None);
        assert_eq!(id("project-management/templates/task.md"), None);
        assert_eq!(id("docs/decisions/README.md"), None);
    }

    #[test]
    fn uids_are_lower_case_v4() {
        let uid = new_uid();
        assert!(is_uid(&uid), "{uid}");
        assert!(!is_uid(&uid.to_uppercase()));
        assert!(!is_uid("8f6d0d1c-4a1e-3d2b-9c0e-2b7f2f9f3a11"));
    }

    #[test]
    fn entries_round_trip_and_reject_mismatches() {
        let entry = Entry::issued(
            RegId::parse("TSK-100").unwrap(),
            new_uid(),
            "Say \"hi\"",
            "a@example.com",
            "main",
        );
        let text = entry.render();
        assert_eq!(Entry::parse("ids/TSK/100.toml", &text).unwrap(), entry);
        let problems = Entry::parse("ids/TSK/101.toml", &text).unwrap_err();
        assert!(problems[0].contains("expected TSK-101"), "{problems:?}");
        let problems = Entry::parse("ids/TSK/100.toml", "id = \"TSK-100\"\n").unwrap_err();
        assert!(problems.iter().any(|p| p.contains("lacks field uid")));
    }

    #[test]
    fn frontmatter_values_are_read_without_full_parse() {
        let text = "---\nid: TSK-100\nuid: \"abc\" # hidden\nstatus: todo\n---\nuid: body\n";
        assert_eq!(frontmatter_value(text, "uid").as_deref(), Some("abc"));
        assert_eq!(frontmatter_value(text, "id").as_deref(), Some("TSK-100"));
        assert_eq!(frontmatter_value(text, "title"), None);
        assert_eq!(frontmatter_value("no frontmatter", "id"), None);
    }
}

#[cfg(test)]
mod r15_text_regressions {
    #[test]
    fn r15_frontmatter_keeps_value_unicode_space() {
        assert_eq!(
            super::frontmatter_value("---\nid: TSK-238\u{a0}\n---\n", "id"),
            Some("TSK-238\u{a0}".to_string())
        );
    }
}

#[cfg(test)]
mod r16_core_regressions {
    #[test]
    fn r16_scalar_quotes_are_removed_once() {
        assert_eq!(
            super::frontmatter_value("---\nid: \"'TSK-238'\"\n---\n", "id"),
            Some("'TSK-238'".to_string())
        );
    }
}
