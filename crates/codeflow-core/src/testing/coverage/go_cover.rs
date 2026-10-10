//! Go coverage profile parser (`go test -coverprofile`).

use std::collections::HashMap;
use std::path::Path;

use crate::testing::coverage::FileCoverage;
use crate::testing::error::TestingError;

/// Parse a Go coverage profile (coverage.out).
///
/// # Errors
///
/// Returns `TestingError::CoverageParseError` on I/O failure.
pub fn parse_go_cover(path: &Path) -> Result<Vec<FileCoverage>, TestingError> {
    let content = std::fs::read_to_string(path).map_err(|e| TestingError::CoverageParseError {
        path: path.to_path_buf(),
        message: format!("failed to read file: {e}"),
    })?;

    parse_go_cover_str(&content, path)
}

/// Parse Go coverage from a string.
pub(crate) fn parse_go_cover_str(
    content: &str,
    source_path: &Path,
) -> Result<Vec<FileCoverage>, TestingError> {
    // Go coverage format:
    // mode: set|count|atomic
    // file:start_line.start_col,end_line.end_col num_stmts count
    let mut file_data: HashMap<String, (u64, u64)> = HashMap::new();

    for (index, line) in content.split_terminator('\n').enumerate() {
        let invalid = || TestingError::CoverageParseError {
            path: source_path.to_path_buf(),
            message: format!("line {}: malformed Go coverage record", index + 1),
        };
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        if line.starts_with("mode:") {
            if !matches!(line, "mode: set" | "mode: count" | "mode: atomic") {
                return Err(invalid());
            }
            continue;
        }

        // Parse: file:start.col,end.col stmts count
        let colon_pos = line.rfind(':').ok_or_else(invalid)?;

        let file_path = &line[..colon_pos];
        let rest = &line[colon_pos + 1..];

        // Split rest by whitespace to get the last two fields
        let parts: Vec<&str> = rest.split(' ').filter(|part| !part.is_empty()).collect();
        if parts.len() != 3 || file_path.is_empty() {
            return Err(invalid());
        }
        let (start, end) = parts[0].split_once(',').ok_or_else(invalid)?;
        for position in [start, end] {
            let (line, column) = position.split_once('.').ok_or_else(invalid)?;
            line.parse::<u64>().map_err(|_| invalid())?;
            column.parse::<u64>().map_err(|_| invalid())?;
        }
        let num_stmts: u64 = parts[1].parse().map_err(|_| invalid())?;
        let count: u64 = parts[2].parse().map_err(|_| invalid())?;

        let entry = file_data.entry(file_path.to_string()).or_insert((0, 0));
        entry.0 += num_stmts;
        if count > 0 {
            entry.1 += num_stmts;
        }
    }

    let mut results: Vec<FileCoverage> = file_data
        .into_iter()
        .map(|(path, (found, hit))| {
            let percent = FileCoverage::compute_percent(found, hit);
            FileCoverage {
                path,
                lines_found: found,
                lines_hit: hit,
                percent,
            }
        })
        .collect();

    results.sort_by(|a, b| a.path.cmp(&b.path));

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
    fn test_parse_go_cover_basic() {
        let content = "mode: set\nexample.com/pkg/main.go:1.1,5.1 3 1\nexample.com/pkg/main.go:6.1,10.1 2 0\n";
        let result = parse_go_cover_str(content, &path("coverage.out")).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, "example.com/pkg/main.go");
        assert_eq!(result[0].lines_found, 5);
        assert_eq!(result[0].lines_hit, 3);
        assert!((result[0].percent - 60.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_go_cover_multiple_files() {
        let content = "mode: set\na.go:1.1,3.1 2 1\nb.go:1.1,5.1 4 1\nb.go:6.1,8.1 2 0\n";
        let result = parse_go_cover_str(content, &path("coverage.out")).unwrap();
        assert_eq!(result.len(), 2);
        // Sorted alphabetically
        assert_eq!(result[0].path, "a.go");
        assert_eq!(result[0].lines_found, 2);
        assert_eq!(result[0].lines_hit, 2);
        assert_eq!(result[1].path, "b.go");
        assert_eq!(result[1].lines_found, 6);
        assert_eq!(result[1].lines_hit, 4);
    }

    #[test]
    fn test_parse_go_cover_empty() {
        let content = "mode: set\n";
        let result = parse_go_cover_str(content, &path("coverage.out")).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_go_cover_blank_input() {
        let result = parse_go_cover_str("", &path("coverage.out")).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_go_cover_file_not_found() {
        let result = parse_go_cover(&PathBuf::from("/nonexistent/coverage.out"));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_go_cover_count_mode() {
        let content = "mode: count\na.go:1.1,5.1 3 5\na.go:6.1,8.1 2 0\n";
        let result = parse_go_cover_str(content, &path("coverage.out")).unwrap();
        assert_eq!(result[0].lines_found, 5);
        assert_eq!(result[0].lines_hit, 3);
    }
}

#[cfg(test)]
mod r15_text_regressions {
    #[test]
    fn r15_go_coverage_keeps_filename_unicode_space() {
        let rows = super::parse_go_cover_str(
            "mode: set\n\u{a0}file.go:1.1,2.1 1 1\n",
            std::path::Path::new("coverage.out"),
        )
        .unwrap();
        assert_eq!(rows[0].path, "\u{a0}file.go");
    }
}

#[cfg(test)]
mod r22_regressions {
    use super::*;

    #[test]
    fn r22_go_malformed_records_name_file_and_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("coverage.out");
        std::fs::write(&path, "mode: set\n").unwrap();
        assert!(parse_go_cover(&path).unwrap().is_empty());
        for record in [
            "broken",
            "mode: unknown",
            "a.go:1.1,2.1",
            "a.go:1.1,2.1 nope 0",
            "a.go:1.1,2.1 1 nope",
            "a.go:broken 1 1",
        ] {
            std::fs::write(&path, format!("mode: set\n{record}\n")).unwrap();
            let error = parse_go_cover(&path).unwrap_err().to_string();
            assert!(
                error.contains("coverage.out") && error.contains("line 2"),
                "{error}"
            );
        }
    }
}
