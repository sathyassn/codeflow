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
use std::io::Write;
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
    next: impl Fn() -> Result<String, StoreError>,
) -> impl FnMut(&str) -> Result<(String, String), StoreError> {
    move |_| Ok((next()?, crate::ids::new_uid()))
}

pub(crate) fn planning_target(pm_root: &Path) -> Result<String, StoreError> {
    Ok(
        crate::workgraph::default_work_target(repository_root(pm_root))
            .map_err(|error| StoreError::Invalid(error.to_string()))?
            .map_or_else(
                || "none".to_string(),
                |target| {
                    target
                        .strip_prefix("origin/")
                        .unwrap_or(&target)
                        .to_string()
                },
            ),
    )
}

fn repository_root(pm_root: &Path) -> &Path {
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

fn direct_record_stems(dir: &Path) -> Result<Vec<String>, StoreError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut stems = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let stem = if path.extension().is_some_and(|extension| extension == "md") {
            record_stem(&path)
        } else if entry.file_type()?.is_dir() {
            path.file_name()
                .and_then(|value| value.to_str())
                .map(str::to_owned)
        } else {
            None
        };
        if let Some(stem) = stem {
            stems.push(stem);
        }
    }
    Ok(stems)
}

fn task_record_stems(pm_root: &Path) -> Result<Vec<String>, StoreError> {
    Ok(crate::workgraph::layout::task_record_files(pm_root)?
        .iter()
        .filter_map(|path| record_stem(path))
        .collect())
}

/// Next free epic id (`EPC-NNN`, max visible sequence + 1).
///
/// # Errors
///
/// Returns an error if the epic directory or one of its entries cannot be read.
pub fn next_epic_id(pm_root: &Path) -> Result<String, StoreError> {
    let next = direct_record_stems(&pm_root.join("epics"))?
        .iter()
        .filter_map(|id| sequence(id, "EPC-"))
        .max()
        .unwrap_or(0)
        + 1;
    Ok(format!("EPC-{next:03}"))
}

/// Next free spec id (`SPC-NNN`, max visible sequence + 1).
///
/// # Errors
///
/// Returns an error if the spec directory or one of its entries cannot be read.
pub fn next_spec_id(pm_root: &Path) -> Result<String, StoreError> {
    let next = direct_record_stems(&pm_root.join("specs"))?
        .iter()
        .filter_map(|id| sequence(id, "SPC-"))
        .max()
        .unwrap_or(0)
        + 1;
    Ok(format!("SPC-{next:03}"))
}

/// Next free independent task id (`TSK-NNN`, max canonical or legacy
/// reservation sequence + 1).
///
/// # Errors
///
/// Returns an error if a task inventory directory or entry cannot be read.
pub fn next_task_id(pm_root: &Path) -> Result<String, StoreError> {
    let next = task_record_stems(pm_root)?
        .iter()
        .filter_map(|id| task_reservation_sequence(id))
        .max()
        .unwrap_or(0)
        + 1;
    Ok(format!("TSK-{next:03}"))
}

/// Whether a stable work-item id exists in the visible checkout.
///
/// # Errors
///
/// Returns an error if the work-item inventory cannot be read.
pub fn work_item_exists(pm_root: &Path, id: &str) -> Result<bool, StoreError> {
    let paths = if crate::workgraph::is_valid_epic_format_id(id) {
        crate::workgraph::layout::epic_record_files(pm_root)?
    } else if crate::workgraph::is_valid_task_format_id(id) {
        crate::workgraph::layout::task_record_files(pm_root)?
    } else {
        return Ok(false);
    };
    Ok(paths
        .iter()
        .any(|path| record_stem(path).as_deref() == Some(id)))
}

/// Whether an epic exists in the visible checkout.
///
/// # Errors
///
/// Returns an error if the epic inventory cannot be read.
pub fn epic_exists(pm_root: &Path, id: &str) -> Result<bool, StoreError> {
    Ok(crate::workgraph::is_valid_epic_format_id(id) && work_item_exists(pm_root, id)?)
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

fn write_new(path: &Path, content: &str) -> Result<(), StoreError> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    if let Err(error) = file.write_all(content.as_bytes()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(StoreError::Io(error));
    }
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
    let (id, uid) = allocate(&planning_target(pm_root)?)?;
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
    let dir = pm_root.join("epics");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{id}.md"));
    write_new(&path, &content)?;
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
        (Some(epic), None | Some("")) if epic_exists(pm_root, epic)? => {}
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
    if integration_target.is_some_and(str::is_empty) {
        return Err(StoreError::Invalid(
            "integration target cannot be empty".to_string(),
        ));
    }
    if integration_target.is_some_and(|target| !super::is_stable_work_target(target)) {
        return Err(StoreError::Invalid(
            "integration target must be a stable non-task branch name".to_string(),
        ));
    }
    let resolved_target = match integration_target {
        Some(target) => Some(target.to_owned()),
        None => crate::workgraph::default_work_target(repository_root)
            .map_err(|error| StoreError::Invalid(error.to_string()))?,
    }
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
    let dir = pm_root.join("tasks");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{id}.md"));
    write_new(&path, &content)?;
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
    let targets = work_item_ids
        .iter()
        .map(|id| {
            find_work_item_path(pm_root, id)?
                .ok_or_else(|| StoreError::NotFound(format!("work-item:{id}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let (id, uid) = allocate(&planning_target(pm_root)?)?;
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
    let dir = pm_root.join("specs");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{id}.md"));
    write_new(&path, &content)?;
    let mut written: Vec<(&PathBuf, String)> = Vec::new();
    for target in &targets {
        let original = match fs::read_to_string(target) {
            Ok(original) => original,
            Err(error) => {
                restore(&path, &written);
                return Err(error.into());
            }
        };
        if let Err(error) = append_spec_reference(target, &id) {
            restore(&path, &written);
            return Err(error);
        }
        written.push((target, original));
    }
    Ok(NewRecord { id, uid, path })
}

fn restore(spec: &Path, written: &[(&PathBuf, String)]) {
    for (target, original) in written {
        let _ = crate::file_lock::atomic_write(target, original.as_bytes());
    }
    let _ = fs::remove_file(spec);
}

fn find_work_item_path(pm_root: &Path, id: &str) -> Result<Option<PathBuf>, StoreError> {
    let paths = if crate::workgraph::is_valid_epic_format_id(id) {
        crate::workgraph::layout::epic_record_files(pm_root)?
    } else if crate::workgraph::is_valid_task_format_id(id) {
        crate::workgraph::layout::task_record_files(pm_root)?
    } else {
        return Ok(None);
    };
    Ok(paths
        .into_iter()
        .find(|path| record_stem(path).as_deref() == Some(id)))
}

fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let rest = content
        .strip_prefix("---\r\n")
        .or_else(|| content.strip_prefix("---\n"))?;
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if super::record_text::without_line_ending(line) == "---" {
            return Some((&rest[..offset], &rest[offset + line.len()..]));
        }
        offset += line.len();
    }
    None
}

fn append_spec_reference(path: &Path, spec_id: &str) -> Result<(), StoreError> {
    let content = fs::read_to_string(path)?;
    let (yaml, body) = split_frontmatter(&content).ok_or_else(|| StoreError::Yaml {
        path: path.display().to_string(),
        message: "missing frontmatter delimiters".to_string(),
    })?;
    let data =
        serde_yaml::from_str::<serde_yaml::Mapping>(yaml).map_err(|error| StoreError::Yaml {
            path: path.display().to_string(),
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
                "{}: specs must be a YAML list",
                path.display()
            )));
        }
    }

    // Preserve the consumer-owned frontmatter byte-for-byte apart from the
    // `specs` value. Round-tripping a hand-authored mapping through serde_yaml
    // discards comments and ordering, including the scaffold's guidance.
    let updated_yaml = insert_yaml_sequence_value(yaml, "specs", spec_id)?;
    crate::file_lock::atomic_write(path, format!("---\n{updated_yaml}---\n{body}").as_bytes())?;
    Ok(())
}

fn insert_yaml_sequence_value(yaml: &str, key: &str, value: &str) -> Result<String, StoreError> {
    let mut offset = 0;
    for line_with_ending in yaml.split_inclusive('\n') {
        let line = super::record_text::without_line_ending(line_with_ending);
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
            let replacement = if existing.trim_matches([' ', '\t']).is_empty() {
                value.to_string()
            } else {
                format!("{existing}, {value}")
            };
            let mut updated = yaml.to_string();
            updated.replace_range(open + 1..close, &replacement);
            return Ok(updated);
        }

        if after_key.trim_matches([' ', '\t']).is_empty()
            || after_key.trim_start_matches([' ', '\t']).starts_with('#')
        {
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
        assert_eq!(next_epic_id(dir.path()).unwrap(), "EPC-001");
        assert_eq!(next_spec_id(dir.path()).unwrap(), "SPC-001");
        assert_eq!(next_task_id(dir.path()).unwrap(), "TSK-001");
    }

    #[test]
    fn task_allocator_reserves_legacy_namespace() {
        let dir = project();
        seed(dir.path(), "tasks", "TSK-041-008");
        seed(dir.path(), "tasks", "TSK-009");
        assert_eq!(next_task_id(dir.path()).unwrap(), "TSK-042");
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
        assert!(work_item_exists(dir.path(), "EPC-001").unwrap());
        assert!(work_item_exists(dir.path(), "TSK-001").unwrap());
        assert!(!work_item_exists(dir.path(), "SPC-001").unwrap());
        assert!(!work_item_exists(dir.path(), "not-an-id").unwrap());
    }

    #[test]
    fn exclusive_creation_never_overwrites() {
        let dir = project();
        let path = dir.path().join("tasks/TSK-001.md");
        write_new(&path, "first").unwrap();
        assert!(write_new(&path, "second").is_err());
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
