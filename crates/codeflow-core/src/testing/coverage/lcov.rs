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

    for (index, line) in content.split_terminator('\n').enumerate() {
        let invalid = || TestingError::CoverageParseError {
            path: source_path.to_path_buf(),
            message: format!("line {}: malformed LCOV record", index + 1),
        };
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            continue;
        }

        if let Some(sf) = line.strip_prefix("SF:") {
            current_file = Some(sf.to_string());
            lines_found = 0;
            lines_hit = 0;
        } else if let Some(da) = line.strip_prefix("DA:") {
            // LCOV permits an optional checksum after the execution count.
            let parts: Vec<_> = da.split(',').collect();
            if !(2..=3).contains(&parts.len()) || current_file.is_none() {
                return Err(invalid());
            }
            parts[0].parse::<u64>().map_err(|_| invalid())?;
            let count = parts[1].parse::<u64>().map_err(|_| invalid())?;
            lines_found += 1;
            if count > 0 {
                lines_hit += 1;
            }
        } else if let Some(lf) = line.strip_prefix("LF:") {
            lines_found = lf.parse().map_err(|_| invalid())?;
        } else if let Some(lh) = line.strip_prefix("LH:") {
            lines_hit = lh.parse().map_err(|_| invalid())?;
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

#[cfg(test)]
mod r15_text_regressions {
    #[test]
    fn r15_lcov_keeps_filename_unicode_space() {
        let rows = super::parse_lcov_str(
            "SF:file.rs\u{a0}\nDA:1,1\nend_of_record\n",
            std::path::Path::new("coverage.info"),
        )
        .unwrap();
        assert_eq!(rows[0].path, "file.rs\u{a0}");
    }
}

#[cfg(test)]
mod r22_regressions {
    use super::*;

    #[test]
    fn r22_lcov_malformed_records_name_file_and_line() {
        let path = Path::new("cov.info");
        assert!(parse_lcov_str("", path).unwrap().is_empty());
        for record in ["DA:1", "DA:x,1", "DA:1,x", "LF:x", "LH:x"] {
            let error = parse_lcov_str(&format!("SF:file.rs\n{record}\nend_of_record\n"), path)
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("cov.info") && error.contains("line 2"),
                "{error}"
            );
        }
        assert_eq!(
            parse_lcov_str("SF:a\nDA:1,1,checksum\nend_of_record\n", path).unwrap()[0].lines_hit,
            1
        );
    }
}
