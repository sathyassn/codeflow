//! Coverage threshold engine.
//!
//! Evaluates `CoverageRule` arrays against `FileCoverage` sets using:
//! - First-match-wins for `per_file` and `changed_files` scopes
//! - All-rules-evaluated for `per_package`, `per_module`, `global`

use crate::testing::config::{CoverageException, CoverageRule, CoverageScope};
use crate::testing::coverage::FileCoverage;

/// Result of evaluating thresholds for a single file.
#[derive(Debug, Clone)]
pub struct ThresholdResult {
    pub file: String,
    pub coverage_percent: f64,
    pub threshold: u32,
    pub pass: bool,
    pub rule_scope: CoverageScope,
    pub exception_applied: bool,
}

/// Result of evaluating thresholds for a global/package/module scope.
#[derive(Debug, Clone)]
pub struct AggregateThresholdResult {
    pub scope: CoverageScope,
    pub coverage_percent: f64,
    pub threshold: u32,
    pub pass: bool,
    pub matched_files: usize,
}

/// Evaluate coverage rules against file coverage data.
///
/// Returns per-file threshold results for file-level scopes
/// (`per_file`, `changed_files`).
#[must_use]
pub fn evaluate_file_thresholds(
    rules: &[CoverageRule],
    coverages: &[FileCoverage],
    changed_files: &[String],
    exceptions: &[CoverageException],
) -> Vec<ThresholdResult> {
    let mut results = Vec::new();

    for cov in coverages {
        let is_changed = changed_files.iter().any(|cf| cf == &cov.path);

        // Check for per-file exception first
        let exception = exceptions.iter().find(|e| e.file == cov.path);

        // Find first matching rule (first-match-wins for file-level scopes)
        let mut matched = false;
        for rule in rules {
            match rule.scope {
                CoverageScope::PerFile => {
                    if matches_globs(&cov.path, &rule.include, &rule.exclude) {
                        let threshold = exception.map_or(rule.minimum, |e| e.threshold);
                        results.push(ThresholdResult {
                            file: cov.path.clone(),
                            coverage_percent: cov.percent,
                            threshold,
                            pass: cov.percent >= f64::from(threshold),
                            rule_scope: CoverageScope::PerFile,
                            exception_applied: exception.is_some(),
                        });
                        matched = true;
                        break;
                    }
                }
                CoverageScope::ChangedFiles => {
                    if is_changed {
                        let threshold = exception.map_or(rule.minimum, |e| e.threshold);
                        results.push(ThresholdResult {
                            file: cov.path.clone(),
                            coverage_percent: cov.percent,
                            threshold,
                            pass: cov.percent >= f64::from(threshold),
                            rule_scope: CoverageScope::ChangedFiles,
                            exception_applied: exception.is_some(),
                        });
                        matched = true;
                        break;
                    }
                }
                CoverageScope::PerPackage | CoverageScope::PerModule | CoverageScope::Global => {
                    // Non-file-level scopes don't participate in first-match-wins
                }
            }
        }

        // If no file-level rule matched but this is a changed file,
        // still need to report it (with no threshold applied)
        if !matched && is_changed {
            // No rule matched this changed file
        }
    }

    results
}

/// Evaluate aggregate (non-file-level) coverage rules.
#[must_use]
pub fn evaluate_aggregate_thresholds(
    rules: &[CoverageRule],
    coverages: &[FileCoverage],
) -> Vec<AggregateThresholdResult> {
    let mut results = Vec::new();

    for rule in rules {
        match rule.scope {
            CoverageScope::Global => {
                let matching: Vec<&FileCoverage> = coverages
                    .iter()
                    .filter(|c| matches_globs(&c.path, &rule.include, &rule.exclude))
                    .collect();

                let total_found: u64 = matching.iter().map(|c| c.lines_found).sum();
                let total_hit: u64 = matching.iter().map(|c| c.lines_hit).sum();
                let percent = FileCoverage::compute_percent(total_found, total_hit);

                results.push(AggregateThresholdResult {
                    scope: CoverageScope::Global,
                    coverage_percent: percent,
                    threshold: rule.minimum,
                    pass: percent >= f64::from(rule.minimum),
                    matched_files: matching.len(),
                });
            }
            CoverageScope::PerPackage | CoverageScope::PerModule => {
                let matching: Vec<&FileCoverage> = coverages
                    .iter()
                    .filter(|c| matches_globs(&c.path, &rule.include, &rule.exclude))
                    .collect();

                let total_found: u64 = matching.iter().map(|c| c.lines_found).sum();
                let total_hit: u64 = matching.iter().map(|c| c.lines_hit).sum();
                let percent = FileCoverage::compute_percent(total_found, total_hit);

                results.push(AggregateThresholdResult {
                    scope: rule.scope.clone(),
                    coverage_percent: percent,
                    threshold: rule.minimum,
                    pass: percent >= f64::from(rule.minimum),
                    matched_files: matching.len(),
                });
            }
            CoverageScope::PerFile | CoverageScope::ChangedFiles => {
                // File-level scopes handled by evaluate_file_thresholds
            }
        }
    }

    results
}

/// Match a file path against include/exclude glob patterns.
fn matches_globs(path: &str, include: &[String], exclude: &[String]) -> bool {
    // Empty include = match all
    let included = if include.is_empty() {
        true
    } else {
        include
            .iter()
            .any(|pattern| glob::Pattern::new(pattern).is_ok_and(|p| p.matches(path)))
    };

    if !included {
        return false;
    }

    // Check excludes
    let excluded = exclude
        .iter()
        .any(|pattern| glob::Pattern::new(pattern).is_ok_and(|p| p.matches(path)));

    !excluded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cov(path: &str, found: u64, hit: u64) -> FileCoverage {
        FileCoverage {
            path: path.to_string(),
            lines_found: found,
            lines_hit: hit,
            percent: FileCoverage::compute_percent(found, hit),
        }
    }

    fn make_rule(scope: CoverageScope, minimum: u32) -> CoverageRule {
        CoverageRule {
            scope,
            include: Vec::new(),
            exclude: Vec::new(),
            minimum,
        }
    }

    #[test]
    fn test_empty_rules() {
        let results = evaluate_file_thresholds(&[], &[make_cov("a.rs", 100, 90)], &[], &[]);
        assert!(results.is_empty());
    }

    #[test]
    fn test_per_file_basic() {
        let rules = vec![make_rule(CoverageScope::PerFile, 85)];
        let coverages = vec![make_cov("a.rs", 100, 90), make_cov("b.rs", 100, 80)];
        let results = evaluate_file_thresholds(&rules, &coverages, &[], &[]);
        assert_eq!(results.len(), 2);
        assert!(results[0].pass);
        assert!(!results[1].pass);
    }

    #[test]
    fn test_changed_files_only() {
        let rules = vec![make_rule(CoverageScope::ChangedFiles, 85)];
        let coverages = vec![make_cov("a.rs", 100, 90), make_cov("b.rs", 100, 80)];
        let changed = vec!["a.rs".to_string()];
        let results = evaluate_file_thresholds(&rules, &coverages, &changed, &[]);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file, "a.rs");
        assert!(results[0].pass);
    }

    #[test]
    fn test_first_match_wins_per_file() {
        let rules = vec![
            CoverageRule {
                scope: CoverageScope::PerFile,
                include: vec!["legacy/**".to_string()],
                exclude: Vec::new(),
                minimum: 70,
            },
            make_rule(CoverageScope::PerFile, 85),
        ];
        let coverages = vec![
            make_cov("legacy/old.rs", 100, 75),
            make_cov("src/new.rs", 100, 75),
        ];
        let results = evaluate_file_thresholds(&rules, &coverages, &[], &[]);
        assert_eq!(results.len(), 2);
        // legacy/old.rs matches first rule (70% threshold) -> pass
        assert_eq!(results[0].threshold, 70);
        assert!(results[0].pass);
        // src/new.rs matches second rule (85% threshold) -> fail
        assert_eq!(results[1].threshold, 85);
        assert!(!results[1].pass);
    }

    #[test]
    fn test_exception_overrides_threshold() {
        let rules = vec![make_rule(CoverageScope::PerFile, 85)];
        let coverages = vec![make_cov("worker.rs", 100, 79)];
        let exceptions = vec![CoverageException {
            file: "worker.rs".to_string(),
            threshold: 79,
            reason: "process spawning".to_string(),
            remove_when: "mock harness".to_string(),
        }];
        let results = evaluate_file_thresholds(&rules, &coverages, &[], &exceptions);
        assert_eq!(results.len(), 1);
        assert!(results[0].pass);
        assert!(results[0].exception_applied);
        assert_eq!(results[0].threshold, 79);
    }

    #[test]
    fn test_global_threshold() {
        let rules = vec![make_rule(CoverageScope::Global, 80)];
        let coverages = vec![make_cov("a.rs", 100, 90), make_cov("b.rs", 100, 70)];
        let results = evaluate_aggregate_thresholds(&rules, &coverages);
        assert_eq!(results.len(), 1);
        assert!((results[0].coverage_percent - 80.0).abs() < f64::EPSILON);
        assert!(results[0].pass);
    }

    #[test]
    fn test_global_threshold_fail() {
        let rules = vec![make_rule(CoverageScope::Global, 90)];
        let coverages = vec![make_cov("a.rs", 100, 80)];
        let results = evaluate_aggregate_thresholds(&rules, &coverages);
        assert_eq!(results.len(), 1);
        assert!(!results[0].pass);
    }

    #[test]
    fn test_per_package_with_include() {
        let rules = vec![CoverageRule {
            scope: CoverageScope::PerPackage,
            include: vec!["src/core/**".to_string()],
            exclude: Vec::new(),
            minimum: 90,
        }];
        let coverages = vec![
            make_cov("src/core/a.rs", 100, 95),
            make_cov("src/core/b.rs", 100, 85),
            make_cov("src/cli/c.rs", 100, 50),
        ];
        let results = evaluate_aggregate_thresholds(&rules, &coverages);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].matched_files, 2);
        // (95+85)/2 = 90%
        assert!((results[0].coverage_percent - 90.0).abs() < f64::EPSILON);
        assert!(results[0].pass);
    }

    #[test]
    fn test_exclude_globs() {
        let rules = vec![CoverageRule {
            scope: CoverageScope::PerFile,
            include: vec!["**/*.rs".to_string()],
            exclude: vec!["test_*.rs".to_string()],
            minimum: 85,
        }];
        let coverages = vec![
            make_cov("main.rs", 100, 90),
            make_cov("test_main.rs", 100, 50),
        ];
        let results = evaluate_file_thresholds(&rules, &coverages, &[], &[]);
        // test_main.rs excluded, only main.rs matches
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file, "main.rs");
    }

    #[test]
    fn test_overlapping_globs_first_wins() {
        let rules = vec![
            CoverageRule {
                scope: CoverageScope::PerFile,
                include: vec!["**/*.rs".to_string()],
                exclude: Vec::new(),
                minimum: 70,
            },
            CoverageRule {
                scope: CoverageScope::PerFile,
                include: vec!["**/*.rs".to_string()],
                exclude: Vec::new(),
                minimum: 90,
            },
        ];
        let coverages = vec![make_cov("a.rs", 100, 75)];
        let results = evaluate_file_thresholds(&rules, &coverages, &[], &[]);
        assert_eq!(results.len(), 1);
        // First rule wins (70%), so passes
        assert_eq!(results[0].threshold, 70);
        assert!(results[0].pass);
    }

    #[test]
    fn test_changed_files_with_git_diff() {
        let rules = vec![make_rule(CoverageScope::ChangedFiles, 85)];
        let coverages = vec![
            make_cov("src/changed.rs", 100, 90),
            make_cov("src/unchanged.rs", 100, 50),
        ];
        let changed = vec!["src/changed.rs".to_string()];
        let results = evaluate_file_thresholds(&rules, &coverages, &changed, &[]);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file, "src/changed.rs");
        assert!(results[0].pass);
    }

    #[test]
    fn test_multiple_aggregate_rules() {
        let rules = vec![
            CoverageRule {
                scope: CoverageScope::PerPackage,
                include: vec!["src/core/**".to_string()],
                exclude: Vec::new(),
                minimum: 90,
            },
            CoverageRule {
                scope: CoverageScope::PerPackage,
                include: vec!["src/**".to_string()],
                exclude: Vec::new(),
                minimum: 80,
            },
        ];
        let coverages = vec![
            make_cov("src/core/a.rs", 100, 95),
            make_cov("src/cli/b.rs", 100, 85),
        ];
        let results = evaluate_aggregate_thresholds(&rules, &coverages);
        // Both rules evaluated independently
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_matches_globs_default_include() {
        // Empty include = match all
        assert!(matches_globs("any/file.rs", &[], &[]));
    }

    #[test]
    fn test_matches_globs_with_include() {
        let include = vec!["src/**".to_string()];
        assert!(matches_globs("src/main.rs", &include, &[]));
        assert!(!matches_globs("tests/main.rs", &include, &[]));
    }

    #[test]
    fn test_matches_globs_with_exclude() {
        let include = vec!["**/*.rs".to_string()];
        let exclude = vec!["tests/**".to_string()];
        assert!(matches_globs("src/main.rs", &include, &exclude));
        assert!(!matches_globs("tests/main.rs", &include, &exclude));
    }

    #[test]
    fn test_empty_coverage_set() {
        let rules = vec![make_rule(CoverageScope::PerFile, 85)];
        let results = evaluate_file_thresholds(&rules, &[], &[], &[]);
        assert!(results.is_empty());
    }

    #[test]
    fn test_global_empty_coverage_is_100() {
        let rules = vec![make_rule(CoverageScope::Global, 85)];
        let results = evaluate_aggregate_thresholds(&rules, &[]);
        // No files = 100% (nothing to cover)
        assert!(results[0].pass);
    }
}
