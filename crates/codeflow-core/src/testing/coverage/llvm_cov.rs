//! `llvm-cov export -format=json` parser: the report `cargo llvm-cov --json`
//! writes, with or without `--summary-only`.
//!
//! Each file's summary gives its line coverage, and the export's totals give
//! the report's line, function and region coverage. The totals are taken only
//! when they equal the sum of the file summaries, so a report whose totals
//! were changed by hand, or cut short, is refused rather than recorded.

use std::path::Path;

use serde::Deserialize;

use crate::testing::coverage::{CoverageCount, CoverageTotals, FileCoverage};
use crate::testing::error::TestingError;

/// The `type` every llvm-cov JSON export carries.
const EXPORT_TYPE: &str = "llvm.coverage.json.export";

#[derive(Debug, Deserialize)]
struct Export {
    #[serde(rename = "type")]
    kind: String,
    data: Vec<Data>,
}

#[derive(Debug, Deserialize)]
struct Data {
    files: Vec<File>,
    totals: Summary,
}

#[derive(Debug, Deserialize)]
struct File {
    filename: String,
    summary: Summary,
}

#[derive(Debug, Deserialize)]
struct Summary {
    lines: Metric,
    functions: Metric,
    regions: Metric,
}

#[derive(Debug, Deserialize)]
struct Metric {
    count: u64,
    covered: u64,
    // llvm-cov also emits `percent` (and `notcovered` for regions); the
    // percentage is recomputed from the counts, so it is not modelled.
}

/// A parsed llvm-cov export: each file's line coverage and the totals.
#[derive(Debug, Clone)]
pub struct LlvmCoverage {
    pub files: Vec<FileCoverage>,
    pub totals: CoverageTotals,
}

/// Parse an llvm-cov JSON export file.
///
/// # Errors
///
/// Returns `TestingError::CoverageParseError` on I/O or JSON failure, on an
/// export of another type or with no data, and when the totals differ from
/// the sum of the file summaries.
pub fn parse_llvm_cov(path: &Path) -> Result<LlvmCoverage, TestingError> {
    let content = std::fs::read_to_string(path).map_err(|e| TestingError::CoverageParseError {
        path: path.to_path_buf(),
        message: format!("failed to read file: {e}"),
    })?;
    parse_llvm_cov_str(&content, path)
}

/// Parse an llvm-cov JSON export from a string.
pub(crate) fn parse_llvm_cov_str(
    json: &str,
    source_path: &Path,
) -> Result<LlvmCoverage, TestingError> {
    let error = |message: String| TestingError::CoverageParseError {
        path: source_path.to_path_buf(),
        message,
    };
    let export: Export = serde_json::from_str(json).map_err(|e| error(format!("{e}")))?;
    if export.kind != EXPORT_TYPE {
        return Err(error(format!(
            "not an llvm-cov JSON export: type {:?}, expected {EXPORT_TYPE:?}",
            export.kind
        )));
    }
    if export.data.is_empty() {
        return Err(error("llvm-cov export has no data".to_string()));
    }
    let mut files = Vec::new();
    let mut totals = [(0u64, 0u64); 3];
    for data in export.data {
        let mut sums = [(0u64, 0u64); 3];
        for file in data.files {
            for (sum, metric) in sums.iter_mut().zip(metrics(&file.summary)) {
                sum.0 += metric.count;
                sum.1 += metric.covered;
            }
            files.push(FileCoverage {
                path: file.filename,
                lines_found: file.summary.lines.count,
                lines_hit: file.summary.lines.covered,
                percent: FileCoverage::compute_percent(
                    file.summary.lines.count,
                    file.summary.lines.covered,
                ),
            });
        }
        for ((name, sum), metric) in ["lines", "functions", "regions"]
            .iter()
            .zip(sums)
            .zip(metrics(&data.totals))
        {
            if sum != (metric.count, metric.covered) {
                return Err(error(format!(
                    "llvm-cov totals do not match its files: {name} total {}/{}, files {}/{}",
                    metric.covered, metric.count, sum.1, sum.0
                )));
            }
        }
        for (total, sum) in totals.iter_mut().zip(sums) {
            total.0 += sum.0;
            total.1 += sum.1;
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let [lines, functions, regions] =
        totals.map(|(count, covered)| CoverageCount::new(count, covered));
    Ok(LlvmCoverage {
        files,
        totals: CoverageTotals {
            lines,
            functions: Some(functions),
            regions: Some(regions),
        },
    })
}

fn metrics(summary: &Summary) -> [&Metric; 3] {
    [&summary.lines, &summary.functions, &summary.regions]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn summary(lines: (u64, u64), functions: (u64, u64), regions: (u64, u64)) -> String {
        format!(
            r#"{{"lines":{{"count":{},"covered":{},"percent":0}},"functions":{{"count":{},"covered":{},"percent":0}},"regions":{{"count":{},"covered":{},"notcovered":0,"percent":0}}}}"#,
            lines.0, lines.1, functions.0, functions.1, regions.0, regions.1
        )
    }

    fn export(kind: &str, totals: &str) -> String {
        format!(
            r#"{{"type":"{kind}","version":"2.0.1","data":[{{"files":[
                {{"filename":"/repo/src/a.rs","summary":{}}},
                {{"filename":"/repo/src/b.rs","summary":{}}}
            ],"totals":{totals}}}]}}"#,
            summary((10, 9), (2, 2), (20, 18)),
            summary((30, 21), (4, 3), (40, 30)),
        )
    }

    fn parse(json: &str) -> Result<LlvmCoverage, TestingError> {
        parse_llvm_cov_str(json, &PathBuf::from("coverage.json"))
    }

    #[test]
    fn reads_files_and_all_three_totals() {
        let json = export(EXPORT_TYPE, &summary((40, 30), (6, 5), (60, 48)));
        let report = parse(&json).unwrap();
        assert_eq!(report.files.len(), 2);
        assert_eq!(report.files[0].path, "/repo/src/a.rs");
        assert_eq!(
            (report.files[1].lines_found, report.files[1].lines_hit),
            (30, 21)
        );
        assert_eq!(report.totals.lines, CoverageCount::new(40, 30));
        assert!((report.totals.lines.percent - 75.0).abs() < f64::EPSILON);
        assert_eq!(report.totals.functions, Some(CoverageCount::new(6, 5)));
        assert_eq!(report.totals.regions, Some(CoverageCount::new(60, 48)));
    }

    #[test]
    fn refuses_totals_that_differ_from_the_files() {
        for totals in [
            summary((40, 31), (6, 5), (60, 48)),
            summary((40, 30), (6, 6), (60, 48)),
            summary((40, 30), (6, 5), (61, 48)),
        ] {
            let error = parse(&export(EXPORT_TYPE, &totals))
                .unwrap_err()
                .to_string();
            assert!(error.contains("do not match its files"), "{error}");
        }
    }

    #[test]
    fn refuses_another_type_no_data_and_missing_measures() {
        let totals = summary((40, 30), (6, 5), (60, 48));
        let error = parse(&export("other.export", &totals))
            .unwrap_err()
            .to_string();
        assert!(error.contains("not an llvm-cov JSON export"), "{error}");
        let error = parse(&format!(r#"{{"type":"{EXPORT_TYPE}","data":[]}}"#))
            .unwrap_err()
            .to_string();
        assert!(error.contains("no data"), "{error}");
        let no_regions = r#"{"type":"llvm.coverage.json.export","data":[{"files":[],
            "totals":{"lines":{"count":0,"covered":0},"functions":{"count":0,"covered":0}}}]}"#;
        assert!(parse(no_regions).is_err());
        assert!(parse("not json").is_err());
        assert!(parse_llvm_cov(&PathBuf::from("/nonexistent/coverage.json")).is_err());
    }
}
