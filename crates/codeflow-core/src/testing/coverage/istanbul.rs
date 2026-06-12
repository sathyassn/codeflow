//! Istanbul summary JSON coverage parser.

use std::path::Path;

use serde::Deserialize;

use crate::testing::coverage::FileCoverage;
use crate::testing::error::TestingError;

/// Istanbul summary entry for a single file.
#[derive(Debug, Deserialize)]
struct IstanbulFileSummary {
    lines: IstanbulMetric,
}

#[derive(Debug, Deserialize)]
struct IstanbulMetric {
    total: u64,
    covered: u64,
    #[allow(dead_code)]
    pct: f64,
}

/// Parse an istanbul coverage-summary.json file.
///
/// # Errors
///
/// Returns `TestingError::CoverageParseError` on I/O or JSON parse failure.
pub fn parse_istanbul(path: &Path) -> Result<Vec<FileCoverage>, TestingError> {
    let content = std::fs::read_to_string(path).map_err(|e| TestingError::CoverageParseError {
        path: path.to_path_buf(),
        message: format!("failed to read file: {e}"),
    })?;

    parse_istanbul_str(&content, path)
}

/// Parse istanbul summary from a string.
pub(crate) fn parse_istanbul_str(
    json: &str,
    source_path: &Path,
) -> Result<Vec<FileCoverage>, TestingError> {
    let data: std::collections::HashMap<String, IstanbulFileSummary> =
        serde_json::from_str(json).map_err(|e| TestingError::CoverageParseError {
            path: source_path.to_path_buf(),
            message: format!("{e}"),
        })?;

    let mut results: Vec<FileCoverage> = data
        .into_iter()
        .filter(|(key, _)| key != "total")
        .map(|(file_path, summary)| {
            let percent = FileCoverage::compute_percent(summary.lines.total, summary.lines.covered);
            FileCoverage {
                path: file_path,
                lines_found: summary.lines.total,
                lines_hit: summary.lines.covered,
                percent,
            }
        })
        .collect();

    // Sort for deterministic output
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
    fn test_parse_istanbul_basic() {
        let json = r#"{
            "total": {"lines": {"total": 100, "covered": 80, "pct": 80}},
            "src/index.js": {"lines": {"total": 50, "covered": 40, "pct": 80}},
            "src/utils.js": {"lines": {"total": 50, "covered": 40, "pct": 80}}
        }"#;
        let result = parse_istanbul_str(json, &path("summary.json")).unwrap();
        assert_eq!(result.len(), 2);
        // Sorted alphabetically
        assert_eq!(result[0].path, "src/index.js");
        assert_eq!(result[1].path, "src/utils.js");
        assert_eq!(result[0].lines_found, 50);
        assert_eq!(result[0].lines_hit, 40);
    }

    #[test]
    fn test_parse_istanbul_excludes_total() {
        let json = r#"{
            "total": {"lines": {"total": 100, "covered": 90, "pct": 90}},
            "src/app.ts": {"lines": {"total": 100, "covered": 90, "pct": 90}}
        }"#;
        let result = parse_istanbul_str(json, &path("summary.json")).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, "src/app.ts");
    }

    #[test]
    fn test_parse_istanbul_empty() {
        let json = "{}";
        let result = parse_istanbul_str(json, &path("summary.json")).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_istanbul_invalid_json() {
        let result = parse_istanbul_str("not json", &path("bad.json"));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_istanbul_file_not_found() {
        let result = parse_istanbul(&PathBuf::from("/nonexistent/summary.json"));
        assert!(result.is_err());
    }
}
