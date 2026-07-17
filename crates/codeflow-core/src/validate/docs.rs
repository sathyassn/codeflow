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
//! * epic frontmatter `capabilities[]` / `adrs[]` resolve back
//!
//! Tier-graceful: an absent layer (no registry, no decisions dir, no
//! project-management) skips its checks with a note — only references that
//! point INTO a present layer and miss are errors.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::capability::{parse_capabilities, CAPABILITY_STATUS_VALUES};
use crate::validate::{get_string_field, parse_frontmatter};

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
    /// Skipped-layer notes (absent tiers).
    pub notes: Vec<String>,
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
}

/// Run the docs-integrity lint over a repository root.
#[must_use]
pub fn lint_docs(repo_root: &Path) -> DocsLintReport {
    let mut report = DocsLintReport::default();

    let graph = build_graph(repo_root, &mut report);

    lint_capabilities(repo_root, &graph, &mut report);
    lint_adrs(repo_root, &mut report);
    lint_epics(repo_root, &graph, &mut report);

    report
}

fn build_graph(repo_root: &Path, report: &mut DocsLintReport) -> DocGraph {
    // Capability ids.
    let caps_path = repo_root.join("docs/capabilities.md");
    let capabilities = if let Ok(content) = std::fs::read_to_string(&caps_path) {
        let (entries, _) = parse_capabilities(&content);
        Some(entries.into_iter().map(|e| e.id).collect())
    } else {
        report
            .notes
            .push("docs/capabilities.md absent — capability checks skipped".to_string());
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
        report
            .notes
            .push("docs/decisions/ absent — ADR checks skipped".to_string());
        None
    };

    // Epic ids from project-management/epics file stems.
    let epics_dir = repo_root.join("project-management/epics");
    let epics = if epics_dir.is_dir() {
        Some(
            epic_files(&epics_dir)
                .iter()
                .filter_map(|p| p.file_stem().and_then(|s| s.to_str()))
                .map(ToString::to_string)
                .collect(),
        )
    } else {
        report
            .notes
            .push("project-management/epics/ absent — epic reference checks skipped".to_string());
        None
    };

    DocGraph {
        capabilities,
        adrs,
        epics,
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

    for entry in entries {
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

    for path in epic_files(&epics_dir) {
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
    }
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

/// Epic markdown files under `dir`, accepting BOTH layouts:
/// * flat — `epics/EPC-001.md`
/// * nested — `epics/EPC-001/EPC-001.md`, where the epic gets its own
///   directory so `tasks/` and specs can live alongside it.
///
/// In the nested layout only the epic file itself (named for its directory) is
/// returned; sibling task/spec files are ignored. Sorted for stable output.
fn epic_files(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return paths;
    };
    for entry in rd.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "md") {
            paths.push(path);
        } else if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                let nested = path.join(format!("{name}.md"));
                if nested.is_file() {
                    paths.push(nested);
                }
            }
        }
    }
    paths.sort();
    paths
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
            "project-management/epics/EPC-001/tasks/TASK-001.md",
            "---\nid: TASK-001\n---\n",
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
        assert_eq!(report.notes.len(), 3, "notes: {:?}", report.notes);
        assert!(report.notes.iter().any(|n| n.contains("capabilities.md")));
        assert!(report.notes.iter().any(|n| n.contains("decisions")));
        assert!(report
            .notes
            .iter()
            .any(|n| n.contains("project-management")));
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
            .any(|n| n.contains("project-management")));
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
}
