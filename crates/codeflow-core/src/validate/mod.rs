//! YAML frontmatter validation for task and epic markdown files.
//!
//! Validates the machine-readable record half of v2's markdown truth
//! (charter D17): required fields, enum values, format-id shape (shared
//! with `workgraph::format_id`), template sentinels, and required body
//! sections (configurable so they track the shipped templates).
//!
//! v2 trim from v1: autorun cross-field checks, scope-policy/file-scope
//! claims, pathflow stage fields, auto-merge/raise-pr flags, area types,
//! and PII file-scope warnings died with their subsystems (charter §3.2,
//! D22).

use std::collections::HashMap;
use std::path::Path;

use thiserror::Error;

use crate::workgraph::{is_valid_epic_format_id, is_valid_task_format_id};

/// Errors from validation I/O and parsing.
#[derive(Debug, Error)]
pub enum ValidateError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid frontmatter: {0}")]
    InvalidFrontmatter(String),

    #[error("yaml error: {0}")]
    Yaml(String),
}

/// A blocking validation error found in a markdown file.
#[derive(Debug, Clone)]
pub struct ValidationError {
    pub field: String,
    pub message: String,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.field.is_empty() {
            write!(f, "{}", self.message)
        } else {
            write!(f, "{}: {}", self.field, self.message)
        }
    }
}

/// A non-blocking validation warning.
#[derive(Debug, Clone)]
pub struct ValidationWarning {
    pub field: String,
    pub message: String,
}

impl std::fmt::Display for ValidationWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.field.is_empty() {
            write!(f, "{}", self.message)
        } else {
            write!(f, "{}: {}", self.field, self.message)
        }
    }
}

/// Validation options.
///
/// Required body sections are configurable so validation tracks the
/// scaffold's templates rather than hardcoding ceremony.
#[derive(Debug, Clone)]
pub struct ValidateOptions {
    pub task_required_sections: Vec<String>,
    pub epic_required_sections: Vec<String>,
}

impl Default for ValidateOptions {
    fn default() -> Self {
        Self {
            task_required_sections: vec![
                "## Description".into(),
                "## Acceptance Criteria".into(),
            ],
            epic_required_sections: vec![
                "## Summary".into(),
                "## Acceptance Criteria".into(),
            ],
        }
    }
}

// ---------------------------------------------------------------------------
// Frontmatter parsing
// ---------------------------------------------------------------------------

/// Parse YAML frontmatter from markdown content.
///
/// Returns the parsed YAML as a map and the remaining body content.
/// The frontmatter must be enclosed between two `---` delimiter lines at
/// the start of the content.
///
/// # Errors
///
/// Returns `ValidateError::InvalidFrontmatter` if delimiters are missing
/// or the YAML is invalid.
pub fn parse_frontmatter(
    content: &[u8],
) -> Result<(HashMap<String, serde_yaml::Value>, Vec<u8>), ValidateError> {
    let s = String::from_utf8_lossy(content);

    // Strip BOM if present.
    let s = s.strip_prefix('\u{FEFF}').unwrap_or(&s);

    if !s.starts_with("---") {
        return Err(ValidateError::InvalidFrontmatter(
            "missing opening delimiter".into(),
        ));
    }

    let after_opener = match s.find('\n') {
        Some(idx) => &s[idx + 1..],
        None => {
            return Err(ValidateError::InvalidFrontmatter(
                "no content after opening delimiter".into(),
            ));
        }
    };

    let closing = find_closing_delim(after_opener)
        .ok_or_else(|| ValidateError::InvalidFrontmatter("missing closing delimiter".into()))?;

    let yaml_str = &after_opener[..closing];

    let body_start = &after_opener[closing..];
    let body = match body_start.find('\n') {
        Some(idx) => body_start.as_bytes()[idx + 1..].to_vec(),
        None => Vec::new(),
    };

    let data: HashMap<String, serde_yaml::Value> =
        serde_yaml::from_str(yaml_str).map_err(|e| ValidateError::Yaml(e.to_string()))?;

    if data.is_empty() {
        return Err(ValidateError::InvalidFrontmatter(
            "empty YAML content".into(),
        ));
    }

    Ok((data, body))
}

fn find_closing_delim(s: &str) -> Option<usize> {
    let mut pos = 0;
    while pos < s.len() {
        let rest = &s[pos..];
        let line_end = rest.find('\n');
        let line = match line_end {
            Some(idx) => &rest[..idx],
            None => rest,
        };
        let trimmed = line.trim_end_matches([' ', '\t', '\r']);
        if trimmed == "---" {
            return Some(pos);
        }
        match line_end {
            Some(idx) => pos += idx + 1,
            None => break,
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Work intent labels — the v2 branch-prefix set (charter §6.4).
const WORK_TYPE_VALUES: &[&str] = &[
    "feat",
    "fix",
    "docs",
    "refactor",
    "test",
    "chore",
    "ci",
    "hotfix",
    "plan",
    "spike",
    "experiment",
];

const PRIORITY_VALUES: &[&str] = &["low", "normal", "high", "critical"];

const ESTIMATE_VALUES: &[&str] = &["XS", "S", "M", "L", "XL"];

pub(crate) fn get_string_field(data: &HashMap<String, serde_yaml::Value>, key: &str) -> String {
    match data.get(key) {
        Some(serde_yaml::Value::String(s)) => {
            if s == "null" || s == "~" {
                String::new()
            } else {
                s.clone()
            }
        }
        Some(serde_yaml::Value::Bool(b)) => b.to_string(),
        Some(serde_yaml::Value::Number(n)) => n.to_string(),
        Some(serde_yaml::Value::Null) | None => String::new(),
        Some(v) => format!("{v:?}"),
    }
}

pub(crate) fn is_field_empty(data: &HashMap<String, serde_yaml::Value>, key: &str) -> bool {
    match data.get(key) {
        None | Some(serde_yaml::Value::Null) => true,
        Some(serde_yaml::Value::String(s)) => s.is_empty() || s == "null" || s == "~",
        Some(serde_yaml::Value::Sequence(seq)) => seq.is_empty(),
        _ => false,
    }
}

fn validate_required_fields(
    data: &HashMap<String, serde_yaml::Value>,
    required: &[&str],
) -> Vec<ValidationError> {
    required
        .iter()
        .filter(|&&field| is_field_empty(data, field))
        .map(|&field| ValidationError {
            field: field.into(),
            message: "required field is missing or empty".into(),
        })
        .collect()
}

fn validate_enum(
    data: &HashMap<String, serde_yaml::Value>,
    field: &str,
    allowed: &[&str],
) -> Vec<ValidationError> {
    let val = get_string_field(data, field);
    if val.is_empty() {
        return Vec::new();
    }
    if allowed.contains(&val.as_str()) {
        return Vec::new();
    }
    vec![ValidationError {
        field: field.into(),
        message: format!(
            "invalid value \"{val}\", must be one of: {}",
            allowed.join(", ")
        ),
    }]
}

fn validate_optional_enum(
    data: &HashMap<String, serde_yaml::Value>,
    field: &str,
    allowed: &[&str],
) -> Vec<ValidationError> {
    if is_field_empty(data, field) {
        return Vec::new();
    }
    validate_enum(data, field, allowed)
}

fn validate_array_fields(
    data: &HashMap<String, serde_yaml::Value>,
    fields: &[&str],
) -> Vec<ValidationError> {
    let mut errs = Vec::new();
    for &field in fields {
        match data.get(field) {
            None | Some(serde_yaml::Value::Null | serde_yaml::Value::Sequence(_)) => {}
            Some(v) => {
                errs.push(ValidationError {
                    field: field.into(),
                    message: format!("must be an array, got {v:?}"),
                });
            }
        }
    }
    errs
}

fn check_template_sentinels(
    data: &HashMap<String, serde_yaml::Value>,
    fields: &[&str],
) -> Vec<ValidationError> {
    let mut errs = Vec::new();
    for &field in fields {
        let val = get_string_field(data, field);
        if val.is_empty() {
            continue;
        }
        if val.contains('{') || val.contains('}') {
            errs.push(ValidationError {
                field: field.into(),
                message: "contains unfilled template sentinel".into(),
            });
        }
        if val.to_uppercase().contains("PLACEHOLDER") {
            errs.push(ValidationError {
                field: field.into(),
                message: "contains placeholder value".into(),
            });
        }
    }
    errs
}

fn validate_sections(body: &[u8], required: &[String]) -> Vec<ValidationError> {
    let body_str = String::from_utf8_lossy(body);
    let mut errs = Vec::new();
    for section in required {
        let has_section =
            body_str.contains(&format!("\n{section}")) || body_str.starts_with(section.as_str());
        if !has_section {
            errs.push(ValidationError {
                field: "body".into(),
                message: format!("missing required section \"{section}\""),
            });
        }
    }
    errs
}

fn check_filename_match(
    path: &Path,
    data: &HashMap<String, serde_yaml::Value>,
) -> Vec<ValidationWarning> {
    let fid = get_string_field(data, "format_id");
    if fid.is_empty() {
        return Vec::new();
    }
    let base = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    if base != fid {
        return vec![ValidationWarning {
            field: "format_id".into(),
            message: format!(
                "filename \"{}\" does not match format_id \"{fid}\"",
                path.file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default()
            ),
        }];
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// Task validation
// ---------------------------------------------------------------------------

const TASK_REQUIRED_FIELDS: &[&str] = &["id", "format_id", "epic_id", "title", "status", "work_type"];

const TASK_STATUS_VALUES: &[&str] = &["todo", "blocked", "in_progress", "complete", "cancelled"];

const TASK_ARRAY_FIELDS: &[&str] = &["acceptance", "tests"];

const TASK_TEMPLATE_SENTINEL_FIELDS: &[&str] = &["id", "title", "epic_id"];

/// Validate a task markdown file at the given path.
///
/// Returns blocking validation errors and non-blocking warnings.
///
/// # Errors
///
/// Returns `ValidateError` on I/O or frontmatter parsing failures.
pub fn validate_task(
    path: &Path,
    opts: &ValidateOptions,
) -> Result<(Vec<ValidationError>, Vec<ValidationWarning>), ValidateError> {
    let content = std::fs::read(path)?;
    let (data, body) = parse_frontmatter(&content)?;

    let mut errs = Vec::new();
    let mut warns = Vec::new();

    // Required fields.
    errs.extend(validate_required_fields(&data, TASK_REQUIRED_FIELDS));

    // Template sentinels.
    errs.extend(check_template_sentinels(
        &data,
        TASK_TEMPLATE_SENTINEL_FIELDS,
    ));

    // Format id shape (shared with workgraph::format_id).
    let fid = get_string_field(&data, "format_id");
    if !fid.is_empty() && !is_valid_task_format_id(&fid) {
        errs.push(ValidationError {
            field: "format_id".into(),
            message: "does not match pattern TSK-NNN-NNN".into(),
        });
    }

    // Enum validations.
    errs.extend(validate_enum(&data, "status", TASK_STATUS_VALUES));
    errs.extend(validate_enum(&data, "work_type", WORK_TYPE_VALUES));
    errs.extend(validate_optional_enum(&data, "priority", PRIORITY_VALUES));
    errs.extend(validate_optional_enum(&data, "estimate", ESTIMATE_VALUES));

    // Array fields.
    errs.extend(validate_array_fields(&data, TASK_ARRAY_FIELDS));

    // Section checks.
    errs.extend(validate_sections(&body, &opts.task_required_sections));

    // Warnings: filename vs format_id.
    warns.extend(check_filename_match(path, &data));

    Ok((errs, warns))
}

// ---------------------------------------------------------------------------
// Epic validation
// ---------------------------------------------------------------------------

const EPIC_REQUIRED_FIELDS: &[&str] = &["id", "format_id", "title", "status", "work_type"];

const EPIC_STATUS_VALUES: &[&str] = &[
    "draft",
    "planning",
    "in_progress",
    "blocked",
    "complete",
    "archived",
];

const EPIC_TEMPLATE_SENTINEL_FIELDS: &[&str] = &["id", "title"];

/// Validate an epic markdown file at the given path.
///
/// Returns blocking validation errors and non-blocking warnings.
///
/// # Errors
///
/// Returns `ValidateError` on I/O or frontmatter parsing failures.
pub fn validate_epic(
    path: &Path,
    opts: &ValidateOptions,
) -> Result<(Vec<ValidationError>, Vec<ValidationWarning>), ValidateError> {
    let content = std::fs::read(path)?;
    let (data, body) = parse_frontmatter(&content)?;

    let mut errs = Vec::new();
    let mut warns = Vec::new();

    // Required fields.
    errs.extend(validate_required_fields(&data, EPIC_REQUIRED_FIELDS));

    // Template sentinels.
    errs.extend(check_template_sentinels(
        &data,
        EPIC_TEMPLATE_SENTINEL_FIELDS,
    ));

    // Format id shape (shared with workgraph::format_id).
    let fid = get_string_field(&data, "format_id");
    if !fid.is_empty() && !is_valid_epic_format_id(&fid) {
        errs.push(ValidationError {
            field: "format_id".into(),
            message: "does not match pattern EPC-NNN".into(),
        });
    }

    // Enum validations.
    errs.extend(validate_enum(&data, "status", EPIC_STATUS_VALUES));
    errs.extend(validate_enum(&data, "work_type", WORK_TYPE_VALUES));
    errs.extend(validate_optional_enum(&data, "priority", PRIORITY_VALUES));

    // Section checks.
    errs.extend(validate_sections(&body, &opts.epic_required_sections));

    // Warnings: filename vs format_id.
    warns.extend(check_filename_match(path, &data));

    Ok((errs, warns))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::models::{EpicStatus, TaskStatus};

    use super::*;

    fn valid_task_content() -> String {
        r#"---
id: "task-01abc"
format_id: "TSK-022-014"
epic_id: "epic-01xyz"
title: "Test task"
status: "in_progress"
work_type: "feat"
priority: "high"
estimate: "M"
acceptance:
  - "All tests pass"
tests:
  - "crates/codeflow-core/src/"
---
## Description
Some description

## Acceptance Criteria
Some criteria
"#
        .to_string()
    }

    fn valid_epic_content() -> String {
        r#"---
id: "epic-01xyz"
format_id: "EPC-022"
title: "Test epic"
status: "in_progress"
work_type: "feat"
priority: "high"
---
## Summary
Summary

## Acceptance Criteria
Criteria
"#
        .to_string()
    }

    // -- parse_frontmatter --

    #[test]
    fn test_parse_frontmatter_valid() {
        let content = b"---\ntitle: hello\nstatus: draft\n---\nBody here";
        let (data, body) = parse_frontmatter(content).unwrap();
        assert_eq!(get_string_field(&data, "title"), "hello");
        assert!(String::from_utf8_lossy(&body).contains("Body here"));
    }

    #[test]
    fn test_parse_frontmatter_missing_opener() {
        let content = b"No frontmatter here";
        assert!(parse_frontmatter(content).is_err());
    }

    #[test]
    fn test_parse_frontmatter_missing_closer() {
        let content = b"---\ntitle: hello\nNo closing delimiter";
        assert!(parse_frontmatter(content).is_err());
    }

    #[test]
    fn test_parse_frontmatter_empty_yaml() {
        let content = b"---\n---\nBody";
        assert!(parse_frontmatter(content).is_err());
    }

    #[test]
    fn test_parse_frontmatter_with_bom() {
        let mut content = Vec::from(b"\xEF\xBB\xBF" as &[u8]);
        content.extend_from_slice(b"---\ntitle: hello\n---\nBody");
        let (data, _) = parse_frontmatter(&content).unwrap();
        assert_eq!(get_string_field(&data, "title"), "hello");
    }

    // -- task validation --

    #[test]
    fn test_validate_task_valid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("TSK-022-014.md");
        std::fs::write(&path, valid_task_content()).unwrap();

        let opts = ValidateOptions::default();
        let (errs, warns) = validate_task(&path, &opts).unwrap();
        assert!(errs.is_empty(), "Expected no errors, got: {errs:?}");
        assert!(warns.is_empty(), "Expected no warnings, got: {warns:?}");
    }

    #[test]
    fn test_validate_task_missing_required_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.md");
        std::fs::write(&path, "---\ntitle: hello\n---\nBody\n").unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_task(&path, &opts).unwrap();
        // Should flag missing: id, format_id, epic_id, status, work_type.
        let missing: Vec<_> = errs
            .iter()
            .filter(|e| e.message.contains("required field"))
            .collect();
        assert_eq!(missing.len(), 5, "Expected 5 missing-field errors: {errs:?}");
    }

    #[test]
    fn test_validate_task_invalid_status() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("TSK-022-014.md");
        let content =
            valid_task_content().replace("status: \"in_progress\"", "status: \"INVALID\"");
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_task(&path, &opts).unwrap();
        assert!(errs.iter().any(|e| e.field == "status"));
    }

    #[test]
    fn test_validate_task_v1_work_type_rejected() {
        // v1 used uppercase work types (FEAT); v2 uses the branch-prefix set.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("TSK-022-014.md");
        let content = valid_task_content().replace("work_type: \"feat\"", "work_type: \"FEAT\"");
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_task(&path, &opts).unwrap();
        assert!(errs.iter().any(|e| e.field == "work_type"));
    }

    #[test]
    fn test_validate_task_v1_format_id_rejected() {
        // v1 area-prefixed ids (INF-TSK-022-014) are not valid v2 ids.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("INF-TSK-022-014.md");
        let content = valid_task_content().replace(
            "format_id: \"TSK-022-014\"",
            "format_id: \"INF-TSK-022-014\"",
        );
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_task(&path, &opts).unwrap();
        assert!(errs.iter().any(|e| e.field == "format_id"));
    }

    #[test]
    fn test_validate_task_invalid_estimate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("TSK-022-014.md");
        let content = valid_task_content().replace("estimate: \"M\"", "estimate: \"HUGE\"");
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_task(&path, &opts).unwrap();
        assert!(errs.iter().any(|e| e.field == "estimate"));
    }

    #[test]
    fn test_validate_task_acceptance_must_be_array() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("TSK-022-014.md");
        let content = valid_task_content().replace(
            "acceptance:\n  - \"All tests pass\"",
            "acceptance: 42",
        );
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_task(&path, &opts).unwrap();
        assert!(errs.iter().any(|e| e.field == "acceptance"));
    }

    #[test]
    fn test_validate_task_template_sentinel() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.md");
        let content =
            valid_task_content().replace("title: \"Test task\"", "title: \"{PLACEHOLDER}\"");
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_task(&path, &opts).unwrap();
        assert!(errs.iter().any(
            |e| e.message.contains("template sentinel") || e.message.contains("placeholder")
        ));
    }

    #[test]
    fn test_validate_task_missing_sections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("TSK-022-014.md");
        let content = valid_task_content().replace("## Acceptance Criteria", "## Something Else");
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_task(&path, &opts).unwrap();
        let section_errs: Vec<_> = errs.iter().filter(|e| e.field == "body").collect();
        assert_eq!(section_errs.len(), 1, "Expected one section error: {errs:?}");
        assert!(section_errs[0].message.contains("Acceptance Criteria"));
    }

    #[test]
    fn test_validate_task_custom_sections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("TSK-022-014.md");
        std::fs::write(&path, valid_task_content()).unwrap();

        let opts = ValidateOptions {
            task_required_sections: vec!["## Verification".into()],
            ..Default::default()
        };
        let (errs, _) = validate_task(&path, &opts).unwrap();
        assert!(errs.iter().any(|e| e.message.contains("Verification")));
    }

    #[test]
    fn test_validate_task_filename_mismatch_warns() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wrong-name.md");
        std::fs::write(&path, valid_task_content()).unwrap();

        let opts = ValidateOptions::default();
        let (_, warns) = validate_task(&path, &opts).unwrap();
        assert!(warns.iter().any(|w| w.field == "format_id"));
    }

    // -- epic validation --

    #[test]
    fn test_validate_epic_valid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("EPC-022.md");
        std::fs::write(&path, valid_epic_content()).unwrap();

        let opts = ValidateOptions::default();
        let (errs, warns) = validate_epic(&path, &opts).unwrap();
        assert!(errs.is_empty(), "Expected no errors, got: {errs:?}");
        assert!(warns.is_empty(), "Expected no warnings, got: {warns:?}");
    }

    #[test]
    fn test_validate_epic_missing_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.md");
        std::fs::write(&path, "---\ntitle: hello\n---\nBody\n").unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_epic(&path, &opts).unwrap();
        // Should flag missing: id, format_id, status, work_type.
        let missing: Vec<_> = errs
            .iter()
            .filter(|e| e.message.contains("required field"))
            .collect();
        assert_eq!(missing.len(), 4, "Expected 4 missing-field errors: {errs:?}");
    }

    #[test]
    fn test_validate_epic_invalid_format_id() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.md");
        let content =
            valid_epic_content().replace("format_id: \"EPC-022\"", "format_id: \"invalid\"");
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_epic(&path, &opts).unwrap();
        assert!(errs.iter().any(|e| e.field == "format_id"));
    }

    #[test]
    fn test_validate_epic_v1_format_id_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("INF-EPC-022.md");
        let content =
            valid_epic_content().replace("format_id: \"EPC-022\"", "format_id: \"INF-EPC-022\"");
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_epic(&path, &opts).unwrap();
        assert!(errs.iter().any(|e| e.field == "format_id"));
    }

    #[test]
    fn test_validate_epic_invalid_status() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.md");
        let content = valid_epic_content().replace("status: \"in_progress\"", "status: \"BAD\"");
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_epic(&path, &opts).unwrap();
        assert!(errs.iter().any(|e| e.field == "status"));
    }

    #[test]
    fn test_validate_epic_missing_sections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("EPC-022.md");
        let content = valid_epic_content().replace("## Summary", "## Intro");
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_epic(&path, &opts).unwrap();
        assert!(errs.iter().any(|e| e.message.contains("Summary")));
    }

    // -- helpers --

    #[test]
    fn test_filename_match_warning() {
        let mut data = HashMap::new();
        data.insert(
            "format_id".to_string(),
            serde_yaml::Value::String("TSK-001-001".into()),
        );
        let warns = check_filename_match(Path::new("wrong-name.md"), &data);
        assert!(!warns.is_empty());
    }

    #[test]
    fn test_get_string_field_null() {
        let mut data = HashMap::new();
        data.insert("key".to_string(), serde_yaml::Value::String("null".into()));
        assert!(get_string_field(&data, "key").is_empty());
    }

    #[test]
    fn test_get_string_field_tilde() {
        let mut data = HashMap::new();
        data.insert("key".to_string(), serde_yaml::Value::String("~".into()));
        assert!(get_string_field(&data, "key").is_empty());
    }

    #[test]
    fn test_is_field_empty_variants() {
        let mut data = HashMap::new();
        data.insert("null_field".to_string(), serde_yaml::Value::Null);
        data.insert(
            "empty_seq".to_string(),
            serde_yaml::Value::Sequence(vec![]),
        );
        data.insert(
            "filled".to_string(),
            serde_yaml::Value::String("x".into()),
        );
        assert!(is_field_empty(&data, "null_field"));
        assert!(is_field_empty(&data, "empty_seq"));
        assert!(is_field_empty(&data, "absent"));
        assert!(!is_field_empty(&data, "filled"));
    }

    // -- enum consts stay in lockstep with the model enums --

    #[test]
    fn test_task_status_values_match_models() {
        for s in TASK_STATUS_VALUES {
            assert!(
                TaskStatus::from_str(s).is_ok(),
                "validate const '{s}' must parse as models::TaskStatus"
            );
        }
    }

    #[test]
    fn test_epic_status_values_match_models() {
        for s in EPIC_STATUS_VALUES {
            assert!(
                EpicStatus::from_str(s).is_ok(),
                "validate const '{s}' must parse as models::EpicStatus"
            );
        }
    }
}
