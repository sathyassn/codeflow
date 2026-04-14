//! Integration test: concurrent worktree test isolation.
//!
//! Verifies that two worktrees running `codeflow test` concurrently
//! produce independent `.state/test-reports/` artifacts with no
//! cross-contamination.

use codeflow_core::worktree::WorktreePaths;

#[test]
fn test_worktree_paths_produce_independent_test_report_dirs() {
    let wp_a = WorktreePaths::new("/project/.git-worktrees/worktree-ses-aaa");
    let wp_b = WorktreePaths::new("/project/.git-worktrees/worktree-ses-bbb");

    let reports_a = wp_a.test_reports_dir();
    let reports_b = wp_b.test_reports_dir();

    assert_ne!(
        reports_a, reports_b,
        "test-reports dirs must differ per worktree"
    );
    assert!(
        reports_a.to_string_lossy().contains("worktree-ses-aaa"),
        "path should contain worktree ID: {reports_a:?}"
    );
    assert!(
        reports_b.to_string_lossy().contains("worktree-ses-bbb"),
        "path should contain worktree ID: {reports_b:?}"
    );
}

#[test]
fn test_worktree_paths_produce_independent_coverage_dirs() {
    let wp_a = WorktreePaths::new("/project/.git-worktrees/worktree-ses-111");
    let wp_b = WorktreePaths::new("/project/.git-worktrees/worktree-ses-222");

    let cov_a = wp_a.coverage_dir();
    let cov_b = wp_b.coverage_dir();

    assert_ne!(cov_a, cov_b, "coverage dirs must differ per worktree");
    assert!(
        cov_a.to_string_lossy().contains("worktree-ses-111"),
        "path should contain worktree ID: {cov_a:?}"
    );
    assert!(
        cov_b.to_string_lossy().contains("worktree-ses-222"),
        "path should contain worktree ID: {cov_b:?}"
    );
}

#[test]
fn test_concurrent_worktree_file_isolation() {
    // Create two real temp worktree directories and verify file writes
    // don't cross-contaminate.
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();

    let wp_a = WorktreePaths::new(dir_a.path());
    let wp_b = WorktreePaths::new(dir_b.path());

    // Create test-reports directories
    let reports_a = wp_a.test_reports_dir();
    let reports_b = wp_b.test_reports_dir();
    std::fs::create_dir_all(&reports_a).unwrap();
    std::fs::create_dir_all(&reports_b).unwrap();

    // Write a report in worktree A
    let report_a_path = reports_a.join("run-001.ctrf.json");
    std::fs::write(&report_a_path, r#"{"worktree": "a"}"#).unwrap();

    // Write a different report in worktree B
    let report_b_path = reports_b.join("run-002.ctrf.json");
    std::fs::write(&report_b_path, r#"{"worktree": "b"}"#).unwrap();

    // Verify A's directory only has A's file
    let a_files: Vec<_> = std::fs::read_dir(&reports_a)
        .unwrap()
        .filter_map(std::result::Result::ok)
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(a_files.len(), 1);
    assert_eq!(a_files[0], "run-001.ctrf.json");

    // Verify B's directory only has B's file
    let b_files: Vec<_> = std::fs::read_dir(&reports_b)
        .unwrap()
        .filter_map(std::result::Result::ok)
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(b_files.len(), 1);
    assert_eq!(b_files[0], "run-002.ctrf.json");

    // Verify content is correct (no cross-contamination)
    let a_content = std::fs::read_to_string(&report_a_path).unwrap();
    assert!(
        a_content.contains("\"a\""),
        "worktree A should have A's content"
    );
    let b_content = std::fs::read_to_string(&report_b_path).unwrap();
    assert!(
        b_content.contains("\"b\""),
        "worktree B should have B's content"
    );
}

#[test]
fn test_concurrent_coverage_dir_isolation() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();

    let wp_a = WorktreePaths::new(dir_a.path());
    let wp_b = WorktreePaths::new(dir_b.path());

    let cov_a = wp_a.coverage_dir();
    let cov_b = wp_b.coverage_dir();
    std::fs::create_dir_all(&cov_a).unwrap();
    std::fs::create_dir_all(&cov_b).unwrap();

    // Write coverage in A
    std::fs::write(cov_a.join("lcov.info"), "SF:a.rs\nend_of_record\n").unwrap();

    // Write different coverage in B
    std::fs::write(cov_b.join("lcov.info"), "SF:b.rs\nend_of_record\n").unwrap();

    // Verify isolation
    let a_content = std::fs::read_to_string(cov_a.join("lcov.info")).unwrap();
    assert!(a_content.contains("a.rs"));
    assert!(!a_content.contains("b.rs"));

    let b_content = std::fs::read_to_string(cov_b.join("lcov.info")).unwrap();
    assert!(b_content.contains("b.rs"));
    assert!(!b_content.contains("a.rs"));
}
