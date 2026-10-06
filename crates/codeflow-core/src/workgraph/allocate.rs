//! Deterministic id allocation and scaffolding for durable work records.
//!
//! New records use independent `EPC-NNN`, `SPC-NNN`, and `TSK-NNN` ids and
//! write the canonical flat layout. Historical nested epics/tasks and
//! `TSK-NNN-NNN` task ids remain visible to allocation so a new id never
//! collides with an existing legacy namespace.
//!
//! Every record gets a hidden `uid` (SPC-013 R-1). Where durable work is
//! tracked, the CLI passes an allocator that issues the id from the shared
//! registry (`crate::ids`); the default allocator scans the visible checkout
//! and selects max + 1. Either way the record is created exclusively, so a
//! same-checkout race fails instead of overwriting.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use crate::scaffold::template::TemplateContext;
use crate::workgraph::store::StoreError;

/// A newly scaffolded record and the file written.
#[derive(Debug, Clone)]
pub struct NewRecord {
    /// Stable record id.
    pub id: String,
    /// The hidden `uid` written into the frontmatter.
    pub uid: String,
    /// Canonical record path.
    pub path: PathBuf,
}

/// Chooses a new record's `(id, uid)` once its inputs are validated. The
/// argument is the record's intended integration target, or `none`.
pub type Allocator<'a> = dyn FnMut(&str) -> Result<(String, String), StoreError> + 'a;

fn checkout_allocator(
    next: impl Fn() -> String,
) -> impl FnMut(&str) -> Result<(String, String), StoreError> {
    move |_| Ok((next(), crate::ids::new_uid()))
}

pub(crate) fn planning_target(pm_root: &Path) -> String {
    crate::workgraph::default_work_target(repository_root(pm_root)).map_or_else(
        || "none".to_string(),
        |target| {
            target
                .strip_prefix("origin/")
                .unwrap_or(&target)
                .to_string()
        },
    )
}

/// The repository root a records folder belongs to: the parent of a folder
/// named `project-management`, else the folder itself. Record writes are
/// contained beneath it (issue 94).
pub(crate) fn repository_root(pm_root: &Path) -> &Path {
    if pm_root
        .file_name()
        .is_some_and(|name| name == "project-management")
    {
        pm_root.parent().unwrap_or(pm_root)
    } else {
        pm_root
    }
}

fn sequence(id: &str, prefix: &str) -> Option<u32> {
    id.strip_prefix(prefix)?.parse().ok()
}

/// Reserve the first sequence of either `TSK-NNN` or historical
/// `TSK-NNN-NNN`. Reserving the legacy epic segment prevents a canonical
/// `TSK-051` from becoming visually ambiguous beside `TSK-051-008`.
fn task_reservation_sequence(id: &str) -> Option<u32> {
    let rest = id.strip_prefix("TSK-")?;
    rest.split('-').next()?.parse().ok()
}

fn record_stem(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|value| value.to_str())
        .map(str::to_owned)
}

fn direct_record_stems(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().is_some_and(|extension| extension == "md") {
                record_stem(&path)
            } else if path.is_dir() {
                path.file_name()
                    .and_then(|value| value.to_str())
                    .map(str::to_owned)
            } else {
                None
            }
        })
        .collect()
}

fn task_record_stems(pm_root: &Path) -> Vec<String> {
    crate::workgraph::layout::task_record_files(pm_root)
        .iter()
        .filter_map(|path| record_stem(path))
        .collect()
}

/// Next free epic id (`EPC-NNN`, max visible sequence + 1).
#[must_use]
pub fn next_epic_id(pm_root: &Path) -> String {
    let next = direct_record_stems(&pm_root.join("epics"))
        .iter()
        .filter_map(|id| sequence(id, "EPC-"))
        .max()
        .unwrap_or(0)
        + 1;
    format!("EPC-{next:03}")
}

/// Next free spec id (`SPC-NNN`, max visible sequence + 1).
#[must_use]
pub fn next_spec_id(pm_root: &Path) -> String {
    let next = direct_record_stems(&pm_root.join("specs"))
        .iter()
        .filter_map(|id| sequence(id, "SPC-"))
        .max()
        .unwrap_or(0)
        + 1;
    format!("SPC-{next:03}")
}

/// Next free independent task id (`TSK-NNN`, max canonical or legacy
/// reservation sequence + 1).
#[must_use]
pub fn next_task_id(pm_root: &Path) -> String {
    let next = task_record_stems(pm_root)
        .iter()
        .filter_map(|id| task_reservation_sequence(id))
        .max()
        .unwrap_or(0)
        + 1;
    format!("TSK-{next:03}")
}

/// Whether a stable work-item id exists in the visible checkout.
#[must_use]
pub fn work_item_exists(pm_root: &Path, id: &str) -> bool {
    let paths = if crate::workgraph::is_valid_epic_format_id(id) {
        crate::workgraph::layout::epic_record_files(pm_root)
    } else if crate::workgraph::is_valid_task_format_id(id) {
        crate::workgraph::layout::task_record_files(pm_root)
    } else {
        return false;
    };
    paths
        .iter()
        .any(|path| record_stem(path).as_deref() == Some(id))
}

/// Whether an epic exists in the visible checkout.
#[must_use]
pub fn epic_exists(pm_root: &Path, id: &str) -> bool {
    crate::workgraph::is_valid_epic_format_id(id) && work_item_exists(pm_root, id)
}

fn today() -> String {
    super::now_rfc3339()[..10].to_string()
}

fn render(template: &str, values: &[(&str, &str)]) -> String {
    let mut context = TemplateContext::new();
    for (key, value) in values {
        context.set(*key, *value);
    }
    context.substitute(template)
}

fn yaml_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

fn record_title(title: &str) -> Result<&str, StoreError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(StoreError::Invalid(
            "record title cannot be empty".to_string(),
        ));
    }
    if title.contains(['\r', '\n']) {
        return Err(StoreError::Invalid(
            "record title must be one line".to_string(),
        ));
    }
    Ok(title)
}

/// Create a new record exclusively, with its folders, refusing a link at
/// any component beneath the repository root (issue 94).
fn write_new(pm_root: &Path, path: &Path, content: &str) -> Result<(), StoreError> {
    let root = repository_root(pm_root);
    let relative = crate::contained::relative_to(root, path)?;
    crate::contained::create_new(root, &relative, content.as_bytes())?;
    Ok(())
}

/// Allocate and scaffold a canonical epic.
///
/// # Errors
///
/// Returns an I/O error when the destination cannot be created or written.
pub fn create_epic(pm_root: &Path, template: &str, title: &str) -> Result<NewRecord, StoreError> {
    create_epic_with(
        pm_root,
        template,
        title,
        &mut checkout_allocator(|| next_epic_id(pm_root)),
    )
}

/// Scaffold a canonical epic with the id and `uid` `allocate` chooses.
///
/// # Errors
///
/// Returns the allocator's error, or an I/O error when the destination
/// cannot be created or written.
pub fn create_epic_with(
    pm_root: &Path,
    template: &str,
    title: &str,
    allocate: &mut Allocator<'_>,
) -> Result<NewRecord, StoreError> {
    let title = record_title(title)?;
    let (id, uid) = allocate(&planning_target(pm_root))?;
    let nnn = id.strip_prefix("EPC-").unwrap_or(id.as_str());
    let date = today();
    let title_yaml = yaml_string(title);
    let content = render(
        template,
        &[
            ("NNN", nnn),
            ("UID", uid.as_str()),
            ("TITLE", title),
            ("TITLE_YAML", title_yaml.as_str()),
            ("DATE", date.as_str()),
        ],
    );
    let path = pm_root.join("epics").join(format!("{id}.md"));
    write_new(pm_root, &path, &content)?;
    Ok(NewRecord { id, uid, path })
}

/// Allocate and scaffold a canonical task.
///
/// Exactly one task relationship is required: an existing `epic_id`, or a
/// non-empty `standalone_reason`.
///
/// # Errors
///
/// Returns an invalid-record error for ambiguous or missing parentage or an
/// empty integration target, a not-found error for a missing epic, or an I/O
/// error when the record cannot be created.
pub fn create_task(
    pm_root: &Path,
    template: &str,
    epic_id: Option<&str>,
    standalone_reason: Option<&str>,
    integration_target: Option<&str>,
    title: &str,
) -> Result<NewRecord, StoreError> {
    create_task_with(
        pm_root,
        template,
        epic_id,
        standalone_reason,
        integration_target,
        title,
        &mut checkout_allocator(|| next_task_id(pm_root)),
    )
}

/// Scaffold a canonical task with the id and `uid` `allocate` chooses,
/// after its parentage and integration target are validated.
///
/// # Errors
///
/// As [`create_task`], plus the allocator's error.
pub fn create_task_with(
    pm_root: &Path,
    template: &str,
    epic_id: Option<&str>,
    standalone_reason: Option<&str>,
    integration_target: Option<&str>,
    title: &str,
    allocate: &mut Allocator<'_>,
) -> Result<NewRecord, StoreError> {
    let title = record_title(title)?;
    match (epic_id, standalone_reason.map(str::trim)) {
        (Some(epic), None | Some("")) if epic_exists(pm_root, epic) => {}
        (Some(epic), None | Some("")) => {
            return Err(StoreError::NotFound(format!("epic:{epic}")));
        }
        (None, Some(reason)) if !reason.is_empty() => {}
        (Some(_), Some(reason)) if !reason.is_empty() => {
            return Err(StoreError::Invalid(
                "task cannot have both epic_id and standalone_reason".to_string(),
            ));
        }
        _ => {
            return Err(StoreError::Invalid(
                "task requires an existing epic or a non-empty standalone reason".to_string(),
            ));
        }
    }

    let date = today();
    let epic_value = epic_id.unwrap_or("null");
    let repository_root = repository_root(pm_root);
    if integration_target.is_some_and(|target| target.trim().is_empty()) {
        return Err(StoreError::Invalid(
            "integration target cannot be empty".to_string(),
        ));
    }
    if integration_target.is_some_and(|target| !super::is_stable_work_target(target)) {
        return Err(StoreError::Invalid(
            "integration target must be a stable non-task branch name".to_string(),
        ));
    }
    let resolved_target = integration_target
        .map(str::trim)
        .map(str::to_owned)
        .or_else(|| crate::workgraph::default_work_target(repository_root))
        .ok_or_else(|| {
            StoreError::Invalid(
                "integration target must resolve to a local or remote-tracking branch".to_string(),
            )
        })?;
    if !super::work_target_resolves(repository_root, &resolved_target) {
        return Err(StoreError::Invalid(format!(
            "integration target '{resolved_target}' does not resolve to a local or remote-tracking branch"
        )));
    }
    let target = resolved_target
        .strip_prefix("origin/")
        .unwrap_or(&resolved_target)
        .to_string();
    let (id, uid) = allocate(&target)?;
    let nnn = id.strip_prefix("TSK-").unwrap_or(id.as_str());
    let target_value = yaml_string(&target);
    let title_yaml = yaml_string(title);
    let reason_value = standalone_reason
        .filter(|reason| !reason.trim().is_empty())
        .map_or_else(|| "null".to_string(), yaml_string);
    let content = render(
        template,
        &[
            ("NNN", nnn),
            ("UID", uid.as_str()),
            ("EPIC_ID", epic_value),
            ("STANDALONE_REASON", reason_value.as_str()),
            ("TITLE", title),
            ("TITLE_YAML", title_yaml.as_str()),
            ("DATE", date.as_str()),
            ("TARGET_BRANCH", target_value.as_str()),
        ],
    );
    let path = pm_root.join("tasks").join(format!("{id}.md"));
    write_new(pm_root, &path, &content)?;
    Ok(NewRecord { id, uid, path })
}

/// Allocate a spec and link it from an existing epic or task.
///
/// # Errors
///
/// As [`create_spec_for`].
pub fn create_spec(
    pm_root: &Path,
    template: &str,
    work_item_id: &str,
    title: &str,
) -> Result<NewRecord, StoreError> {
    create_spec_for(pm_root, template, &[work_item_id.to_string()], title)
}

/// Allocate a spec and link it from every consuming epic or task.
///
/// The consumers own the relationship through their `specs` lists (SPC-013
/// R-65); the spec keeps no consumer list. Every consumer is written in the
/// same change, or none is: a failure restores the ones already written.
///
/// # Errors
///
/// Returns a not-found error when a consuming epic or task does not exist,
/// or an I/O/record error when creation or reference linking fails.
pub fn create_spec_for(
    pm_root: &Path,
    template: &str,
    work_item_ids: &[String],
    title: &str,
) -> Result<NewRecord, StoreError> {
    create_spec_with(
        pm_root,
        template,
        work_item_ids,
        title,
        &mut checkout_allocator(|| next_spec_id(pm_root)),
    )
}

/// Scaffold a spec with the id and `uid` `allocate` chooses and link it
/// from every consuming work item, as [`create_spec_for`] does.
///
/// # Errors
///
/// As [`create_spec_for`], plus the allocator's error.
pub fn create_spec_with(
    pm_root: &Path,
    template: &str,
    work_item_ids: &[String],
    title: &str,
    allocate: &mut Allocator<'_>,
) -> Result<NewRecord, StoreError> {
    let title = record_title(title)?;
    if work_item_ids.is_empty() {
        return Err(StoreError::Invalid(
            "a spec needs at least one consuming epic or task".to_string(),
        ));
    }
    let mut targets: Vec<PathBuf> = Vec::new();
    for id in work_item_ids {
        let target = find_work_item_path(pm_root, id)
            .ok_or_else(|| StoreError::NotFound(format!("work-item:{id}")))?;
        // A consumer named twice is linked once, so its first snapshot is
        // the one a rollback restores.
        if !targets.contains(&target) {
            targets.push(target);
        }
    }
    let (id, uid) = allocate(&planning_target(pm_root))?;
    let nnn = id.strip_prefix("SPC-").unwrap_or(id.as_str());
    let date = today();
    let title_yaml = yaml_string(title);
    let content = render(
        template,
        &[
            ("NNN", nnn),
            ("UID", uid.as_str()),
            ("TITLE", title),
            ("TITLE_YAML", title_yaml.as_str()),
            ("DATE", date.as_str()),
        ],
    );
    let path = pm_root.join("specs").join(format!("{id}.md"));
    let root = repository_root(pm_root);
    let tree = crate::contained::Tree::open(root)?;
    let spec = crate::contained::relative_to(root, &path)?;
    tree.create_new(&spec, content.as_bytes())?;
    // Each consumer's bytes are kept before its write is tried, so a write
    // that fails after its rename is restored too.
    let mut touched: Vec<(String, Vec<u8>)> = Vec::new();
    for target in &targets {
        let linked = crate::contained::relative_to(root, target)
            .map_err(StoreError::from)
            .and_then(|relative| {
                let original = tree.read(&relative, u64::MAX)?;
                touched.push((relative.clone(), original));
                append_spec_reference(&tree, &relative, &id)
            });
        if let Err(error) = linked {
            return Err(restore(&tree, &spec, &touched, error));
        }
    }
    Ok(NewRecord { id, uid, path })
}

/// Put back every consumer touched and remove the new spec; a restore that
/// fails is added to the error, never dropped.
fn restore(
    tree: &crate::contained::Tree,
    spec: &str,
    touched: &[(String, Vec<u8>)],
    error: StoreError,
) -> StoreError {
    let mut failed = Vec::new();
    for (relative, original) in touched {
        if let Err(restore) = tree.write(relative, original) {
            failed.push(format!("{relative} ({restore})"));
        }
    }
    if let Err(remove) = tree.remove(spec) {
        failed.push(format!("{spec} ({remove})"));
    }
    if failed.is_empty() {
        error
    } else {
        StoreError::Invalid(format!(
            "{error}; restoring after it also failed for {}",
            failed.join(", ")
        ))
    }
}

fn find_work_item_path(pm_root: &Path, id: &str) -> Option<PathBuf> {
    let paths = if crate::workgraph::is_valid_epic_format_id(id) {
        crate::workgraph::layout::epic_record_files(pm_root)
    } else if crate::workgraph::is_valid_task_format_id(id) {
        crate::workgraph::layout::task_record_files(pm_root)
    } else {
        return None;
    };
    paths
        .into_iter()
        .find(|path| record_stem(path).as_deref() == Some(id))
}

fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let rest = content
        .strip_prefix("---\r\n")
        .or_else(|| content.strip_prefix("---\n"))?;
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            return Some((&rest[..offset], &rest[offset + line.len()..]));
        }
        offset += line.len();
    }
    None
}

fn append_spec_reference(
    tree: &crate::contained::Tree,
    path: &str,
    spec_id: &str,
) -> Result<(), StoreError> {
    let content = String::from_utf8(tree.read(path, u64::MAX)?)
        .map_err(|error| StoreError::Invalid(format!("{path}: {error}")))?;
    let (yaml, body) = split_frontmatter(&content).ok_or_else(|| StoreError::Yaml {
        path: path.to_string(),
        message: "missing frontmatter delimiters".to_string(),
    })?;
    let data =
        serde_yaml::from_str::<serde_yaml::Mapping>(yaml).map_err(|error| StoreError::Yaml {
            path: path.to_string(),
            message: error.to_string(),
        })?;
    let key = serde_yaml::Value::String("specs".to_string());
    match data.get(&key) {
        Some(serde_yaml::Value::Sequence(values))
            if values.contains(&serde_yaml::Value::String(spec_id.to_string())) =>
        {
            return Ok(());
        }
        Some(serde_yaml::Value::Sequence(_)) | None => {}
        Some(_) => {
            return Err(StoreError::Invalid(format!(
                "{path}: specs must be a YAML list"
            )));
        }
    }

    // Preserve the consumer-owned frontmatter byte-for-byte apart from the
    // `specs` value. Round-tripping a hand-authored mapping through serde_yaml
    // discards comments and ordering, including the scaffold's guidance.
    let updated_yaml = insert_yaml_sequence_value(yaml, "specs", spec_id)?;
    tree.write(path, format!("---\n{updated_yaml}---\n{body}").as_bytes())?;
    Ok(())
}

fn insert_yaml_sequence_value(yaml: &str, key: &str, value: &str) -> Result<String, StoreError> {
    let mut offset = 0;
    for line_with_ending in yaml.split_inclusive('\n') {
        let line = line_with_ending.trim_end_matches(['\r', '\n']);
        let Some(after_key) = line
            .strip_prefix(key)
            .and_then(|rest| rest.strip_prefix(':'))
        else {
            offset += line_with_ending.len();
            continue;
        };

        let line_start = offset;
        let line_end = line_start + line.len();
        let value_start = line_start + key.len() + 1;
        if let Some(open) = after_key.find('[') {
            let Some(close) = after_key[open + 1..].find(']') else {
                return Err(StoreError::Invalid(format!(
                    "{key}: unsupported multiline flow sequence"
                )));
            };
            let open = value_start + open;
            let close = value_start + open.saturating_sub(value_start) + 1 + close;
            let existing = &yaml[open + 1..close];
            let replacement = if existing.trim().is_empty() {
                value.to_string()
            } else {
                format!("{existing}, {value}")
            };
            let mut updated = yaml.to_string();
            updated.replace_range(open + 1..close, &replacement);
            return Ok(updated);
        }

        if after_key.trim().is_empty() || after_key.trim_start().starts_with('#') {
            let newline = if line_with_ending.ends_with("\r\n") {
                "\r\n"
            } else {
                "\n"
            };
            let mut updated = yaml.to_string();
            let insertion = if line_with_ending.ends_with('\n') {
                line_start + line_with_ending.len()
            } else {
                line_end
            };
            let prefix = if line_with_ending.ends_with('\n') {
                ""
            } else {
                newline
            };
            updated.insert_str(insertion, &format!("{prefix}  - {value}{newline}"));
            return Ok(updated);
        }

        return Err(StoreError::Invalid(format!(
            "{key}: unsupported YAML sequence style"
        )));
    }

    let mut updated = yaml.to_string();
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    write!(updated, "{key}:\n  - {value}\n").expect("writing to String cannot fail");
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPIC_TEMPLATE: &str = "---\nid: EPC-{{NNN}}\ntitle: {{TITLE_YAML}}\nstatus: planning\nwork_type: feat\nspecs: []\ncreated: {{DATE}}\n---\n\n# EPC-{{NNN}} — {{TITLE}}\n";
    const TASK_TEMPLATE: &str = "---\nid: TSK-{{NNN}}\nepic_id: {{EPIC_ID}}\nstandalone_reason: {{STANDALONE_REASON}}\ntitle: {{TITLE_YAML}}\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: []\nintegration_target: {{TARGET_BRANCH}}\ncreated: {{DATE}}\n---\n\n# TSK-{{NNN}} — {{TITLE}}\n";
    const SPEC_TEMPLATE: &str = "---\nid: SPC-{{NNN}}\ntitle: {{TITLE_YAML}}\nstatus: draft\ncreated: {{DATE}}\n---\n\n# SPC-{{NNN}} — {{TITLE}}\n";

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-b", "main"]);
        git(dir.path(), &["config", "user.email", "test@example.com"]);
        git(dir.path(), &["config", "user.name", "Test"]);
        fs::write(dir.path().join(".keep"), "").unwrap();
        git(dir.path(), &["add", ".keep"]);
        git(dir.path(), &["commit", "-m", "fixture"]);
        for child in ["epics", "specs", "tasks"] {
            fs::create_dir_all(dir.path().join(child)).unwrap();
        }
        dir
    }

    fn git(root: &Path, args: &[&str]) {
        assert!(
            crate::git::command()
                .arg("-C")
                .arg(root)
                .args(args)
                .status()
                .unwrap()
                .success(),
            "git {args:?}"
        );
    }

    fn seed(root: &Path, dir: &str, id: &str) {
        fs::write(
            root.join(dir).join(format!("{id}.md")),
            format!("---\nid: {id}\n---\n"),
        )
        .unwrap();
    }

    #[test]
    fn independent_allocators_start_at_one() {
        let dir = project();
        assert_eq!(next_epic_id(dir.path()), "EPC-001");
        assert_eq!(next_spec_id(dir.path()), "SPC-001");
        assert_eq!(next_task_id(dir.path()), "TSK-001");
    }

    #[test]
    fn task_allocator_reserves_legacy_namespace() {
        let dir = project();
        seed(dir.path(), "tasks", "TSK-041-008");
        seed(dir.path(), "tasks", "TSK-009");
        assert_eq!(next_task_id(dir.path()), "TSK-042");
    }

    #[test]
    fn creates_linked_and_standalone_tasks_without_format_id() {
        let dir = project();
        create_epic(dir.path(), EPIC_TEMPLATE, "Outcome").unwrap();
        git(
            dir.path(),
            &["branch", "integration/EPC-001-outcome", "main"],
        );

        let linked = create_task(
            dir.path(),
            TASK_TEMPLATE,
            Some("EPC-001"),
            None,
            Some("integration/EPC-001-outcome"),
            "Linked",
        )
        .unwrap();
        let linked_body = fs::read_to_string(linked.path).unwrap();
        assert_eq!(linked.id, "TSK-001");
        assert!(linked_body.contains("epic_id: EPC-001"));
        assert!(linked_body.contains("integration_target: \"integration/EPC-001-outcome\""));
        assert!(linked_body.contains("standalone_reason: null"));
        assert!(!linked_body.contains("format_id"));

        let standalone = create_task(
            dir.path(),
            TASK_TEMPLATE,
            None,
            Some("bounded correction"),
            None,
            "Standalone",
        )
        .unwrap();
        let standalone_body = fs::read_to_string(standalone.path).unwrap();
        assert_eq!(standalone.id, "TSK-002");
        assert!(standalone_body.contains("epic_id: null"));
        assert!(standalone_body.contains("bounded correction"));
    }

    #[test]
    fn rejects_ambiguous_or_unjustified_task_parentage() {
        let dir = project();
        create_epic(dir.path(), EPIC_TEMPLATE, "Outcome").unwrap();
        assert!(matches!(
            create_task(
                dir.path(),
                TASK_TEMPLATE,
                Some("EPC-001"),
                Some("also standalone"),
                None,
                "bad"
            ),
            Err(StoreError::Invalid(_))
        ));
        assert!(matches!(
            create_task(
                dir.path(),
                TASK_TEMPLATE,
                Some("EPC-001"),
                None,
                Some("refs/heads/task/TSK-001-self"),
                "self-authorizing"
            ),
            Err(StoreError::Invalid(_))
        ));
        assert!(matches!(
            create_task(dir.path(), TASK_TEMPLATE, None, None, None, "bad"),
            Err(StoreError::Invalid(_))
        ));
        assert!(matches!(
            create_task(
                dir.path(),
                TASK_TEMPLATE,
                Some("EPC-999"),
                None,
                None,
                "missing"
            ),
            Err(StoreError::NotFound(_))
        ));
        assert!(matches!(
            create_task(
                dir.path(),
                TASK_TEMPLATE,
                Some("EPC-001"),
                None,
                Some("  "),
                "empty target"
            ),
            Err(StoreError::Invalid(_))
        ));
    }

    #[test]
    fn spec_creation_links_the_consuming_work_item() {
        let dir = project();
        let epic = create_epic(dir.path(), EPIC_TEMPLATE, "Outcome").unwrap();
        let spec = create_spec(dir.path(), SPEC_TEMPLATE, "EPC-001", "Interface contract").unwrap();
        assert_eq!(spec.id, "SPC-001");
        let epic_body = fs::read_to_string(epic.path).unwrap();
        assert!(epic_body.contains("specs: [SPC-001]"), "{epic_body}");
        assert!(!fs::read_to_string(spec.path).unwrap().contains("epic_id:"));

        git(
            dir.path(),
            &[
                "update-ref",
                "refs/remotes/origin/integration/EPC-001-outcome",
                "HEAD",
            ],
        );
        let task = create_task(
            dir.path(),
            TASK_TEMPLATE,
            Some("EPC-001"),
            None,
            Some("origin/integration/EPC-001-outcome"),
            "Work",
        )
        .unwrap();
        let task_body = fs::read_to_string(&task.path).unwrap();
        assert!(task_body.contains("integration_target: \"integration/EPC-001-outcome\""));
        create_spec(dir.path(), SPEC_TEMPLATE, "TSK-001", "Task contract").unwrap();
        assert!(fs::read_to_string(task.path)
            .unwrap()
            .contains("specs: [SPC-002]"));
        assert!(matches!(
            create_spec(dir.path(), SPEC_TEMPLATE, "TSK-999", "Missing"),
            Err(StoreError::NotFound(_))
        ));
        assert!(matches!(
            create_spec(dir.path(), SPEC_TEMPLATE, "not-an-id", "Malformed"),
            Err(StoreError::NotFound(_))
        ));
    }

    #[test]
    fn spec_linking_preserves_frontmatter_comments_and_order() {
        let dir = project();
        let commented_epic = EPIC_TEMPLATE.replace(
            "specs: []",
            "specs: [] # consumer-owned references; keep this guidance",
        );
        let epic = create_epic(dir.path(), &commented_epic, "Outcome").unwrap();
        create_spec(dir.path(), SPEC_TEMPLATE, "EPC-001", "Contract").unwrap();

        let after = fs::read_to_string(epic.path).unwrap();
        assert!(
            after.contains("specs: [SPC-001] # consumer-owned references; keep this guidance"),
            "{after}"
        );
        assert!(
            after.find("status:").unwrap() < after.find("specs:").unwrap(),
            "unrelated frontmatter ordering changed: {after}"
        );
    }

    #[test]
    fn yaml_sequence_insertion_preserves_block_and_missing_key_shapes() {
        let block = "id: EPC-001\nspecs: # linked contracts\n  - SPC-001\ncreated: now\n";
        let block = insert_yaml_sequence_value(block, "specs", "SPC-002").unwrap();
        assert_eq!(
            block,
            "id: EPC-001\nspecs: # linked contracts\n  - SPC-002\n  - SPC-001\ncreated: now\n"
        );

        let missing = insert_yaml_sequence_value("id: EPC-001\n", "specs", "SPC-001").unwrap();
        assert_eq!(missing, "id: EPC-001\nspecs:\n  - SPC-001\n");
    }

    #[test]
    fn invalid_work_item_ids_do_not_scan_records() {
        let dir = project();
        seed(dir.path(), "epics", "EPC-001");
        seed(dir.path(), "tasks", "TSK-001");
        assert!(work_item_exists(dir.path(), "EPC-001"));
        assert!(work_item_exists(dir.path(), "TSK-001"));
        assert!(!work_item_exists(dir.path(), "SPC-001"));
        assert!(!work_item_exists(dir.path(), "not-an-id"));
    }

    #[test]
    fn exclusive_creation_never_overwrites() {
        let dir = project();
        let path = dir.path().join("tasks/TSK-001.md");
        write_new(dir.path(), &path, "first").unwrap();
        assert!(write_new(dir.path(), &path, "second").is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "first");
    }

    #[test]
    fn record_titles_are_one_line_and_yaml_safe() {
        let dir = project();
        let epic = create_epic(dir.path(), EPIC_TEMPLATE, "Account: recovery #1").unwrap();
        let body = fs::read_to_string(epic.path).unwrap();
        assert!(body.contains("title: \"Account: recovery #1\""));
        assert!(body.contains("# EPC-001 — Account: recovery #1"));

        assert!(matches!(
            create_epic(dir.path(), EPIC_TEMPLATE, " \n "),
            Err(StoreError::Invalid(_))
        ));
        assert!(matches!(
            create_epic(dir.path(), EPIC_TEMPLATE, "line one\nline two"),
            Err(StoreError::Invalid(_))
        ));
    }

    /// Every file beneath `dir`, by relative path, read without following.
    #[cfg(unix)]
    fn snapshot(dir: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
        let mut out = std::collections::BTreeMap::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(current) = stack.pop() {
            for entry in fs::read_dir(&current).unwrap().flatten() {
                let path = entry.path();
                let kind = entry.file_type().unwrap();
                if kind.is_dir() {
                    stack.push(path.clone());
                }
                let bytes = if kind.is_file() {
                    fs::read(&path).unwrap()
                } else {
                    Vec::new()
                };
                out.insert(path.strip_prefix(dir).unwrap().to_path_buf(), bytes);
            }
        }
        out
    }

    /// A project whose `project-management` holds the three kind folders,
    /// with `linked` (relative to the project) replaced by a link to
    /// `outside`. Issue 94: a branch can commit such a link.
    #[cfg(unix)]
    fn planted(linked: &str, outside: &Path) -> (tempfile::TempDir, PathBuf) {
        let dir = project();
        let pm = dir.path().join("project-management");
        for child in ["epics", "specs", "tasks"] {
            fs::create_dir_all(pm.join(child)).unwrap();
        }
        let at = dir.path().join(linked);
        fs::remove_dir_all(&at).unwrap();
        std::os::unix::fs::symlink(outside, &at).unwrap();
        (dir, pm)
    }

    #[cfg(unix)]
    #[test]
    fn record_creation_refuses_a_link_above_the_record() {
        for linked in [
            "project-management",
            "project-management/epics",
            "project-management/tasks",
            "project-management/specs",
        ] {
            let outside = tempfile::tempdir().unwrap();
            for child in ["epics", "specs", "tasks"] {
                fs::create_dir_all(outside.path().join(child)).unwrap();
            }
            fs::write(
                outside.path().join("epics/EPC-001.md"),
                "---\nid: EPC-001\nspecs: []\n---\n",
            )
            .unwrap();
            let (dir, pm) = planted(linked, outside.path());
            if !linked.ends_with("epics") && linked != "project-management" {
                fs::write(
                    pm.join("epics/EPC-001.md"),
                    "---\nid: EPC-001\nspecs: []\n---\n",
                )
                .unwrap();
            }
            let before = snapshot(outside.path());
            let epic = create_epic(&pm, EPIC_TEMPLATE, "Outcome");
            let task = create_task(&pm, TASK_TEMPLATE, None, Some("fix"), Some("main"), "Fix");
            let spec = create_spec(&pm, SPEC_TEMPLATE, "EPC-001", "Contract");
            assert_eq!(
                snapshot(outside.path()),
                before,
                "{linked}: nothing outside"
            );
            let refused = |result: &Result<NewRecord, StoreError>| {
                result.as_ref().is_err_and(|error| {
                    error.to_string().contains("symbolic link")
                        || matches!(error, StoreError::NotFound(_))
                })
            };
            match linked {
                "project-management" => {
                    assert!(
                        refused(&epic) && refused(&task) && refused(&spec),
                        "{linked}"
                    );
                }
                "project-management/epics" => {
                    assert!(refused(&epic), "{epic:?}");
                    assert!(task.is_ok(), "{task:?}");
                }
                "project-management/tasks" => {
                    assert!(refused(&task), "{task:?}");
                    assert!(epic.is_ok() && spec.is_ok(), "{epic:?} {spec:?}");
                }
                _ => {
                    assert!(refused(&spec), "{spec:?}");
                    let epic_text = fs::read_to_string(pm.join("epics/EPC-001.md")).unwrap();
                    assert!(!epic_text.contains("SPC-"), "{epic_text}");
                }
            }
            drop(dir);
        }
    }

    /// The old temporary name beside a consumer (`<record>.tmp`) is not
    /// written through: a link planted there is left alone.
    #[cfg(unix)]
    #[test]
    fn spec_linking_never_writes_through_a_planted_temporary_name() {
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("sentinel"), "outside").unwrap();
        let dir = project();
        let pm = dir.path().join("project-management");
        let epic = create_epic(&pm, EPIC_TEMPLATE, "Outcome").unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("sentinel"),
            pm.join("epics/EPC-001.tmp"),
        )
        .unwrap();
        create_spec(&pm, SPEC_TEMPLATE, "EPC-001", "Contract").unwrap();
        assert_eq!(
            fs::read_to_string(outside.path().join("sentinel")).unwrap(),
            "outside"
        );
        let meta = fs::symlink_metadata(&epic.path).unwrap();
        assert!(meta.file_type().is_file(), "the epic stays a regular file");
        assert!(fs::read_to_string(&epic.path)
            .unwrap()
            .contains("specs: [SPC-001]"));
    }

    /// A consumer write that fails part way restores every consumer already
    /// linked, the failing one included even when its rename had landed,
    /// and removes the new spec; a consumer named twice is linked once.
    #[test]
    fn spec_linking_failure_restores_every_consumer() {
        let dir = project();
        let pm = dir.path().join("project-management");
        let epic = create_epic(&pm, EPIC_TEMPLATE, "Outcome").unwrap();
        let task = create_task(&pm, TASK_TEMPLATE, None, Some("fix"), Some("main"), "Fix").unwrap();
        let epic_before = fs::read(&epic.path).unwrap();
        let task_before = fs::read(&task.path).unwrap();
        // The task's write fails after its rename landed: its bytes already
        // name the spec when the error returns.
        crate::contained::fault::arm(
            crate::contained::fault::Point::AfterRename,
            &format!("project-management/tasks/{}.md", task.id),
        );
        let consumers = [
            "EPC-001".to_string(),
            "EPC-001".to_string(),
            task.id.clone(),
        ];
        let result = create_spec_for(&pm, SPEC_TEMPLATE, &consumers, "Contract");
        assert!(result.is_err(), "{result:?}");
        assert_eq!(
            fs::read(&epic.path).unwrap(),
            epic_before,
            "the epic is restored"
        );
        assert_eq!(
            fs::read(&task.path).unwrap(),
            task_before,
            "the task is restored"
        );
        assert_eq!(
            fs::read_dir(pm.join("specs")).unwrap().count(),
            0,
            "the spec is removed"
        );
        let spec = create_spec_for(&pm, SPEC_TEMPLATE, &consumers, "Contract").unwrap();
        let linked = fs::read_to_string(&epic.path).unwrap();
        assert_eq!(linked.matches(spec.id.as_str()).count(), 1, "{linked}");
    }

    #[test]
    fn task_creation_requires_an_existing_integration_branch() {
        let dir = project();
        create_epic(dir.path(), EPIC_TEMPLATE, "Outcome").unwrap();
        assert!(matches!(
            create_task(
                dir.path(),
                TASK_TEMPLATE,
                Some("EPC-001"),
                None,
                Some("integration/EPC-001-missing"),
                "Work"
            ),
            Err(StoreError::Invalid(message))
                if message.contains("does not resolve")
        ));
    }
}
