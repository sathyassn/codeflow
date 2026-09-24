//! Catalog-derived scans. Callers opt in; the real tree switches in TSK-085.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::{Catalog, Lifecycle};

/// One exact, case-sensitive, token-bounded routing identity occurrence.
#[derive(Debug, PartialEq, Eq)]
pub struct Finding {
    pub path: PathBuf,
    pub line: usize,
    pub token: String,
}

/// Scan operative instructions for selectors, aliases and pinned identities.
///
/// # Errors
/// Returns filesystem read failures rather than silently skipping a surface.
pub fn instruction_selectors(root: &Path, catalog: &Catalog) -> Result<Vec<Finding>, String> {
    scan(root, catalog, false)
}

/// Scan nonhistorical text for retired catalog selectors and identities.
///
/// # Errors
/// Returns filesystem read failures rather than silently skipping a surface.
pub fn retired_selectors(root: &Path, catalog: &Catalog) -> Result<Vec<Finding>, String> {
    scan(root, catalog, true)
}

fn catalog_or_fixture(path: &str) -> bool {
    [
        "assets/base/agents",
        ".agents",
        ".claude",
        ".codeflow/.baseline/.agents",
        ".codeflow/.baseline/.claude",
    ]
    .iter()
    .any(|prefix| {
        path == format!("{prefix}/skills/cf-model-orchestrator/resources/current-ensemble.json")
    }) || [
        "assets/base/agents",
        ".agents",
        ".claude",
        ".codeflow/.baseline/.agents",
        ".codeflow/.baseline/.claude",
    ]
    .iter()
    .any(|prefix| {
        let kit = format!("{prefix}/skills/cf-evaluate-model/resources/");
        path == format!("{kit}fixtures.json") || path.starts_with(&format!("{kit}fixtures/"))
    })
}

fn operative(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name == "SKILL.md"
        || matches!(
            name,
            "AGENTS.md.tmpl" | "AGENTS.minimal.md.tmpl" | "CLAUDE.md.tmpl"
        )
        || path.contains("/skills/")
        || ((path.contains("/agents/") || path.starts_with("agents/")) && name.contains("reviewer"))
        || path.contains("/workflows/")
}

fn historical(path: &str) -> bool {
    path == "CHANGELOG.md"
        || [
            "docs/decisions/",
            "docs/verification/",
            "project-management/",
            "docs/plan/",
        ]
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

fn token_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-')
}

fn contains_token(line: &str, token: &str) -> bool {
    line.match_indices(token).any(|(start, matched)| {
        let end = start + matched.len();
        let before = &line[..start];
        let after = &line[end..];
        !before.chars().next_back().is_some_and(token_char)
            && !after.chars().next().is_some_and(token_char)
            && !before
                .strip_suffix('.')
                .is_some_and(|s| s.chars().next_back().is_some_and(token_char))
            && !after
                .strip_prefix('.')
                .is_some_and(|s| s.chars().next().is_some_and(token_char))
    })
}

fn scan(root: &Path, catalog: &Catalog, retired: bool) -> Result<Vec<Finding>, String> {
    let tokens: BTreeSet<&str> = catalog
        .lines
        .iter()
        .flat_map(|line| &line.versions)
        .filter(|version| !retired || version.lifecycle == Lifecycle::Retired)
        .flat_map(|version| {
            std::iter::once(version.alias.as_str())
                .chain(std::iter::once(version.pinned_id.as_str()))
                .chain(version.selectors.values().map(String::as_str))
        })
        .collect();
    let mut pending = vec![root.to_path_buf()];
    let mut findings = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            let path = entry.path();
            if kind.is_dir() {
                if !matches!(
                    entry.file_name().to_str(),
                    Some(".git" | "target" | ".worktrees" | "node_modules")
                ) {
                    pending.push(path);
                }
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            let relative = path.strip_prefix(root).map_err(|e| e.to_string())?;
            let normalized = relative.to_string_lossy().replace('\\', "/");
            if catalog_or_fixture(&normalized)
                || if retired {
                    historical(&normalized)
                } else {
                    !operative(&normalized)
                }
            {
                continue;
            }
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            let Ok(text) = std::str::from_utf8(&bytes) else {
                continue;
            };
            for (index, line) in text.lines().enumerate() {
                for token in &tokens {
                    if contains_token(line, token) {
                        findings.push(Finding {
                            path: relative.to_path_buf(),
                            line: index + 1,
                            token: (*token).to_owned(),
                        });
                    }
                }
            }
        }
    }
    findings.sort_by(|a, b| (&a.path, a.line, &a.token).cmp(&(&b.path, b.line, &b.token)));
    Ok(findings)
}
