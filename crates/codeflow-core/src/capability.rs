//! Capability catalogs from legacy fences, checkout records and pinned Git trees.
//!
//! The registry is the WHAT layer of the knowledge model (charter §5): one
//! entry per user-meaningful capability, each a heading plus a fenced yaml
//! block. This parser is shared by the `status` view (capability table) and
//! the `validate --docs` referential-integrity lint, which needs line
//! numbers for `file:line`-style messages.

pub mod bootstrap;
pub mod coverage;
pub mod create;
pub mod migrate;

use serde::Deserialize;

mod evidence;
pub(crate) use evidence::resolve_evidence;
pub use evidence::{
    is_typed_evidence, parse_evidence, Evidence, TestIdentity, CAPABILITY_MAX_EVIDENCE_FILE_BYTES,
    EVIDENCE_KINDS, GAP_PROOFS, VERIFICATION_RECORD_DIR,
};
pub mod impact;

/// Legal capability status values (charter §5: planned → building →
/// shipped → deprecated; deprecate, never delete).
pub const CAPABILITY_STATUS_VALUES: &[&str] = &["planned", "building", "shipped", "deprecated"];

/// A parsed capability entry from the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityEntry {
    /// Capability id (`CAP-###`).
    pub id: String,
    pub name: String,
    pub area: String,
    /// Status string as written (validated by the docs lint, not here).
    pub status: String,
    /// What proves the capability works, as typed entries ([`parse_evidence`]).
    /// A legacy fence may still hold free-form tags.
    pub verified_by: Vec<String>,
    /// `EPC-###` ids that built or changed it.
    pub epics: Vec<String>,
    /// Singular epic relationship, accepted for parity with record frontmatter.
    pub epic_id: Vec<String>,
    /// Specification ids related to this capability.
    pub specs: Vec<String>,
    /// Records that must land before this capability.
    pub depends_on: Vec<String>,
    /// Other capability ids explicitly related to this record.
    pub capabilities: Vec<String>,
    /// ADR ids that shaped it.
    pub adrs: Vec<String>,
    /// General related record ids.
    pub related: Vec<String>,
    /// Record ids that supersede this capability.
    pub superseded_by: Vec<String>,
    /// 1-based line number of the entry's yaml block opening fence.
    pub line: usize,
}

/// A problem found while parsing the registry (malformed yaml block).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityParseIssue {
    /// 1-based line number of the offending yaml block's opening fence.
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawCapability {
    #[serde(deserialize_with = "bounded_string")]
    id: String,
    #[serde(deserialize_with = "bounded_string")]
    name: String,
    #[serde(deserialize_with = "bounded_string")]
    area: String,
    #[serde(deserialize_with = "bounded_string")]
    status: String,
    #[serde(deserialize_with = "bounded_strings")]
    verified_by: Vec<String>,
    #[serde(deserialize_with = "bounded_reference_list")]
    epics: Vec<String>,
    #[serde(deserialize_with = "bounded_reference_list")]
    epic_id: Vec<String>,
    #[serde(deserialize_with = "bounded_reference_list")]
    specs: Vec<String>,
    #[serde(deserialize_with = "bounded_reference_list")]
    depends_on: Vec<String>,
    #[serde(deserialize_with = "bounded_reference_list")]
    capabilities: Vec<String>,
    #[serde(deserialize_with = "bounded_reference_list")]
    adrs: Vec<String>,
    #[serde(deserialize_with = "bounded_reference_list")]
    related: Vec<String>,
    #[serde(deserialize_with = "bounded_reference_list")]
    superseded_by: Vec<String>,
}

impl RawCapability {
    fn into_entry(self, line: usize) -> CapabilityEntry {
        let raw = self;
        CapabilityEntry {
            id: raw.id,
            name: raw.name,
            area: raw.area,
            status: raw.status,
            verified_by: raw.verified_by,
            epics: raw.epics,
            epic_id: raw.epic_id,
            specs: raw.specs,
            depends_on: raw.depends_on,
            capabilities: raw.capabilities,
            adrs: raw.adrs,
            related: raw.related,
            superseded_by: raw.superseded_by,
            line,
        }
    }
}

/// Blank out every character inside an HTML comment (`<!-- ... -->`),
/// preserving newlines so 1-based line numbers stay accurate. An unterminated
/// comment runs to end of input. This keeps example capability entries that
/// live inside comments — such as the scaffold template's `<!-- ... ##
/// CAP-001 ... -->` — from being parsed as real entries (which would surface a
/// phantom capability named `<name>` in a fresh repo's `status`/`orient`).
fn mask_html_comments(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        let body = &rest[start..];
        let end = body[4..].find("-->").map_or(body.len(), |rel| 4 + rel + 3);
        for ch in body[..end].chars() {
            out.push(if ch == '\n' { '\n' } else { ' ' });
        }
        rest = &body[end..];
    }
    out.push_str(rest);
    out
}

/// Parse capability entries out of registry markdown content.
///
/// Scans for fenced ` ```yaml ` blocks whose yaml carries an `id` starting
/// with `CAP-`; other yaml blocks are ignored. Content inside HTML comment
/// blocks is skipped, so example entries embedded in comments are not parsed.
/// Malformed blocks that look like capability entries are reported as parse
/// issues, never silently dropped (charter principle 8: legible degradation).
#[must_use]
pub fn parse_capabilities(content: &str) -> (Vec<CapabilityEntry>, Vec<CapabilityParseIssue>) {
    let mut entries = Vec::new();
    let mut issues = Vec::new();
    if content.len() > CAPABILITY_MAX_FILE_BYTES {
        issues.push(CapabilityParseIssue {
            line: 1,
            message: format!("input exceeds {CAPABILITY_MAX_FILE_BYTES} bytes"),
        });
        return (entries, issues);
    }

    let masked = mask_html_comments(content);
    let lines: Vec<&str> = masked.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim_matches([' ', '\t']);
        let closing = match trimmed {
            "```yaml" | "```yml" => "```",
            "~~~yaml" | "~~~yml" => "~~~",
            _ => {
                i += 1;
                continue;
            }
        };
        let fence_line = i + 1; // 1-based
        let block_start = i + 1;
        let mut j = block_start;
        while j < lines.len() && lines[j].trim_matches([' ', '\t']) != closing {
            j += 1;
        }
        let block = lines[block_start..j].join("\n");
        i = j + 1;

        // Only yaml blocks that look like capability entries participate.
        if !block.contains("CAP-") {
            continue;
        }

        match serde_yaml::from_str::<RawCapability>(&block) {
            Ok(raw) if raw.id.starts_with("CAP-") => entries.push(raw.into_entry(fence_line)),
            Ok(_) => {} // yaml mentioning CAP- without a CAP id — not an entry
            Err(e) => issues.push(CapabilityParseIssue {
                line: fence_line,
                message: format!("malformed capability yaml block: {e}"),
            }),
        }
    }

    (entries, issues)
}

/// Maximum Unicode characters in a frontmatter scalar or list item.
pub const CAPABILITY_MAX_VALUE_CHARS: usize = 1024;
/// Maximum items in a frontmatter list.
pub const CAPABILITY_MAX_LIST_ITEMS: usize = 64;
/// Maximum keys in a record's frontmatter.
pub const CAPABILITY_MAX_FRONTMATTER_KEYS: usize = 32;
/// Maximum bytes per catalog input, including the legacy Markdown and vocabulary.
pub const CAPABILITY_MAX_FILE_BYTES: usize = 256 * 1024;
/// Bound the aggregate work of an untrusted tree catalog before reading blobs.
pub const CAPABILITY_MAX_TREE_FILES: usize = 10_000;
/// Bound aggregate blob output before the batch reader allocates it.
pub const CAPABILITY_MAX_TREE_BYTES: usize = 64 * 1024 * 1024;

const CATALOG_DIR: &str = "docs/capabilities";
const LEGACY_PATH: &str = "docs/capabilities.md";
const VOCABULARY_PATH: &str = "docs/capabilities/vocabulary.yaml";
const PROJECT_PATH: &str = ".codeflow/project.toml";
const STAGES: &[&str] = &["unassessed", "experimental", "beta", "stable"];
const QUALITIES: &[&str] = &[
    "functional-suitability",
    "performance-efficiency",
    "compatibility",
    "interaction-capability",
    "reliability",
    "security",
    "maintainability",
    "flexibility",
    "safety",
];

const RECORD_KEYS: &[&str] = &[
    "id",
    "uid",
    "name",
    "summary",
    "status",
    "stage",
    "kind",
    "quality",
    "measure",
    "audience",
    "publish",
    "area",
    "group",
    "tags",
    "verified_by",
    "since",
    "former_ids",
    "epics",
    "epic_id",
    "adrs",
    "specs",
    "depends_on",
    "related",
    "superseded_by",
    "capabilities",
];

/// One catalog entry, with its source and optional modern facets.
/// Legacy fences have no uid or modern facets; their identity remains their id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityRecord {
    pub entry: CapabilityEntry,
    pub file: std::path::PathBuf,
    pub uid: Option<String>,
    pub summary: Option<String>,
    pub stage: Option<String>,
    pub kind: Option<String>,
    pub quality: Option<String>,
    pub measure: Option<String>,
    pub audience: Vec<String>,
    pub publish: bool,
    pub group: Option<String>,
    pub tags: Vec<String>,
    pub since: Option<String>,
    pub former_ids: Vec<String>,
    pub prose: String,
}

/// A project-owned facet definition.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CapabilityTerm {
    #[serde(deserialize_with = "bounded_string")]
    pub name: String,
    #[serde(deserialize_with = "bounded_string")]
    pub about: String,
}

/// The vocabulary from the same revision as the records.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CapabilityVocabulary {
    #[serde(deserialize_with = "bounded_string")]
    pub inventory: String,
    #[serde(default, deserialize_with = "bounded_terms")]
    pub groups: Vec<CapabilityTerm>,
    #[serde(default, deserialize_with = "bounded_terms")]
    pub tags: Vec<CapabilityTerm>,
}

/// A validated catalog. Both input adapters produce the same relative paths.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CapabilityCatalog {
    /// Validated entries sorted by id. An empty inventory has no entries.
    pub records: Vec<CapabilityRecord>,
    pub vocabulary: Option<CapabilityVocabulary>,
    /// Whether a legacy file or a modern record exists, including an empty
    /// legacy file. This describes the documentation layer, not inventory coverage.
    pub present: bool,
}

/// A catalog refusal with its source location.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{file}:{line}: {message}")]
pub struct CapabilityCatalogError {
    pub file: String,
    pub line: usize,
    pub message: String,
}

fn catalog_error(file: &str, message: impl Into<String>) -> CapabilityCatalogError {
    CapabilityCatalogError {
        file: file.into(),
        line: 1,
        message: message.into(),
    }
}

// The closed field set and duplicate-key refusals imply the 32-key bound.
const _: () = assert!(RECORD_KEYS.len() <= CAPABILITY_MAX_FRONTMATTER_KEYS);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordFields {
    #[serde(deserialize_with = "bounded_string")]
    id: String,
    #[serde(deserialize_with = "bounded_string")]
    name: String,
    #[serde(deserialize_with = "bounded_string")]
    area: String,
    #[serde(deserialize_with = "bounded_string")]
    status: String,
    #[serde(deserialize_with = "bounded_string")]
    uid: String,
    #[serde(deserialize_with = "bounded_string")]
    summary: String,
    #[serde(deserialize_with = "bounded_string")]
    kind: String,
    #[serde(default, deserialize_with = "bounded_optional_string")]
    stage: Option<String>,
    #[serde(default, deserialize_with = "bounded_optional_string")]
    quality: Option<String>,
    #[serde(default, deserialize_with = "bounded_optional_string")]
    measure: Option<String>,
    #[serde(default, deserialize_with = "bounded_optional_string")]
    group: Option<String>,
    #[serde(default, deserialize_with = "bounded_optional_string")]
    since: Option<String>,
    #[serde(deserialize_with = "bounded_strings")]
    audience: Vec<String>,
    #[serde(default, deserialize_with = "bounded_strings")]
    verified_by: Vec<String>,
    #[serde(default, deserialize_with = "bounded_strings")]
    tags: Vec<String>,
    #[serde(default, deserialize_with = "bounded_strings")]
    former_ids: Vec<String>,
    #[serde(default, deserialize_with = "bounded_reference_list")]
    epics: Vec<String>,
    #[serde(default, deserialize_with = "bounded_reference_list")]
    epic_id: Vec<String>,
    #[serde(default, deserialize_with = "bounded_reference_list")]
    adrs: Vec<String>,
    #[serde(default, deserialize_with = "bounded_reference_list")]
    specs: Vec<String>,
    #[serde(default, deserialize_with = "bounded_reference_list")]
    depends_on: Vec<String>,
    #[serde(default, deserialize_with = "bounded_reference_list")]
    capabilities: Vec<String>,
    #[serde(default, deserialize_with = "bounded_reference_list")]
    related: Vec<String>,
    #[serde(default, deserialize_with = "bounded_reference_list")]
    superseded_by: Vec<String>,
    #[serde(default)]
    publish: bool,
}

fn numeric_capability_id(id: &str) -> bool {
    id.strip_prefix("CAP-")
        .is_some_and(|digits| digits.len() >= 3 && digits.bytes().all(|c| c.is_ascii_digit()))
}

fn slug(value: &str) -> bool {
    !value.is_empty()
        && value.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

/// Check a record's repository-relative path, without opening it.
///
/// # Errors
/// Refuses absolute, traversing, nested or incorrectly named paths.
pub fn validate_capability_path(path: &str) -> Result<(), CapabilityCatalogError> {
    let Some(name) = path.strip_prefix("docs/capabilities/") else {
        return Err(catalog_error(
            path,
            "record must be directly under docs/capabilities/",
        ));
    };
    let valid = name
        .strip_suffix(".md")
        .and_then(|stem| {
            let rest = stem.strip_prefix("CAP-")?;
            let (digits, name) = rest.split_once('-')?;
            Some(numeric_capability_id(&format!("CAP-{digits}")) && slug(name))
        })
        .unwrap_or(false);
    if !valid {
        return Err(catalog_error(path, "record must be directly under docs/capabilities/ as CAP-NNN-<slug>.md; absolute paths, '..' and nested paths are refused"));
    }
    Ok(())
}

struct BoundedString(String);
impl<'de> Deserialize<'de> for BoundedString {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(StringVisitor).map(Self)
    }
}
struct StringVisitor;
impl serde::de::Visitor<'_> for StringVisitor {
    type Value = String;
    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "a string of at most {CAPABILITY_MAX_VALUE_CHARS} characters"
        )
    }
    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<String, E> {
        if value.chars().take(CAPABILITY_MAX_VALUE_CHARS + 1).count() > CAPABILITY_MAX_VALUE_CHARS {
            return Err(E::custom(format!(
                "value exceeds {CAPABILITY_MAX_VALUE_CHARS} characters"
            )));
        }
        Ok(value.to_owned())
    }
}
fn bounded_string<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    BoundedString::deserialize(deserializer).map(|value| value.0)
}
fn bounded_optional_string<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Option::<BoundedString>::deserialize(deserializer).map(|value| value.map(|value| value.0))
}
struct StringListVisitor<const ALLOW_SCALAR: bool>;
impl<'de, const ALLOW_SCALAR: bool> serde::de::Visitor<'de> for StringListVisitor<ALLOW_SCALAR> {
    type Value = Vec<String>;
    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "a list of at most {CAPABILITY_MAX_LIST_ITEMS} strings"
        )
    }
    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, sequence: A) -> Result<Vec<String>, A::Error> {
        BoundedListVisitor::<BoundedString>(std::marker::PhantomData)
            .visit_seq(sequence)
            .map(|values| values.into_iter().map(|value| value.0).collect())
    }

    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Vec<String>, E> {
        if ALLOW_SCALAR {
            StringVisitor.visit_str(value).map(|value| vec![value])
        } else {
            Err(E::custom("expected a list of strings"))
        }
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<Vec<String>, E> {
        if ALLOW_SCALAR {
            Ok(Vec::new())
        } else {
            Err(E::custom("expected a list of strings"))
        }
    }
}
struct BoundedListVisitor<T>(std::marker::PhantomData<T>);
impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for BoundedListVisitor<T> {
    type Value = Vec<T>;
    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "a list of at most {CAPABILITY_MAX_LIST_ITEMS} items"
        )
    }
    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut sequence: A) -> Result<Vec<T>, A::Error> {
        let mut values = Vec::new();
        while values.len() < CAPABILITY_MAX_LIST_ITEMS {
            let Some(value) = sequence.next_element::<T>()? else {
                return Ok(values);
            };
            values.push(value);
        }
        // Refuse item 65 without reading its value or an aliased subtree.
        sequence.next_element_seed(RejectExtraItem)?;
        Ok(values)
    }
}
fn bounded_terms<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<CapabilityTerm>, D::Error> {
    deserializer.deserialize_seq(BoundedListVisitor::<CapabilityTerm>(
        std::marker::PhantomData,
    ))
}
struct RejectExtraItem;
impl<'de> serde::de::DeserializeSeed<'de> for RejectExtraItem {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, _deserializer: D) -> Result<(), D::Error> {
        Err(serde::de::Error::custom(format!(
            "list exceeds {CAPABILITY_MAX_LIST_ITEMS} items"
        )))
    }
}
fn bounded_strings<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    deserializer.deserialize_seq(StringListVisitor::<false>)
}
fn bounded_reference_list<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    deserializer.deserialize_any(StringListVisitor::<true>)
}
fn frontmatter_fence(line: &str) -> bool {
    line.trim_end_matches([' ', '\t', '\r', '\n']) == "---"
}

fn parse_record(file: &str, content: &str) -> Result<CapabilityRecord, CapabilityCatalogError> {
    let mut lines = content.split_inclusive('\n');
    if !lines.next().is_some_and(frontmatter_fence) {
        return Err(catalog_error(file, "record requires YAML frontmatter"));
    }
    let mut yaml = String::new();
    let mut closed = false;
    for line in lines.by_ref() {
        if frontmatter_fence(line) {
            closed = true;
            break;
        }
        yaml.push_str(line);
    }
    if !closed {
        return Err(catalog_error(file, "unterminated YAML frontmatter"));
    }
    let raw: RecordFields = serde_yaml::from_str(&yaml)
        .map_err(|error| catalog_error(file, format!("frontmatter YAML: {error}")))?;
    let uid = uuid::Uuid::parse_str(&raw.uid)
        .map_err(|error| catalog_error(file, format!("uid: {error}")))?
        .to_string();
    Ok(CapabilityRecord {
        entry: CapabilityEntry {
            id: raw.id,
            name: raw.name,
            area: raw.area,
            status: raw.status,
            verified_by: raw.verified_by,
            epics: raw.epics,
            epic_id: raw.epic_id,
            adrs: raw.adrs,
            specs: raw.specs,
            depends_on: raw.depends_on,
            capabilities: raw.capabilities,
            related: raw.related,
            superseded_by: raw.superseded_by,
            line: 1,
        },
        file: file.into(),
        uid: Some(uid),
        summary: Some(raw.summary),
        stage: raw.stage,
        kind: Some(raw.kind),
        quality: raw.quality,
        measure: raw.measure,
        audience: raw.audience,
        publish: raw.publish,
        group: raw.group,
        tags: raw.tags,
        since: raw.since,
        former_ids: raw.former_ids,
        prose: lines.collect(),
    })
}

fn validate_record(
    record: &CapabilityRecord,
    areas: &[String],
    vocabulary: Option<&CapabilityVocabulary>,
) -> Result<(), CapabilityCatalogError> {
    let file = record.file.to_string_lossy();
    let bad = |key: &str, reason: &str| CapabilityCatalogError {
        file: file.to_string(),
        line: record.entry.line,
        message: format!("{key}: {reason}"),
    };
    let entry = &record.entry;
    if !CAPABILITY_STATUS_VALUES.contains(&entry.status.as_str()) {
        return Err(bad(
            "status",
            &format!(
                "\"{}\" is not one of: {}",
                entry.status,
                CAPABILITY_STATUS_VALUES.join(", ")
            ),
        ));
    }
    if entry.status == "shipped"
        && entry
            .verified_by
            .iter()
            .all(|value| value.trim().is_empty())
    {
        return Err(bad(
            "verified_by",
            "shipped capability has empty verified_by",
        ));
    }
    if record.uid.is_none() {
        return Ok(());
    }
    if !numeric_capability_id(&entry.id) {
        return Err(bad("id", "expected CAP- followed by at least three digits"));
    }

    let expected = format!("{CATALOG_DIR}/{}-{}.md", entry.id, entry.name);
    if expected != file {
        return Err(bad("id/name", "must match the record filename"));
    }
    if !slug(&entry.name) {
        return Err(bad("name", "expected a lowercase slug"));
    }
    if record.summary.as_ref().is_none_or(|s| s.trim().is_empty()) {
        return Err(bad("summary", "must not be empty"));
    }
    if !areas.contains(&entry.area) {
        return Err(bad(
            "area",
            "not present in this revision's .codeflow/project.toml areas",
        ));
    }
    if !matches!(
        record.kind.as_deref(),
        Some("functional" | "non-functional")
    ) {
        return Err(bad("kind", "expected functional or non-functional"));
    }
    if record.kind.as_deref() == Some("non-functional") {
        if record.quality.is_none() {
            return Err(bad("quality", "required for non-functional capabilities"));
        }
        if record.measure.as_ref().is_none_or(|s| s.trim().is_empty()) {
            return Err(bad("measure", "required for non-functional capabilities"));
        }
    }
    if record
        .quality
        .as_ref()
        .is_some_and(|q| !QUALITIES.contains(&q.as_str()))
    {
        return Err(bad("quality", "unknown quality characteristic"));
    }
    if record.audience.is_empty()
        || record
            .audience
            .iter()
            .any(|a| !["external", "internal", "agent"].contains(&a.as_str()))
    {
        return Err(bad(
            "audience",
            "expected a nonempty list of external, internal or agent",
        ));
    }
    validate_evidence(record)?;
    if record
        .former_ids
        .iter()
        .any(|id| !numeric_capability_id(id))
    {
        return Err(bad("former_ids", "expected numeric CAP ids"));
    }
    validate_reference_ids(record)?;
    validate_project_facets(record, vocabulary)
}

fn validate_reference_ids(record: &CapabilityRecord) -> Result<(), CapabilityCatalogError> {
    let entry = &record.entry;
    for (key, values, prefix) in [
        ("epics", &entry.epics, Some("EPC-")),
        ("epic_id", &entry.epic_id, Some("EPC-")),
        ("adrs", &entry.adrs, Some("ADR-")),
        ("specs", &entry.specs, Some("SPC-")),
        ("capabilities", &entry.capabilities, Some("CAP-")),
        ("depends_on", &entry.depends_on, None),
        ("related", &entry.related, None),
        ("superseded_by", &entry.superseded_by, None),
    ] {
        for id in values {
            let valid = numeric_capability_id(id) || crate::ids::RegId::parse(id).is_some();
            if !valid || prefix.is_some_and(|prefix| !id.starts_with(prefix)) {
                return Err(catalog_error(
                    &record.file.to_string_lossy(),
                    format!("{key}: expected a record id, found {id:?}"),
                ));
            }
        }
    }
    Ok(())
}

fn validate_project_facets(
    record: &CapabilityRecord,
    vocabulary: Option<&CapabilityVocabulary>,
) -> Result<(), CapabilityCatalogError> {
    let file = record.file.to_string_lossy();
    let bad = |key: &str, reason: &str| catalog_error(&file, format!("{key}: {reason}"));
    if let Some(group) = &record.group {
        if !vocabulary.is_some_and(|v| v.groups.iter().any(|term| &term.name == group)) {
            return Err(bad(
                "group",
                "not present in this revision's vocabulary.yaml",
            ));
        }
    }
    for tag in &record.tags {
        if !vocabulary.is_some_and(|v| v.tags.iter().any(|term| &term.name == tag)) {
            return Err(bad(
                "tags",
                "not present in this revision's vocabulary.yaml",
            ));
        }
    }
    Ok(())
}

fn validate_evidence(record: &CapabilityRecord) -> Result<(), CapabilityCatalogError> {
    let file = record.file.to_string_lossy();
    let bad = |key: &str, reason: &str| catalog_error(&file, format!("{key}: {reason}"));
    let entry = &record.entry;
    if record
        .stage
        .as_ref()
        .is_some_and(|s| !STAGES.contains(&s.as_str()))
    {
        return Err(bad("stage", "unknown maturity stage"));
    }
    if entry.status == "shipped" && (record.stage.is_none() || entry.verified_by.is_empty()) {
        return Err(bad("stage/verified_by", "both are required when shipped"));
    }
    let mut proof = false;
    for value in &entry.verified_by {
        let evidence = parse_evidence(value).map_err(|reason| {
            bad(
                "verified_by",
                &format!("{}: entry {value:?}: {reason}", entry.id),
            )
        })?;
        proof |= evidence.is_proof();
    }
    if matches!(record.stage.as_deref(), Some("beta" | "stable")) && !proof {
        return Err(bad(
            "verified_by",
            "beta and stable require non-gap evidence",
        ));
    }
    if entry.status == "shipped" && !proof && record.stage.as_deref() != Some("unassessed") {
        return Err(bad(
            "stage",
            &format!(
                "{}: a shipped capability whose only evidence is gap: must carry stage: unassessed",
                entry.id
            ),
        ));
    }
    Ok(())
}

fn parse_vocabulary(content: &str) -> Result<CapabilityVocabulary, CapabilityCatalogError> {
    let vocabulary: CapabilityVocabulary = serde_yaml::from_str(content).map_err(|error| {
        catalog_error(VOCABULARY_PATH, format!("malformed vocabulary: {error}"))
    })?;
    if !["complete", "incomplete"].contains(&vocabulary.inventory.as_str()) {
        return Err(catalog_error(
            VOCABULARY_PATH,
            "inventory: expected complete or incomplete",
        ));
    }
    for (key, terms) in [("groups", &vocabulary.groups), ("tags", &vocabulary.tags)] {
        let mut names = std::collections::BTreeSet::new();
        for term in terms {
            if !slug(&term.name) || !names.insert(&term.name) {
                return Err(catalog_error(
                    VOCABULARY_PATH,
                    format!("{key}: names must be unique lowercase slugs"),
                ));
            }
        }
    }
    Ok(vocabulary)
}

#[derive(Deserialize)]
struct AreaConfig {
    areas: Vec<String>,
}

fn legacy_record(entry: CapabilityEntry) -> CapabilityRecord {
    CapabilityRecord {
        entry,
        file: LEGACY_PATH.into(),
        uid: None,
        summary: None,
        stage: None,
        kind: None,
        quality: None,
        measure: None,
        audience: vec![],
        publish: false,
        group: None,
        tags: vec![],
        since: None,
        former_ids: vec![],
        prose: String::new(),
    }
}

/// The 0-based line and level of each heading outside quotes, lists and
/// fences. Shared by the legacy reader and `capability migrate`.
fn top_level_headings(content: &str) -> Vec<(usize, pulldown_cmark::HeadingLevel)> {
    use pulldown_cmark::{Event, Parser, Tag};
    let offsets: Vec<_> = content
        .split_inclusive('\n')
        .scan(0, |offset, line| {
            let start = *offset;
            *offset += line.len();
            Some(start)
        })
        .collect();
    let mut headings = Vec::new();
    let mut depth = 0usize;
    for (event, span) in Parser::new(content).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) if depth == 0 => {
                headings.push((
                    offsets
                        .partition_point(|start| *start <= span.start)
                        .saturating_sub(1),
                    level,
                ));
                depth += 1;
            }
            Event::Start(_) => depth += 1,
            Event::End(_) => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    headings
}

/// Keep each legacy entry's surrounding Markdown with the parsed entry.
/// An equal-or-higher heading ends an entry's section; global guidance and
/// group headings do not become capability prose. Quoted and fenced headings
/// do not divide records. YAML parsing stays in `parse_capabilities`.
fn legacy_records(content: &str, entries: Vec<CapabilityEntry>) -> Vec<CapabilityRecord> {
    let lines: Vec<_> = content.split_inclusive('\n').collect();
    let masked = mask_html_comments(content);
    let syntax: Vec<_> = masked.lines().collect();
    let headings = top_level_headings(content);
    let mut previous = 0;
    let starts: Vec<_> = entries
        .iter()
        .map(|entry| {
            let fence = entry.line - 1;
            let start = headings
                .iter()
                .rev()
                .find(|(line, _)| *line < fence && *line >= previous)
                .copied()
                .map_or((fence, None), |(line, level)| (line, Some(level)));
            previous = fence + 1;
            start
        })
        .collect();
    entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            let fence = entry.line - 1;
            let marker = if syntax[fence].trim().starts_with('~') {
                "~~~"
            } else {
                "```"
            };
            let end_fence = syntax[fence + 1..]
                .iter()
                .position(|line| line.trim() == marker)
                .map_or(lines.len(), |n| fence + n + 2);
            let (start, level) = starts[index];
            let next = starts.get(index + 1).map_or(lines.len(), |(line, _)| *line);
            let end = headings
                .iter()
                .find(|(line, heading)| {
                    *line >= end_fence && level.is_some_and(|level| *heading <= level)
                })
                .map_or(next, |(line, _)| next.min(*line));
            let mut record = legacy_record(entry);
            record.prose = lines[start..fence].concat();
            if end_fence <= end {
                record.prose.push_str(&lines[end_fence..end].concat());
            }
            record
        })
        .collect()
}

fn parse_catalog(
    files: &std::collections::BTreeMap<String, Vec<u8>>,
) -> Result<CapabilityCatalog, CapabilityCatalogError> {
    let (catalog, issues) = parse_catalog_with_issues(files);
    issues.into_iter().next().map_or(Ok(catalog), Err)
}

fn parse_catalog_with_issues(
    files: &std::collections::BTreeMap<String, Vec<u8>>,
) -> (CapabilityCatalog, Vec<CapabilityCatalogError>) {
    let mut catalog = CapabilityCatalog {
        present: files.contains_key(LEGACY_PATH),
        ..CapabilityCatalog::default()
    };
    let mut issues = Vec::new();
    let mut texts = std::collections::BTreeMap::new();
    for (file, bytes) in files {
        if bytes.len() > CAPABILITY_MAX_FILE_BYTES {
            issues.push(catalog_error(
                file,
                format!("input exceeds {CAPABILITY_MAX_FILE_BYTES} bytes"),
            ));
            continue;
        }
        match std::str::from_utf8(bytes) {
            Ok(text) => {
                texts.insert(file.as_str(), text);
            }
            Err(error) => issues.push(catalog_error(file, format!("invalid UTF-8: {error}"))),
        }
    }
    if let Some(content) = texts.get(LEGACY_PATH) {
        let (entries, errors) = parse_capabilities(content);
        issues.extend(errors.into_iter().map(|error| CapabilityCatalogError {
            file: LEGACY_PATH.into(),
            line: error.line,
            message: error.message,
        }));
        catalog.records.extend(legacy_records(content, entries));
    }
    if let Some(content) = texts.get(VOCABULARY_PATH) {
        match parse_vocabulary(content) {
            Ok(vocabulary) => catalog.vocabulary = Some(vocabulary),
            Err(error) => issues.push(error),
        }
    }
    let legacy_ids: std::collections::BTreeSet<_> =
        catalog.records.iter().map(|r| r.entry.id.clone()).collect();
    for (file, content) in &texts {
        if !file.starts_with("docs/capabilities/") || *file == VOCABULARY_PATH {
            continue;
        }
        catalog.present = true;
        if let Err(error) = validate_capability_path(file) {
            issues.push(error);
            continue;
        }
        let name = file.trim_start_matches("docs/capabilities/");
        let id = name.split('-').take(2).collect::<Vec<_>>().join("-");
        if !content.lines().next().is_some_and(frontmatter_fence) && legacy_ids.contains(&id) {
            continue;
        }
        match parse_record(file, content) {
            Ok(record) => catalog.records.push(record),
            Err(error) => issues.push(error),
        }
    }
    let mut areas = Vec::new();
    if catalog.records.iter().any(|r| r.uid.is_some()) {
        match texts.get(PROJECT_PATH) {
            Some(content) => match toml::from_str::<AreaConfig>(content) {
                Ok(config) => areas = config.areas,
                Err(error) => issues.push(catalog_error(PROJECT_PATH, format!("areas: {error}"))),
            },
            None => issues.push(catalog_error(
                PROJECT_PATH,
                "areas: project configuration is required for catalog records",
            )),
        }
    }
    let mut ids = std::collections::BTreeMap::new();
    let mut uids = std::collections::BTreeMap::new();
    for record in &catalog.records {
        let file = record.file.to_string_lossy();
        for (key, value, seen) in [
            ("id", Some(&record.entry.id), &mut ids),
            ("uid", record.uid.as_ref(), &mut uids),
        ] {
            if let Some(value) = value {
                if let Some(previous) = seen.insert(value.clone(), file.to_string()) {
                    issues.push(catalog_error(
                        &file,
                        format!("{key}: duplicate {value}; also in {previous}"),
                    ));
                }
            }
        }
    }
    for record in &catalog.records {
        if let Err(error) = validate_record(record, &areas, catalog.vocabulary.as_ref()) {
            issues.push(error);
        }
    }
    catalog.records.sort_by(|a, b| a.entry.id.cmp(&b.entry.id));
    (catalog, issues)
}

fn selected_file(path: &str) -> bool {
    path == LEGACY_PATH
        || path == PROJECT_PATH
        || path == VOCABULARY_PATH
        || path
            .strip_prefix("docs/capabilities/")
            .is_some_and(|name| name.starts_with("CAP-") || name.contains("/CAP-"))
}

fn record_file(path: &str) -> bool {
    path.starts_with("docs/capabilities/") && path != VOCABULARY_PATH
}

fn check_mode(path: &str, mode: &str) -> Result<(), CapabilityCatalogError> {
    if mode != "100644" && (mode != "100755" || record_file(path)) {
        return Err(catalog_error(
            path,
            format!("expected a regular non-executable file, found mode {mode}"),
        ));
    }
    Ok(())
}

fn collect_catalog_paths(
    tree: &crate::contained::Tree,
    directory: &str,
    paths: &mut Vec<String>,
    issues: &mut Vec<CapabilityCatalogError>,
) -> bool {
    let entries = match tree.entries(directory, usize::MAX) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
        Err(error) => {
            issues.push(catalog_error(directory, error.to_string()));
            return true;
        }
    };
    for name in entries {
        let Ok(name) = name.into_string() else {
            issues.push(catalog_error(directory, "non-UTF-8 catalog filename"));
            continue;
        };
        let path = format!("{directory}/{name}");
        if selected_file(&path) {
            paths.push(path);
        } else {
            match tree.metadata(&path) {
                Ok(metadata) if metadata.is_dir() => {
                    collect_catalog_paths(tree, &path, paths, issues);
                }
                Ok(_) => {}
                Err(error) => issues.push(catalog_error(&path, error.to_string())),
            }
        }
    }
    true
}

/// Read and validate a checkout through repository-confined file handles.
/// References inside records are syntax-checked data, never opened or executed.
///
/// # Errors
/// Returns a source-addressed refusal for unreadable or invalid input.
pub fn load_capability_catalog(
    root: &std::path::Path,
) -> Result<CapabilityCatalog, CapabilityCatalogError> {
    let (catalog, issues) = inspect_capability_catalog(root);
    issues.into_iter().next().map_or(Ok(catalog), Err)
}

/// Retain parsed records and every refusal for collect-all documentation checks.
/// Presence describes the file or folder, independently of schema validity.
pub(crate) fn inspect_capability_catalog(
    root: &std::path::Path,
) -> (CapabilityCatalog, Vec<CapabilityCatalogError>) {
    let tree = match crate::contained::Tree::open(root) {
        Ok(tree) => tree,
        Err(error) => {
            return (
                CapabilityCatalog::default(),
                vec![catalog_error("<checkout>", error.to_string())],
            )
        }
    };
    let mut files = std::collections::BTreeMap::new();
    let mut issues = Vec::new();
    let mut paths = vec![LEGACY_PATH.to_string(), PROJECT_PATH.to_string()];
    let mut present = collect_catalog_paths(&tree, CATALOG_DIR, &mut paths, &mut issues);
    let index = match git2::Repository::discover(root) {
        Ok(repo) => match repo.index() {
            Ok(index) => Some(index),
            Err(error) => {
                issues.push(catalog_error("<index>", error.to_string()));
                None
            }
        },
        Err(error) if error.code() == git2::ErrorCode::NotFound => None,
        Err(error) => {
            issues.push(catalog_error("<index>", error.to_string()));
            None
        }
    };
    for path in paths {
        let read = tree.metadata(&path).and_then(|metadata| {
            if metadata.len() > CAPABILITY_MAX_FILE_BYTES as u64 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("input exceeds {CAPABILITY_MAX_FILE_BYTES} bytes"),
                ));
            }
            // The bounded read also enforces the limit if the file changes
            // after metadata was read; this precheck supplies a size diagnostic.
            tree.read(&path, CAPABILITY_MAX_FILE_BYTES as u64)
        });
        let bytes = match read {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                present |= path == LEGACY_PATH || record_file(&path);
                issues.push(catalog_error(&path, error.to_string()));
                continue;
            }
        };
        present |= path == LEGACY_PATH || record_file(&path);
        if record_file(&path) {
            if let Err(error) = validate_capability_path(&path) {
                issues.push(error);
                continue;
            }
            // The index is authoritative even when core.fileMode=false or the
            // filesystem reports synthetic execute bits. Untracked files have
            // no Git mode yet; a later indexed/tree read checks the added mode.
            if let Some(entry) = index
                .as_ref()
                .and_then(|index| index.get_path(std::path::Path::new(&path), 0))
            {
                if let Err(error) = check_mode(&path, &format!("{:o}", entry.mode)) {
                    issues.push(error);
                    continue;
                }
            }
        }
        files.insert(path, bytes);
    }
    let (mut catalog, parse_issues) = parse_catalog_with_issues(&files);
    catalog.present |= present;
    issues.extend(parse_issues);
    (catalog, issues)
}

/// Read and validate a pinned Git tree, independent of checkout content.
/// Only blob objects are read; no repository code or evidence reference runs.
///
/// # Errors
/// Returns a refusal for an invalid revision, unsafe mode, or invalid catalog.
pub fn load_capability_catalog_from_tree(
    root: &std::path::Path,
    revision: &str,
) -> Result<CapabilityCatalog, CapabilityCatalogError> {
    let git = crate::ids::git::Git::new(root);
    let tree = git
        .run(&[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{revision}^{{tree}}"),
        ])
        .map_err(|error| catalog_error("<tree>", error.to_string()))?;
    let repo = git2::Repository::discover(root)
        .map_err(|error| catalog_error("<tree>", error.to_string()))?;
    let entries = catalog_tree_entries(&repo, tree.trim())?;
    let odb = repo
        .odb()
        .map_err(|error| catalog_error("<tree>", error.to_string()))?;
    let mut total = 0usize;
    let mut selected = Vec::new();
    for (mode, blob, path) in entries {
        if ["docs", CATALOG_DIR, ".codeflow"].contains(&path.as_str()) {
            return Err(catalog_error(
                &path,
                "catalog input parent must be a real directory",
            ));
        }
        if selected_file(&path) {
            if path.starts_with("docs/capabilities/") && path != VOCABULARY_PATH {
                validate_capability_path(&path)?;
            }
            check_mode(&path, &mode)?;
            let oid = git2::Oid::from_str(&blob)
                .map_err(|error| catalog_error(&path, error.to_string()))?;
            let (size, kind) = odb
                .read_header(oid)
                .map_err(|error| catalog_error(&path, error.to_string()))?;
            if kind != git2::ObjectType::Blob {
                return Err(catalog_error(&path, "catalog input must be a blob"));
            }
            if size > CAPABILITY_MAX_FILE_BYTES {
                return Err(catalog_error(
                    &path,
                    format!("input exceeds {CAPABILITY_MAX_FILE_BYTES} bytes"),
                ));
            }
            total = total
                .checked_add(size)
                .ok_or_else(|| catalog_error(&path, "catalog byte count overflow"))?;
            if total > CAPABILITY_MAX_TREE_BYTES || selected.len() >= CAPABILITY_MAX_TREE_FILES {
                return Err(catalog_error(&path, format!("catalog exceeds {CAPABILITY_MAX_TREE_FILES} files or {CAPABILITY_MAX_TREE_BYTES} bytes")));
            }
            selected.push((path, blob));
        }
    }
    let ids: Vec<_> = selected.iter().map(|(_, blob)| blob.clone()).collect();
    let blobs = git
        .blobs(&ids)
        .map_err(|error| catalog_error("<tree>", error.to_string()))?;
    let mut files = std::collections::BTreeMap::new();
    for (path, blob) in selected {
        let content = blobs
            .get(&blob)
            .ok_or_else(|| catalog_error(&path, "blob is missing"))?;
        files.insert(path, content.clone());
    }
    parse_catalog(&files)
}

/// Inspect only the catalog and its exact configuration inputs. Do not list
/// unrelated documentation or configuration subtrees before applying bounds.
fn catalog_tree_entries(
    repo: &git2::Repository,
    revision: &str,
) -> Result<Vec<(String, String, String)>, CapabilityCatalogError> {
    fn directory<'r>(
        repo: &'r git2::Repository,
        tree: &git2::Tree<'_>,
        name: &str,
        path: &str,
    ) -> Result<Option<git2::Tree<'r>>, CapabilityCatalogError> {
        let Some(entry) = tree.get_name(name) else {
            return Ok(None);
        };
        if entry.filemode() != 0o040_000 {
            return Err(catalog_error(
                path,
                "catalog input parent must be a real directory",
            ));
        }
        bounded_catalog_tree(repo, entry.id(), path).map(Some)
    }
    fn row(entry: &git2::TreeEntry<'_>, path: String) -> (String, String, String) {
        (
            format!("{:o}", entry.filemode()),
            entry.id().to_string(),
            path,
        )
    }
    let id = git2::Oid::from_str(revision).map_err(|e| catalog_error("<tree>", e.to_string()))?;
    let tree = bounded_catalog_tree(repo, id, "<tree>")?;
    let mut entries = Vec::new();
    if let Some(docs) = directory(repo, &tree, "docs", "docs")? {
        if let Some(legacy) = docs.get_name("capabilities.md") {
            entries.push(row(&legacy, LEGACY_PATH.into()));
        }
        if let Some(catalog) = directory(repo, &docs, "capabilities", CATALOG_DIR)? {
            entries.extend(catalog_entries(repo, catalog.id())?);
        }
    }
    if let Some(config) = directory(repo, &tree, ".codeflow", ".codeflow")? {
        if let Some(project) = config.get_name("project.toml") {
            entries.push(row(&project, PROJECT_PATH.into()));
        }
    }
    Ok(entries)
}

/// Inspect nested directories only to find misplaced records. Unrelated
/// companion assets remain allowed, and traversal itself has a work bound.
fn catalog_entries(
    repo: &git2::Repository,
    catalog: git2::Oid,
) -> Result<Vec<(String, String, String)>, CapabilityCatalogError> {
    let mut pending = vec![(catalog, CATALOG_DIR.to_owned())];
    let mut entries = Vec::new();
    let mut count = 0usize;
    let mut path_bytes = 0usize;
    let mut tree_bytes = 0usize;
    while let Some((id, parent)) = pending.pop() {
        let (size, kind) = repo
            .odb()
            .and_then(|odb| odb.read_header(id))
            .map_err(|e| catalog_error(&parent, e.to_string()))?;
        tree_bytes = tree_bytes.saturating_add(size);
        if kind != git2::ObjectType::Tree || tree_bytes > CAPABILITY_MAX_TREE_BYTES {
            return Err(catalog_error(
                CATALOG_DIR,
                "catalog directory inventory exceeds the byte bound or is not a tree",
            ));
        }
        let tree = repo
            .find_tree(id)
            .map_err(|e| catalog_error(&parent, e.to_string()))?;
        count = count.saturating_add(tree.len());
        if count > CAPABILITY_MAX_TREE_FILES {
            return Err(catalog_error(
                CATALOG_DIR,
                format!("catalog exceeds {CAPABILITY_MAX_TREE_FILES} entries"),
            ));
        }
        for entry in &tree {
            let name = entry
                .name()
                .map_err(|_| catalog_error(&parent, "catalog path is not UTF-8"))?;
            path_bytes = path_bytes
                .saturating_add(parent.len())
                .saturating_add(1)
                .saturating_add(name.len());
            if path_bytes > CAPABILITY_MAX_TREE_BYTES {
                return Err(catalog_error(
                    CATALOG_DIR,
                    "catalog path inventory exceeds the byte bound",
                ));
            }
            let path = format!("{parent}/{name}");
            if selected_file(&path) {
                // The caller rejects a nested record or a non-blob mode,
                // naming the exact input just as the checkout reader does.
                entries.push((
                    format!("{:o}", entry.filemode()),
                    entry.id().to_string(),
                    path,
                ));
            } else if entry.kind() == Some(git2::ObjectType::Tree) {
                pending.push((entry.id(), path));
            }
        }
    }
    Ok(entries)
}

fn bounded_catalog_tree<'r>(
    repo: &'r git2::Repository,
    id: git2::Oid,
    path: &str,
) -> Result<git2::Tree<'r>, CapabilityCatalogError> {
    let (size, kind) = repo
        .odb()
        .and_then(|odb| odb.read_header(id))
        .map_err(|e| catalog_error(path, e.to_string()))?;
    if kind != git2::ObjectType::Tree || size > CAPABILITY_MAX_TREE_BYTES {
        return Err(catalog_error(
            path,
            "catalog input directory exceeds the byte bound or is not a tree",
        ));
    }
    repo.find_tree(id)
        .map_err(|e| catalog_error(path, e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const REGISTRY: &str = r"# myproject — capabilities

## CAP-001 — record validation

```yaml
id: CAP-001
name: record validation
area: engine
status: shipped
verified_by: [validate-core]
epics: [EPC-001]
adrs: [ADR-0001]
```

Validates epic and task frontmatter.

## CAP-002 — status view

```yaml
id: CAP-002
name: status view
area: engine
status: building
verified_by: []
epics: [EPC-002]
adrs: []
```

Generated status views.
";

    #[test]
    fn parses_all_entries() {
        let (entries, issues) = parse_capabilities(REGISTRY);
        assert!(issues.is_empty(), "unexpected issues: {issues:?}");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "CAP-001");
        assert_eq!(entries[0].status, "shipped");
        assert_eq!(entries[0].verified_by, vec!["validate-core"]);
        assert_eq!(entries[0].epics, vec!["EPC-001"]);
        assert_eq!(entries[0].adrs, vec!["ADR-0001"]);
        assert_eq!(entries[1].id, "CAP-002");
        assert!(entries[1].verified_by.is_empty());
    }

    #[test]
    fn parses_tilde_fenced_entries_without_treating_other_fences_as_records() {
        let content = "~~~yaml\nid: CAP-101\nname: tilde\narea: test\nstatus: planned\n~~~\n\n~~~text\nid: CAP-999\n~~~\n";
        let (entries, issues) = parse_capabilities(content);
        assert!(issues.is_empty());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "CAP-101");
    }

    #[test]
    fn records_fence_line_numbers() {
        let (entries, _) = parse_capabilities(REGISTRY);
        // First fence is on line 5 of the registry constant.
        assert_eq!(entries[0].line, 5);
        assert!(entries[1].line > entries[0].line);
    }

    /// TSK-103 AC-8: a capability may list a standalone task, which has no
    /// epic to reach it through.
    #[test]
    fn a_capability_may_list_a_standalone_task() {
        let content = "```yaml\nid: CAP-101\nname: fix\narea: test\nstatus: planned\nrelated: [TSK-004]\ndepends_on: [TSK-005]\n```\n";
        let (entries, issues) = parse_capabilities(content);
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(entries[0].related, ["TSK-004"]);
        assert_eq!(entries[0].depends_on, ["TSK-005"]);
        assert!(entries[0].epics.is_empty());
    }

    #[test]
    fn ignores_non_capability_yaml_blocks() {
        let content = "```yaml\nfoo: bar\n```\n";
        let (entries, issues) = parse_capabilities(content);
        assert!(entries.is_empty());
        assert!(issues.is_empty());
    }

    #[test]
    fn malformed_capability_block_is_an_issue() {
        let content = "intro\n\n```yaml\nid: CAP-003\nname: [unclosed\n```\n";
        let (entries, issues) = parse_capabilities(content);
        assert!(entries.is_empty());
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].line, 3);
        assert!(issues[0].message.contains("malformed"));
    }

    #[test]
    fn empty_content_yields_nothing() {
        let (entries, issues) = parse_capabilities("");
        assert!(entries.is_empty());
        assert!(issues.is_empty());
    }

    #[test]
    fn unclosed_fence_consumes_to_eof_without_panic() {
        let content = "```yaml\nid: CAP-009\nname: x\narea: a\nstatus: planned";
        let (entries, issues) = parse_capabilities(content);
        assert_eq!(entries.len(), 1, "issues: {issues:?}");
        assert_eq!(entries[0].id, "CAP-009");
    }

    #[test]
    fn skips_capability_entries_inside_html_comments() {
        // The scaffold template ships its example entry inside an HTML comment;
        // parsing it as real produced a phantom capability named `<name>` in a
        // fresh repo's status/orient. A comment-only registry yields nothing.
        const COMMENTED: &str = r"# myproject — capabilities

<!-- Entry format (example, not a real capability):

     ## CAP-001 — <name>

     ```yaml
     id: CAP-001
     name: <name>
     area: <area>
     status: planned
     verified_by: []
     epics: []
     adrs: []
     ```

     One paragraph describing the capability.
-->
";
        let (entries, issues) = parse_capabilities(COMMENTED);
        assert!(entries.is_empty(), "phantom entries: {entries:?}");
        assert!(issues.is_empty(), "unexpected issues: {issues:?}");
    }

    #[test]
    fn status_values_constant_matches_charter() {
        assert_eq!(
            CAPABILITY_STATUS_VALUES,
            &["planned", "building", "shipped", "deprecated"]
        );
    }
}
