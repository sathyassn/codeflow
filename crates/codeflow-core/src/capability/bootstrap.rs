//! Resumable capability inventory work, using the project's record allocator.

use std::path::Path;

use fs2::FileExt;

use crate::contained::{relative_to, Tree};
use crate::workgraph::{layout, NewRecord};

pub const GUIDE: &str = ".codeflow/rules/capabilities.md";
pub const CHECKLIST: &str = "docs/capability-bootstrap.md";
const PENDING: &str = ".codeflow/capability-bootstrap.pending";
const MAX_RECORD_BYTES: u64 = 1024 * 1024;
const EPIC_MARKER: &str = "<!-- codeflow:capability-bootstrap v1 epic -->";
const CHECKLIST_MARKER: &str = "<!-- codeflow:capability-bootstrap v1 checklist -->";

/// Ordered outputs, shared by tracked work and the tracking-free checklist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Inventory,
    Classification,
    Evidence,
    Review,
}

impl Stage {
    pub const ALL: [Self; 4] = [
        Self::Inventory,
        Self::Classification,
        Self::Evidence,
        Self::Review,
    ];

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Inventory => "inventory",
            Self::Classification => "classification",
            Self::Evidence => "evidence",
            Self::Review => "review",
        }
    }

    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::Inventory => "Inventory the project's capabilities",
            Self::Classification => "Classify the capability inventory",
            Self::Evidence => "Record capability evidence and gaps",
            Self::Review => "Review the capability catalog for completeness",
        }
    }

    fn marker(self) -> String {
        format!("<!-- codeflow:capability-bootstrap v1 {} -->", self.name())
    }
}

/// Templates loaded from the shipped assets by the CLI.
pub struct Templates {
    pub epic: String,
    pub task: String,
    pub epic_body: String,
    pub stages: [String; 4],
}

/// One allocation request; the CLI uses the same path as epic/task new.
pub struct Creation<'a> {
    pub stage: Option<Stage>,
    pub epic: Option<&'a str>,
    pub template: String,
    pub title: &'static str,
}

/// What an adopter can resume after this invocation.
#[derive(Debug)]
pub struct Progress {
    pub epic: Option<String>,
    pub next: Option<Stage>,
    pub checklist: bool,
    pub inventory_complete: bool,
}

/// Whether the catalog has a nonempty, explicitly complete inventory.
///
/// # Errors
/// Returns catalog parsing and vocabulary errors instead of masking them.
pub fn complete(root: &Path) -> Result<bool, String> {
    let catalog = super::load_capability_catalog(root).map_err(|error| error.to_string())?;
    Ok(complete_for(&catalog))
}

/// Inventory completeness for a catalog the caller already loaded.
#[must_use]
pub fn complete_for(catalog: &super::CapabilityCatalog) -> bool {
    !catalog.records.is_empty()
        && catalog
            .vocabulary
            .as_ref()
            .is_some_and(|v| v.inventory == "complete")
}

/// Offer for a catalog the caller already loaded; never changes project state.
#[must_use]
pub fn offer_for(catalog: &super::CapabilityCatalog) -> Option<&'static str> {
    (!complete_for(catalog)).then_some("capability inventory incomplete; run `codeflow capability bootstrap` to start or resume inventory, classification, evidence and review")
}

/// Offer only; never allocates records or changes the project's policy.
#[must_use]
pub fn offer(root: &Path) -> Option<&'static str> {
    super::load_capability_catalog(root)
        .ok()
        .and_then(|catalog| offer_for(&catalog))
}

/// Read an installed bootstrap body before falling back to its shipped copy.
///
/// # Errors
/// Refuses unknown template names, links, oversized files and invalid UTF-8.
pub fn load_body(root: &Path, name: &str, embedded: &str) -> Result<String, String> {
    if name != "epic" && !Stage::ALL.iter().any(|stage| stage.name() == name) {
        return Err("unknown capability bootstrap template".into());
    }
    let tree = Tree::open(root).map_err(|error| error.to_string())?;
    let path = format!(".codeflow/templates/capability-bootstrap/{name}.md");
    Ok(read_optional(&tree, &path)?.unwrap_or_else(|| embedded.to_string()))
}

#[derive(Debug)]
struct Existing {
    id: String,
    status: String,
    epic: Option<String>,
    dependencies: Vec<String>,
}

fn frontmatter(text: &str) -> Result<serde_yaml::Value, String> {
    let yaml = text
        .strip_prefix("---\n")
        .and_then(|s| s.split_once("\n---").map(|(yaml, _)| yaml))
        .ok_or("bootstrap record has no frontmatter")?;
    serde_yaml::from_str(yaml).map_err(|error| error.to_string())
}

fn existing(
    tree: &Tree,
    paths: &[std::path::PathBuf],
    marker: &str,
) -> Result<Option<Existing>, String> {
    let mut found = None;
    for path in paths {
        let relative = relative_to(tree.path(), path).map_err(|error| error.to_string())?;
        let bytes = tree
            .read(&relative, MAX_RECORD_BYTES)
            .map_err(|error| error.to_string())?;
        let text = std::str::from_utf8(&bytes).map_err(|error| error.to_string())?;
        if !text.lines().any(|line| line == marker) {
            continue;
        }
        if found.is_some() {
            return Err(format!(
                "multiple records carry {marker}; reconcile them before resuming"
            ));
        }
        let value = frontmatter(text)?;
        let id = value["id"]
            .as_str()
            .ok_or("bootstrap record has no id")?
            .to_string();
        let expected_kind = if marker == EPIC_MARKER {
            crate::ids::Kind::Epc
        } else {
            crate::ids::Kind::Tsk
        };
        if crate::ids::RegId::parse(&id).is_none_or(|parsed| parsed.kind() != expected_kind)
            || path.file_stem().and_then(|name| name.to_str()) != Some(id.as_str())
        {
            return Err(format!(
                "bootstrap record {relative} has an inconsistent id"
            ));
        }
        let status = value["status"]
            .as_str()
            .ok_or("bootstrap record has no status")?
            .to_string();
        let epic = value["epic_id"].as_str().map(str::to_string);
        let dependencies = crate::workgraph::deps::parse_dependencies(value.get("depends_on"))?
            .into_iter()
            .map(|edge| edge.id)
            .collect();
        if !matches!(
            status.as_str(),
            "draft" | "planning" | "in_progress" | "todo" | "blocked" | "complete"
        ) {
            return Err(format!(
                "bootstrap record {id} has status {status}; reconcile it before resuming"
            ));
        }
        found = Some(Existing {
            id,
            status,
            epic,
            dependencies,
        });
    }
    Ok(found)
}

/// Replace the standard sections, keeping project-specific sections intact.
fn with_body(template: &str, body: &str) -> String {
    let mut result = template.to_string();
    for section in body
        .split("\n## ")
        .filter(|s| !s.trim_matches([' ', '\t', '\r', '\n']).is_empty())
    {
        let section = section.strip_prefix("## ").unwrap_or(section);
        let (name, content) = section.split_once('\n').unwrap_or((section, ""));
        let heading = format!("\n## {name}\n");
        if let Some(start) = result.find(&heading) {
            let content_start = start + heading.len();
            let end = result[content_start..]
                .find("\n## ")
                .map_or(result.len(), |n| content_start + n);
            result.replace_range(
                content_start..end,
                &format!("\n{}\n", content.trim_matches(['\r', '\n'])),
            );
        } else {
            result.push_str(&heading);
            result.push('\n');
            result.push_str(content.trim_matches(['\r', '\n']));
            result.push('\n');
        }
    }
    result
}

fn read_optional(tree: &Tree, path: &str) -> Result<Option<String>, String> {
    match tree.read(path, MAX_RECORD_BYTES) {
        Ok(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|error| error.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn existing_work(tree: &Tree) -> Result<(Option<Existing>, Vec<Option<Existing>>), String> {
    let files = layout::bounded_record_files(&tree.path().join("project-management"), 20_000)
        .map_err(|_| "cannot inventory bootstrap work records".to_string())?;
    let epic = existing(tree, &files.epics, EPIC_MARKER)?;
    let mut stages = Vec::new();
    for stage in Stage::ALL {
        stages.push(existing(tree, &files.tasks, &stage.marker())?);
    }
    // Preflight the entire existing graph before making any new allocation.
    let mut previous = None;
    let mut missing = epic.is_none();
    for record in &stages {
        if let Some(record) = record {
            if missing
                || record.epic.as_deref() != epic.as_ref().map(|e| e.id.as_str())
                || record.dependencies != previous.iter().cloned().collect::<Vec<String>>()
            {
                return Err(format!("bootstrap record {} has inconsistent membership or dependencies; reconcile before resuming", record.id));
            }
            previous = Some(record.id.clone());
        } else {
            missing = true;
        }
    }
    if let Some(pending) = read_optional(tree, PENDING)? {
        let recovered = epic
            .as_ref()
            .is_some_and(|e| pending == EPIC_MARKER && !e.id.is_empty())
            || Stage::ALL
                .iter()
                .zip(&stages)
                .any(|(stage, record)| pending == stage.marker() && record.is_some());
        if !recovered {
            return Err(format!("bootstrap allocation was interrupted before its record was confirmed ({pending}); recover its registry reservation using the allocation diagnostic before removing {PENDING} and rerunning; this visible crash marker is intentionally not ignored so the interrupted allocation is not lost; no new id was allocated"));
        }
        tree.remove(PENDING).map_err(|error| error.to_string())?;
    }
    Ok((epic, stages))
}

/// Create one epic and four tasks, or a checklist where tracking is absent.
/// Markers are written in the allocation itself, so a crash after a record write
/// resumes that record. An interrupted reservation with no record refuses new
/// allocation until the existing reservation is recovered.
///
/// # Errors
/// Refuses invalid catalogs, unsafe paths, concurrent invocations, ambiguous
/// prior work, changed dependency edges and allocation failures.
pub fn run(
    root: &Path,
    tracking: bool,
    templates: &Templates,
    mut create: impl FnMut(Creation<'_>) -> Result<NewRecord, String>,
) -> Result<Progress, String> {
    let tree = Tree::open(root).map_err(|error| error.to_string())?;
    let lock = tree
        .lock_file(".codeflow/capability-bootstrap.lock")
        .map_err(|error| error.to_string())?;
    lock.try_lock_exclusive()
        .map_err(|_| "capability bootstrap is already running in this checkout".to_string())?;
    if complete(root)? {
        return Ok(Progress {
            epic: None,
            next: None,
            checklist: false,
            inventory_complete: true,
        });
    }
    if !tracking {
        return checklist(&tree, templates);
    }
    if read_optional(&tree, CHECKLIST)?
        .as_deref()
        .is_some_and(|text| text.lines().any(|line| line == CHECKLIST_MARKER))
    {
        return Err(format!("{CHECKLIST} already records bootstrap progress; finish or reconcile that checklist by hand before removing it and starting tracked bootstrap work; no epic or task was created"));
    }
    let (mut epic, mut stages) = existing_work(&tree)?;
    if epic.is_none() {
        tree.write(PENDING, EPIC_MARKER.as_bytes())
            .map_err(|error| error.to_string())?;
        let template = format!(
            "{}\n{EPIC_MARKER}\n",
            with_body(&templates.epic, &templates.epic_body)
        );
        let record = create(Creation {
            stage: None,
            epic: None,
            template,
            title: "Build the initial capability catalog",
        })?;
        epic = Some(Existing {
            id: record.id,
            status: "draft".into(),
            epic: None,
            dependencies: Vec::new(),
        });
        tree.remove(PENDING).map_err(|error| error.to_string())?;
    }
    let epic_id = &epic.as_ref().ok_or("bootstrap epic missing")?.id;
    let mut previous = None;
    let mut next = None;
    for (index, stage) in Stage::ALL.into_iter().enumerate() {
        if stages[index].is_none() {
            tree.write(PENDING, stage.marker().as_bytes())
                .map_err(|error| error.to_string())?;
            let body = templates.stages[index].replace(
                "(serves bootstrap AC-1)",
                &format!("(serves {epic_id} AC-1)"),
            );
            let template = with_body(&templates.task, &body);
            let dependencies = previous
                .as_ref()
                .map_or_else(|| "[]".to_string(), |id: &String| format!("[{id}]"));
            let template = crate::workgraph::status_verb::set_frontmatter_value(
                &template,
                "depends_on",
                &dependencies,
            )?;
            let template = format!("{template}\n{}\n", stage.marker());
            let record = create(Creation {
                stage: Some(stage),
                epic: Some(epic_id),
                template,
                title: stage.title(),
            })?;
            stages[index] = Some(Existing {
                id: record.id,
                status: "todo".into(),
                epic: Some(epic_id.clone()),
                dependencies: previous.iter().cloned().collect(),
            });
            tree.remove(PENDING).map_err(|error| error.to_string())?;
        }
        let record = stages[index].as_ref().ok_or("bootstrap task missing")?;
        if next.is_none() && record.status != "complete" {
            next = Some(stage);
        }
        previous = Some(record.id.clone());
    }
    Ok(Progress {
        epic: Some(epic_id.clone()),
        next,
        checklist: false,
        inventory_complete: false,
    })
}

fn checklist(tree: &Tree, templates: &Templates) -> Result<Progress, String> {
    let text = if let Some(text) = read_optional(tree, CHECKLIST)? {
        if !text.lines().any(|line| line == CHECKLIST_MARKER) {
            return Err(format!("{CHECKLIST} already exists and is not a bootstrap checklist; move it before retrying"));
        }
        text
    } else {
        let mut text = format!("# Initial capability catalog\n\n{CHECKLIST_MARKER}\n\nTracking is not enabled. Complete these stages in order using {GUIDE}.\n\n");
        for (index, stage) in Stage::ALL.into_iter().enumerate() {
            text.push_str("- [ ] ");
            text.push_str(stage.name());
            text.push_str("\n\n### ");
            text.push_str(stage.name());
            text.push_str("\n\n");
            text.push_str(&templates.stages[index].replace("(serves bootstrap AC-1)", ""));
            text.push_str("\n\n");
        }
        tree.create_new(CHECKLIST, text.as_bytes())
            .map_err(|error| error.to_string())?;
        text
    };
    let next = Stage::ALL.into_iter().find(|stage| {
        !text
            .lines()
            .any(|line| line == format!("- [x] {}", stage.name()))
    });
    Ok(Progress {
        epic: None,
        next,
        checklist: true,
        inventory_complete: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workgraph::allocate;

    fn templates() -> Templates {
        Templates {
            epic: include_str!("../../../../assets/base/pm/epic.md.tmpl").into(),
            task: include_str!("../../../../assets/base/pm/task.md.tmpl").into(),
            epic_body: include_str!("../../../../assets/base/pm/capability-bootstrap/epic.md")
                .into(),
            stages: [
                include_str!("../../../../assets/base/pm/capability-bootstrap/inventory.md").into(),
                include_str!("../../../../assets/base/pm/capability-bootstrap/classification.md")
                    .into(),
                include_str!("../../../../assets/base/pm/capability-bootstrap/evidence.md").into(),
                include_str!("../../../../assets/base/pm/capability-bootstrap/review.md").into(),
            ],
        }
    }

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let repository = git2::Repository::init(dir.path()).unwrap();
        let tree_id = repository.treebuilder(None).unwrap().write().unwrap();
        let tree = repository.find_tree(tree_id).unwrap();
        let signature =
            git2::Signature::now("Bootstrap fixture", "bootstrap@example.test").unwrap();
        repository
            .commit(
                Some("refs/heads/main"),
                &signature,
                &signature,
                "chore: seed",
                &tree,
                &[],
            )
            .unwrap();
        repository.set_head("refs/heads/main").unwrap();
        dir
    }

    fn create(root: &Path, creation: &Creation<'_>) -> Result<NewRecord, String> {
        let pm = root.join("project-management");
        if creation.stage.is_some() {
            allocate::create_task(
                &pm,
                &creation.template,
                creation.epic,
                None,
                Some("main"),
                creation.title,
            )
        } else {
            allocate::create_epic(&pm, &creation.template, creation.title)
        }
        .map_err(|error| error.to_string())
    }

    #[test]
    fn interrupted_after_record_write_recovers_without_allocating_a_duplicate() {
        let dir = repo();
        let root = dir.path();
        let templates = templates();
        let mut writes = 0;
        let result = run(root, true, &templates, |creation| {
            let record = create(root, &creation)?;
            writes += 1;
            if writes == 3 {
                Err("interrupted after classification write".into())
            } else {
                Ok(record)
            }
        });
        assert!(result.unwrap_err().contains("interrupted"));
        let progress = run(root, true, &templates, |creation| create(root, &creation)).unwrap();
        assert_eq!(progress.epic.as_deref(), Some("EPC-001"));
        assert_eq!(progress.next, Some(Stage::Inventory));
        let files = layout::bounded_record_files(&root.join("project-management"), 100).unwrap();
        assert_eq!(files.epics.len(), 1);
        assert_eq!(files.tasks.len(), 4);
        assert!(!root.join(PENDING).exists());
    }

    #[test]
    fn interrupted_unconfirmed_reservation_refuses_to_allocate_again() {
        let dir = repo();
        let root = dir.path();
        let templates = templates();
        assert!(run(root, true, &templates, |_| Err("reservation failed".into())).is_err());
        let result = run(root, true, &templates, |_| {
            panic!("must not allocate another id")
        });
        assert!(result
            .unwrap_err()
            .contains("recover its registry reservation"));
    }

    #[test]
    fn changed_dependency_or_duplicate_marker_refuses_before_allocating() {
        let dir = repo();
        let root = dir.path();
        let templates = templates();
        run(root, true, &templates, |creation| create(root, &creation)).unwrap();
        let path = root.join("project-management/tasks/TSK-002.md");
        let original = std::fs::read_to_string(&path).unwrap();
        std::fs::write(
            &path,
            original.replace("depends_on: [TSK-001]", "depends_on: []"),
        )
        .unwrap();
        assert!(run(root, true, &templates, |_| panic!("must not allocate"))
            .unwrap_err()
            .contains("inconsistent membership or dependencies"));
        std::fs::write(&path, original).unwrap();
        std::fs::copy(
            root.join("project-management/tasks/TSK-001.md"),
            root.join("project-management/tasks/TSK-099.md"),
        )
        .unwrap();
        assert!(run(root, true, &templates, |_| panic!("must not allocate"))
            .unwrap_err()
            .contains("multiple records"));
    }

    #[test]
    fn checklist_collision_is_preserved_and_invalid_catalog_is_not_hidden() {
        let dir = repo();
        let root = dir.path();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join(CHECKLIST), "Project-owned notes").unwrap();
        assert!(run(root, false, &templates(), |_| panic!("no tracking"))
            .unwrap_err()
            .contains("already exists"));
        assert_eq!(
            std::fs::read_to_string(root.join(CHECKLIST)).unwrap(),
            "Project-owned notes"
        );
        std::fs::create_dir_all(root.join("docs/capabilities")).unwrap();
        std::fs::write(
            root.join("docs/capabilities/vocabulary.yaml"),
            "inventory: unknown\n",
        )
        .unwrap();
        assert!(complete(root).is_err());
        assert!(offer(root).is_none());
    }

    #[test]
    fn project_specific_template_sections_survive_bootstrap_content() {
        let template = format!(
            "{}\n## Project review convention\n\nKeep the sample review evidence.\n",
            templates().task
        );
        let text = with_body(&template, &templates().stages[0]);
        assert!(text.contains("## Project review convention\n\nKeep the sample review evidence."));
        assert!(text.contains("Inventory the project"));
        assert!(!text.contains("<!-- One paragraph:"));
    }

    #[cfg(unix)]
    #[test]
    fn linked_output_is_refused_without_writing_outside_the_checkout() {
        let dir = repo();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("docs")).unwrap();
        assert!(run(dir.path(), false, &templates(), |_| panic!("no allocation")).is_err());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    }

    #[test]
    fn installed_bodies_override_embedded_defaults_and_invalid_reads_refuse() {
        let dir = repo();
        let root = dir.path();
        assert_eq!(
            load_body(root, "review", "shipped body").unwrap(),
            "shipped body"
        );
        let folder = root.join(".codeflow/templates/capability-bootstrap");
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("review.md");
        std::fs::write(&path, "project review convention").unwrap();
        assert_eq!(
            load_body(root, "review", "shipped body").unwrap(),
            "project review convention"
        );
        std::fs::write(path, [0xff]).unwrap();
        assert!(load_body(root, "review", "shipped body").is_err());
        assert!(load_body(root, "../../outside", "shipped body").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn installed_body_links_are_refused_instead_of_using_fallbacks() {
        let dir = repo();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("review.md");
        std::fs::write(&target, "outside body").unwrap();
        let folder = dir.path().join(".codeflow/templates/capability-bootstrap");
        std::fs::create_dir_all(&folder).unwrap();
        std::os::unix::fs::symlink(&target, folder.join("review.md")).unwrap();
        assert!(load_body(dir.path(), "review", "shipped body").is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "outside body");
        std::fs::remove_file(target).unwrap();
        assert!(load_body(dir.path(), "review", "shipped body").is_err());
    }

    #[test]
    fn template_sections_preserve_indentation_and_non_ascii_spacing() {
        let body = "## Description\n\n    \u{a0}Meaningful spacing\u{a0}\n";
        let rendered = with_body(&templates().task, body);
        assert!(rendered.contains("\n\n    \u{a0}Meaningful spacing\u{a0}\n"));
    }

    #[test]
    fn pending_marker_requires_exact_writer_bytes() {
        let dir = repo();
        let root = dir.path();
        let templates = templates();
        run(root, true, &templates, |creation| create(root, &creation)).unwrap();
        let pending = format!("\u{a0}{EPIC_MARKER}\u{a0}");
        std::fs::write(root.join(PENDING), &pending).unwrap();
        let result = run(root, true, &templates, |_| panic!("must not allocate"));
        assert!(result
            .unwrap_err()
            .contains("recover its registry reservation"));
        assert_eq!(
            std::fs::read_to_string(root.join(PENDING)).unwrap(),
            pending
        );
    }
}
