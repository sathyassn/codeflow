//! YAML frontmatter validation for task, epic, and spec markdown files.
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

pub mod docs;
pub mod portal;

use std::collections::HashMap;
use std::path::Path;

use thiserror::Error;

use crate::workgraph::{
    is_canonical_task_format_id, is_stable_work_target, is_valid_epic_format_id,
    is_valid_spec_format_id, is_valid_task_format_id,
};

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
    pub spec_required_sections: Vec<String>,
}

/// Blocking structural and referential findings for the complete durable
/// workgraph. This is the shared gate used by `validate --docs`, `work start`,
/// task-branch pre-commit, and task-branch CI.
#[derive(Debug, Default)]
pub struct WorkgraphValidationReport {
    pub checked_records: usize,
    pub issues: Vec<String>,
    pub warnings: Vec<String>,
    pub notes: Vec<String>,
}

impl WorkgraphValidationReport {
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }
}

/// Validate every durable record and every relationship in one pass.
#[must_use]
pub fn validate_workgraph(repo_root: &Path) -> WorkgraphValidationReport {
    let pm_root = repo_root.join("project-management");
    let opts = ValidateOptions::default();
    let mut report = WorkgraphValidationReport::default();
    let mut identities = HashMap::<String, String>::new();

    let record_sets = [
        (
            crate::workgraph::layout::epic_record_files(&pm_root),
            validate_epic
                as fn(
                    &Path,
                    &ValidateOptions,
                )
                    -> Result<(Vec<ValidationError>, Vec<ValidationWarning>), ValidateError>,
            is_valid_epic_format_id as fn(&str) -> bool,
        ),
        (
            crate::workgraph::layout::spec_record_files(&pm_root),
            validate_spec,
            is_valid_spec_format_id,
        ),
        (
            crate::workgraph::layout::task_record_files(&pm_root),
            validate_task,
            is_valid_task_format_id,
        ),
    ];
    for (files, validator, valid_identity) in record_sets {
        for path in files {
            report.checked_records += 1;
            let relative = path.strip_prefix(repo_root).unwrap_or(&path);
            let display = relative.to_string_lossy().replace('\\', "/");
            if let Ok(content) = std::fs::read(&path) {
                if let Ok((data, _)) = parse_frontmatter(&content) {
                    if let Some(identity) = supported_identity(&data, valid_identity) {
                        if let Some(first) = identities.get(&identity) {
                            report.issues.push(format!(
                                "duplicate work id {identity}: {first} and {display}"
                            ));
                        } else {
                            identities.insert(identity, display.clone());
                        }
                    }
                }
            }
            match validator(&path, &opts) {
                Ok((errors, warnings)) => {
                    report.issues.extend(
                        errors
                            .into_iter()
                            .map(|error| format!("{display}: {error}")),
                    );
                    report.warnings.extend(
                        warnings
                            .into_iter()
                            .map(|warning| format!("{display}: {warning}")),
                    );
                }
                Err(error) => report.issues.push(format!("{display}: {error}")),
            }
        }
    }

    let lifecycle = crate::workgraph::lifecycle::validate_lifecycle(repo_root);
    report.issues.extend(lifecycle.errors);
    report.warnings.extend(lifecycle.warnings);

    let docs = docs::lint_docs(repo_root);
    report
        .issues
        .extend(docs.issues.into_iter().map(|issue| issue.to_string()));
    report.notes = docs.notes;
    report.issues.sort();
    report.issues.dedup();
    report.warnings.sort();
    report.warnings.dedup();
    report
}

fn supported_identity(
    data: &HashMap<String, serde_yaml::Value>,
    valid: fn(&str) -> bool,
) -> Option<String> {
    let id = get_string_field(data, "id");
    if valid(&id) {
        return Some(id);
    }
    let alias = get_string_field(data, "format_id");
    valid(&alias).then_some(alias)
}

impl Default for ValidateOptions {
    fn default() -> Self {
        Self {
            task_required_sections: vec!["## Description".into(), "## Acceptance Criteria".into()],
            epic_required_sections: vec!["## Summary".into(), "## Acceptance Criteria".into()],
            spec_required_sections: vec![
                "## Summary".into(),
                "## Behavior".into(),
                "## Open questions".into(),
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

pub(crate) fn canonical_identity(
    path: &Path,
    data: &HashMap<String, serde_yaml::Value>,
    valid: impl Fn(&str) -> bool,
    expected: &str,
) -> (String, Vec<ValidationError>, Vec<ValidationWarning>) {
    let id = get_string_field(data, "id");
    let alias = get_string_field(data, "format_id");
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let canonical = if valid(&id) {
        if !alias.is_empty() && alias != id {
            errors.push(ValidationError {
                field: "format_id".into(),
                message: format!("conflicts with canonical id {id}"),
            });
        }
        id
    } else if valid(&alias) {
        warnings.push(ValidationWarning {
            field: "format_id".into(),
            message: "historical dual-identity record; new records use one stable id".into(),
        });
        alias
    } else {
        if !id.is_empty() {
            errors.push(ValidationError {
                field: "id".into(),
                message: format!("does not match {expected}"),
            });
        }
        String::new()
    };
    // The hidden record identity (SPC-013 R-1): optional until a line's
    // backfill, and a lower-case UUIDv4 whenever present.
    let uid = get_string_field(data, "uid");
    if !uid.is_empty() && !crate::ids::is_uid(&uid) {
        errors.push(ValidationError {
            field: "uid".into(),
            message: "must be the lower-case UUIDv4 `new` wrote; a uid is never edited".into(),
        });
    }
    let base = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    if !canonical.is_empty() && base != canonical {
        errors.push(ValidationError {
            field: "id".into(),
            message: format!(
                "filename \"{}\" does not match stable id \"{canonical}\"",
                path.file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default()
            ),
        });
    }
    (canonical, errors, warnings)
}

// ---------------------------------------------------------------------------
// Task validation
// ---------------------------------------------------------------------------

const TASK_REQUIRED_FIELDS: &[&str] = &["id", "title", "status", "work_type"];

const TASK_STATUS_VALUES: &[&str] = &["todo", "blocked", "in_progress", "complete", "cancelled"];

const TASK_ARRAY_FIELDS: &[&str] = &["acceptance", "tests", "specs", "depends_on"];

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

    let (id, identity_errors, identity_warnings) =
        canonical_identity(path, &data, is_valid_task_format_id, "TSK-NNN");
    errs.extend(identity_errors);
    warns.extend(identity_warnings);
    let integration_target = get_string_field(&data, "integration_target");
    if is_canonical_task_format_id(&id) && integration_target.trim().is_empty() {
        errs.push(ValidationError {
            field: "integration_target".into(),
            message: "required for canonical TSK-NNN records".into(),
        });
    } else if !integration_target.trim().is_empty() && !is_stable_work_target(&integration_target) {
        errs.push(ValidationError {
            field: "integration_target".into(),
            message:
                "must name a stable non-task branch, not a tag, object ID, or revision expression"
                    .into(),
        });
    }

    let epic_id = get_string_field(&data, "epic_id");
    let standalone_reason = get_string_field(&data, "standalone_reason");
    match (epic_id.is_empty(), standalone_reason.trim().is_empty()) {
        (true, true) => errs.push(ValidationError {
            field: "standalone_reason".into(),
            message: "required when epic_id is null".into(),
        }),
        (false, false) => errs.push(ValidationError {
            field: "standalone_reason".into(),
            message: "must be null when epic_id is present".into(),
        }),
        _ => {}
    }
    if !epic_id.is_empty() && !is_valid_epic_format_id(&epic_id) {
        errs.push(ValidationError {
            field: "epic_id".into(),
            message: "does not match EPC-NNN".into(),
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

    // Legacy checkbox records keep this rule; a record whose Closeout carries
    // an acceptance block records its results there instead (R-50, R-54).
    let body_text = String::from_utf8_lossy(&body);
    let has_acceptance_block = crate::workgraph::record_text::acceptance_blocks(&body_text)
        .iter()
        .any(|block| !block.is_superseded());
    if get_string_field(&data, "status") == "complete"
        && body_text.contains("- [ ]")
        && !has_acceptance_block
    {
        errs.push(ValidationError {
            field: "body".into(),
            message: "complete task still has unchecked acceptance criteria".into(),
        });
    }

    let repo_root = record_repo_root(path);
    errs.extend(awaiting_selection_errors(
        &data,
        &body_text,
        repo_root.as_deref(),
    ));
    if epic_id.is_empty() && get_string_field(&data, "status") == "complete" {
        if let Some(root) = repo_root.as_deref() {
            let pulls = landed_pull_requests(root, &id);
            if pulls > 1 {
                warns.push(ValidationWarning {
                    field: "standalone_reason".into(),
                    message: format!(
                        "standalone task {id} was completed by {pulls} pull requests; a standalone task is one reviewable pull request (SPC-013 R-66)"
                    ),
                });
            }
        }
    }

    Ok((errs, warns))
}

/// The repository root of a record under `project-management/`.
fn record_repo_root(record: &Path) -> Option<std::path::PathBuf> {
    record
        .ancestors()
        .find(|dir| {
            dir.file_name()
                .is_some_and(|name| name == "project-management")
        })
        .and_then(Path::parent)
        .map(Path::to_path_buf)
}

/// A join awaiting selection (SPC-013 R-43) is a valid record that no
/// context starts: it is `blocked` with the reason "awaiting selection" and
/// the referenced plan or decision path as its revisit event, and that path
/// exists.
fn awaiting_selection_errors(
    data: &std::collections::HashMap<String, serde_yaml::Value>,
    body: &str,
    repo_root: Option<&Path>,
) -> Vec<ValidationError> {
    let path = get_string_field(data, "awaiting_selection");
    if path.trim().is_empty() {
        return Vec::new();
    }
    let mut errs = Vec::new();
    let mut fail = |message: String| {
        errs.push(ValidationError {
            field: "awaiting_selection".into(),
            message,
        });
    };
    if get_string_field(data, "status") != "blocked" {
        fail("a join awaiting selection is `blocked`".into());
    }
    let blocker = crate::workgraph::record_text::parse_blocker(body).unwrap_or_default();
    if blocker.reason != "awaiting selection" {
        fail("its `## Blocker` reason is `awaiting selection`".into());
    }
    if blocker.revisit.trim_matches('`') != path {
        fail(format!("its `## Blocker` revisit event is `{path}`"));
    }
    if let Some(root) = repo_root {
        let relative = Path::new(&path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
            || !root.join(relative).exists()
        {
            fail(format!(
                "`{path}` must be a path that exists in the repository"
            ));
        }
    }
    errs
}

/// How many merged pull requests landed a branch carrying `task_id`, counted
/// from merge commit subjects (`Merge pull request #N from <prefix>/TSK-NNN-...`
/// or `Merge branch '<prefix>/TSK-NNN-...'`). Zero when git is unavailable.
fn landed_pull_requests(repo_root: &Path, task_id: &str) -> usize {
    let Ok(out) = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["log", "--merges", "--format=%s", "HEAD"])
        .output()
    else {
        return 0;
    };
    let needle = format!("/{task_id}-");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|subject| subject.starts_with("Merge") && subject.contains(&needle))
        .count()
}

// ---------------------------------------------------------------------------
// Epic validation
// ---------------------------------------------------------------------------

const EPIC_REQUIRED_FIELDS: &[&str] = &["id", "title", "status", "work_type"];

const EPIC_STATUS_VALUES: &[&str] = &[
    "draft",
    "planning",
    "in_progress",
    "blocked",
    "complete",
    "cancelled",
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

    let (_id, identity_errors, identity_warnings) =
        canonical_identity(path, &data, is_valid_epic_format_id, "EPC-NNN");
    errs.extend(identity_errors);
    warns.extend(identity_warnings);

    // Enum validations.
    errs.extend(validate_enum(&data, "status", EPIC_STATUS_VALUES));
    errs.extend(validate_enum(&data, "work_type", WORK_TYPE_VALUES));
    errs.extend(validate_optional_enum(&data, "priority", PRIORITY_VALUES));

    // Section checks.
    errs.extend(validate_sections(&body, &opts.epic_required_sections));

    errs.extend(validate_array_fields(
        &data,
        &["capabilities", "adrs", "specs"],
    ));

    Ok((errs, warns))
}

// ---------------------------------------------------------------------------
// Spec validation
// ---------------------------------------------------------------------------

const SPEC_REQUIRED_FIELDS: &[&str] = &["id", "title", "status"];
/// `implemented` stays readable on older specs; it is derived and never
/// written on new ones (SPC-013 R-32, R-51).
const SPEC_STATUS_VALUES: &[&str] = &["draft", "approved", "superseded", "implemented"];

/// Validate a specification record.
///
/// # Errors
///
/// Returns `ValidateError` on I/O or frontmatter parsing failures.
pub fn validate_spec(
    path: &Path,
    opts: &ValidateOptions,
) -> Result<(Vec<ValidationError>, Vec<ValidationWarning>), ValidateError> {
    let content = std::fs::read(path)?;
    let (data, body) = parse_frontmatter(&content)?;
    let mut errors = validate_required_fields(&data, SPEC_REQUIRED_FIELDS);
    let mut warnings = Vec::new();
    errors.extend(check_template_sentinels(&data, &["id", "title"]));
    let (_id, identity_errors, identity_warnings) =
        canonical_identity(path, &data, is_valid_spec_format_id, "SPC-NNN");
    errors.extend(identity_errors);
    warnings.extend(identity_warnings);
    errors.extend(validate_enum(&data, "status", SPEC_STATUS_VALUES));
    errors.extend(validate_sections(&body, &opts.spec_required_sections));
    errors.extend(validate_array_fields(&data, &["supersedes"]));

    if matches!(
        get_string_field(&data, "status").as_str(),
        "approved" | "implemented"
    ) && section_has_unresolved_questions(&body)
    {
        errors.push(ValidationError {
            field: "body".into(),
            message: "approved or implemented spec must leave open questions \
                      blank or use an explicit resolved marker"
                .into(),
        });
    }
    Ok((errors, warnings))
}

#[cfg(test)]
fn section_has_content(body: &[u8], heading: &str) -> bool {
    !visible_section_text(body, heading).trim().is_empty()
}

pub(crate) fn section_has_unresolved_questions(body: &[u8]) -> bool {
    let text = visible_section_text(body, "## Open questions");
    let marker = text.trim().trim_end_matches(['.', ';']);
    if marker.is_empty() {
        return false;
    }
    let lower = marker.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "none" | "n/a" | "not applicable" | "all resolved"
    ) {
        return false;
    }
    let contradicts_resolution = marker.contains('?')
        || [
            "unresolved",
            "except",
            "pending",
            "remaining",
            "not resolved",
        ]
        .iter()
        .any(|phrase| lower.contains(phrase));
    if contradicts_resolution {
        return true;
    }
    let none_with_resolution = lower.strip_prefix("none").is_some_and(|suffix| {
        let suffix = suffix.trim_start();
        suffix
            .chars()
            .next()
            .is_some_and(|character| "-—:;".contains(character))
            && suffix
                .split(|character: char| !character.is_alphabetic())
                .any(|word| word == "resolved")
    });
    let natural_resolution = ["all resolved", "no open questions", "resolved"]
        .iter()
        .any(|prefix| lower.starts_with(prefix));
    !(none_with_resolution || natural_resolution)
}

fn visible_section_text(body: &[u8], heading: &str) -> String {
    let text = String::from_utf8_lossy(body);
    let Some(start) = text.find(heading) else {
        return String::new();
    };
    let after = &text[start + heading.len()..];
    let end = after
        .find("\n## ")
        .or_else(|| after.find("\r\n## "))
        .unwrap_or(after.len());
    let section = &after[..end];
    let mut visible = String::new();
    let mut remainder = section;
    while let Some(comment_start) = remainder.find("<!--") {
        visible.push_str(&remainder[..comment_start]);
        let after_open = &remainder[comment_start + "<!--".len()..];
        let Some(comment_end) = after_open.find("-->") else {
            remainder = "";
            break;
        };
        remainder = &after_open[comment_end + "-->".len()..];
    }
    visible.push_str(remainder);
    visible
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::models::{EpicStatus, TaskStatus};

    use super::*;

    fn valid_task_content() -> String {
        r#"---
id: "TSK-022"
epic_id: "EPC-022"
standalone_reason: null
integration_target: main
title: "Test task"
status: "in_progress"
work_type: "feat"
priority: "high"
estimate: "M"
acceptance:
  - "All tests pass"
tests:
  - "crates/codeflow-core/src/"
specs: []
depends_on: []
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
id: "EPC-022"
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
        let path = dir.path().join("TSK-022.md");
        std::fs::write(&path, valid_task_content()).unwrap();

        let opts = ValidateOptions::default();
        let (errs, warns) = validate_task(&path, &opts).unwrap();
        assert!(errs.is_empty(), "Expected no errors, got: {errs:?}");
        assert!(warns.is_empty(), "Expected no warnings, got: {warns:?}");
    }

    /// TSK-103 AC-5: a join awaiting selection is a valid blocked record
    /// whose Blocker names the selection; anything less is an error.
    #[test]
    fn a_join_awaiting_selection_validates_only_when_held_by_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("project-management/tasks")).unwrap();
        std::fs::create_dir_all(root.join("docs/plan")).unwrap();
        std::fs::write(root.join("docs/plan/choice.md"), "# Choice\n").unwrap();
        let path = root.join("project-management/tasks/TSK-004.md");
        let record = |status: &str, reason: &str, revisit: &str, path: &str| {
            format!(
                "---\nid: TSK-004\nepic_id: EPC-001\nstandalone_reason: null\nintegration_target: main\ntitle: join\nstatus: {status}\nwork_type: feat\ndepends_on: []\nawaiting_selection: {path}\ncreated: 2026-09-27\n---\n\n# TSK-004\n\n## Blocker\n\n- reason: {reason}\n- owner: primary\n- revisit: {revisit}\n"
            )
        };
        let errors = |content: String| {
            std::fs::write(&path, content).unwrap();
            validate_task(&path, &ValidateOptions::default())
                .unwrap()
                .0
                .into_iter()
                .filter(|error| error.field == "awaiting_selection")
                .map(|error| error.message)
                .collect::<Vec<_>>()
        };
        let good = "docs/plan/choice.md";
        assert!(errors(record("blocked", "awaiting selection", good, good)).is_empty());
        assert!(errors(record("todo", "awaiting selection", good, good))[0].contains("`blocked`"));
        assert!(errors(record("blocked", "wait", good, good))[0].contains("awaiting selection"));
        assert!(
            errors(record("blocked", "awaiting selection", "later", good))[0].contains("revisit")
        );
        let missing = "docs/plan/missing.md";
        assert!(
            errors(record("blocked", "awaiting selection", missing, missing))
                .iter()
                .any(|message| message.contains("exists"))
        );
    }

    #[test]
    fn test_validate_task_missing_required_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.md");
        std::fs::write(&path, "---\ntitle: hello\n---\nBody\n").unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_task(&path, &opts).unwrap();
        // Required-field errors are separate from relationship/body errors.
        let missing: Vec<_> = errs
            .iter()
            .filter(|e| e.message.contains("required field"))
            .collect();
        assert_eq!(
            missing.len(),
            3,
            "Expected 3 missing-field errors: {errs:?}"
        );
    }

    #[test]
    fn test_validate_task_invalid_status() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("TSK-022.md");
        let content =
            valid_task_content().replace("status: \"in_progress\"", "status: \"INVALID\"");
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_task(&path, &opts).unwrap();
        assert!(errs.iter().any(|e| e.field == "status"));
    }

    #[test]
    fn test_validate_task_rejects_task_branch_as_integration_target() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("TSK-022.md");
        let content = valid_task_content().replace(
            "integration_target: main",
            "integration_target: task/TSK-022-self",
        );
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_task(&path, &opts).unwrap();
        assert!(errs.iter().any(|error| {
            error.field == "integration_target" && error.message.contains("stable non-task branch")
        }));
    }

    #[test]
    fn test_validate_task_rejects_full_object_id_as_integration_target() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("TSK-022.md");
        let content = valid_task_content().replace(
            "integration_target: main",
            "integration_target: 0123456789abcdef0123456789abcdef01234567",
        );
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_task(&path, &opts).unwrap();
        assert!(errs.iter().any(|error| {
            error.field == "integration_target" && error.message.contains("stable non-task branch")
        }));
    }

    #[test]
    fn test_validate_task_v1_work_type_rejected() {
        // v1 used uppercase work types (FEAT); v2 uses the branch-prefix set.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("TSK-022.md");
        let content = valid_task_content().replace("work_type: \"feat\"", "work_type: \"FEAT\"");
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_task(&path, &opts).unwrap();
        assert!(errs.iter().any(|e| e.field == "work_type"));
    }

    #[test]
    fn test_validate_task_area_prefixed_id_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("INF-TSK-022.md");
        let content = valid_task_content().replace("id: \"TSK-022\"", "id: \"INF-TSK-022\"");
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_task(&path, &opts).unwrap();
        assert!(errs.iter().any(|e| e.field == "id"));
    }

    #[test]
    fn test_validate_task_invalid_estimate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("TSK-022.md");
        let content = valid_task_content().replace("estimate: \"M\"", "estimate: \"HUGE\"");
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_task(&path, &opts).unwrap();
        assert!(errs.iter().any(|e| e.field == "estimate"));
    }

    #[test]
    fn test_validate_task_acceptance_must_be_array() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("TSK-022.md");
        let content =
            valid_task_content().replace("acceptance:\n  - \"All tests pass\"", "acceptance: 42");
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
        assert!(errs
            .iter()
            .any(|e| e.message.contains("template sentinel") || e.message.contains("placeholder")));
    }

    #[test]
    fn test_validate_task_missing_sections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("TSK-022.md");
        let content = valid_task_content().replace("## Acceptance Criteria", "## Something Else");
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_task(&path, &opts).unwrap();
        let section_errs: Vec<_> = errs.iter().filter(|e| e.field == "body").collect();
        assert_eq!(
            section_errs.len(),
            1,
            "Expected one section error: {errs:?}"
        );
        assert!(section_errs[0].message.contains("Acceptance Criteria"));
    }

    #[test]
    fn test_validate_task_custom_sections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("TSK-022.md");
        std::fs::write(&path, valid_task_content()).unwrap();

        let opts = ValidateOptions {
            task_required_sections: vec!["## Verification".into()],
            ..Default::default()
        };
        let (errs, _) = validate_task(&path, &opts).unwrap();
        assert!(errs.iter().any(|e| e.message.contains("Verification")));
    }

    #[test]
    fn test_validate_task_filename_mismatch_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wrong-name.md");
        std::fs::write(&path, valid_task_content()).unwrap();

        let opts = ValidateOptions::default();
        let (errors, _) = validate_task(&path, &opts).unwrap();
        assert!(errors.iter().any(|error| error.field == "id"));
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
        // Required-field errors are separate from body errors.
        let missing: Vec<_> = errs
            .iter()
            .filter(|e| e.message.contains("required field"))
            .collect();
        assert_eq!(
            missing.len(),
            3,
            "Expected 3 missing-field errors: {errs:?}"
        );
    }

    #[test]
    fn test_validate_epic_invalid_id() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.md");
        let content = valid_epic_content().replace("id: \"EPC-022\"", "id: \"invalid\"");
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_epic(&path, &opts).unwrap();
        assert!(errs.iter().any(|e| e.field == "id"));
    }

    #[test]
    fn test_validate_epic_area_prefixed_id_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("INF-EPC-022.md");
        let content = valid_epic_content().replace("id: \"EPC-022\"", "id: \"INF-EPC-022\"");
        std::fs::write(&path, content).unwrap();

        let opts = ValidateOptions::default();
        let (errs, _) = validate_epic(&path, &opts).unwrap();
        assert!(errs.iter().any(|e| e.field == "id"));
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

    #[test]
    fn complete_workgraph_rejects_duplicate_identity_across_layouts() {
        let dir = tempfile::tempdir().unwrap();
        let pm = dir.path().join("project-management");
        std::fs::create_dir_all(pm.join("epics/EPC-022")).unwrap();
        std::fs::create_dir_all(pm.join("specs")).unwrap();
        std::fs::create_dir_all(pm.join("tasks")).unwrap();
        std::fs::write(pm.join("epics/EPC-022.md"), valid_epic_content()).unwrap();
        std::fs::write(pm.join("epics/EPC-022/EPC-022.md"), valid_epic_content()).unwrap();

        let report = validate_workgraph(dir.path());
        assert!(!report.is_clean());
        assert!(report.issues.iter().any(|issue| {
            issue.contains("duplicate work id EPC-022")
                && issue.contains("project-management/epics/EPC-022.md")
                && issue.contains("project-management/epics/EPC-022/EPC-022.md")
        }));
    }

    // -- helpers --

    #[test]
    fn test_filename_match_warning() {
        let mut data = HashMap::new();
        data.insert(
            "format_id".to_string(),
            serde_yaml::Value::String("TSK-001-001".into()),
        );
        let (_, errors, _) = canonical_identity(
            Path::new("wrong-name.md"),
            &data,
            is_valid_task_format_id,
            "TSK-NNN",
        );
        assert!(!errors.is_empty());
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
        data.insert("empty_seq".to_string(), serde_yaml::Value::Sequence(vec![]));
        data.insert("filled".to_string(), serde_yaml::Value::String("x".into()));
        assert!(is_field_empty(&data, "null_field"));
        assert!(is_field_empty(&data, "empty_seq"));
        assert!(is_field_empty(&data, "absent"));
        assert!(!is_field_empty(&data, "filled"));
    }

    #[test]
    fn section_content_ignores_complete_and_unclosed_html_comments() {
        assert!(!section_has_content(
            b"## Open questions\n<!--\nplaceholder\nspans lines\n-->\n## Decisions\nDone\n",
            "## Open questions"
        ));
        assert!(!section_has_content(
            b"## Open questions\n<!-- unfinished placeholder\nstill a comment\n",
            "## Open questions"
        ));
    }

    #[test]
    fn section_content_detects_text_around_multiple_html_comments() {
        assert!(section_has_content(
            b"## Open questions\n<!-- first -->\nMaterial question\n<!-- second -->\n## Decisions\n",
            "## Open questions"
        ));
        assert!(section_has_content(
            b"## Open questions\n<!-- placeholder --> actual question\n## Decisions\n",
            "## Open questions"
        ));
    }

    #[test]
    fn resolved_open_question_markers_are_not_unresolved_questions() {
        for marker in [
            "",
            "None",
            "N/A",
            "Not applicable.",
            "All resolved",
            "All resolved during planning.",
            "No open questions remain.",
            "Resolved — see SPC-008.",
            "None — all resolved during planning.",
        ] {
            let body =
                format!("## Open questions\n{marker}\n<!-- placeholder -->\n## Decisions\nDone\n");
            assert!(
                !section_has_unresolved_questions(body.as_bytes()),
                "{marker:?} must describe a resolved section"
            );
        }
        assert!(section_has_unresolved_questions(
            b"## Open questions\nWhich recovery channel is authoritative?\n## Decisions\n"
        ));
        assert!(section_has_unresolved_questions(
            b"## Open questions\nNone are resolved yet.\n## Decisions\n"
        ));
        assert!(section_has_unresolved_questions(
            b"## Open questions\nAll resolved except the recovery channel.\n## Decisions\n"
        ));
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
