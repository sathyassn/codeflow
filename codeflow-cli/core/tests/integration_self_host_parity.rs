//! Integration tests for self-host PR body parity (INF-TSK-046-006, AC 7).
//!
//! Verifies that the generic engine's `render_pr_body` produces output that is
//! semantically equivalent to the fixture captured from the current production
//! output.  Uses the same two-target structure as the project's own
//! `test-config.json` (rust-core with coverage + 10 exceptions, shell-scripts
//! without coverage) to act as a regression guard.
//!
//! Tests are unit-level (no subprocess invocation) so they remain fast and
//! deterministic regardless of the environment.

use codeflow_core::testing::config::CoverageException;
use codeflow_core::testing::coverage::FileCoverage;
use codeflow_core::testing::pr_body::{TargetPrData, render_pr_body};
use codeflow_core::testing::report::{CanonicalTestReport, CtrfStatus, CtrfTest};
use codeflow_core::testing::runner::TargetRunResult;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn find_project_root() -> std::path::PathBuf {
    let mut dir = std::env::current_dir().unwrap();
    loop {
        if dir.join(".codeflow").exists() && dir.join(".claude").exists() {
            return dir;
        }
        assert!(
            dir.pop(),
            "cannot find project root (.codeflow + .claude) from {}",
            std::env::current_dir().unwrap().display()
        );
    }
}

fn load_fixture(root: &std::path::Path) -> String {
    let path = root
        .join("codeflow-cli")
        .join("core")
        .join("tests")
        .join("fixtures")
        .join("self_host_pr_body_pre_migration.md");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read fixture at {}: {e}", path.display()))
}

/// Build a `TargetRunResult` with no artifact paths (deterministic — no
/// filesystem dependency).
fn make_run_result(name: &str, duration_ms: u64) -> TargetRunResult {
    TargetRunResult {
        target_name: name.to_string(),
        exit_code: 0,
        stdout: String::new(),
        stderr: String::new(),
        duration_ms,
        report_path: None,
        coverage_path: None,
    }
}

/// Build a `CanonicalTestReport` from a flat list of (name, passed) pairs.
fn make_report(target_name: &str, passed: u64) -> CanonicalTestReport {
    let tests: Vec<CtrfTest> = (0..passed)
        .map(|i| CtrfTest {
            name: format!("test_{i}"),
            status: CtrfStatus::Passed,
            duration: 10.0,
            suite: Some("suite".to_string()),
            message: None,
            trace: None,
            tags: vec![],
            flaky: false,
        })
        .collect();
    CanonicalTestReport::new(target_name, tests)
}

/// The 10 coverage exceptions configured in the project's `test-config.json`
/// for the `rust-core` target.
fn rust_core_exceptions() -> Vec<CoverageException> {
    vec![
        CoverageException {
            file: "core/src/autorun/worker.rs".to_string(),
            threshold: 79,
            reason: "Autorun worker contains process spawning (tmux sessions, Claude Code invocation), async timeout handling, and worktree lifecycle management that require real process infrastructure to exercise. 20 tests cover all testable paths via injectable trait mocks (TmuxProvider, ClaudeProvider, WorktreeProvider). The uncovered lines are in run_worker() async execution, process output parsing, cleanup-on-crash paths, and the timeout salvage push (branches 1152-1192 in run()) which requires a real git worktree with unpushed commits to exercise — the worktree path includes a dynamically-generated session ID making pre-test setup impossible without refactoring. These paths cannot be unit-tested without spawning real tmux+Claude processes or a real git infrastructure.".to_string(),
            remove_when: String::new(),
        },
        CoverageException {
            file: "cli/src/cmd/interactive.rs".to_string(),
            threshold: 65,
            reason: "68+ tests cover all extractable pure functions and inner implementations. Remaining uncovered lines are structurally untestable: run_launch() calls exec(2) which replaces the process (cannot return to test harness); run_status/run_list/run_cleanup call detect_project_root() which requires real CWD with .git directory. Outer wrapper functions (sweep_stale_session_dirs, clean_session_worktree_map, mark_stale_worktree_entries) delegate to tested inner functions but are themselves uncovered because they construct paths from project_dir. Additionally, the --watch TUI dashboard added in INF-TSK-044-005 (run_status_tui event loop, render_session_header, render_session_table, render_session_detail at lines 757-1250) requires raw terminal mode and cannot be unit-tested, further reducing line coverage.".to_string(),
            remove_when: String::new(),
        },
        CoverageException {
            file: "cli/src/cmd/init.rs".to_string(),
            threshold: 70,
            reason: "init.rs TUI wizard contains ratatui event loop (run_tui_wizard_with_runner) and 8 step rendering functions (render_step_*) that require a live terminal in raw mode -- cannot be unit-tested without spawning a real terminal. Injectable CommandRunner trait ensures business logic (handle_*_input, check_prerequisites, run_setup, run_verification_checks, run_non_interactive) is fully tested. 54 unit tests cover all extractable pure functions.".to_string(),
            remove_when: String::new(),
        },
        CoverageException {
            file: "cli/src/cmd/autorun.rs".to_string(),
            threshold: 67,
            reason: "Large CLI command file (7500+ lines) with tmux process management, batch orchestration, and interactive terminal output. Three TUI event loops (run_status_watch, run_batches_tui, run_batch_list/detail navigation) are structurally untestable. All extractable pure functions covered by 23 unit tests.".to_string(),
            remove_when: String::new(),
        },
        CoverageException {
            file: "core/src/tui/mod.rs".to_string(),
            threshold: 0,
            reason: "Pure module declaration file (pub mod data/theme/widgets) with no executable code. llvm-cov reports 0% but there is nothing to test.".to_string(),
            remove_when: String::new(),
        },
        CoverageException {
            file: "core/src/tui/widgets/mod.rs".to_string(),
            threshold: 0,
            reason: "Pure module re-export file (pub mod + pub use) with no executable code. llvm-cov reports 0% but there is nothing to test.".to_string(),
            remove_when: String::new(),
        },
        CoverageException {
            file: "core/src/pathflow/mod.rs".to_string(),
            threshold: 0,
            reason: "Pure module declaration file (pub mod checkpoint/gates/sentinel/transitions and a file_lock re-export) with no executable code. llvm-cov reports N/A but there is nothing to test.".to_string(),
            remove_when: String::new(),
        },
        CoverageException {
            file: "core/src/testing/mod.rs".to_string(),
            threshold: 0,
            reason: "Pure module declaration file (pub mod config/coverage/doctor/error/pr_body/report/runner/setup/threshold) with no executable code. llvm-cov reports 0% but there is nothing to test.".to_string(),
            remove_when: String::new(),
        },
        CoverageException {
            file: "core/tests/integration_testing_setup.rs".to_string(),
            threshold: 0,
            reason: "Integration test file — compiled as a separate test binary, not instrumented by llvm-cov unit coverage. llvm-cov reports N/A because integration tests run in their own binary outside the library instrumentation scope.".to_string(),
            remove_when: String::new(),
        },
        CoverageException {
            file: "core/tests/integration_self_host_parity.rs".to_string(),
            threshold: 0,
            reason: "Integration test file — compiled as a separate test binary, not instrumented by llvm-cov unit coverage. llvm-cov reports N/A because integration tests run in their own binary outside the library instrumentation scope.".to_string(),
            remove_when: String::new(),
        },
    ]
}

/// Build the two-target `TargetPrData` slice that mirrors the project's own
/// `test-config.json` structure.
fn make_self_host_targets() -> Vec<TargetPrData> {
    // rust-core: coverage present, 10 configured exceptions
    let rust_core = TargetPrData {
        name: "rust-core".to_string(),
        mode: "full".to_string(),
        report: Some(make_report("rust-core", 2400)),
        run_result: Some(make_run_result("rust-core", 45_000)),
        // Synthetic per-file coverages — values don't affect section-structure assertions.
        file_coverages: vec![FileCoverage {
            path: "core/src/testing/pr_body.rs".to_string(),
            lines_found: 200,
            lines_hit: 182,
            percent: 91.0,
        }],
        threshold_results: vec![],
        exceptions: rust_core_exceptions(),
        workspace_coverage: Some(91.0),
        coverage_summary: Some("2640 files evaluated, 2631 pass, 0 fail".to_string()),
        coverage_na_reason: None,
    };

    // shell-scripts: no coverage configured
    let shell_scripts = TargetPrData {
        name: "shell-scripts".to_string(),
        mode: "full".to_string(),
        report: Some(make_report("shell-scripts", 240)),
        run_result: Some(make_run_result("shell-scripts", 8_000)),
        file_coverages: vec![],
        threshold_results: vec![],
        exceptions: vec![],
        workspace_coverage: None,
        coverage_summary: None,
        coverage_na_reason: Some("no coverage configured".to_string()),
    };

    vec![rust_core, shell_scripts]
}

// ---------------------------------------------------------------------------
// AC 7: Fixture structure — five canonical sections present
// ---------------------------------------------------------------------------

#[test]
fn pr_body_fixture_has_five_canonical_section_titles() {
    let root = find_project_root();
    let fixture = load_fixture(&root);

    assert!(
        fixture.contains("### 1. Overall Test Pass Status"),
        "fixture must contain section 1 title"
    );
    assert!(
        fixture.contains("### 2. Overall Coverage"),
        "fixture must contain section 2 title"
    );
    assert!(
        fixture.contains("### 3. Modified File Coverage"),
        "fixture must contain section 3 title"
    );
    // Section 4 (failures) is omitted when all tests pass — that is expected.
    // Section 5 (slowest) is omitted when no tests are present in the fixture — expected.
}

// ---------------------------------------------------------------------------
// AC 7: render_pr_body produces output structurally matching the fixture
// ---------------------------------------------------------------------------

#[test]
fn render_pr_body_produces_five_section_structure() {
    let targets = make_self_host_targets();
    let md = render_pr_body(&targets, &[], 1);

    assert!(
        md.contains("### 1. Overall Test Pass Status"),
        "rendered output must contain section 1"
    );
    assert!(
        md.contains("### 2. Overall Coverage"),
        "rendered output must contain section 2"
    );
    assert!(
        md.contains("### 3. Modified File Coverage"),
        "rendered output must contain section 3"
    );
    // Section 4 absent when no failures — correct
    assert!(
        !md.contains("### 4. Test Failures"),
        "section 4 must be absent when all tests pass"
    );
    // Section 5 present because report contains tests
    assert!(
        md.contains("### 5. Slowest Tests"),
        "section 5 must be present when tests exist"
    );
}

#[test]
fn render_pr_body_includes_both_target_names() {
    let targets = make_self_host_targets();
    let md = render_pr_body(&targets, &[], 1);

    assert!(
        md.contains("rust-core"),
        "rendered output must include rust-core target"
    );
    assert!(
        md.contains("shell-scripts"),
        "rendered output must include shell-scripts target"
    );
}

#[test]
fn render_pr_body_section1_has_per_target_rows_and_totals() {
    let targets = make_self_host_targets();
    let md = render_pr_body(&targets, &[], 1);

    // rust-core row: 2400 passed, 0 failed
    assert!(
        md.contains("| rust-core |"),
        "section 1 must have rust-core row"
    );
    assert!(
        md.contains("| shell-scripts |"),
        "section 1 must have shell-scripts row"
    );
    // Total row (bold markdown)
    assert!(
        md.contains("| **Total** |"),
        "section 1 must have total row"
    );
    assert!(
        md.contains("1 consecutive clean"),
        "section 1 must state consecutive clean run count"
    );
}

#[test]
fn render_pr_body_section2_has_coverage_pct_and_na_row() {
    let targets = make_self_host_targets();
    let md = render_pr_body(&targets, &[], 1);

    // rust-core has workspace_coverage set
    assert!(
        md.contains("91.0%"),
        "section 2 must contain rust-core coverage percentage"
    );
    // shell-scripts has coverage_na_reason
    assert!(
        md.contains("N/A"),
        "section 2 must contain N/A for shell-scripts"
    );
    assert!(
        md.contains("no coverage configured"),
        "section 2 must state reason for N/A"
    );
}

#[test]
fn render_pr_body_section2_exempted_files_count_matches_config() {
    // The rust-core target has exactly 10 configured exceptions in test-config.json
    // (9 original + 1 for this test file itself).
    // The rendered Exempted Files table must contain 10 rows (one per exception file).
    let targets = make_self_host_targets();
    let md = render_pr_body(&targets, &[], 1);

    assert!(
        md.contains("#### Exempted Files (per-target)"),
        "section 2 must contain Exempted Files subsection"
    );

    // Count exemption rows: they start with "| rust-core |" and contain a file path
    // (either "core/src/", "cli/src/", or "core/tests/").
    let exemption_rows = md
        .lines()
        .filter(|l| {
            l.starts_with("| rust-core |")
                && (l.contains("core/src/") || l.contains("cli/src/") || l.contains("core/tests/"))
        })
        .count();
    assert_eq!(
        exemption_rows, 10,
        "section 2 exempted files table must have exactly 10 rust-core rows; got {exemption_rows}"
    );
}

#[test]
fn render_pr_body_section2_exemption_file_names_match_config() {
    // Verify that the 10 exception file paths from test-config.json appear in the output.
    let targets = make_self_host_targets();
    let md = render_pr_body(&targets, &[], 1);

    let expected_files = [
        "core/src/autorun/worker.rs",
        "cli/src/cmd/interactive.rs",
        "cli/src/cmd/init.rs",
        "cli/src/cmd/autorun.rs",
        "core/src/tui/mod.rs",
        "core/src/tui/widgets/mod.rs",
        "core/src/pathflow/mod.rs",
        "core/src/testing/mod.rs",
        "core/tests/integration_testing_setup.rs",
        "core/tests/integration_self_host_parity.rs",
    ];
    for file in &expected_files {
        assert!(
            md.contains(file),
            "exempted files table must contain {file}"
        );
    }
}

#[test]
fn render_pr_body_section3_present_with_header_row() {
    let targets = make_self_host_targets();
    let md = render_pr_body(&targets, &[], 1);

    // Section 3 always rendered (even when no modified files)
    assert!(
        md.contains("### 3. Modified File Coverage"),
        "section 3 must be present"
    );
    assert!(
        md.contains("| Target | File | Coverage | Threshold | Status |"),
        "section 3 must have canonical header row"
    );
}

// ---------------------------------------------------------------------------
// AC 7: Fixture parity — fixture reflects current engine output shape
// ---------------------------------------------------------------------------

#[test]
fn fixture_section_titles_match_render_pr_body_output() {
    let root = find_project_root();
    let fixture = load_fixture(&root);
    let targets = make_self_host_targets();
    let md = render_pr_body(&targets, &[], 1);

    // Both fixture and live render must agree on the three always-present titles.
    for title in &[
        "### 1. Overall Test Pass Status",
        "### 2. Overall Coverage",
        "### 3. Modified File Coverage",
    ] {
        assert!(
            fixture.contains(title),
            "fixture must contain section title: {title}"
        );
        assert!(
            md.contains(title),
            "rendered output must contain section title: {title}"
        );
    }
}

#[test]
fn fixture_has_ten_exception_entries() {
    let root = find_project_root();
    let fixture = load_fixture(&root);

    // Count exemption rows: lines starting with "| rust-core |" that contain a file path.
    let exemption_rows = fixture
        .lines()
        .filter(|l| {
            l.starts_with("| rust-core |")
                && (l.contains("core/src/") || l.contains("cli/src/") || l.contains("core/tests/"))
        })
        .count();
    assert_eq!(
        exemption_rows, 10,
        "fixture must contain exactly 10 rust-core exemption rows; got {exemption_rows}"
    );
}

#[test]
fn fixture_and_render_agree_on_exempted_files_subsection_presence() {
    let root = find_project_root();
    let fixture = load_fixture(&root);
    let targets = make_self_host_targets();
    let md = render_pr_body(&targets, &[], 1);

    // Both must contain the exempted files subsection
    assert!(
        fixture.contains("Exempted Files"),
        "fixture must contain Exempted Files subsection"
    );
    assert!(
        md.contains("Exempted Files"),
        "rendered output must contain Exempted Files subsection"
    );
}

// ---------------------------------------------------------------------------
// AC 7: Empty-targets guard — no targets produces the "no config" message
// ---------------------------------------------------------------------------

#[test]
fn render_pr_body_empty_targets_produces_no_config_message() {
    let md = render_pr_body(&[], &[], 1);
    assert!(
        md.contains("No test targets configured"),
        "empty targets must produce the no-config message"
    );
    assert!(
        !md.contains("### 1."),
        "empty targets must not produce section headers"
    );
}
