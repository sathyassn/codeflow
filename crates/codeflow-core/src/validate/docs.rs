//! Referential-integrity lint over the doc graph (`validate --docs`).
//!
//! Charter §5 maintenance matrix: malformed records and dangling graph IDs
//! fail loud in CI and at pre-push. `verified_by` must be nonempty for shipped
//! capabilities; the validator does not infer whether a named test tag exists.
//! Checks:
//!
//! * capability `epics[]` resolve to `project-management/epics/` files
//! * capability `adrs[]` resolve to `docs/decisions/` ADR files
//! * capability status legal; shipped capabilities have nonempty `verified_by`
//! * ADR `status` legal (`proposed|accepted|superseded`); superseded ADRs
//!   carry `superseded_by`
//! * capability `epics[]` and epic `capabilities[]` links are reciprocal;
//!   epic `adrs[]` references resolve
//! * epic/task `specs[]` resolve to specification records
//! * task `epic_id` resolves, or an explicit standalone reason exists
//! * task `depends_on[]` references resolve and form an acyclic graph
//!
//! Tier-graceful: an absent layer (no registry, no decisions dir, no
//! project-management) skips its checks with a note — only references that
//! point INTO a present layer and miss are errors.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::capability::{parse_capabilities, CAPABILITY_STATUS_VALUES};
use crate::validate::{get_string_field, parse_frontmatter};
use crate::workgraph::{is_valid_epic_format_id, is_valid_spec_format_id, is_valid_task_format_id};

/// Legal ADR status values (charter §5; ADR template).
pub const ADR_STATUS_VALUES: &[&str] = &["proposed", "accepted", "superseded"];

/// A blocking docs-lint finding, addressable as `file:line`.
#[derive(Debug, Clone)]
pub struct DocsLintIssue {
    /// Repo-relative file path.
    pub file: PathBuf,
    /// 1-based line number (best effort; 1 when unknown).
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for DocsLintIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.file.display(), self.line, self.message)
    }
}

/// Result of the docs lint.
#[derive(Debug, Default)]
pub struct DocsLintReport {
    pub issues: Vec<DocsLintIssue>,
    /// Skipped-layer notes (absent tiers), each with the step that clears it.
    pub notes: Vec<crate::remedy::Finding>,
}

impl DocsLintReport {
    /// `true` when no blocking issue was found.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }
}

/// What exists in the repo, gathered once. Each set holds resolvable ids;
/// `None` means that layer is absent (its checks are skipped with a note).
struct DocGraph {
    /// Capability ids present in the registry.
    capabilities: Option<BTreeSet<String>>,
    /// ADR ids resolvable in `docs/decisions/`.
    adrs: Option<BTreeSet<String>>,
    /// Epic format ids resolvable in `project-management/epics/`.
    epics: Option<BTreeSet<String>>,
    /// Spec ids resolvable in `project-management/specs/`.
    specs: Option<BTreeSet<String>>,
}

struct TaskGraphIndex {
    graph: BTreeMap<String, Vec<String>>,
    files_by_id: BTreeMap<String, PathBuf>,
    dependency_lines_by_id: BTreeMap<String, usize>,
}

/// Run the docs-integrity lint over a repository root.
#[must_use]
pub fn lint_docs(repo_root: &Path) -> DocsLintReport {
    let mut report = DocsLintReport::default();

    let graph = build_graph(repo_root, &mut report);

    lint_capabilities(repo_root, &graph, &mut report);
    lint_adrs(repo_root, &mut report);
    lint_epics(repo_root, &graph, &mut report);
    lint_capability_epic_reciprocity(repo_root, &mut report);
    lint_tasks(repo_root, &graph, &mut report);

    report
}

fn lint_capability_epic_reciprocity(repo_root: &Path, report: &mut DocsLintReport) {
    let capabilities_path = repo_root.join("docs/capabilities.md");
    let Ok(content) = std::fs::read_to_string(&capabilities_path) else {
        return;
    };
    let (entries, _) = parse_capabilities(&content);
    let capability_epics: BTreeMap<String, (BTreeSet<String>, usize)> = entries
        .into_iter()
        .map(|entry| (entry.id, (entry.epics.into_iter().collect(), entry.line)))
        .collect();
    let mut epic_capabilities = BTreeMap::<String, (BTreeSet<String>, PathBuf)>::new();
    for path in crate::workgraph::layout::epic_record_files(&repo_root.join("project-management")) {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok((data, _)) = parse_frontmatter(&bytes) else {
            continue;
        };
        let format_id = get_string_field(&data, "format_id");
        let epic_id = if is_valid_epic_format_id(&format_id) {
            format_id
        } else {
            get_string_field(&data, "id")
        };
        if epic_id.is_empty() {
            continue;
        }
        let relative = path.strip_prefix(repo_root).unwrap_or(&path).to_path_buf();
        epic_capabilities.insert(
            epic_id,
            (
                string_list(&data, "capabilities").into_iter().collect(),
                relative,
            ),
        );
    }

    for (capability, (epics, line)) in &capability_epics {
        for epic in epics {
            if let Some((backlinks, _)) = epic_capabilities.get(epic) {
                if !backlinks.contains(capability) {
                    report.issues.push(DocsLintIssue {
                        file: PathBuf::from("docs/capabilities.md"),
                        line: *line,
                        message: format!(
                            "{capability} links {epic}, but that epic does not link back in capabilities[]"
                        ),
                    });
                }
            }
        }
    }
    for (epic, (capabilities, path)) in epic_capabilities {
        for capability in capabilities {
            if let Some((backlinks, _)) = capability_epics.get(&capability) {
                if !backlinks.contains(&epic) {
                    report.issues.push(DocsLintIssue {
                        file: path.clone(),
                        line: 1,
                        message: format!(
                            "{epic} links {capability}, but that capability does not link back in epics[]"
                        ),
                    });
                }
            }
        }
    }
}

fn build_graph(repo_root: &Path, report: &mut DocsLintReport) -> DocGraph {
    // Capability ids.
    let caps_path = repo_root.join("docs/capabilities.md");
    let capabilities = if let Ok(content) = std::fs::read_to_string(&caps_path) {
        let (entries, _) = parse_capabilities(&content);
        Some(entries.into_iter().map(|e| e.id).collect())
    } else {
        report.notes.push(crate::remedy::Finding::new(
            "docs/capabilities.md absent — capability checks skipped",
            crate::remedy::DOCS_LAYER_ABSENT.with(&[("path", "docs/capabilities.md")]),
        ));
        None
    };

    // ADR ids: from frontmatter `id` and from the filename stem prefix
    // (ADR-0001-stack-choice.md resolves ADR-0001).
    let decisions = repo_root.join("docs/decisions");
    let adrs = if decisions.is_dir() {
        let mut ids = BTreeSet::new();
        for path in adr_files(&decisions) {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                ids.insert(stem.to_string());
                if let Some(short) = adr_id_prefix(stem) {
                    ids.insert(short);
                }
            }
            if let Ok(content) = std::fs::read(&path) {
                if let Ok((data, _)) = parse_frontmatter(&content) {
                    let id = get_string_field(&data, "id");
                    if !id.is_empty() {
                        ids.insert(id);
                    }
                }
            }
        }
        Some(ids)
    } else {
        report.notes.push(crate::remedy::Finding::new(
            "docs/decisions/ absent — ADR checks skipped",
            crate::remedy::DOCS_LAYER_ABSENT.with(&[("path", "docs/decisions/")]),
        ));
        None
    };

    // Epic ids from project-management/epics file stems.
    let epics_dir = repo_root.join("project-management/epics");
    let epics = if epics_dir.is_dir() {
        Some(
            crate::workgraph::layout::epic_record_files(&repo_root.join("project-management"))
                .iter()
                .filter_map(|p| p.file_stem().and_then(|s| s.to_str()))
                .map(ToString::to_string)
                .collect(),
        )
    } else {
        report.notes.push(crate::remedy::Finding::new(
            "project-management/epics/ absent — epic reference checks skipped",
            crate::remedy::DOCS_LAYER_ABSENT.with(&[("path", "project-management/epics/")]),
        ));
        None
    };

    let specs_dir = repo_root.join("project-management/specs");
    let specs = if specs_dir.is_dir() {
        Some(
            crate::workgraph::layout::spec_record_files(&repo_root.join("project-management"))
                .iter()
                .filter_map(|path| path.file_stem().and_then(|stem| stem.to_str()))
                .map(str::to_owned)
                .collect(),
        )
    } else {
        report.notes.push(crate::remedy::Finding::new(
            "project-management/specs/ absent — spec reference checks skipped",
            crate::remedy::DOCS_LAYER_ABSENT.with(&[("path", "project-management/specs/")]),
        ));
        None
    };

    DocGraph {
        capabilities,
        adrs,
        epics,
        specs,
    }
}

fn lint_capabilities(repo_root: &Path, graph: &DocGraph, report: &mut DocsLintReport) {
    let rel = PathBuf::from("docs/capabilities.md");
    let Ok(content) = std::fs::read_to_string(repo_root.join(&rel)) else {
        return; // absence already noted
    };

    let (entries, parse_issues) = parse_capabilities(&content);
    for issue in parse_issues {
        report.issues.push(DocsLintIssue {
            file: rel.clone(),
            line: issue.line,
            message: issue.message,
        });
    }

    let mut seen_capabilities = BTreeSet::new();
    for entry in entries {
        if !seen_capabilities.insert(entry.id.clone()) {
            report.issues.push(DocsLintIssue {
                file: rel.clone(),
                line: entry.line,
                message: format!("{}: duplicate capability id", entry.id),
            });
        }
        // Status legality.
        if !CAPABILITY_STATUS_VALUES.contains(&entry.status.as_str()) {
            report.issues.push(DocsLintIssue {
                file: rel.clone(),
                line: entry.line,
                message: format!(
                    "{}: status \"{}\" is not one of: {}",
                    entry.id,
                    entry.status,
                    CAPABILITY_STATUS_VALUES.join(", ")
                ),
            });
        }

        // Shipped capabilities must carry verification evidence.
        if entry.status == "shipped" && entry.verified_by.iter().all(|t| t.trim().is_empty()) {
            report.issues.push(DocsLintIssue {
                file: rel.clone(),
                line: entry.line,
                message: format!(
                    "{}: shipped capability has empty verified_by — name the test tags that prove it",
                    entry.id
                ),
            });
        }

        // epics[] resolve to project-management epic files.
        if let Some(epic_ids) = &graph.epics {
            for epic_ref in &entry.epics {
                if !epic_ids.contains(epic_ref) {
                    report.issues.push(DocsLintIssue {
                        file: rel.clone(),
                        line: entry.line,
                        message: format!(
                            "{}: epics[] references {epic_ref} but no epic file exists at project-management/epics/{epic_ref}.md or project-management/epics/{epic_ref}/{epic_ref}.md",
                            entry.id
                        ),
                    });
                }
            }
        }

        // adrs[] resolve to docs/decisions files.
        if let Some(adr_ids) = &graph.adrs {
            for adr_ref in &entry.adrs {
                if !adr_ids.contains(adr_ref) {
                    report.issues.push(DocsLintIssue {
                        file: rel.clone(),
                        line: entry.line,
                        message: format!(
                            "{}: adrs[] references {adr_ref} but no matching file exists in docs/decisions/",
                            entry.id
                        ),
                    });
                }
            }
        }
    }
}

fn lint_adrs(repo_root: &Path, report: &mut DocsLintReport) {
    let decisions = repo_root.join("docs/decisions");
    if !decisions.is_dir() {
        return; // absence already noted
    }

    for path in adr_files(&decisions) {
        let rel = path.strip_prefix(repo_root).unwrap_or(&path).to_path_buf();
        let Ok(content) = std::fs::read(&path) else {
            continue;
        };
        let Ok((data, _)) = parse_frontmatter(&content) else {
            report.issues.push(DocsLintIssue {
                file: rel,
                line: 1,
                message: "ADR has no parseable YAML frontmatter".to_string(),
            });
            continue;
        };

        let status = get_string_field(&data, "status");
        let status_line = find_line(&content, "status:").unwrap_or(1);

        if status.is_empty() {
            report.issues.push(DocsLintIssue {
                file: rel,
                line: 1,
                message: "ADR frontmatter is missing status".to_string(),
            });
            continue;
        }

        if !ADR_STATUS_VALUES.contains(&status.as_str()) {
            report.issues.push(DocsLintIssue {
                file: rel,
                line: status_line,
                message: format!(
                    "ADR status \"{status}\" is not one of: {}",
                    ADR_STATUS_VALUES.join(", ")
                ),
            });
            continue;
        }

        if status == "superseded" {
            let superseded_by = get_string_field(&data, "superseded_by");
            if superseded_by.is_empty() {
                report.issues.push(DocsLintIssue {
                    file: rel,
                    line: status_line,
                    message: "superseded ADR has no superseded_by — name the superseding ADR id"
                        .to_string(),
                });
            }
        }
    }
}

fn lint_epics(repo_root: &Path, graph: &DocGraph, report: &mut DocsLintReport) {
    let epics_dir = repo_root.join("project-management/epics");
    if !epics_dir.is_dir() {
        return; // absence already noted
    }

    for path in crate::workgraph::layout::epic_record_files(&repo_root.join("project-management")) {
        let rel = path.strip_prefix(repo_root).unwrap_or(&path).to_path_buf();
        let Ok(content) = std::fs::read(&path) else {
            continue;
        };
        let Ok((data, _)) = parse_frontmatter(&content) else {
            continue; // frontmatter shape is `validate`'s job, not the lint's
        };

        // capabilities[] resolve back into the registry.
        if let Some(capability_ids) = &graph.capabilities {
            for cap_ref in string_list(&data, "capabilities") {
                if !capability_ids.contains(&cap_ref) {
                    report.issues.push(DocsLintIssue {
                        file: rel.clone(),
                        line: find_line(&content, &cap_ref).unwrap_or(1),
                        message: format!(
                            "epic references capability {cap_ref} which is not in docs/capabilities.md"
                        ),
                    });
                }
            }
        }

        // adrs[] resolve to decisions files.
        if let Some(adr_ids) = &graph.adrs {
            for adr_ref in string_list(&data, "adrs") {
                if !adr_ids.contains(&adr_ref) {
                    report.issues.push(DocsLintIssue {
                        file: rel.clone(),
                        line: find_line(&content, &adr_ref).unwrap_or(1),
                        message: format!(
                            "epic references {adr_ref} but no matching file exists in docs/decisions/"
                        ),
                    });
                }
            }
        }

        if let Some(spec_ids) = &graph.specs {
            for spec_ref in string_list(&data, "specs") {
                if !spec_ids.contains(&spec_ref) {
                    report.issues.push(DocsLintIssue {
                        file: rel.clone(),
                        line: find_line(&content, &spec_ref).unwrap_or(1),
                        message: format!(
                            "epic references {spec_ref} but no matching specification exists"
                        ),
                    });
                }
            }
        }
    }
}

#[derive(Debug)]
struct TaskGraphRecord {
    file: PathBuf,
    id: String,
    epic_id: Option<String>,
    standalone_reason: Option<String>,
    specs: Vec<String>,
    depends_on: Vec<String>,
    dependency_line: usize,
}

fn lint_tasks(repo_root: &Path, graph: &DocGraph, report: &mut DocsLintReport) {
    let files = crate::workgraph::layout::task_record_files(&repo_root.join("project-management"));
    if files.is_empty() {
        if !repo_root.join("project-management/tasks").is_dir() {
            report.notes.push(crate::remedy::Finding::new(
                "project-management/tasks/ absent — task graph checks skipped",
                crate::remedy::DOCS_LAYER_ABSENT.with(&[("path", "project-management/tasks/")]),
            ));
        }
        return;
    }

    let records: Vec<_> = files
        .iter()
        .filter_map(|path| parse_task_graph_record(repo_root, path, report))
        .collect();
    lint_task_graph_records(&records, graph, report);
}

fn parse_task_graph_record(
    repo_root: &Path,
    path: &Path,
    report: &mut DocsLintReport,
) -> Option<TaskGraphRecord> {
    let rel = path.strip_prefix(repo_root).unwrap_or(path).to_path_buf();
    let content = std::fs::read(path).ok()?;
    let (data, _) = parse_frontmatter(&content).ok()?; // general validation owns malformed YAML

    let id = stable_record_id(&data, is_valid_task_format_id);
    if id.is_empty() {
        report.issues.push(DocsLintIssue {
            file: rel,
            line: 1,
            message: "task has no supported TSK-NNN or legacy TSK-NNN-NNN identity".to_string(),
        });
        return None;
    }

    let id_line = find_line(&content, "id:").unwrap_or(1);
    let file_stem = path.file_stem().and_then(|stem| stem.to_str());
    if file_stem != Some(id.as_str()) {
        report.issues.push(DocsLintIssue {
            file: rel.clone(),
            line: id_line,
            message: format!(
                "task id {id} does not match filename {}.md",
                file_stem.unwrap_or("<non-utf8>")
            ),
        });
    }

    let canonical = data.get("depends_on");
    let legacy = data.get("dependencies");
    if canonical.is_some() && legacy.is_some() {
        report.issues.push(DocsLintIssue {
            file: rel,
            line: find_line(&content, "dependencies:").unwrap_or(1),
            message: "task defines both depends_on and legacy dependencies — keep only depends_on"
                .to_string(),
        });
        return None;
    }

    let (field, value) = canonical
        .map(|value| ("depends_on", value))
        .or_else(|| legacy.map(|value| ("dependencies", value)))
        .unzip();
    let dependency_line = field
        .and_then(|field| find_line(&content, &format!("{field}:")))
        .unwrap_or(1);
    let field_name = field.unwrap_or("depends_on");
    let depends_on = match crate::workgraph::deps::parse_dependencies(value) {
        Ok(deps) => deps.into_iter().map(|dep| dep.id).collect(),
        Err(error) => {
            report.issues.push(DocsLintIssue {
                file: rel.clone(),
                line: dependency_line,
                message: format!("task {id} {field_name}: {error}"),
            });
            return None;
        }
    };

    Some(TaskGraphRecord {
        file: rel,
        id,
        epic_id: optional_string(&data, "epic_id"),
        standalone_reason: optional_string(&data, "standalone_reason"),
        specs: string_list(&data, "specs"),
        depends_on,
        dependency_line,
    })
}

fn lint_task_graph_records(
    records: &[TaskGraphRecord],
    doc_graph: &DocGraph,
    report: &mut DocsLintReport,
) {
    let index = index_task_records(records, report);
    for record in records {
        if index.files_by_id.get(&record.id) != Some(&record.file) {
            continue;
        }
        lint_task_parent(record, doc_graph, report);
        lint_task_specs(record, doc_graph, report);
        lint_task_dependencies(record, &index.graph, report);
    }
    report_task_cycle(
        &index.graph,
        &index.files_by_id,
        &index.dependency_lines_by_id,
        report,
    );
}

fn index_task_records(records: &[TaskGraphRecord], report: &mut DocsLintReport) -> TaskGraphIndex {
    let mut graph = BTreeMap::new();
    let mut files_by_id: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut dependency_lines_by_id = BTreeMap::new();
    for record in records {
        if let Some(first) = files_by_id.get(&record.id) {
            report.issues.push(DocsLintIssue {
                file: record.file.clone(),
                line: 1,
                message: format!(
                    "duplicate task id {} also appears in {}",
                    record.id,
                    first.display()
                ),
            });
            continue;
        }
        files_by_id.insert(record.id.clone(), record.file.clone());
        dependency_lines_by_id.insert(record.id.clone(), record.dependency_line);
        graph.insert(record.id.clone(), record.depends_on.clone());
    }
    TaskGraphIndex {
        graph,
        files_by_id,
        dependency_lines_by_id,
    }
}

fn lint_task_parent(record: &TaskGraphRecord, doc_graph: &DocGraph, report: &mut DocsLintReport) {
    match record.epic_id.as_deref() {
        Some(epic_id) => {
            if !is_valid_epic_format_id(epic_id)
                || doc_graph
                    .epics
                    .as_ref()
                    .is_some_and(|ids| !ids.contains(epic_id))
            {
                report.issues.push(DocsLintIssue {
                    file: record.file.clone(),
                    line: 1,
                    message: format!(
                        "task {} references epic {epic_id}, but no matching epic exists",
                        record.id
                    ),
                });
            }
            if record
                .standalone_reason
                .as_deref()
                .is_some_and(|reason| !reason.trim().is_empty())
            {
                report.issues.push(DocsLintIssue {
                    file: record.file.clone(),
                    line: 1,
                    message: format!("task {} has both epic_id and standalone_reason", record.id),
                });
            }
        }
        None => {
            if record
                .standalone_reason
                .as_deref()
                .is_none_or(|reason| reason.trim().is_empty())
            {
                report.issues.push(DocsLintIssue {
                    file: record.file.clone(),
                    line: 1,
                    message: format!("standalone task {} requires standalone_reason", record.id),
                });
            }
        }
    }
}

fn lint_task_specs(record: &TaskGraphRecord, doc_graph: &DocGraph, report: &mut DocsLintReport) {
    if let Some(spec_ids) = &doc_graph.specs {
        for spec_id in &record.specs {
            if !is_valid_spec_format_id(spec_id) || !spec_ids.contains(spec_id) {
                report.issues.push(DocsLintIssue {
                    file: record.file.clone(),
                    line: 1,
                    message: format!(
                        "task {} references missing specification {spec_id}",
                        record.id
                    ),
                });
            }
        }
    }
}

fn lint_task_dependencies(
    record: &TaskGraphRecord,
    graph: &BTreeMap<String, Vec<String>>,
    report: &mut DocsLintReport,
) {
    let mut seen = BTreeSet::new();
    for dependency in &record.depends_on {
        let message = if !seen.insert(dependency) {
            Some(format!(
                "task {} repeats dependency {dependency}",
                record.id
            ))
        } else if dependency == &record.id {
            Some(format!("task {} depends on itself", record.id))
        } else if !graph.contains_key(dependency) {
            Some(format!(
                "task {} depends on {dependency}, but no task with that id exists",
                record.id
            ))
        } else {
            None
        };
        if let Some(message) = message {
            report.issues.push(DocsLintIssue {
                file: record.file.clone(),
                line: record.dependency_line,
                message,
            });
        }
    }
}

fn report_task_cycle(
    graph: &BTreeMap<String, Vec<String>>,
    files_by_id: &BTreeMap<String, PathBuf>,
    dependency_lines_by_id: &BTreeMap<String, usize>,
    report: &mut DocsLintReport,
) {
    if let Some(cycle) = find_task_cycle(graph) {
        let first = cycle.first().expect("cycle is nonempty");
        report.issues.push(DocsLintIssue {
            file: files_by_id
                .get(first)
                .cloned()
                .unwrap_or_else(|| PathBuf::from("project-management/tasks")),
            line: dependency_lines_by_id.get(first).copied().unwrap_or(1),
            message: format!("task dependency cycle: {}", cycle.join(" -> ")),
        });
    }
}

fn find_task_cycle(graph: &BTreeMap<String, Vec<String>>) -> Option<Vec<String>> {
    let mut finished = BTreeSet::new();

    for root in graph.keys() {
        if finished.contains(root) {
            continue;
        }
        let mut stack = vec![(
            root.clone(),
            graph.get(root).cloned().unwrap_or_default().into_iter(),
        )];
        let mut path = vec![root.clone()];
        let mut positions = BTreeMap::from([(root.clone(), 0_usize)]);

        while let Some((node, dependencies)) = stack.last_mut() {
            let Some(dependency) = dependencies.next() else {
                let completed = node.clone();
                stack.pop();
                path.pop();
                positions.remove(&completed);
                finished.insert(completed);
                continue;
            };
            if !graph.contains_key(&dependency) || dependency == *node {
                continue; // dangling/self edges receive their own diagnostics
            }
            if let Some(start) = positions.get(&dependency).copied() {
                let mut cycle = path[start..].to_vec();
                cycle.push(dependency);
                return Some(cycle);
            }
            if finished.contains(&dependency) {
                continue;
            }
            positions.insert(dependency.clone(), path.len());
            path.push(dependency.clone());
            stack.push((
                dependency.clone(),
                graph
                    .get(&dependency)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter(),
            ));
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// `ADR-0001-stack-choice` → `ADR-0001` (the id portion of a stem).
fn adr_id_prefix(stem: &str) -> Option<String> {
    let rest = stem.strip_prefix("ADR-")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    Some(format!("ADR-{digits}"))
}

/// Markdown files named `ADR-*` in a directory, sorted.
fn adr_files(dir: &Path) -> Vec<PathBuf> {
    md_files(dir)
        .into_iter()
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("ADR-"))
        })
        .collect()
}

/// All `.md` files directly in a directory, sorted.
fn md_files(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|ext| ext == "md"))
                .collect()
        })
        .unwrap_or_default();
    paths.sort();
    paths
}

/// 1-based line number of the first line containing `needle`.
fn find_line(content: &[u8], needle: &str) -> Option<usize> {
    let text = String::from_utf8_lossy(content);
    text.lines().position(|l| l.contains(needle)).map(|i| i + 1)
}

fn string_list(
    data: &std::collections::HashMap<String, serde_yaml::Value>,
    key: &str,
) -> Vec<String> {
    match data.get(key) {
        Some(serde_yaml::Value::Sequence(seq)) => seq
            .iter()
            .filter_map(|v| v.as_str().map(ToString::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

fn optional_string(
    data: &std::collections::HashMap<String, serde_yaml::Value>,
    key: &str,
) -> Option<String> {
    let value = get_string_field(data, key);
    (!value.trim().is_empty()).then_some(value)
}

fn stable_record_id(
    data: &std::collections::HashMap<String, serde_yaml::Value>,
    valid: impl Fn(&str) -> bool,
) -> String {
    let id = get_string_field(data, "id");
    if valid(&id) {
        return id;
    }
    let alias = get_string_field(data, "format_id");
    if valid(&alias) {
        alias
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, content: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn capability_block(id: &str, status: &str, epics: &str, adrs: &str, verified: &str) -> String {
        format!(
            "## {id} — thing\n\n```yaml\nid: {id}\nname: thing\narea: engine\nstatus: {status}\nverified_by: {verified}\nepics: {epics}\nadrs: {adrs}\n```\n\nProse.\n"
        )
    }

    fn adr(root: &Path, file: &str, id: &str, status: &str, superseded_by: &str) {
        write(
            root,
            &format!("docs/decisions/{file}"),
            &format!(
                "---\nid: {id}\ntitle: t\ndate: 2026-06-11\nstatus: {status}\nsuperseded_by: {superseded_by}\n---\n\n# {id}\n"
            ),
        );
    }

    fn epic_file(root: &Path, format_id: &str, caps: &str, adrs: &str) {
        write(
            root,
            &format!("project-management/epics/{format_id}.md"),
            &format!(
                "---\nid: {format_id}\ntitle: epic\ncapabilities: {caps}\nadrs: {adrs}\n---\n\n## Intent\n"
            ),
        );
    }

    /// Nested (dogfood) layout: `epics/EPC-001/EPC-001.md`.
    fn nested_epic_file(root: &Path, format_id: &str, caps: &str, adrs: &str) {
        write(
            root,
            &format!("project-management/epics/{format_id}/{format_id}.md"),
            &format!(
                "---\nid: {format_id}\ntitle: epic\ncapabilities: {caps}\nadrs: {adrs}\n---\n\n## Intent\n"
            ),
        );
    }

    fn task_file(root: &Path, format_id: &str, dependencies: &str) {
        write(
            root,
            &format!("project-management/tasks/{format_id}.md"),
            &format!(
                "---\nid: {format_id}\nformat_id: {format_id}\nepic_id: EPC-001\ntitle: task\nstatus: todo\nwork_type: feat\ndepends_on: {dependencies}\ncreated: 2026-07-25\n---\n\n## Acceptance Criteria\n"
            ),
        );
    }

    /// A repo where every reference resolves: clean lint.
    fn clean_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "docs/capabilities.md",
            &format!(
                "# caps\n\n{}",
                capability_block(
                    "CAP-001",
                    "shipped",
                    "[EPC-001]",
                    "[ADR-0001]",
                    "[gate-core]"
                )
            ),
        );
        adr(
            root,
            "ADR-0001-stack-choice.md",
            "ADR-0001",
            "accepted",
            "null",
        );
        epic_file(root, "EPC-001", "[CAP-001]", "[ADR-0001]");
        dir
    }

    #[test]
    fn duplicate_capability_ids_are_loud() {
        let dir = clean_repo();
        let path = dir.path().join("docs/capabilities.md");
        let mut registry = std::fs::read_to_string(&path).unwrap();
        registry.push_str(&capability_block(
            "CAP-001",
            "building",
            "[EPC-001]",
            "[ADR-0001]",
            "[]",
        ));
        std::fs::write(path, registry).unwrap();
        let report = lint_docs(dir.path());
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.message == "CAP-001: duplicate capability id"));
    }

    #[test]
    fn clean_graph_lints_clean() {
        let dir = clean_repo();
        let report = lint_docs(dir.path());
        assert!(report.is_clean(), "issues: {:?}", report.issues);
    }

    #[test]
    fn dangling_capability_epic_ref_fails_with_file_line() {
        let dir = clean_repo();
        // CAP-002 references an epic that has no file.
        let registry = std::fs::read_to_string(dir.path().join("docs/capabilities.md")).unwrap();
        write(
            dir.path(),
            "docs/capabilities.md",
            &format!(
                "{registry}\n{}",
                capability_block("CAP-002", "building", "[EPC-999]", "[]", "[]")
            ),
        );

        let report = lint_docs(dir.path());
        assert!(!report.is_clean());
        let issue = report
            .issues
            .iter()
            .find(|i| i.message.contains("EPC-999"))
            .expect("dangling epic ref reported");
        assert_eq!(issue.file, PathBuf::from("docs/capabilities.md"));
        assert!(issue.line > 1, "line should point at the entry: {issue}");
        let rendered = issue.to_string();
        assert!(
            rendered.starts_with("docs/capabilities.md:"),
            "file:line format: {rendered}"
        );
        assert!(rendered.contains("project-management/epics/EPC-999.md"));
    }

    #[test]
    fn nested_epic_layout_resolves_from_capability() {
        // The canonical dogfood layout gives each epic its own directory:
        // epics/EPC-001/EPC-001.md, so tasks/ and specs live alongside it.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "docs/capabilities.md",
            &format!(
                "# caps\n\n{}",
                capability_block(
                    "CAP-001",
                    "shipped",
                    "[EPC-001]",
                    "[ADR-0001]",
                    "[gate-core]"
                )
            ),
        );
        adr(
            root,
            "ADR-0001-stack-choice.md",
            "ADR-0001",
            "accepted",
            "null",
        );
        nested_epic_file(root, "EPC-001", "[CAP-001]", "[ADR-0001]");
        // A sibling task file inside the epic dir must NOT be treated as an epic.
        write(
            root,
            "project-management/epics/EPC-001/tasks/TSK-001-001.md",
            "---\nid: TSK-001-001\nformat_id: TSK-001-001\nepic_id: EPC-001\ntitle: task\nstatus: todo\nwork_type: feat\ndepends_on: []\ncreated: 2026-07-25\n---\n",
        );

        let report = lint_docs(root);
        assert!(
            report.is_clean(),
            "nested epic must resolve; issues: {:?}",
            report.issues
        );
    }

    #[test]
    fn nested_epic_backrefs_are_linted() {
        // Proves lint_epics actually reads the nested epic file, not just flat.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "docs/capabilities.md",
            &format!(
                "# caps\n\n{}",
                capability_block("CAP-001", "building", "[EPC-001]", "[]", "[]")
            ),
        );
        nested_epic_file(root, "EPC-001", "[CAP-404]", "[]");

        let report = lint_docs(root);
        assert!(
            report.issues.iter().any(|i| i.message.contains("CAP-404")
                && i.file.as_path() == Path::new("project-management/epics/EPC-001/EPC-001.md")),
            "nested epic back-ref must be linted; issues: {:?}",
            report.issues
        );
    }

    #[test]
    fn capability_and_epic_links_must_be_reciprocal() {
        let dir = clean_repo();
        epic_file(dir.path(), "EPC-001", "[]", "[ADR-0001]");

        let report = lint_docs(dir.path());
        assert!(report.issues.iter().any(|issue| {
            issue.message.contains("CAP-001 links EPC-001")
                && issue.message.contains("does not link back")
        }));
    }

    #[test]
    fn epic_to_capability_link_requires_the_registry_backlink() {
        let dir = clean_repo();
        let registry = std::fs::read_to_string(dir.path().join("docs/capabilities.md")).unwrap();
        std::fs::write(
            dir.path().join("docs/capabilities.md"),
            registry.replace("epics: [EPC-001]", "epics: []"),
        )
        .unwrap();

        let report = lint_docs(dir.path());
        assert!(report.issues.iter().any(|issue| {
            issue.message.contains("EPC-001 links CAP-001")
                && issue.message.contains("does not link back")
        }));
    }

    #[test]
    fn reciprocal_links_use_the_stable_format_id_for_legacy_epics() {
        let dir = clean_repo();
        let path = dir.path().join("project-management/epics/EPC-001.md");
        let epic = std::fs::read_to_string(&path).unwrap();
        std::fs::write(
            path,
            epic.replace("id: EPC-001", "id: epic-01a\nformat_id: EPC-001"),
        )
        .unwrap();

        let report = lint_docs(dir.path());
        assert!(
            report.is_clean(),
            "stable reciprocal identity should remain clean: {:?}",
            report.issues
        );
    }

    #[test]
    fn flat_and_nested_epic_layouts_coexist() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "docs/capabilities.md",
            &format!(
                "# caps\n\n{}\n{}",
                capability_block("CAP-001", "building", "[EPC-001]", "[]", "[]"),
                capability_block("CAP-002", "building", "[EPC-002]", "[]", "[]"),
            ),
        );
        epic_file(root, "EPC-001", "[CAP-001]", "[]"); // flat
        nested_epic_file(root, "EPC-002", "[CAP-002]", "[]"); // nested

        let report = lint_docs(root);
        assert!(
            report.is_clean(),
            "both epic layouts must resolve; issues: {:?}",
            report.issues
        );
    }

    #[test]
    fn dangling_capability_adr_ref_fails() {
        let dir = clean_repo();
        write(
            dir.path(),
            "docs/capabilities.md",
            &format!(
                "# caps\n\n{}",
                capability_block("CAP-001", "building", "[EPC-001]", "[ADR-0404]", "[]")
            ),
        );
        // Keep the epic back-ref resolvable.
        epic_file(dir.path(), "EPC-001", "[CAP-001]", "[ADR-0001]");

        let report = lint_docs(dir.path());
        assert!(report
            .issues
            .iter()
            .any(|i| i.message.contains("ADR-0404") && i.message.contains("docs/decisions")));
    }

    #[test]
    fn superseded_adr_without_superseded_by_fails() {
        let dir = clean_repo();
        adr(
            dir.path(),
            "ADR-0002-old-way.md",
            "ADR-0002",
            "superseded",
            "null",
        );

        let report = lint_docs(dir.path());
        assert!(!report.is_clean());
        let issue = report
            .issues
            .iter()
            .find(|i| i.message.contains("superseded_by"))
            .expect("missing superseded_by reported");
        assert_eq!(
            issue.file,
            PathBuf::from("docs/decisions/ADR-0002-old-way.md")
        );
        assert!(issue.line > 1, "should point at the status line: {issue}");
    }

    #[test]
    fn superseded_adr_with_superseded_by_passes() {
        let dir = clean_repo();
        adr(
            dir.path(),
            "ADR-0002-old-way.md",
            "ADR-0002",
            "superseded",
            "ADR-0003",
        );
        adr(
            dir.path(),
            "ADR-0003-new-way.md",
            "ADR-0003",
            "accepted",
            "null",
        );

        let report = lint_docs(dir.path());
        assert!(report.is_clean(), "issues: {:?}", report.issues);
    }

    #[test]
    fn illegal_adr_status_fails() {
        let dir = clean_repo();
        adr(
            dir.path(),
            "ADR-0002-bad.md",
            "ADR-0002",
            "rejected",
            "null",
        );

        let report = lint_docs(dir.path());
        assert!(report.issues.iter().any(|i| i.message.contains("rejected")
            && i.message.contains("proposed, accepted, superseded")));
    }

    #[test]
    fn shipped_capability_with_empty_verified_by_fails() {
        let dir = clean_repo();
        write(
            dir.path(),
            "docs/capabilities.md",
            &format!(
                "# caps\n\n{}",
                capability_block("CAP-001", "shipped", "[EPC-001]", "[ADR-0001]", "[]")
            ),
        );
        epic_file(dir.path(), "EPC-001", "[CAP-001]", "[ADR-0001]");

        let report = lint_docs(dir.path());
        assert!(report
            .issues
            .iter()
            .any(|i| i.message.contains("verified_by")));
    }

    #[test]
    fn illegal_capability_status_fails() {
        let dir = clean_repo();
        write(
            dir.path(),
            "docs/capabilities.md",
            &format!(
                "# caps\n\n{}",
                capability_block("CAP-001", "wip", "[EPC-001]", "[]", "[]")
            ),
        );
        epic_file(dir.path(), "EPC-001", "[CAP-001]", "[ADR-0001]");

        let report = lint_docs(dir.path());
        assert!(report.issues.iter().any(|i| i.message.contains("\"wip\"")));
    }

    #[test]
    fn epic_with_dangling_capability_back_ref_fails() {
        let dir = clean_repo();
        epic_file(dir.path(), "EPC-002", "[CAP-404]", "[]");

        let report = lint_docs(dir.path());
        let issue = report
            .issues
            .iter()
            .find(|i| i.message.contains("CAP-404"))
            .expect("dangling back-ref reported");
        assert_eq!(
            issue.file,
            PathBuf::from("project-management/epics/EPC-002.md")
        );
    }

    #[test]
    fn epic_with_dangling_adr_ref_fails() {
        let dir = clean_repo();
        epic_file(dir.path(), "EPC-002", "[]", "[ADR-0777]");

        let report = lint_docs(dir.path());
        assert!(report.issues.iter().any(|i| i.message.contains("ADR-0777")));
    }

    #[test]
    fn absent_tiers_skip_with_notes_and_lint_clean() {
        let dir = tempfile::tempdir().unwrap();
        let report = lint_docs(dir.path());
        assert!(report.is_clean());
        assert_eq!(report.notes.len(), 5, "notes: {:?}", report.notes);
        assert!(report
            .notes
            .iter()
            .any(|n| n.text.contains("capabilities.md")));
        assert!(report.notes.iter().any(|n| n.text.contains("decisions")));
        assert!(report
            .notes
            .iter()
            .any(|n| n.text.contains("project-management")));
    }

    #[test]
    fn missing_pm_dir_skips_epic_resolution() {
        // Registry references an epic, but the PM tier is absent entirely:
        // skip with note, no error (tier-graceful).
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "docs/capabilities.md",
            &format!(
                "# caps\n\n{}",
                capability_block("CAP-001", "building", "[EPC-001]", "[]", "[]")
            ),
        );

        let report = lint_docs(dir.path());
        assert!(report.is_clean(), "issues: {:?}", report.issues);
        assert!(report
            .notes
            .iter()
            .any(|n| n.text.contains("project-management")));
    }

    #[test]
    fn adr_id_prefix_extraction() {
        assert_eq!(
            adr_id_prefix("ADR-0001-stack-choice"),
            Some("ADR-0001".to_string())
        );
        assert_eq!(adr_id_prefix("ADR-0002"), Some("ADR-0002".to_string()));
        assert_eq!(adr_id_prefix("notes"), None);
        assert_eq!(adr_id_prefix("ADR-x"), None);
    }

    #[test]
    fn clean_task_dependency_graph_passes() {
        let dir = clean_repo();
        task_file(dir.path(), "TSK-001-001", "[]");
        task_file(dir.path(), "TSK-001-002", "[TSK-001-001]");
        task_file(dir.path(), "TSK-001-003", "[TSK-001-001, TSK-001-002]");

        let report = lint_docs(dir.path());
        assert!(report.is_clean(), "issues: {:?}", report.issues);
    }

    /// TSK-103 AC-10: a dependency is a bare id or `{id, kind, pin}`; the
    /// object form resolves and joins the cycle check like a bare id, and a
    /// malformed entry is reported.
    #[test]
    fn dependency_kinds_are_linted_like_bare_ids() {
        let dir = clean_repo();
        task_file(dir.path(), "TSK-001-001", "[]");
        task_file(dir.path(), "TSK-001-002", "[TSK-001-001]");
        task_file(
            dir.path(),
            "TSK-001-003",
            "[{id: TSK-001-001, kind: research, pin: 0123abcd}, {id: TSK-001-002, kind: decision}]",
        );
        let report = lint_docs(dir.path());
        assert!(report.is_clean(), "issues: {:?}", report.issues);

        task_file(
            dir.path(),
            "TSK-001-004",
            "[{id: TSK-001-404, kind: research, pin: 0123abcd}]",
        );
        task_file(dir.path(), "TSK-001-005", "[{id: TSK-001-001, kind: code}]");
        let report = lint_docs(dir.path());
        let messages: Vec<&str> = report
            .issues
            .iter()
            .map(|issue| issue.message.as_str())
            .collect();
        assert!(
            messages.iter().any(|m| m.contains("TSK-001-404")),
            "{messages:?}"
        );
        assert!(
            messages.iter().any(|m| m.contains("bare id")),
            "{messages:?}"
        );
    }

    #[test]
    fn nested_task_dependencies_are_linted() {
        let dir = clean_repo();
        task_file(dir.path(), "TSK-001-001", "[]");
        write(
            dir.path(),
            "project-management/epics/EPC-002/tasks/TSK-002-001.md",
            "---\nid: TSK-002-001\nformat_id: TSK-002-001\nepic_id: EPC-002\ntitle: nested task\nstatus: todo\nwork_type: feat\ndepends_on: [TSK-002-404]\ncreated: 2026-07-25\n---\n",
        );

        let report = lint_docs(dir.path());
        let issue = report
            .issues
            .iter()
            .find(|issue| issue.message.contains("TSK-002-404"))
            .expect("nested task dependency reported");
        assert_eq!(
            issue.file,
            PathBuf::from("project-management/epics/EPC-002/tasks/TSK-002-001.md")
        );
    }

    #[cfg(unix)]
    #[test]
    fn task_discovery_does_not_follow_symlinked_epic_directories() {
        use std::os::unix::fs::symlink;

        let dir = clean_repo();
        let external = tempfile::tempdir().unwrap();
        write(
            external.path(),
            "tasks/TSK-999-001.md",
            "---\nid: TSK-999-001\nformat_id: TSK-999-001\nepic_id: EPC-999\ntitle: external task\nstatus: todo\nwork_type: feat\ndepends_on: [TSK-999-404]\ncreated: 2026-07-25\n---\n",
        );
        let epics = dir.path().join("project-management/epics");
        std::fs::create_dir_all(&epics).unwrap();
        symlink(external.path(), epics.join("EPC-999")).unwrap();

        let report = lint_docs(dir.path());
        assert!(
            report
                .issues
                .iter()
                .all(|issue| !issue.message.contains("TSK-999")),
            "followed symlinked task directory: {:?}",
            report.issues
        );
    }

    #[cfg(unix)]
    #[test]
    fn task_discovery_does_not_follow_symlinked_flat_task_directory() {
        use std::os::unix::fs::symlink;

        let dir = clean_repo();
        let external = tempfile::tempdir().unwrap();
        write(
            external.path(),
            "TSK-999-001.md",
            "---\nid: TSK-999-001\nformat_id: TSK-999-001\nepic_id: EPC-999\ntitle: external task\nstatus: todo\nwork_type: feat\ndepends_on: [TSK-999-404]\ncreated: 2026-07-25\n---\n",
        );
        let project_management = dir.path().join("project-management");
        std::fs::create_dir_all(&project_management).unwrap();
        symlink(external.path(), project_management.join("tasks")).unwrap();

        let report = lint_docs(dir.path());
        assert!(
            report
                .issues
                .iter()
                .all(|issue| !issue.message.contains("TSK-999")),
            "followed symlinked flat task directory: {:?}",
            report.issues
        );
    }

    #[cfg(unix)]
    #[test]
    fn task_discovery_does_not_follow_symlinked_nested_task_directory() {
        use std::os::unix::fs::symlink;

        let dir = clean_repo();
        let external = tempfile::tempdir().unwrap();
        write(
            external.path(),
            "TSK-999-001.md",
            "---\nid: TSK-999-001\nformat_id: TSK-999-001\nepic_id: EPC-999\ntitle: external task\nstatus: todo\nwork_type: feat\ndepends_on: [TSK-999-404]\ncreated: 2026-07-25\n---\n",
        );
        let epic = dir.path().join("project-management/epics/EPC-999");
        std::fs::create_dir_all(&epic).unwrap();
        symlink(external.path(), epic.join("tasks")).unwrap();

        let report = lint_docs(dir.path());
        assert!(
            report
                .issues
                .iter()
                .all(|issue| !issue.message.contains("TSK-999")),
            "followed symlinked nested task directory: {:?}",
            report.issues
        );
    }

    #[test]
    fn legacy_task_dependencies_alias_passes() {
        let dir = clean_repo();
        write(
            dir.path(),
            "project-management/tasks/TSK-001-001.md",
            "---\nid: TSK-001-001\nformat_id: TSK-001-001\nepic_id: EPC-001\ntitle: task\nstatus: todo\nwork_type: feat\ndependencies: []\ncreated: 2026-07-25\n---\n",
        );

        let report = lint_docs(dir.path());
        assert!(report.is_clean(), "issues: {:?}", report.issues);
    }

    #[test]
    fn conflicting_dependency_spellings_fail() {
        let dir = clean_repo();
        write(
            dir.path(),
            "project-management/tasks/TSK-001-001.md",
            "---\nid: TSK-001-001\nformat_id: TSK-001-001\nepic_id: EPC-001\ntitle: task\nstatus: todo\nwork_type: feat\ndepends_on: []\ndependencies: []\ncreated: 2026-07-25\n---\n",
        );

        let report = lint_docs(dir.path());
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.message.contains("both depends_on")));
    }

    #[test]
    fn malformed_task_dependencies_fail() {
        let dir = clean_repo();
        write(
            dir.path(),
            "project-management/tasks/TSK-001-001.md",
            "---\nid: TSK-001-001\nformat_id: TSK-001-001\nepic_id: EPC-001\ntitle: task\nstatus: todo\nwork_type: feat\ndepends_on: TSK-001-000\ncreated: 2026-07-25\n---\n",
        );

        let report = lint_docs(dir.path());
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.message.contains("must be a YAML list")));
    }

    #[test]
    fn malformed_task_format_id_fails() {
        let dir = clean_repo();
        write(
            dir.path(),
            "project-management/tasks/not-a-task-id.md",
            "---\nid: not-a-task-id\nformat_id: not-a-task-id\nepic_id: EPC-001\ntitle: task\nstatus: todo\nwork_type: feat\ndepends_on: []\ncreated: 2026-07-25\n---\n",
        );

        let report = lint_docs(dir.path());
        let issue = report
            .issues
            .iter()
            .find(|issue| issue.message.contains("supported TSK-NNN"))
            .expect("malformed task format id reported");
        assert_eq!(
            issue.file,
            PathBuf::from("project-management/tasks/not-a-task-id.md")
        );
        assert_eq!(issue.line, 1);
    }

    #[test]
    fn task_identity_and_reference_errors_fail() {
        let dir = clean_repo();
        task_file(dir.path(), "TSK-001-001", "[TSK-001-001]");
        task_file(dir.path(), "TSK-001-002", "[TSK-001-404]");
        write(
            dir.path(),
            "project-management/tasks/wrong-name.md",
            "---\nid: TSK-001-003\nformat_id: TSK-001-003\nepic_id: EPC-001\ntitle: task\nstatus: todo\nwork_type: feat\ndepends_on: []\ncreated: 2026-07-25\n---\n",
        );
        write(
            dir.path(),
            "project-management/tasks/duplicate.md",
            "---\nid: TSK-001-003\nformat_id: TSK-001-003\nepic_id: EPC-001\ntitle: duplicate\nstatus: todo\nwork_type: feat\ndepends_on: []\ncreated: 2026-07-25\n---\n",
        );

        let report = lint_docs(dir.path());
        for marker in [
            "depends on itself",
            "no task with that id exists",
            "does not match filename",
            "duplicate task id",
        ] {
            assert!(
                report
                    .issues
                    .iter()
                    .any(|issue| issue.message.contains(marker)),
                "missing {marker}: {:?}",
                report.issues
            );
        }
    }

    #[test]
    fn self_dependency_is_not_duplicated_as_a_cycle() {
        let dir = clean_repo();
        task_file(dir.path(), "TSK-001-001", "[TSK-001-001]");

        let report = lint_docs(dir.path());
        assert_eq!(
            report
                .issues
                .iter()
                .filter(|issue| issue.message.contains("depends on itself"))
                .count(),
            1
        );
        assert!(report
            .issues
            .iter()
            .all(|issue| !issue.message.contains("task dependency cycle")));
    }

    #[test]
    fn task_dependency_cycle_fails_with_path() {
        let dir = clean_repo();
        write(
            dir.path(),
            "project-management/tasks/TSK-001-001.md",
            "---\nid: TSK-001-001\nformat_id: TSK-001-001\nepic_id: EPC-001\ntitle: task\nstatus: todo\nwork_type: feat\n\n\n\ndepends_on: [TSK-001-003]\ncreated: 2026-07-25\n---\n",
        );
        task_file(dir.path(), "TSK-001-002", "[TSK-001-001]");
        task_file(dir.path(), "TSK-001-003", "[TSK-001-002]");

        let report = lint_docs(dir.path());
        let issue = report
            .issues
            .iter()
            .find(|issue| issue.message.contains("task dependency cycle"))
            .expect("cycle reported");
        assert!(issue.message.matches("TSK-001-").count() >= 4);
        assert_eq!(issue.line, 11);
    }

    #[test]
    fn task_cycle_detector_matches_exhaustive_small_graph_oracle() {
        for node_count in 0..=4 {
            let nodes: Vec<String> = (0..node_count).map(|index| format!("T{index}")).collect();
            let possible_edges: Vec<(usize, usize)> = (0..node_count)
                .flat_map(|from| {
                    (0..node_count)
                        .filter(move |to| *to != from)
                        .map(move |to| (from, to))
                })
                .collect();
            for mask in 0_u64..(1_u64 << possible_edges.len()) {
                let mut graph: BTreeMap<String, Vec<String>> = nodes
                    .iter()
                    .cloned()
                    .map(|node| (node, Vec::new()))
                    .collect();
                for (bit, (from, to)) in possible_edges.iter().enumerate() {
                    if mask & (1_u64 << bit) != 0 {
                        graph
                            .get_mut(&nodes[*from])
                            .expect("node")
                            .push(nodes[*to].clone());
                    }
                }
                let detector_is_acyclic = find_task_cycle(&graph).is_none();
                let oracle_is_acyclic = has_topological_order(&nodes, &graph);
                assert_eq!(
                    detector_is_acyclic, oracle_is_acyclic,
                    "graph mismatch: {graph:?}"
                );
            }
        }
    }

    fn has_topological_order(nodes: &[String], graph: &BTreeMap<String, Vec<String>>) -> bool {
        fn visit(
            nodes: &[String],
            graph: &BTreeMap<String, Vec<String>>,
            order: &mut Vec<String>,
            used: &mut BTreeSet<String>,
        ) -> bool {
            if order.len() == nodes.len() {
                let positions: BTreeMap<&str, usize> = order
                    .iter()
                    .enumerate()
                    .map(|(index, node)| (node.as_str(), index))
                    .collect();
                return order.iter().all(|node| {
                    graph[node]
                        .iter()
                        .all(|dependency| positions[dependency.as_str()] < positions[node.as_str()])
                });
            }
            for node in nodes {
                if used.insert(node.clone()) {
                    order.push(node.clone());
                    if visit(nodes, graph, order, used) {
                        return true;
                    }
                    order.pop();
                    used.remove(node);
                }
            }
            false
        }

        visit(nodes, graph, &mut Vec::new(), &mut BTreeSet::new())
    }
}
