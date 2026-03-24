//! YAML frontmatter validation for task and epic markdown files.
//!
//! Ports Go's `internal/validate` package: `ParseFrontmatter`,
//! `ValidateTask` (12+ checks), and `ValidateEpic` (7+ checks).

use std::collections::HashMap;
use std::path::Path;

use crate::error::ValidateError;

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
#[derive(Debug, Clone)]
pub struct ValidateOptions {
    pub protected_branches: Vec<String>,
}

impl Default for ValidateOptions {
    fn default() -> Self {
        Self {
            protected_branches: vec![
                "main".into(),
                "master".into(),
                "release/*".into(),
                "production".into(),
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

const WORK_TYPE_VALUES: &[&str] = &[
    "FEAT", "FIX", "HTFX", "RFCT", "DOCS", "TEST", "CHOR", "CICD", "SPKE", "PLAN",
];

const AREA_TYPE_VALUES: &[&str] = &["INF", "PLN", "DOC", "TST", "SEC", "FRM", "PRJ", "AUT"];

const PRIORITY_VALUES: &[&str] = &["low", "normal", "high", "critical"];

const PII_PATTERNS: &[&str] = &[
    "auth",
    "login",
    "user",
    "session",
    "password",
    "credential",
    "token",
    "account",
    "profile",
    "identity",
];

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

pub(crate) fn get_bool_field(data: &HashMap<String, serde_yaml::Value>, key: &str) -> (bool, bool) {
    match data.get(key) {
        Some(serde_yaml::Value::Bool(b)) => (*b, true),
        Some(serde_yaml::Value::String(s)) => match s.as_str() {
            "true" => (true, true),
            "false" => (false, true),
            _ => (false, false),
        },
        _ => (false, false),
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

fn validate_boolean_fields(
    data: &HashMap<String, serde_yaml::Value>,
    fields: &[&str],
) -> Vec<ValidationError> {
    let mut errs = Vec::new();
    for &field in fields {
        match data.get(field) {
            None | Some(serde_yaml::Value::Null | serde_yaml::Value::Bool(_)) => {}
            Some(serde_yaml::Value::String(s)) if s == "true" || s == "false" => {}
            Some(v) => {
                errs.push(ValidationError {
                    field: field.into(),
                    message: format!("must be true or false, got {v:?}"),
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

fn validate_format_id_prefix(
    data: &HashMap<String, serde_yaml::Value>,
    field: &str,
    separator: &str,
) -> Vec<ValidationError> {
    let fid = get_string_field(data, field);
    let area_type = get_string_field(data, "area_type");
    if fid.is_empty() || area_type.is_empty() {
        return Vec::new();
    }
    if let Some(idx) = fid.find(separator) {
        let prefix = &fid[..idx];
        if prefix != area_type {
            return vec![ValidationError {
                field: field.into(),
                message: format!("prefix \"{prefix}\" does not match area_type \"{area_type}\""),
            }];
        }
    }
    Vec::new()
}

fn validate_sections(body: &[u8], required: &[&str]) -> Vec<ValidationError> {
    let body_str = String::from_utf8_lossy(body);
    let mut errs = Vec::new();
    for &section in required {
        let has_section =
            body_str.contains(&format!("\n{section}")) || body_str.starts_with(section);
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

fn check_pii_patterns(data: &HashMap<String, serde_yaml::Value>) -> Vec<ValidationWarning> {
    let scope = match data.get("file_scope") {
        None | Some(serde_yaml::Value::Null) => return Vec::new(),
        Some(serde_yaml::Value::Sequence(seq)) => {
            if seq.is_empty() {
                return Vec::new();
            }
            seq.iter()
                .map(|v| format!("{v:?}").to_lowercase())
                .collect::<Vec<_>>()
                .join(" ")
        }
        Some(serde_yaml::Value::String(s)) => {
            if s.is_empty() || s == "[]" || s == "null" || s == "~" {
                return Vec::new();
            }
            s.to_lowercase()
        }
        Some(v) => format!("{v:?}").to_lowercase(),
    };

    for &pattern in PII_PATTERNS {
        if scope.contains(pattern) {
            return vec![ValidationWarning {
                field: "file_scope".into(),
                message: format!(
                    "contains PII-sensitive pattern \"{pattern}\" — verify PII handling compliance"
                ),
            }];
        }
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// Task validation
// ---------------------------------------------------------------------------

const TASK_REQUIRED_FIELDS: &[&str] = &[
    "id",
    "format_id",
    "epic_id",
    "title",
    "status",
    "area_type",
    "work_type",
];

const TASK_STATUS_VALUES: &[&str] = &["todo", "blocked", "in_progress", "complete", "cancelled"];
const ORIGIN_VALUES: &[&str] = &["planned", "informal", "auto"];
const SCOPE_POLICY_VALUES: &[&str] = &["soft", "hard", "permissive"];
const ESTIMATE_VALUES: &[&str] = &["XS", "S", "M", "L", "XL"];
const STAGE_VALUES: &[&str] = &["dev", "plan", "docs", "test", "review", "qa", "done"];
const STAGE_STATUS_VALUES: &[&str] = &["pending", "in_progress", "complete", "failed"];

const TASK_BOOLEAN_FIELDS: &[&str] = &["autorun_eligible", "raise_pr", "auto_merge"];

const TASK_FORMAT_ID_PATTERN: &str = r"^[A-Z]{2,4}-TSK-[0-9]{3}-[0-9]{3}$";

const TASK_REQUIRED_SECTIONS: &[&str] = &[
    "## Description",
    "## Approach",
    "## Files",
    "## Acceptance Criteria",
    "## Dependencies",
    "## Verification",
    "## Stage Reports",
    "## Notes",
];

const TASK_TEMPLATE_SENTINEL_FIELDS: &[&str] = &["id", "title", "epic_id"];

fn code_work_type(wt: &str) -> bool {
    matches!(
        wt,
        "FEAT" | "FIX" | "RFCT" | "HTFX" | "CHOR" | "CICD" | "TEST"
    )
}

/// Validate a task markdown file at the given path.
///
/// Returns validation errors, warnings, and any I/O or parsing error.
///
/// # Panics
///
/// Panics if the format ID regex fails to compile (hardcoded pattern).
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

    // Format ID pattern.
    let fid = get_string_field(&data, "format_id");
    if !fid.is_empty() {
        let re = regex::Regex::new(TASK_FORMAT_ID_PATTERN).expect("valid regex");
        if !re.is_match(&fid) {
            errs.push(ValidationError {
                field: "format_id".into(),
                message: format!("does not match pattern {TASK_FORMAT_ID_PATTERN}"),
            });
        }
    }

    // Enum validations.
    errs.extend(validate_enum(&data, "status", TASK_STATUS_VALUES));
    errs.extend(validate_enum(&data, "work_type", WORK_TYPE_VALUES));
    errs.extend(validate_enum(&data, "area_type", AREA_TYPE_VALUES));
    errs.extend(validate_optional_enum(&data, "origin", ORIGIN_VALUES));
    errs.extend(validate_optional_enum(
        &data,
        "scope_policy",
        SCOPE_POLICY_VALUES,
    ));
    errs.extend(validate_optional_enum(&data, "priority", PRIORITY_VALUES));
    errs.extend(validate_optional_enum(&data, "estimate", ESTIMATE_VALUES));
    errs.extend(validate_optional_enum(&data, "stage", STAGE_VALUES));
    errs.extend(validate_optional_enum(
        &data,
        "stage_status",
        STAGE_STATUS_VALUES,
    ));

    // Boolean fields.
    errs.extend(validate_boolean_fields(&data, TASK_BOOLEAN_FIELDS));

    // Cross-field: format_id prefix must match area_type.
    errs.extend(validate_format_id_prefix(&data, "format_id", "-TSK-"));

    // Cross-field: auto_merge validations.
    errs.extend(validate_auto_merge(&data, &opts.protected_branches));

    // Cross-field: raise_pr false + auto_merge true.
    errs.extend(validate_raise_pr_auto_merge(&data));

    // Cross-field: autorun_eligible requires acceptance.
    errs.extend(validate_autorun_acceptance(&data));

    // Cross-field: autorun_eligible requires non-empty file_scope.
    errs.extend(validate_autorun_file_scope(&data));

    // Cross-field: autorun_eligible + permissive scope_policy is invalid.
    errs.extend(validate_autorun_scope_policy(&data));

    // Cross-field: code work type with .sh/.py in file_scope requires tests.
    errs.extend(validate_code_task_tests(&data));

    // Section checks.
    errs.extend(validate_sections(&body, TASK_REQUIRED_SECTIONS));

    // Warnings: filename vs format_id.
    warns.extend(check_filename_match(path, &data));

    // Warnings: PII patterns in file_scope.
    warns.extend(check_pii_patterns(&data));

    Ok((errs, warns))
}

fn validate_auto_merge(
    data: &HashMap<String, serde_yaml::Value>,
    protected_branches: &[String],
) -> Vec<ValidationError> {
    let (auto_merge, set) = get_bool_field(data, "auto_merge");
    if !set || !auto_merge {
        return Vec::new();
    }

    let target = get_string_field(data, "target_branch");
    if target.is_empty() {
        return vec![ValidationError {
            field: "auto_merge".into(),
            message: "auto_merge is true but target_branch is not set".into(),
        }];
    }

    for pb in protected_branches {
        if target == *pb {
            return vec![ValidationError {
                field: "auto_merge".into(),
                message: format!(
                    "auto_merge is true but target_branch \"{target}\" is a protected branch"
                ),
            }];
        }
    }
    Vec::new()
}

fn validate_raise_pr_auto_merge(data: &HashMap<String, serde_yaml::Value>) -> Vec<ValidationError> {
    let (raise_pr, raise_pr_set) = get_bool_field(data, "raise_pr");
    let (auto_merge, auto_merge_set) = get_bool_field(data, "auto_merge");

    if raise_pr_set && !raise_pr && auto_merge_set && auto_merge {
        return vec![ValidationError {
            field: "auto_merge".into(),
            message: "auto_merge requires raise_pr to be true".into(),
        }];
    }
    Vec::new()
}

pub(crate) fn validate_autorun_acceptance(
    data: &HashMap<String, serde_yaml::Value>,
) -> Vec<ValidationError> {
    let (autorun, set) = get_bool_field(data, "autorun_eligible");
    if !set || !autorun {
        return Vec::new();
    }
    if is_field_empty(data, "acceptance") {
        return vec![ValidationError {
            field: "autorun_eligible".into(),
            message: "autorun_eligible is true but acceptance is empty".into(),
        }];
    }
    Vec::new()
}

pub(crate) fn validate_autorun_file_scope(
    data: &HashMap<String, serde_yaml::Value>,
) -> Vec<ValidationError> {
    let (autorun, set) = get_bool_field(data, "autorun_eligible");
    if !set || !autorun {
        return Vec::new();
    }
    if is_field_empty(data, "file_scope") {
        return vec![ValidationError {
            field: "autorun_eligible".into(),
            message: "autorun_eligible is true but file_scope is empty".into(),
        }];
    }
    Vec::new()
}

pub(crate) fn validate_autorun_scope_policy(
    data: &HashMap<String, serde_yaml::Value>,
) -> Vec<ValidationError> {
    let (autorun, set) = get_bool_field(data, "autorun_eligible");
    if !set || !autorun {
        return Vec::new();
    }
    let scope_policy = get_string_field(data, "scope_policy");
    if scope_policy == "permissive" {
        return vec![ValidationError {
            field: "scope_policy".into(),
            message: "scope_policy is permissive but autorun_eligible is true — \
                      autorun tasks require soft or hard scope_policy for claim enforcement"
                .into(),
        }];
    }
    Vec::new()
}

fn validate_code_task_tests(data: &HashMap<String, serde_yaml::Value>) -> Vec<ValidationError> {
    let wt = get_string_field(data, "work_type");
    if !code_work_type(&wt) {
        return Vec::new();
    }

    let has_code_file = match data.get("file_scope") {
        Some(serde_yaml::Value::Sequence(seq)) => seq.iter().any(|item| {
            let s = format!("{item:?}");
            s.contains(".sh") || s.contains(".py")
        }),
        Some(serde_yaml::Value::String(s)) => {
            !s.is_empty()
                && s != "[]"
                && s != "null"
                && s != "~"
                && (s.contains(".sh") || s.contains(".py"))
        }
        _ => false,
    };

    if !has_code_file {
        return Vec::new();
    }

    if is_field_empty(data, "tests") {
        return vec![ValidationError {
            field: "tests".into(),
            message:
                "code work type with .sh/.py files in file_scope requires non-empty tests field"
                    .into(),
        }];
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// Epic validation
// ---------------------------------------------------------------------------

const EPIC_REQUIRED_FIELDS: &[&str] = &[
    "id",
    "format_id",
    "title",
    "status",
    "area_type",
    "work_type",
];

const EPIC_STATUS_VALUES: &[&str] = &[
    "draft",
    "planning",
    "in_progress",
    "blocked",
    "complete",
    "archived",
];

const EPIC_FORMAT_ID_PATTERN: &str = r"^[A-Z]{2,4}-EPC-[0-9]{3}$";

const EPIC_REQUIRED_SECTIONS: &[&str] = &[
    "## Summary",
    "## Scope",
    "## Acceptance Criteria",
    "## Tasks",
    "## Dependencies",
    "## Technical Notes",
    "## Related",
];

const EPIC_TEMPLATE_SENTINEL_FIELDS: &[&str] = &["id", "title"];

/// Validate an epic markdown file at the given path.
///
/// Returns validation errors, warnings, and any I/O or parsing error.
///
/// # Panics
///
/// Panics if the format ID regex fails to compile (hardcoded pattern).
///
/// # Errors
///
/// Returns `ValidateError` on I/O or frontmatter parsing failures.
pub fn validate_epic(
    path: &Path,
    _opts: &ValidateOptions,
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

    // Format ID pattern.
    let fid = get_string_field(&data, "format_id");
    if !fid.is_empty() {
        let re = regex::Regex::new(EPIC_FORMAT_ID_PATTERN).expect("valid regex");
        if !re.is_match(&fid) {
            errs.push(ValidationError {
                field: "format_id".into(),
                message: format!("does not match pattern {EPIC_FORMAT_ID_PATTERN}"),
            });
        }
    }

    // Enum validations.
    errs.extend(validate_enum(&data, "status", EPIC_STATUS_VALUES));
    errs.extend(validate_enum(&data, "work_type", WORK_TYPE_VALUES));
    errs.extend(validate_enum(&data, "area_type", AREA_TYPE_VALUES));
    errs.extend(validate_optional_enum(&data, "priority", PRIORITY_VALUES));

    // Cross-field: format_id prefix must match area_type.
    errs.extend(validate_format_id_prefix(&data, "format_id", "-EPC-"));

    // Section checks.
    errs.extend(validate_sections(&body, EPIC_REQUIRED_SECTIONS));

    // Warnings: filename vs format_id.
    warns.extend(check_filename_match(path, &data));

    // Warnings: PII patterns in file_scope.
    warns.extend(check_pii_patterns(&data));

    Ok((errs, warns))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn valid_task_frontmatter() -> String {
        r#"---
id: "task-001"
format_id: "INF-TSK-022-014"
epic_id: "INF-EPC-022"
title: "Test task"
status: "in_progress"
area_type: "INF"
work_type: "FEAT"
---
## Description
Some description

## Approach
Some approach

## Files
Some files

## Acceptance Criteria
Some criteria

## Dependencies
None

## Verification
Steps

## Stage Reports
Reports

## Notes
Notes
"#
        .to_string()
    }

    fn valid_epic_frontmatter() -> String {
        r#"---
id: "epic-001"
format_id: "INF-EPC-022"
title: "Test epic"
status: "in_progress"
area_type: "INF"
work_type: "FEAT"
---
## Summary
Summary

## Scope
Scope

## Acceptance Criteria
Criteria

## Tasks
Tasks

## Dependencies
Deps

## Technical Notes
Notes

## Related
Related
"#
        .to_string()
    }

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
        let result = parse_frontmatter(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_frontmatter_missing_closer() {
        let content = b"---\ntitle: hello\nNo closing delimiter";
        let result = parse_frontmatter(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_frontmatter_empty_yaml() {
        let content = b"---\n---\nBody";
        let result = parse_frontmatter(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_frontmatter_with_bom() {
        let mut content = Vec::from(b"\xEF\xBB\xBF" as &[u8]);
        content.extend_from_slice(b"---\ntitle: hello\n---\nBody");
        let (data, _) = parse_frontmatter(&content).unwrap();
        assert_eq!(get_string_field(&data, "title"), "hello");
    }

    #[test]
    fn test_validate_task_valid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("INF-TSK-022-014.md");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(valid_task_frontmatter().as_bytes()).unwrap();

        let opts = ValidateOptions::default();
        let (errs, warns) = validate_task(&path, &opts).unwrap();
        assert!(errs.is_empty(), "Expected no errors, got: {errs:?}");
        assert!(warns.is_empty(), "Expected no warnings, got: {warns:?}");
    }

    #[test]
    fn test_validate_task_missing_required_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.md");
        let content = "---\ntitle: hello\n---\nBody\n";
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_task(&path, &opts).unwrap();
        // Should flag missing: id, format_id, epic_id, status, area_type, work_type
        assert!(errs.len() >= 6, "Expected >= 6 errors, got {}", errs.len());
    }

    #[test]
    fn test_validate_task_invalid_status() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("INF-TSK-022-014.md");
        let content =
            valid_task_frontmatter().replace("status: \"in_progress\"", "status: \"INVALID\"");
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_task(&path, &opts).unwrap();
        let status_err = errs.iter().find(|e| e.field == "status");
        assert!(status_err.is_some(), "Expected status error");
    }

    #[test]
    fn test_validate_task_format_id_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.md");
        let content = valid_task_frontmatter().replace(
            "format_id: \"INF-TSK-022-014\"",
            "format_id: \"DOC-TSK-022-014\"",
        );
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_task(&path, &opts).unwrap();
        let prefix_err = errs.iter().find(|e| e.message.contains("prefix"));
        assert!(prefix_err.is_some(), "Expected prefix mismatch error");
    }

    #[test]
    fn test_validate_task_auto_merge_protected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("INF-TSK-022-014.md");
        let mut content = valid_task_frontmatter();
        content = content.replace(
            "work_type: \"FEAT\"",
            "work_type: \"FEAT\"\nauto_merge: true\ntarget_branch: \"main\"",
        );
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_task(&path, &opts).unwrap();
        let merge_err = errs.iter().find(|e| e.field == "auto_merge");
        assert!(merge_err.is_some(), "Expected auto_merge error");
    }

    #[test]
    fn test_validate_task_template_sentinel() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.md");
        let content =
            valid_task_frontmatter().replace("title: \"Test task\"", "title: \"{PLACEHOLDER}\"");
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_task(&path, &opts).unwrap();
        let tmpl_err = errs
            .iter()
            .find(|e| e.message.contains("template sentinel") || e.message.contains("placeholder"));
        assert!(tmpl_err.is_some(), "Expected template sentinel error");
    }

    #[test]
    fn test_validate_task_missing_sections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("INF-TSK-022-014.md");
        let content = "---\nid: \"t1\"\nformat_id: \"INF-TSK-022-014\"\nepic_id: \"INF-EPC-022\"\ntitle: \"Test\"\nstatus: \"todo\"\narea_type: \"INF\"\nwork_type: \"FEAT\"\n---\nNo sections here\n";
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_task(&path, &opts).unwrap();
        let section_errs: Vec<_> = errs.iter().filter(|e| e.field == "body").collect();
        assert!(!section_errs.is_empty(), "Expected section errors");
    }

    #[test]
    fn test_validate_epic_valid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("INF-EPC-022.md");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(valid_epic_frontmatter().as_bytes()).unwrap();

        let opts = ValidateOptions::default();
        let (errs, warns) = validate_epic(&path, &opts).unwrap();
        assert!(errs.is_empty(), "Expected no errors, got: {errs:?}");
        assert!(warns.is_empty(), "Expected no warnings, got: {warns:?}");
    }

    #[test]
    fn test_validate_epic_missing_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.md");
        let content = "---\ntitle: hello\n---\nBody\n";
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_epic(&path, &opts).unwrap();
        // Should flag missing: id, format_id, status, area_type, work_type
        assert!(errs.len() >= 5, "Expected >= 5 errors, got {}", errs.len());
    }

    #[test]
    fn test_validate_epic_invalid_format_id() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.md");
        let content = valid_epic_frontmatter()
            .replace("format_id: \"INF-EPC-022\"", "format_id: \"invalid\"");
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_epic(&path, &opts).unwrap();
        let fid_err = errs.iter().find(|e| e.field == "format_id");
        assert!(fid_err.is_some(), "Expected format_id error");
    }

    #[test]
    fn test_validate_epic_invalid_status() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.md");
        let content =
            valid_epic_frontmatter().replace("status: \"in_progress\"", "status: \"BAD\"");
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_epic(&path, &opts).unwrap();
        let status_err = errs.iter().find(|e| e.field == "status");
        assert!(status_err.is_some(), "Expected status error");
    }

    #[test]
    fn test_pii_patterns_detected() {
        let mut data = HashMap::new();
        data.insert(
            "file_scope".to_string(),
            serde_yaml::Value::String("src/auth/login.rs".into()),
        );
        let warns = check_pii_patterns(&data);
        assert!(!warns.is_empty());
    }

    #[test]
    fn test_filename_match_warning() {
        let mut data = HashMap::new();
        data.insert(
            "format_id".to_string(),
            serde_yaml::Value::String("INF-TSK-001-001".into()),
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
    fn test_get_bool_field_string() {
        let mut data = HashMap::new();
        data.insert("key".to_string(), serde_yaml::Value::String("true".into()));
        let (val, set) = get_bool_field(&data, "key");
        assert!(set);
        assert!(val);
    }

    #[test]
    fn test_validate_autorun_acceptance() {
        let mut data = HashMap::new();
        data.insert(
            "autorun_eligible".to_string(),
            serde_yaml::Value::Bool(true),
        );
        // No acceptance field.
        let errs = validate_autorun_acceptance(&data);
        assert!(!errs.is_empty());
        assert!(errs[0].field == "autorun_eligible");
    }

    #[test]
    fn test_validate_raise_pr_auto_merge_contradiction() {
        let mut data = HashMap::new();
        data.insert("raise_pr".to_string(), serde_yaml::Value::Bool(false));
        data.insert("auto_merge".to_string(), serde_yaml::Value::Bool(true));
        let errs = validate_raise_pr_auto_merge(&data);
        assert!(!errs.is_empty());
    }

    #[test]
    fn test_validate_code_task_tests_missing() {
        let mut data = HashMap::new();
        data.insert(
            "work_type".to_string(),
            serde_yaml::Value::String("FEAT".into()),
        );
        data.insert(
            "file_scope".to_string(),
            serde_yaml::Value::Sequence(vec![serde_yaml::Value::String("scripts/test.sh".into())]),
        );
        // No tests field.
        let errs = validate_code_task_tests(&data);
        assert!(!errs.is_empty());
        assert!(errs[0].field == "tests");
    }

    // -- autorun_eligible + file_scope validation (AC #11) --

    #[test]
    fn test_autorun_file_scope_empty_is_error() {
        let mut data = HashMap::new();
        data.insert("autorun_eligible".into(), serde_yaml::Value::Bool(true));
        // file_scope empty.
        let errs = validate_autorun_file_scope(&data);
        assert!(!errs.is_empty());
        assert!(errs[0].field == "autorun_eligible");
        assert!(errs[0].message.contains("file_scope is empty"));
    }

    #[test]
    fn test_autorun_file_scope_present_passes() {
        let mut data = HashMap::new();
        data.insert("autorun_eligible".into(), serde_yaml::Value::Bool(true));
        data.insert(
            "file_scope".into(),
            serde_yaml::Value::Sequence(vec![serde_yaml::Value::String("src/main.rs".into())]),
        );
        let errs = validate_autorun_file_scope(&data);
        assert!(errs.is_empty());
    }

    #[test]
    fn test_autorun_file_scope_not_autorun_passes() {
        let mut data = HashMap::new();
        data.insert("autorun_eligible".into(), serde_yaml::Value::Bool(false));
        // file_scope empty is fine if not autorun.
        let errs = validate_autorun_file_scope(&data);
        assert!(errs.is_empty());
    }

    // -- autorun_eligible + scope_policy validation (AC #12) --

    #[test]
    fn test_autorun_permissive_is_error() {
        let mut data = HashMap::new();
        data.insert("autorun_eligible".into(), serde_yaml::Value::Bool(true));
        data.insert(
            "scope_policy".into(),
            serde_yaml::Value::String("permissive".into()),
        );
        let errs = validate_autorun_scope_policy(&data);
        assert!(!errs.is_empty());
        assert!(errs[0].field == "scope_policy");
        assert!(errs[0].message.contains("permissive"));
    }

    #[test]
    fn test_autorun_soft_passes() {
        let mut data = HashMap::new();
        data.insert("autorun_eligible".into(), serde_yaml::Value::Bool(true));
        data.insert(
            "scope_policy".into(),
            serde_yaml::Value::String("soft".into()),
        );
        let errs = validate_autorun_scope_policy(&data);
        assert!(errs.is_empty());
    }

    #[test]
    fn test_autorun_hard_passes() {
        let mut data = HashMap::new();
        data.insert("autorun_eligible".into(), serde_yaml::Value::Bool(true));
        data.insert(
            "scope_policy".into(),
            serde_yaml::Value::String("hard".into()),
        );
        let errs = validate_autorun_scope_policy(&data);
        assert!(errs.is_empty());
    }

    #[test]
    fn test_not_autorun_permissive_passes() {
        let mut data = HashMap::new();
        data.insert("autorun_eligible".into(), serde_yaml::Value::Bool(false));
        data.insert(
            "scope_policy".into(),
            serde_yaml::Value::String("permissive".into()),
        );
        let errs = validate_autorun_scope_policy(&data);
        assert!(errs.is_empty());
    }

    // -- stage validation tests --

    #[test]
    fn test_stage_valid_dev() {
        let mut data = HashMap::new();
        data.insert("stage".into(), serde_yaml::Value::String("dev".into()));
        let errs = validate_optional_enum(&data, "stage", STAGE_VALUES);
        assert!(errs.is_empty());
    }

    #[test]
    fn test_stage_valid_plan() {
        let mut data = HashMap::new();
        data.insert("stage".into(), serde_yaml::Value::String("plan".into()));
        let errs = validate_optional_enum(&data, "stage", STAGE_VALUES);
        assert!(errs.is_empty());
    }

    #[test]
    fn test_stage_valid_docs() {
        let mut data = HashMap::new();
        data.insert("stage".into(), serde_yaml::Value::String("docs".into()));
        let errs = validate_optional_enum(&data, "stage", STAGE_VALUES);
        assert!(errs.is_empty());
    }

    #[test]
    fn test_stage_valid_test() {
        let mut data = HashMap::new();
        data.insert("stage".into(), serde_yaml::Value::String("test".into()));
        let errs = validate_optional_enum(&data, "stage", STAGE_VALUES);
        assert!(errs.is_empty());
    }

    #[test]
    fn test_stage_valid_review() {
        let mut data = HashMap::new();
        data.insert("stage".into(), serde_yaml::Value::String("review".into()));
        let errs = validate_optional_enum(&data, "stage", STAGE_VALUES);
        assert!(errs.is_empty());
    }

    #[test]
    fn test_stage_valid_qa() {
        let mut data = HashMap::new();
        data.insert("stage".into(), serde_yaml::Value::String("qa".into()));
        let errs = validate_optional_enum(&data, "stage", STAGE_VALUES);
        assert!(errs.is_empty());
    }

    #[test]
    fn test_stage_valid_done() {
        let mut data = HashMap::new();
        data.insert("stage".into(), serde_yaml::Value::String("done".into()));
        let errs = validate_optional_enum(&data, "stage", STAGE_VALUES);
        assert!(errs.is_empty());
    }

    #[test]
    fn test_stage_invalid_value_rejected() {
        let mut data = HashMap::new();
        data.insert("stage".into(), serde_yaml::Value::String("invalid".into()));
        let errs = validate_optional_enum(&data, "stage", STAGE_VALUES);
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].field, "stage");
        assert!(errs[0].message.contains("invalid value"));
    }

    #[test]
    fn test_stage_empty_is_ok() {
        let data = HashMap::new();
        let errs = validate_optional_enum(&data, "stage", STAGE_VALUES);
        assert!(errs.is_empty());
    }
}
