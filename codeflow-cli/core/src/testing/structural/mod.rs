//! Structural integrity check — bidirectional source↔test mapping.
//!
//! For a given target with a `structural` block, this module:
//!
//! 1. **Source → Test (missing):** every file matching `source_glob` must map,
//!    via `pattern_map` regex rules, to an expected test path that exists on
//!    disk. Files covered by `exclusions.no_test_required` are skipped.
//! 2. **Test → Source (orphaned):** every file matching `test_glob` must have
//!    a matching source somewhere in `source_glob`. Test files covered by
//!    `exclusions.orphan_allowed` are skipped.
//!
//! The two directions are independent; a target may configure either or both.
//! When no `structural` block is declared, the target is skipped entirely.
//!
//! Glob semantics follow the `glob` crate — `**` matches any number of path
//! components; `*` matches within a single component. Patterns are evaluated
//! repo-relative (the project root is the working directory passed to
//! [`validate_target`]).

use std::path::{Path, PathBuf};

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::testing::config::{
    ExclusionEntry, PatternMapEntry, StructuralConfig, StructuralExclusions, TargetConfig,
};
use crate::testing::error::TestingError;

/// Outcome of a single target's structural check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuralResult {
    /// Target name this result describes.
    pub target: String,
    /// Scanned source files.
    pub sources_scanned: usize,
    /// Scanned test files.
    pub tests_scanned: usize,
    /// Source files excluded via `no_test_required`.
    pub sources_excluded: usize,
    /// Test files excluded via `orphan_allowed`.
    pub tests_excluded: usize,
    /// Source files with no matching test — missing coverage.
    pub missing: Vec<MissingFinding>,
    /// Test files with no matching source — orphans.
    pub orphans: Vec<OrphanFinding>,
    /// True when both directions pass.
    pub pass: bool,
}

impl StructuralResult {
    /// Total number of findings (missing + orphans).
    #[must_use]
    pub fn finding_count(&self) -> usize {
        self.missing.len() + self.orphans.len()
    }
}

/// A source file that has no corresponding test.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingFinding {
    /// Source file path (repo-relative).
    pub source: String,
    /// Test path derived from `pattern_map` (if any rule matched).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_test: Option<String>,
}

/// A test file that doesn't map back to any source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrphanFinding {
    /// Test file path (repo-relative).
    pub test: String,
}

/// Validate structural integrity for a single target.
///
/// Returns `Ok(None)` when the target has no `structural` block (the check is
/// not applicable). Returns `Ok(Some(result))` otherwise — callers inspect
/// `result.pass` to determine success.
///
/// # Errors
///
/// Returns `TestingError::ConfigInvalid` when a pattern in `pattern_map` or an
/// exclusion glob fails to compile. Filesystem walk errors bubble up as
/// `TestingError::Io`.
pub fn validate_target(
    target: &TargetConfig,
    project_dir: &Path,
) -> Result<Option<StructuralResult>, TestingError> {
    let Some(structural) = target.structural.as_ref() else {
        return Ok(None);
    };

    let plan = compile_plan(structural)?;
    let sources = gather_matches(project_dir, &structural.source_glob)?;
    let tests = gather_matches(project_dir, &structural.test_glob)?;

    let mut result = StructuralResult {
        target: target.name.clone(),
        sources_scanned: sources.len(),
        tests_scanned: tests.len(),
        sources_excluded: 0,
        tests_excluded: 0,
        missing: Vec::new(),
        orphans: Vec::new(),
        pass: true,
    };

    // Direction 1: source → test.
    for source in &sources {
        if plan.source_excluded(source) {
            result.sources_excluded += 1;
            continue;
        }
        let expected = plan.derive_test_path(source);
        let hit = expected
            .as_deref()
            .is_some_and(|rel| project_dir.join(rel).is_file());
        if !hit {
            result.missing.push(MissingFinding {
                source: source.clone(),
                expected_test: expected,
            });
        }
    }

    // Direction 2: test → source.
    for test in &tests {
        if plan.test_excluded(test) {
            result.tests_excluded += 1;
            continue;
        }
        if !plan.test_has_source(test, &sources) {
            result.orphans.push(OrphanFinding { test: test.clone() });
        }
    }

    result.pass = result.finding_count() == 0;
    Ok(Some(result))
}

/// Validate structural integrity for every target in the config that declares
/// one. Targets without `structural` are silently skipped.
///
/// # Errors
///
/// Propagates [`validate_target`] errors.
pub fn validate_all(
    targets: &[TargetConfig],
    project_dir: &Path,
) -> Result<Vec<StructuralResult>, TestingError> {
    let mut results = Vec::new();
    for target in targets {
        if !target.enabled {
            continue;
        }
        if let Some(result) = validate_target(target, project_dir)? {
            results.push(result);
        }
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// Compiled plan — precompiled regex + globs per target.
// ---------------------------------------------------------------------------

struct CompiledPattern {
    regex: Regex,
    template: String,
}

struct CompiledExclusion {
    pattern: glob::Pattern,
}

#[derive(Default)]
struct Plan {
    pattern_map: Vec<CompiledPattern>,
    source_excluded: Vec<CompiledExclusion>,
    test_excluded: Vec<CompiledExclusion>,
}

impl Plan {
    fn source_excluded(&self, source: &str) -> bool {
        self.source_excluded
            .iter()
            .any(|e| e.pattern.matches(source))
    }

    fn test_excluded(&self, test: &str) -> bool {
        self.test_excluded.iter().any(|e| e.pattern.matches(test))
    }

    /// Apply the first matching `pattern_map` entry to derive the expected
    /// test path. Returns `None` when no rule matches.
    fn derive_test_path(&self, source: &str) -> Option<String> {
        for entry in &self.pattern_map {
            if let Some(caps) = entry.regex.captures(source) {
                let mut out = String::with_capacity(entry.template.len());
                let template = entry.template.as_bytes();
                let mut i = 0;
                while i < template.len() {
                    let c = template[i];
                    if c == b'$' && i + 1 < template.len() {
                        let next = template[i + 1];
                        if next.is_ascii_digit() {
                            let idx = (next - b'0') as usize;
                            if let Some(m) = caps.get(idx) {
                                out.push_str(m.as_str());
                            }
                            i += 2;
                            continue;
                        }
                    }
                    out.push(c as char);
                    i += 1;
                }
                return Some(out);
            }
        }
        None
    }

    /// Reverse lookup — does the test path correspond to any known source?
    /// Applies `pattern_map` rules in reverse: for each candidate source, the
    /// derived test path must equal `test`.
    fn test_has_source(&self, test: &str, sources: &[String]) -> bool {
        for source in sources {
            if let Some(expected) = self.derive_test_path(source) {
                if expected == test {
                    return true;
                }
            }
        }
        false
    }
}

fn compile_plan(structural: &StructuralConfig) -> Result<Plan, TestingError> {
    let mut plan = Plan::default();

    for entry in &structural.pattern_map {
        let PatternMapEntry { source, test } = entry;
        let regex = Regex::new(source).map_err(|e| TestingError::ConfigInvalid {
            path: PathBuf::from("structural.pattern_map"),
            message: format!("invalid regex `{source}`: {e}"),
        })?;
        plan.pattern_map.push(CompiledPattern {
            regex,
            template: test.clone(),
        });
    }

    if let Some(StructuralExclusions {
        no_test_required,
        orphan_allowed,
    }) = structural.exclusions.as_ref()
    {
        plan.source_excluded = compile_exclusions(no_test_required, "no_test_required")?;
        plan.test_excluded = compile_exclusions(orphan_allowed, "orphan_allowed")?;
    }

    Ok(plan)
}

fn compile_exclusions(
    entries: &[ExclusionEntry],
    field: &str,
) -> Result<Vec<CompiledExclusion>, TestingError> {
    let mut out = Vec::with_capacity(entries.len());
    for e in entries {
        let pattern =
            glob::Pattern::new(&e.pattern).map_err(|err| TestingError::ConfigInvalid {
                path: PathBuf::from(format!("structural.exclusions.{field}")),
                message: format!("invalid glob `{}`: {err}", e.pattern),
            })?;
        out.push(CompiledExclusion { pattern });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// File discovery — walk the repo and collect matches for a set of globs.
// ---------------------------------------------------------------------------

/// Collect every file path matching at least one of the supplied globs. Paths
/// are returned sorted and repo-relative.
///
/// Uses the `glob` crate with `MatchOptions` that honour leading `.` segments
/// so patterns like `.codeflow/**/*.sh` work correctly.
fn gather_matches(project_dir: &Path, patterns: &[String]) -> Result<Vec<String>, TestingError> {
    let mut collected: Vec<String> = Vec::new();
    let options = glob::MatchOptions {
        case_sensitive: true,
        require_literal_separator: true,
        require_literal_leading_dot: false,
    };

    for pattern in patterns {
        // The `glob` crate requires absolute patterns or patterns relative to
        // the current working directory. To keep results stable across CWDs,
        // we temporarily set CWD to project_dir via `chdir` — no, that's
        // racy. Instead, prefix the pattern with project_dir.
        let absolute = project_dir.join(pattern).to_string_lossy().to_string();
        let glob_iter =
            glob::glob_with(&absolute, options).map_err(|e| TestingError::ConfigInvalid {
                path: PathBuf::from("structural.source_glob/test_glob"),
                message: format!("invalid glob `{pattern}`: {e}"),
            })?;
        for entry in glob_iter {
            let path = match entry {
                Ok(p) => p,
                Err(e) => {
                    // Permission/IO errors during walk — surface as IO.
                    return Err(TestingError::Io(std::io::Error::other(e)));
                }
            };
            if !path.is_file() {
                continue;
            }
            if let Ok(rel) = path.strip_prefix(project_dir) {
                let rel_str = rel.to_string_lossy().replace('\\', "/");
                collected.push(rel_str);
            }
        }
    }

    collected.sort();
    collected.dedup();
    Ok(collected)
}

// ---------------------------------------------------------------------------
// Human-readable formatting
// ---------------------------------------------------------------------------

/// Render a structural result as a human-readable summary.
#[must_use]
pub fn format_human(result: &StructuralResult) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "target: {}\n  sources scanned: {} (excluded: {})\n  tests scanned:   {} (excluded: {})",
        result.target,
        result.sources_scanned,
        result.sources_excluded,
        result.tests_scanned,
        result.tests_excluded,
    );
    if result.missing.is_empty() {
        out.push_str("  direction 1 (source→test): PASS\n");
    } else {
        let _ = writeln!(
            out,
            "  direction 1 (source→test): FAIL — {} missing",
            result.missing.len()
        );
        for finding in &result.missing {
            let expected = finding.expected_test.as_deref().unwrap_or("<unmapped>");
            let _ = writeln!(out, "    MISSING: {} → {}", finding.source, expected);
        }
    }
    if result.orphans.is_empty() {
        out.push_str("  direction 2 (test→source): PASS\n");
    } else {
        let _ = writeln!(
            out,
            "  direction 2 (test→source): FAIL — {} orphaned",
            result.orphans.len()
        );
        for finding in &result.orphans {
            let _ = writeln!(out, "    ORPHAN: {}", finding.test);
        }
    }
    out
}

/// Render multiple results as a single human block.
#[must_use]
pub fn format_human_all(results: &[StructuralResult]) -> String {
    use std::fmt::Write as _;
    if results.is_empty() {
        return "no targets declare structural checks\n".to_string();
    }
    let mut out = String::new();
    let total_missing: usize = results.iter().map(|r| r.missing.len()).sum();
    let total_orphans: usize = results.iter().map(|r| r.orphans.len()).sum();
    for r in results {
        out.push_str(&format_human(r));
        out.push('\n');
    }
    let _ = writeln!(
        out,
        "summary: {} target(s), {} missing, {} orphan",
        results.len(),
        total_missing,
        total_orphans,
    );
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::config::{
        ExclusionEntry, ModeCommand, PatternMapEntry, RunnerType, StructuralConfig,
        StructuralExclusions, TargetConfig,
    };
    use std::collections::BTreeMap;
    use std::fs;

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn sample_target() -> TargetConfig {
        let mut modes = BTreeMap::new();
        modes.insert(
            "full".to_string(),
            ModeCommand {
                command: "true".to_string(),
            },
        );
        TargetConfig {
            name: "shell".to_string(),
            enabled: true,
            cwd: None,
            env: BTreeMap::new(),
            runner: RunnerType::Custom,
            modes,
            report: None,
            coverage: None,
            ci_skip: None,
            ci_skip_reason: None,
            structural: Some(StructuralConfig {
                source_glob: vec!["src/*.sh".to_string()],
                test_glob: vec!["tests/test-*.sh".to_string()],
                pattern_map: vec![PatternMapEntry {
                    source: r"^src/(.+)\.sh$".to_string(),
                    test: "tests/test-$1.sh".to_string(),
                }],
                exclusions: None,
            }),
            tags: Vec::new(),
            test_files: Vec::new(),
        }
    }

    #[test]
    fn target_without_structural_block_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let mut target = sample_target();
        target.structural = None;
        let result = validate_target(&target, dir.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn disabled_target_skipped_in_validate_all() {
        let dir = tempfile::tempdir().unwrap();
        let mut target = sample_target();
        target.enabled = false;
        let results = validate_all(std::slice::from_ref(&target), dir.path()).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn pass_when_every_source_has_matching_test() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("src/alpha.sh"), "# alpha");
        write(&root.join("src/beta.sh"), "# beta");
        write(&root.join("tests/test-alpha.sh"), "# test");
        write(&root.join("tests/test-beta.sh"), "# test");

        let target = sample_target();
        let result = validate_target(&target, root).unwrap().unwrap();
        assert!(result.pass);
        assert_eq!(result.sources_scanned, 2);
        assert_eq!(result.tests_scanned, 2);
        assert!(result.missing.is_empty());
        assert!(result.orphans.is_empty());
    }

    #[test]
    fn fail_when_source_has_no_test() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("src/alpha.sh"), "# alpha");
        // deliberately no tests/test-alpha.sh

        let target = sample_target();
        let result = validate_target(&target, root).unwrap().unwrap();
        assert!(!result.pass);
        assert_eq!(result.missing.len(), 1);
        assert_eq!(result.missing[0].source, "src/alpha.sh");
        assert_eq!(
            result.missing[0].expected_test.as_deref(),
            Some("tests/test-alpha.sh")
        );
    }

    #[test]
    fn fail_when_test_has_no_source() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("tests/test-ghost.sh"), "# orphan");

        let target = sample_target();
        let result = validate_target(&target, root).unwrap().unwrap();
        assert!(!result.pass);
        assert_eq!(result.orphans.len(), 1);
        assert_eq!(result.orphans[0].test, "tests/test-ghost.sh");
    }

    #[test]
    fn source_exclusion_skips_missing_test() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("src/alpha.sh"), "# alpha");
        // no test file — would normally fail, but exclusion applies
        let mut target = sample_target();
        target.structural.as_mut().unwrap().exclusions = Some(StructuralExclusions {
            no_test_required: vec![ExclusionEntry {
                pattern: "src/*.sh".to_string(),
                reason: Some("smoke".to_string()),
            }],
            orphan_allowed: Vec::new(),
        });

        let result = validate_target(&target, root).unwrap().unwrap();
        assert!(result.pass);
        assert_eq!(result.sources_excluded, 1);
    }

    #[test]
    fn orphan_exclusion_allows_test_without_source() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("tests/test-framework.sh"), "# framework");
        let mut target = sample_target();
        target.structural.as_mut().unwrap().exclusions = Some(StructuralExclusions {
            no_test_required: Vec::new(),
            orphan_allowed: vec![ExclusionEntry {
                pattern: "tests/test-framework.sh".to_string(),
                reason: None,
            }],
        });

        let result = validate_target(&target, root).unwrap().unwrap();
        assert!(result.pass);
        assert_eq!(result.tests_excluded, 1);
    }

    #[test]
    fn pattern_map_multiple_rules_first_match_wins() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("src/alpha.py"), "# py");
        write(&root.join("tests/test_alpha.py"), "# py test");

        let mut target = sample_target();
        let s = target.structural.as_mut().unwrap();
        s.source_glob = vec!["src/*.py".to_string()];
        s.test_glob = vec!["tests/test_*.py".to_string()];
        s.pattern_map = vec![
            PatternMapEntry {
                source: r"^src/(.+)\.py$".to_string(),
                test: "tests/test_$1.py".to_string(),
            },
            PatternMapEntry {
                source: r"^src/(.+)\.py$".to_string(),
                test: "never/used.py".to_string(),
            },
        ];

        let result = validate_target(&target, root).unwrap().unwrap();
        assert!(result.pass);
    }

    #[test]
    fn invalid_regex_in_pattern_map_errors() {
        let dir = tempfile::tempdir().unwrap();
        let mut target = sample_target();
        target
            .structural
            .as_mut()
            .unwrap()
            .pattern_map
            .push(PatternMapEntry {
                source: "[unclosed".to_string(),
                test: "x".to_string(),
            });
        let err = validate_target(&target, dir.path()).unwrap_err();
        assert!(matches!(err, TestingError::ConfigInvalid { .. }));
    }

    #[test]
    fn invalid_glob_in_exclusion_errors() {
        let dir = tempfile::tempdir().unwrap();
        let mut target = sample_target();
        target.structural.as_mut().unwrap().exclusions = Some(StructuralExclusions {
            no_test_required: vec![ExclusionEntry {
                pattern: "[bad".to_string(),
                reason: None,
            }],
            orphan_allowed: Vec::new(),
        });
        let err = validate_target(&target, dir.path()).unwrap_err();
        assert!(matches!(err, TestingError::ConfigInvalid { .. }));
    }

    #[test]
    fn glob_edge_case_deep_nesting() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("src/a/b/c/deep.sh"), "# deep");
        write(&root.join("tests/test-deep.sh"), "# test");

        let mut target = sample_target();
        let s = target.structural.as_mut().unwrap();
        s.source_glob = vec!["src/**/*.sh".to_string()];
        s.pattern_map = vec![PatternMapEntry {
            source: r"^src/.+/([^/]+)\.sh$".to_string(),
            test: "tests/test-$1.sh".to_string(),
        }];

        let result = validate_target(&target, root).unwrap().unwrap();
        assert!(result.pass, "deep-nested source should match — {result:?}");
    }

    #[test]
    fn derive_test_path_handles_literal_dollar_in_template() {
        let plan = Plan {
            pattern_map: vec![CompiledPattern {
                regex: Regex::new(r"^(.+)\.sh$").unwrap(),
                template: "$1.ok".to_string(),
            }],
            source_excluded: Vec::new(),
            test_excluded: Vec::new(),
        };
        assert_eq!(plan.derive_test_path("foo.sh"), Some("foo.ok".to_string()));
    }

    #[test]
    fn derive_test_path_no_match_returns_none() {
        let plan = Plan {
            pattern_map: vec![CompiledPattern {
                regex: Regex::new(r"^src/(.+)\.py$").unwrap(),
                template: "tests/$1.py".to_string(),
            }],
            source_excluded: Vec::new(),
            test_excluded: Vec::new(),
        };
        assert!(plan.derive_test_path("other/path.sh").is_none());
    }

    #[test]
    fn validate_all_aggregates_multiple_targets() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("src/alpha.sh"), "# alpha");
        write(&root.join("tests/test-alpha.sh"), "# test");

        let target_a = sample_target();
        let mut target_b = sample_target();
        target_b.name = "other".to_string();
        target_b.structural = None; // skipped

        let results = validate_all(&[target_a, target_b], root).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].target, "shell");
    }

    #[test]
    fn format_human_reports_findings() {
        let result = StructuralResult {
            target: "shell".to_string(),
            sources_scanned: 2,
            tests_scanned: 1,
            sources_excluded: 0,
            tests_excluded: 0,
            missing: vec![MissingFinding {
                source: "src/alpha.sh".to_string(),
                expected_test: Some("tests/test-alpha.sh".to_string()),
            }],
            orphans: vec![OrphanFinding {
                test: "tests/ghost.sh".to_string(),
            }],
            pass: false,
        };
        let out = format_human(&result);
        assert!(out.contains("MISSING: src/alpha.sh"));
        assert!(out.contains("ORPHAN: tests/ghost.sh"));
        assert!(out.contains("FAIL"));
    }

    #[test]
    fn format_human_all_reports_summary() {
        let result = StructuralResult {
            target: "shell".to_string(),
            sources_scanned: 0,
            tests_scanned: 0,
            sources_excluded: 0,
            tests_excluded: 0,
            missing: Vec::new(),
            orphans: Vec::new(),
            pass: true,
        };
        let out = format_human_all(std::slice::from_ref(&result));
        assert!(out.contains("summary"));
        assert!(out.contains("1 target(s)"));
    }

    #[test]
    fn format_human_all_empty() {
        let out = format_human_all(&[]);
        assert!(out.contains("no targets"));
    }

    #[test]
    fn finding_count_totals_both_directions() {
        let result = StructuralResult {
            target: "x".to_string(),
            sources_scanned: 0,
            tests_scanned: 0,
            sources_excluded: 0,
            tests_excluded: 0,
            missing: vec![MissingFinding {
                source: "a".to_string(),
                expected_test: None,
            }],
            orphans: vec![
                OrphanFinding {
                    test: "b".to_string(),
                },
                OrphanFinding {
                    test: "c".to_string(),
                },
            ],
            pass: false,
        };
        assert_eq!(result.finding_count(), 3);
    }

    #[test]
    fn serde_roundtrip_of_result() {
        let result = StructuralResult {
            target: "t".to_string(),
            sources_scanned: 1,
            tests_scanned: 1,
            sources_excluded: 0,
            tests_excluded: 0,
            missing: Vec::new(),
            orphans: Vec::new(),
            pass: true,
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: StructuralResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.target, "t");
        assert!(back.pass);
    }
}
