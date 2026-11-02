//! Capability catalogs from legacy fences, checkout records and pinned Git trees.
//!
//! The registry is the WHAT layer of the knowledge model (charter §5): one
//! entry per user-meaningful capability, each a heading plus a fenced yaml
//! block. This parser is shared by the `status` view (capability table) and
//! the `validate --docs` referential-integrity lint, which needs line
//! numbers for `file:line`-style messages.

use serde::Deserialize;

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
    /// Test tags proving the capability works.
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
    id: String,
    name: String,
    area: String,
    status: String,
    verified_by: Vec<String>,
    #[serde(deserialize_with = "deserialize_string_list")]
    epics: Vec<String>,
    #[serde(deserialize_with = "deserialize_string_list")]
    epic_id: Vec<String>,
    #[serde(deserialize_with = "deserialize_string_list")]
    specs: Vec<String>,
    #[serde(deserialize_with = "deserialize_string_list")]
    depends_on: Vec<String>,
    #[serde(deserialize_with = "deserialize_string_list")]
    capabilities: Vec<String>,
    #[serde(deserialize_with = "deserialize_string_list")]
    adrs: Vec<String>,
    #[serde(deserialize_with = "deserialize_string_list")]
    related: Vec<String>,
    #[serde(deserialize_with = "deserialize_string_list")]
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

fn deserialize_string_list<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    Ok(match Option::<OneOrMany>::deserialize(deserializer)? {
        Some(OneOrMany::One(value)) => vec![value],
        Some(OneOrMany::Many(values)) => values,
        None => Vec::new(),
    })
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
    pub name: String,
    pub about: String,
}

/// The vocabulary from the same revision as the records.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CapabilityVocabulary {
    pub inventory: String,
    #[serde(default)]
    pub groups: Vec<CapabilityTerm>,
    #[serde(default)]
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

#[derive(Deserialize)]
struct RecordFields {
    #[serde(flatten)]
    entry: RawCapability,
    uid: String,
    summary: String,
    kind: String,
    audience: Vec<String>,
    stage: Option<String>,
    quality: Option<String>,
    measure: Option<String>,
    #[serde(default)]
    publish: bool,
    group: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    since: Option<String>,
    #[serde(default)]
    former_ids: Vec<String>,
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

fn bounds(file: &str, key: &str, value: &serde_yaml::Value) -> Result<(), CapabilityCatalogError> {
    use serde_yaml::Value;
    let refuse = |message: &str| catalog_error(file, format!("{key}: {message}"));
    match value {
        Value::Sequence(items) => {
            if items.len() > CAPABILITY_MAX_LIST_ITEMS {
                return Err(refuse(&format!(
                    "list exceeds {CAPABILITY_MAX_LIST_ITEMS} items"
                )));
            }
            for item in items {
                bounds(file, key, item)?;
            }
        }
        Value::String(value) if value.chars().count() > CAPABILITY_MAX_VALUE_CHARS => {
            return Err(refuse(&format!(
                "value exceeds {CAPABILITY_MAX_VALUE_CHARS} characters"
            )));
        }
        Value::Mapping(_) | Value::Tagged(_) => {
            return Err(refuse("expected a scalar or list of scalars"))
        }
        _ => {}
    }
    Ok(())
}

fn parse_record(file: &str, content: &str) -> Result<CapabilityRecord, CapabilityCatalogError> {
    let mut lines = content.split_inclusive('\n');
    if lines.next().map(str::trim_end) != Some("---") {
        return Err(catalog_error(file, "record requires YAML frontmatter"));
    }
    let mut yaml = String::new();
    let mut closed = false;
    for line in lines.by_ref() {
        if line.trim_end() == "---" {
            closed = true;
            break;
        }
        yaml.push_str(line);
    }
    if !closed {
        return Err(catalog_error(file, "unterminated YAML frontmatter"));
    }
    let value: serde_yaml::Value = serde_yaml::from_str(&yaml)
        .map_err(|error| catalog_error(file, format!("malformed YAML: {error}")))?;
    let map = value
        .as_mapping()
        .ok_or_else(|| catalog_error(file, "frontmatter must be a mapping"))?;
    if map.len() > CAPABILITY_MAX_FRONTMATTER_KEYS {
        return Err(catalog_error(
            file,
            format!("frontmatter: exceeds {CAPABILITY_MAX_FRONTMATTER_KEYS} keys"),
        ));
    }
    for (key, val) in map {
        let key = key
            .as_str()
            .ok_or_else(|| catalog_error(file, "frontmatter key must be a string"))?;
        if !RECORD_KEYS.contains(&key) {
            return Err(catalog_error(file, format!("{key}: unknown key")));
        }
        bounds(file, key, val)?;
    }
    let raw: RecordFields = serde_yaml::from_value(value)
        .map_err(|error| catalog_error(file, format!("frontmatter: {error}")))?;
    let uid = uuid::Uuid::parse_str(&raw.uid)
        .map_err(|error| catalog_error(file, format!("uid: {error}")))?
        .to_string();
    Ok(CapabilityRecord {
        entry: raw.entry.into_entry(1),
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
    for evidence in &entry.verified_by {
        let Some((kind, reference)) = evidence.split_once(':') else {
            return Err(bad("verified_by", "expected kind: reference"));
        };
        if !["test", "journey", "record", "gap"].contains(&kind) || reference.trim().is_empty() {
            return Err(bad(
                "verified_by",
                "expected test, journey, record or gap with a nonempty reference",
            ));
        }
        proof |= kind != "gap";
    }
    if matches!(record.stage.as_deref(), Some("beta" | "stable")) && !proof {
        return Err(bad(
            "verified_by",
            "beta and stable require non-gap evidence",
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
            bounds(
                VOCABULARY_PATH,
                key,
                &serde_yaml::Value::String(term.name.clone()),
            )?;
            bounds(
                VOCABULARY_PATH,
                key,
                &serde_yaml::Value::String(term.about.clone()),
            )?;
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

fn parse_catalog(
    files: &std::collections::BTreeMap<String, Vec<u8>>,
) -> Result<CapabilityCatalog, CapabilityCatalogError> {
    let mut catalog = CapabilityCatalog {
        present: files.contains_key(LEGACY_PATH),
        ..CapabilityCatalog::default()
    };
    let text = |path: &str| -> Result<Option<&str>, CapabilityCatalogError> {
        files
            .get(path)
            .map(|bytes| {
                std::str::from_utf8(bytes)
                    .map_err(|error| catalog_error(path, format!("invalid UTF-8: {error}")))
            })
            .transpose()
    };
    if let Some(content) = text(LEGACY_PATH)? {
        let (entries, issues) = parse_capabilities(content);
        if let Some(issue) = issues.first() {
            return Err(CapabilityCatalogError {
                file: LEGACY_PATH.into(),
                line: issue.line,
                message: issue.message.clone(),
            });
        }
        catalog
            .records
            .extend(entries.into_iter().map(legacy_record));
    }

    catalog.vocabulary = text(VOCABULARY_PATH)?.map(parse_vocabulary).transpose()?;
    let legacy_ids: std::collections::BTreeSet<_> =
        catalog.records.iter().map(|r| r.entry.id.clone()).collect();
    for file in files
        .keys()
        .filter(|p| p.starts_with("docs/capabilities/") && p.as_str() != VOCABULARY_PATH)
    {
        validate_capability_path(file)?;
        let content = text(file)?.unwrap_or_default();
        // Existing graduated pages are companion prose until migration adds frontmatter.
        let name = file.trim_start_matches("docs/capabilities/");
        let id = name.split('-').take(2).collect::<Vec<_>>().join("-");
        if content.lines().next().map(str::trim_end) != Some("---") && legacy_ids.contains(&id) {
            continue;
        }
        catalog.records.push(parse_record(file, content)?);
        catalog.present = true;
    }
    let modern = catalog.records.iter().any(|r| r.uid.is_some());
    let areas = if modern {
        let content = text(PROJECT_PATH)?.ok_or_else(|| {
            catalog_error(
                PROJECT_PATH,
                "areas: project configuration is required for catalog records",
            )
        })?;
        toml::from_str::<AreaConfig>(content)
            .map_err(|error| catalog_error(PROJECT_PATH, format!("areas: {error}")))?
            .areas
    } else {
        vec![]
    };
    let mut ids = std::collections::BTreeMap::new();
    let mut uids = std::collections::BTreeMap::new();
    for record in &catalog.records {
        let file = record.file.to_string_lossy();
        // Check collisions before schema differences; an overlap is always refused.
        for (key, value, seen) in [
            ("id", Some(&record.entry.id), &mut ids),
            ("uid", record.uid.as_ref(), &mut uids),
        ] {
            if let Some(value) = value {
                if let Some(previous) = seen.insert(value.clone(), file.to_string()) {
                    return Err(catalog_error(
                        &file,
                        format!("{key}: duplicate {value}; also in {previous}"),
                    ));
                }
            }
        }
    }
    for record in &catalog.records {
        validate_record(record, &areas, catalog.vocabulary.as_ref())?;
    }
    catalog.records.sort_by(|a, b| a.entry.id.cmp(&b.entry.id));
    Ok(catalog)
}

fn selected_file(path: &str) -> bool {
    path == LEGACY_PATH
        || path == PROJECT_PATH
        || path == VOCABULARY_PATH
        || path
            .strip_prefix("docs/capabilities/")
            .is_some_and(|name| name.starts_with("CAP-") || name.contains("/CAP-"))
}

fn check_mode(path: &str, mode: &str) -> Result<(), CapabilityCatalogError> {
    if mode != "100644" {
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
) -> Result<(), CapabilityCatalogError> {
    let entries = match tree.entries(directory, usize::MAX) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(catalog_error(directory, error.to_string())),
    };
    for name in entries {
        let name = name
            .into_string()
            .map_err(|_| catalog_error(directory, "non-UTF-8 catalog filename"))?;
        let path = format!("{directory}/{name}");
        if selected_file(&path) {
            if path != VOCABULARY_PATH {
                validate_capability_path(&path)?;
            }
            paths.push(path);
        } else if tree.metadata(&path).is_ok_and(|meta| meta.is_dir()) {
            collect_catalog_paths(tree, &path, paths)?;
        }
    }
    Ok(())
}

/// Read and validate a checkout through repository-confined file handles.
/// References inside records are syntax-checked data, never opened or executed.
///
/// # Errors
/// Returns a source-addressed refusal for unreadable or invalid input.
pub fn load_capability_catalog(
    root: &std::path::Path,
) -> Result<CapabilityCatalog, CapabilityCatalogError> {
    use std::io::Read;
    let tree = crate::contained::Tree::open(root)
        .map_err(|error| catalog_error("<checkout>", error.to_string()))?;
    let mut files = std::collections::BTreeMap::new();
    let mut paths = vec![LEGACY_PATH.to_string(), PROJECT_PATH.to_string()];
    collect_catalog_paths(&tree, CATALOG_DIR, &mut paths)?;
    for path in paths {
        let mut file = match tree.open_regular(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(catalog_error(&path, error.to_string())),
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = file
                .metadata()
                .map_err(|error| catalog_error(&path, error.to_string()))?;
            if meta.permissions().mode() & 0o111 != 0 {
                return Err(catalog_error(&path, "executable mode is refused"));
            }
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|error| catalog_error(&path, error.to_string()))?;
        files.insert(path, bytes);
    }
    parse_catalog(&files)
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
    let entries = git
        .tree(tree.trim(), &["docs", ".codeflow"])
        .map_err(|error| catalog_error("<tree>", error.to_string()))?;
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
