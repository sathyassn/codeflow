//! Coverage format parsers.
//!
//! Supports five coverage formats: lcov, cobertura, istanbul-summary,
//! go-cover and llvm-cov-json.

pub mod cobertura;
pub mod go_cover;
pub mod istanbul;
pub mod lcov;
pub mod llvm_cov;

/// One coverage measure over the whole report: how many items there are, how
/// many ran, and the percentage of them that ran.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CoverageCount {
    pub count: u64,
    pub covered: u64,
    pub percent: f64,
}

impl CoverageCount {
    /// The count with its percentage computed from `count` and `covered`.
    #[must_use]
    pub fn new(count: u64, covered: u64) -> Self {
        Self {
            count,
            covered,
            percent: FileCoverage::compute_percent(count, covered),
        }
    }
}

/// A report's totals. Lines come from every format; functions and regions
/// only from a format that measures them (llvm-cov-json), and are `None`
/// elsewhere rather than a guessed value.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CoverageTotals {
    pub lines: CoverageCount,
    pub functions: Option<CoverageCount>,
    pub regions: Option<CoverageCount>,
}

/// Parsed per-file coverage data.
#[derive(Debug, Clone)]
pub struct FileCoverage {
    /// File path (as reported by the coverage tool).
    pub path: String,
    /// Number of lines instrumented.
    pub lines_found: u64,
    /// Number of lines executed.
    pub lines_hit: u64,
    /// Coverage percentage (0.0 - 100.0).
    pub percent: f64,
}

impl FileCoverage {
    /// Compute percent from `lines_found` and `lines_hit`.
    ///
    /// Line counts are far below 2^52, so the `u64` -> `f64` casts are
    /// lossless in practice.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn compute_percent(lines_found: u64, lines_hit: u64) -> f64 {
        if lines_found == 0 {
            100.0
        } else {
            (lines_hit as f64 / lines_found as f64) * 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_coverage_compute_percent() {
        assert!((FileCoverage::compute_percent(100, 85) - 85.0).abs() < f64::EPSILON);
        assert!((FileCoverage::compute_percent(100, 100) - 100.0).abs() < f64::EPSILON);
        assert!((FileCoverage::compute_percent(100, 0) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_file_coverage_zero_lines() {
        // Zero lines found = 100% (no code to cover)
        assert!((FileCoverage::compute_percent(0, 0) - 100.0).abs() < f64::EPSILON);
    }
}
