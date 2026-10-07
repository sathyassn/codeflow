//! Catalog-derived scans, run over fixture trees and the real tree.

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
        || [
            ".claude/workflows/",
            "assets/base/claude/workflows/",
            ".codeflow/.baseline/.claude/workflows/",
        ]
        .iter()
        .any(|prefix| path.starts_with(prefix))
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
            let normalized = crate::portable_path::slashed(relative);
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
            // Every file is checked, a binary asset or a file that is not
            // valid UTF-8 included: each line is searched in its valid UTF-8
            // runs, and an invalid byte bounds a token as any other
            // non-token character does. Nothing is skipped and nothing is
            // decoded lossily.
            for (index, line) in bytes.split(|byte| *byte == b'\n').enumerate() {
                let line = line.strip_suffix(b"\r").unwrap_or(line);
                for token in &tokens {
                    if line
                        .utf8_chunks()
                        .any(|chunk| contains_token(chunk.valid(), token))
                    {
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

#[cfg(test)]
mod r22_regressions {
    use super::*;

    /// A file that is not valid UTF-8 is still scanned, never skipped and
    /// never refused: a token beside an invalid byte is found on its line,
    /// and a binary asset with no token yields nothing (round 23; main
    /// skipped such a file, and round 22 refused it, which stopped the real
    /// tree scan at its first font file).
    #[test]
    fn r22_catalog_scan_reads_non_utf8_input_as_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = crate::model_catalog::inputs::load_catalog(dir.path()).unwrap();
        assert!(instruction_selectors(dir.path(), &catalog)
            .unwrap()
            .is_empty());
        let token = catalog.lines[0].versions[0].alias.clone();
        let mut skill = b"first line\n\xff ".to_vec();
        skill.extend_from_slice(token.as_bytes());
        skill.extend_from_slice(b" \xfe\r\n");
        std::fs::write(dir.path().join("SKILL.md"), &skill).unwrap();
        let findings = instruction_selectors(dir.path(), &catalog).unwrap();
        assert_eq!(
            findings,
            [Finding {
                path: PathBuf::from("SKILL.md"),
                line: 2,
                token: token.clone(),
            }]
        );
        // A token glued to other letters is still no token.
        let mut glued = b"\xffx".to_vec();
        glued.extend_from_slice(token.as_bytes());
        std::fs::write(dir.path().join("SKILL.md"), &glued).unwrap();
        assert!(instruction_selectors(dir.path(), &catalog)
            .unwrap()
            .is_empty());
        std::fs::write(dir.path().join("SKILL.md"), b"\x00\xff\xfe binary").unwrap();
        assert!(instruction_selectors(dir.path(), &catalog)
            .unwrap()
            .is_empty());
    }
}
