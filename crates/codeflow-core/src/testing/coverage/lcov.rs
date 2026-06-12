//! Lcov format parser (SF/DA/BA records).

use std::path::Path;

use crate::testing::coverage::FileCoverage;
use crate::testing::error::TestingError;

/// Parse an lcov.info file into per-file coverage data.
///
/// # Errors
///
/// Returns `TestingError::CoverageParseError` on malformed input.
pub fn parse_lcov(path: &Path) -> Result<Vec<FileCoverage>, TestingError> {
    let content = std::fs::read_to_string(path).map_err(|e| TestingError::CoverageParseError {
        path: path.to_path_buf(),
        message: format!("failed to read file: {e}"),
    })?;

    parse_lcov_str(&content, path)
}

/// Parse lcov content from a string.
pub(crate) fn parse_lcov_str(
    content: &str,
    source_path: &Path,
) -> Result<Vec<FileCoverage>, TestingError> {
    let mut results = Vec::new();
    let mut current_file: Option<String> = None;
    let mut lines_found = 0u64;
    let mut lines_hit = 0u64;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(sf) = line.strip_prefix("SF:") {
            current_file = Some(sf.to_string());
            lines_found = 0;
            lines_hit = 0;
        } else if line.starts_with("DA:") {
            // DA:line_number,execution_count
            if let Some(parts) = line.strip_prefix("DA:") {
                let parts: Vec<&str> = parts.splitn(2, ',').collect();
                if parts.len() == 2 {
                    lines_found += 1;
                    if let Ok(count) = parts[1].parse::<u64>() {
                        if count > 0 {
                            lines_hit += 1;
                        }
                    }
                }
            }
        } else if let Some(lf) = line.strip_prefix("LF:") {
            // Override with explicit lines found if present
            if let Ok(n) = lf.parse::<u64>() {
                lines_found = n;
            }
        } else if let Some(lh) = line.strip_prefix("LH:") {
            // Override with explicit lines hit if present
            if let Ok(n) = lh.parse::<u64>() {
                lines_hit = n;
            }
        } else if line == "end_of_record" {
            if let Some(ref file_path) = current_file {
                let percent = FileCoverage::compute_percent(lines_found, lines_hit);
                results.push(FileCoverage {
                    path: file_path.clone(),
                    lines_found,
                    lines_hit,
                    percent,
                });
            }
            current_file = None;
            lines_found = 0;
            lines_hit = 0;
        }
    }

    // Handle case where file doesn't end with end_of_record
    if let Some(file_path) = current_file {
        let percent = FileCoverage::compute_percent(lines_found, lines_hit);
        results.push(FileCoverage {
            path: file_path,
            lines_found,
            lines_hit,
            percent,
        });
    }

    if results.is_empty() && !content.trim().is_empty() {
        return Err(TestingError::CoverageParseError {
            path: source_path.to_path_buf(),
            message: "no SF records found in lcov data".to_string(),
        });
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn path(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn test_parse_lcov_basic() {
        let content = "SF:src/main.rs\nDA:1,1\nDA:2,1\nDA:3,0\nLF:3\nLH:2\nend_of_record\n";
        let result = parse_lcov_str(content, &path("lcov.info")).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, "src/main.rs");
        assert_eq!(result[0].lines_found, 3);
        assert_eq!(result[0].lines_hit, 2);
        assert!((result[0].percent - 66.666_666_666_666_66).abs() < 0.01);
    }

    #[test]
    fn test_parse_lcov_multiple_files() {
        let content = "\
SF:src/a.rs\nLF:10\nLH:8\nend_of_record\n\
SF:src/b.rs\nLF:20\nLH:20\nend_of_record\n";
        let result = parse_lcov_str(content, &path("lcov.info")).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].path, "src/a.rs");
        assert_eq!(result[0].lines_found, 10);
        assert_eq!(result[0].lines_hit, 8);
        assert!((result[0].percent - 80.0).abs() < f64::EPSILON);
        assert_eq!(result[1].path, "src/b.rs");
        assert!((result[1].percent - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_lcov_empty_file() {
        let result = parse_lcov_str("", &path("lcov.info")).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_lcov_no_sf_records() {
        let result = parse_lcov_str("TN:test\nsome junk\n", &path("lcov.info"));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_lcov_da_counting() {
        // When LF/LH are absent, count from DA lines
        let content = "SF:src/lib.rs\nDA:1,1\nDA:2,0\nDA:3,1\nDA:4,1\nend_of_record\n";
        let result = parse_lcov_str(content, &path("lcov.info")).unwrap();
        assert_eq!(result[0].lines_found, 4);
        assert_eq!(result[0].lines_hit, 3);
        assert!((result[0].percent - 75.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_lcov_zero_lines() {
        let content = "SF:src/empty.rs\nLF:0\nLH:0\nend_of_record\n";
        let result = parse_lcov_str(content, &path("lcov.info")).unwrap();
        assert_eq!(result[0].lines_found, 0);
        assert!((result[0].percent - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_lcov_file_not_found() {
        let result = parse_lcov(&PathBuf::from("/nonexistent/lcov.info"));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_lcov_no_end_of_record() {
        // Handle missing end_of_record gracefully
        let content = "SF:src/main.rs\nDA:1,1\nDA:2,0\n";
        let result = parse_lcov_str(content, &path("lcov.info")).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].lines_found, 2);
        assert_eq!(result[0].lines_hit, 1);
    }
}
