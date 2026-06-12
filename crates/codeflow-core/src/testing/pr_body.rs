//! PR-body emitter for test results.
//!
//! Renders the five-section Markdown block callers embed in PR bodies as
//! test evidence (summary, coverage, modified-file coverage, failures,
//! slowest tests).

use std::fmt::Write;

use crate::testing::config::CoverageException;
use crate::testing::coverage::FileCoverage;
use crate::testing::report::CanonicalTestReport;
use crate::testing::runner::TargetRunResult;
use crate::testing::threshold::ThresholdResult;

/// Milliseconds to seconds for display. Durations are far below 2^52 ms, so
/// the `u64` -> `f64` cast is lossless in practice.
#[allow(clippy::cast_precision_loss)]
fn ms_to_secs(ms: u64) -> f64 {
    ms as f64 / 1000.0
}

/// Aggregated data for one target in the PR body.
#[derive(Debug)]
pub struct TargetPrData {
    pub name: String,
    pub mode: String,
    pub report: Option<CanonicalTestReport>,
    pub run_result: Option<TargetRunResult>,
    pub file_coverages: Vec<FileCoverage>,
    pub threshold_results: Vec<ThresholdResult>,
    pub exceptions: Vec<CoverageException>,
    pub workspace_coverage: Option<f64>,
    pub coverage_summary: Option<String>,
    pub coverage_na_reason: Option<String>,
}

/// Render the PR body test results section.
///
/// Returns a Markdown string starting at the `### 1.` level (the caller adds
/// the surrounding `## Test Results` heading).
#[must_use]
pub fn render_pr_body(
    targets: &[TargetPrData],
    modified_file_results: &[(String, String, f64, u32, bool)], // (target, file, coverage, threshold, pass)
    consecutive_clean_runs: u32,
) -> String {
    if targets.is_empty() {
        return "No test targets configured. Run `codeflow test setup` to add targets.\n"
            .to_string();
    }

    let mut md = String::new();

    // Section 1: Overall Test Pass Status
    render_pass_status(&mut md, targets, consecutive_clean_runs);

    // Section 2: Overall Coverage
    render_overall_coverage(&mut md, targets);

    // Section 3: Modified File Coverage
    render_modified_file_coverage(&mut md, modified_file_results);

    // Section 4: Test Failures (only when failures exist)
    render_test_failures(&mut md, targets);

    // Section 5: Slowest Tests (optional)
    render_slowest_tests(&mut md, targets);

    md
}

fn render_pass_status(md: &mut String, targets: &[TargetPrData], consecutive_clean_runs: u32) {
    let _ = writeln!(md, "### 1. Overall Test Pass Status\n");
    let _ = writeln!(
        md,
        "| Target | Mode | Passed | Failed | Skipped | Duration |"
    );
    let _ = writeln!(
        md,
        "|--------|------|--------|--------|---------|----------|"
    );

    let mut total_passed = 0u64;
    let mut total_failed = 0u64;
    let mut total_skipped = 0u64;
    let mut total_duration_ms = 0u64;

    for t in targets {
        if let Some(ref report) = t.report {
            let s = &report.results.summary;
            let duration = t.run_result.as_ref().map_or(0, |r| r.duration_ms);
            let _ = writeln!(
                md,
                "| {} | {} | {} | {} | {} | {:.1}s |",
                t.name,
                t.mode,
                s.passed,
                s.failed,
                s.skipped,
                ms_to_secs(duration)
            );
            total_passed += s.passed;
            total_failed += s.failed;
            total_skipped += s.skipped;
            total_duration_ms += duration;
        }
    }

    let _ = writeln!(
        md,
        "| **Total** | | **{}** | **{}** | **{}** | **{:.1}s** |",
        total_passed,
        total_failed,
        total_skipped,
        ms_to_secs(total_duration_ms)
    );
    let _ = writeln!(md, "\nRuns: {consecutive_clean_runs} consecutive clean.\n");
}

fn render_overall_coverage(md: &mut String, targets: &[TargetPrData]) {
    let _ = writeln!(md, "### 2. Overall Coverage\n");
    let _ = writeln!(md, "| Target | Workspace-equivalent | Per-rule summary |");
    let _ = writeln!(md, "|--------|--------------------:|------------------|");

    let mut has_exceptions = false;

    for t in targets {
        if let Some(reason) = &t.coverage_na_reason {
            let _ = writeln!(md, "| {} | N/A | {} |", t.name, reason);
        } else if let Some(pct) = t.workspace_coverage {
            let summary = t.coverage_summary.as_deref().unwrap_or("--");
            let _ = writeln!(md, "| {} | {:.1}% | {} |", t.name, pct, summary);
        }

        if !t.exceptions.is_empty() {
            has_exceptions = true;
        }
    }

    if has_exceptions {
        let _ = writeln!(md, "\n#### Exempted Files (per-target)\n");
        let _ = writeln!(
            md,
            "| Target | File | Coverage | Configured Threshold | Reason |"
        );
        let _ = writeln!(
            md,
            "|--------|------|---------:|---------------------:|--------|"
        );

        for t in targets {
            for exc in &t.exceptions {
                let cov_pct = t
                    .file_coverages
                    .iter()
                    .find(|c| c.path == exc.file)
                    .map_or_else(|| "N/A".to_string(), |c| format!("{:.0}%", c.percent));
                let name = &t.name;
                let file = &exc.file;
                let threshold = exc.threshold;
                let reason = &exc.reason;
                let _ = writeln!(
                    md,
                    "| {name} | {file} | {cov_pct} | {threshold}% | {reason} |",
                );
            }
        }
    }
    let _ = writeln!(md);
}

fn render_modified_file_coverage(md: &mut String, results: &[(String, String, f64, u32, bool)]) {
    let _ = writeln!(md, "### 3. Modified File Coverage\n");
    let _ = writeln!(md, "| Target | File | Coverage | Threshold | Status |");
    let _ = writeln!(md, "|--------|------|---------:|----------:|--------|");

    for (target, file, coverage, threshold, pass) in results {
        let status = if *pass { "PASS" } else { "FAIL" };
        let _ = writeln!(
            md,
            "| {target} | {file} | {coverage:.0}% | {threshold}% | {status} |",
        );
    }
    let _ = writeln!(md);
}

fn render_test_failures(md: &mut String, targets: &[TargetPrData]) {
    let mut has_failures = false;
    let mut rows = Vec::new();

    for t in targets {
        if let Some(ref report) = t.report {
            for test in &report.results.tests {
                if test.status == crate::testing::report::CtrfStatus::Failed {
                    has_failures = true;
                    rows.push((
                        t.name.clone(),
                        test.suite.clone().unwrap_or_default(),
                        test.name.clone(),
                        test.message.clone().unwrap_or_default(),
                    ));
                }
            }
        }
    }

    if !has_failures {
        return;
    }

    let _ = writeln!(md, "### 4. Test Failures\n");
    let _ = writeln!(md, "| Target | Suite | Test | Message |");
    let _ = writeln!(md, "|--------|-------|------|---------|");
    for (target, suite, name, message) in &rows {
        let _ = writeln!(md, "| {target} | {suite} | {name} | {message} |");
    }
    let _ = writeln!(md);
}

fn render_slowest_tests(md: &mut String, targets: &[TargetPrData]) {
    let mut all_tests: Vec<(String, String, f64)> = Vec::new();

    for t in targets {
        if let Some(ref report) = t.report {
            for test in &report.results.tests {
                all_tests.push((t.name.clone(), test.name.clone(), test.duration));
            }
        }
    }

    if all_tests.is_empty() {
        return;
    }

    all_tests.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    let top_10: Vec<_> = all_tests.into_iter().take(10).collect();

    let _ = writeln!(md, "### 5. Slowest Tests (optional)\n");
    let _ = writeln!(md, "| Target | Test | Duration |");
    let _ = writeln!(md, "|--------|------|---------:|");
    for (target, name, duration) in &top_10 {
        let _ = writeln!(md, "| {target} | {name} | {duration:.0}ms |");
    }
    let _ = writeln!(md);
}

// ── Modified-file coverage routing (AC#38, AC#49) ──────────────────────

use crate::testing::config::{RunnerType, TargetConfig};

/// Route a changed file to the target whose cwd is the longest matching path prefix.
///
/// Implements the six-rule algorithm from design doc section 9.5:
/// 1. Normalize target cwd (empty/./missing -> root; trailing slashes stripped)
/// 2. Extension-affinity filter (cargo=*.rs, go=*.go only)
/// 3. Longest-prefix match
/// 4. Tie-break by declaration order
/// 5. Empty-cwd catch-all matches all
/// 6. No match -> "Unattributed"
#[must_use]
pub fn route_changed_file(file: &str, targets: &[TargetConfig]) -> Option<String> {
    let mut best_match: Option<(usize, usize)> = None; // (target_index, cwd_length)

    for (i, target) in targets.iter().enumerate() {
        if !target.enabled {
            continue;
        }

        let cwd = normalize_cwd(target.cwd.as_deref());

        // Rule 2: extension-affinity filter
        if !passes_extension_filter(file, &target.runner) {
            continue;
        }

        // Rule 3: longest-prefix match
        if !is_prefix_of(&cwd, file) {
            continue;
        }

        let cwd_len = cwd.len();
        match best_match {
            None => best_match = Some((i, cwd_len)),
            Some((_, best_len)) => {
                if cwd_len > best_len {
                    // Rule 3: longer prefix wins
                    best_match = Some((i, cwd_len));
                }
                // Rule 4: tie-break by declaration order (first wins, so keep existing)
            }
        }
    }

    best_match.map(|(i, _)| targets[i].name.clone())
}

/// Normalize cwd: empty/./missing -> "" (root); strip trailing slashes; strip leading ./
fn normalize_cwd(cwd: Option<&str>) -> String {
    match cwd {
        None | Some("" | ".") => String::new(),
        Some(s) => {
            let mut normalized = s.to_string();
            // Strip leading "./"
            if let Some(rest) = normalized.strip_prefix("./") {
                normalized = rest.to_string();
            }
            // Strip trailing slashes
            while normalized.ends_with('/') {
                normalized.pop();
            }
            normalized
        }
    }
}

/// Check if file passes the extension-affinity filter for a runner.
fn passes_extension_filter(file: &str, runner: &RunnerType) -> bool {
    let ext = std::path::Path::new(file)
        .extension()
        .and_then(|e| e.to_str());
    match runner {
        RunnerType::Cargo => ext == Some("rs"),
        RunnerType::Go => ext == Some("go"),
        // All other runners: no extension guard
        _ => true,
    }
}

/// Check if normalized cwd is a path prefix of the file.
fn is_prefix_of(cwd: &str, file: &str) -> bool {
    if cwd.is_empty() {
        // Empty cwd = root, matches everything (Rule 5)
        return true;
    }
    // The file must start with the cwd followed by a '/'
    file.starts_with(cwd) && file.as_bytes().get(cwd.len()) == Some(&b'/')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::report::{CanonicalTestReport, CtrfStatus, CtrfTest};

    fn make_target_data(name: &str, passed: u64, failed: u64) -> TargetPrData {
        let tests: Vec<CtrfTest> = (0..passed)
            .map(|i| CtrfTest {
                name: format!("test_{i}"),
                status: CtrfStatus::Passed,
                duration: 100.0,
                suite: Some("suite".to_string()),
                message: None,
                trace: None,
                tags: vec![],
                flaky: false,
            })
            .chain((0..failed).map(|i| CtrfTest {
                name: format!("test_fail_{i}"),
                status: CtrfStatus::Failed,
                duration: 50.0,
                suite: Some("suite".to_string()),
                message: Some(format!("assertion failed in test {i}")),
                trace: None,
                tags: vec![],
                flaky: false,
            }))
            .collect();

        let report = CanonicalTestReport::new(name, tests);
        TargetPrData {
            name: name.to_string(),
            mode: "full".to_string(),
            report: Some(report),
            run_result: Some(TargetRunResult {
                target_name: name.to_string(),
                exit_code: i32::from(failed > 0),
                stdout: String::new(),
                stderr: String::new(),
                duration_ms: 5000,
                report_path: None,
                coverage_path: None,
            }),
            file_coverages: vec![],
            threshold_results: vec![],
            exceptions: vec![],
            workspace_coverage: Some(88.5),
            coverage_summary: Some("changed_files >=85% pass".to_string()),
            coverage_na_reason: None,
        }
    }

    #[test]
    fn test_render_empty_targets() {
        let md = render_pr_body(&[], &[], 1);
        assert!(md.contains("No test targets configured"));
    }

    #[test]
    fn test_render_passing_target() {
        let targets = vec![make_target_data("rust-core", 10, 0)];
        let md = render_pr_body(&targets, &[], 2);
        assert!(md.contains("### 1. Overall Test Pass Status"));
        assert!(md.contains("rust-core"));
        assert!(md.contains("| **10** | **0**"));
        assert!(md.contains("2 consecutive clean"));
        assert!(md.contains("### 2. Overall Coverage"));
        assert!(md.contains("88.5%"));
        assert!(md.contains("### 3. Modified File Coverage"));
        // No failures section when all pass
        assert!(!md.contains("### 4. Test Failures"));
    }

    #[test]
    fn test_render_with_failures() {
        let targets = vec![make_target_data("web", 5, 2)];
        let md = render_pr_body(&targets, &[], 1);
        assert!(md.contains("### 4. Test Failures"));
        assert!(md.contains("assertion failed"));
    }

    #[test]
    fn test_render_modified_file_coverage() {
        let targets = vec![make_target_data("test", 1, 0)];
        let modified = vec![
            (
                "test".to_string(),
                "src/main.rs".to_string(),
                92.0,
                85u32,
                true,
            ),
            (
                "test".to_string(),
                "src/lib.rs".to_string(),
                78.0,
                85,
                false,
            ),
        ];
        let md = render_pr_body(&targets, &modified, 1);
        assert!(md.contains("src/main.rs"));
        assert!(md.contains("PASS"));
        assert!(md.contains("src/lib.rs"));
        assert!(md.contains("FAIL"));
    }

    #[test]
    fn test_render_with_exceptions() {
        let mut target = make_target_data("rust", 1, 0);
        target.exceptions = vec![CoverageException {
            file: "worker.rs".to_string(),
            threshold: 79,
            reason: "process spawning".to_string(),
            remove_when: "mock harness".to_string(),
        }];
        target.file_coverages = vec![FileCoverage {
            path: "worker.rs".to_string(),
            lines_found: 100,
            lines_hit: 79,
            percent: 79.0,
        }];
        let md = render_pr_body(&[target], &[], 1);
        assert!(md.contains("Exempted Files"));
        assert!(md.contains("worker.rs"));
        assert!(md.contains("79%"));
        assert!(md.contains("process spawning"));
    }

    #[test]
    fn test_render_slowest_tests() {
        let targets = vec![make_target_data("test", 5, 0)];
        let md = render_pr_body(&targets, &[], 1);
        assert!(md.contains("### 5. Slowest Tests"));
    }

    #[test]
    fn test_render_coverage_na_reason() {
        let mut target = make_target_data("node", 1, 0);
        target.workspace_coverage = None;
        target.coverage_na_reason = Some("coverage artifact not produced at lcov.info".to_string());
        let md = render_pr_body(&[target], &[], 1);
        assert!(md.contains("N/A"));
        assert!(md.contains("coverage artifact not produced"));
    }

    // ── Modified-file routing tests (AC#49, design doc §9.5) ───────

    use std::collections::BTreeMap;

    fn make_routing_target(name: &str, cwd: Option<&str>, runner: RunnerType) -> TargetConfig {
        TargetConfig {
            name: name.to_string(),
            enabled: true,
            cwd: cwd.map(String::from),
            env: BTreeMap::new(),
            runner,
            modes: BTreeMap::new(),
            report: None,
            coverage: None,
            ci_skip: None,
            ci_skip_reason: None,
            structural: None,
            tags: Vec::new(),
            test_files: Vec::new(),
        }
    }

    #[test]
    fn test_route_longest_prefix_match() {
        let targets = vec![
            make_routing_target("api", Some("services/api"), RunnerType::Custom),
            make_routing_target("web", Some("packages/web"), RunnerType::Custom),
        ];
        assert_eq!(
            route_changed_file("services/api/main.go", &targets),
            Some("api".to_string())
        );
    }

    #[test]
    fn test_route_no_match_returns_none() {
        let targets = vec![
            make_routing_target("api", Some("services/api"), RunnerType::Custom),
            make_routing_target("web", Some("packages/web"), RunnerType::Custom),
        ];
        assert_eq!(route_changed_file("README.md", &targets), None);
    }

    #[test]
    fn test_route_extension_guard_go() {
        let targets = vec![
            make_routing_target("api-go", Some("."), RunnerType::Go),
            make_routing_target("web", Some("."), RunnerType::Vitest),
        ];
        assert_eq!(
            route_changed_file("main.go", &targets),
            Some("api-go".to_string())
        );
    }

    #[test]
    fn test_route_extension_guard_ts_not_go() {
        let targets = vec![
            make_routing_target("api-go", Some("."), RunnerType::Go),
            make_routing_target("web", Some("."), RunnerType::Vitest),
        ];
        assert_eq!(
            route_changed_file("app.ts", &targets),
            Some("web".to_string())
        );
    }

    #[test]
    fn test_route_tie_break_declaration_order() {
        let targets = vec![
            make_routing_target("a", Some("services"), RunnerType::Custom),
            make_routing_target("b", Some("services"), RunnerType::Custom),
        ];
        assert_eq!(
            route_changed_file("services/api/main.rs", &targets),
            Some("a".to_string())
        );
    }

    #[test]
    fn test_route_catchall_empty_cwd() {
        let targets = vec![
            make_routing_target("api", Some("services/api"), RunnerType::Custom),
            make_routing_target("catchall", Some("."), RunnerType::Custom),
        ];
        assert_eq!(
            route_changed_file("tools/deploy.sh", &targets),
            Some("catchall".to_string())
        );
    }

    #[test]
    fn test_normalize_cwd_empty() {
        assert_eq!(normalize_cwd(None), "");
        assert_eq!(normalize_cwd(Some("")), "");
        assert_eq!(normalize_cwd(Some(".")), "");
    }

    #[test]
    fn test_normalize_cwd_strips_dot_slash() {
        assert_eq!(normalize_cwd(Some("./foo")), "foo");
    }

    #[test]
    fn test_normalize_cwd_strips_trailing_slash() {
        assert_eq!(normalize_cwd(Some("services/")), "services");
        assert_eq!(normalize_cwd(Some("services/api/")), "services/api");
    }

    #[test]
    fn test_extension_filter_cargo() {
        assert!(passes_extension_filter("src/main.rs", &RunnerType::Cargo));
        assert!(!passes_extension_filter("src/main.ts", &RunnerType::Cargo));
    }

    #[test]
    fn test_extension_filter_go() {
        assert!(passes_extension_filter("main.go", &RunnerType::Go));
        assert!(!passes_extension_filter("main.rs", &RunnerType::Go));
    }

    #[test]
    fn test_extension_filter_custom_passes_all() {
        assert!(passes_extension_filter("anything.xyz", &RunnerType::Custom));
        assert!(passes_extension_filter("main.rs", &RunnerType::Custom));
    }

    #[test]
    fn test_is_prefix_of_empty_cwd() {
        assert!(is_prefix_of("", "any/file.rs"));
    }

    #[test]
    fn test_is_prefix_of_exact_dir() {
        assert!(is_prefix_of("services/api", "services/api/main.rs"));
        assert!(!is_prefix_of("services/api", "services/api2/main.rs"));
    }

    #[test]
    fn test_route_disabled_target_skipped() {
        let mut target = make_routing_target("t", Some("."), RunnerType::Custom);
        target.enabled = false;
        let targets = vec![target];
        assert_eq!(route_changed_file("file.rs", &targets), None);
    }
}
