//! Bounded read-only checking of explicit delivery forecast allocations.
//!
//! This module never schedules work, approves execution or writes the work
//! graph. The checker reads no adoption state and infers no duration; the
//! outcomes report reads the adoption record and derives completed tasks'
//! timings from git history. See the version-one report contracts.

pub mod adoption;
mod checks;
mod model;
pub mod outcomes;
mod shape;
mod sources;

pub use model::{
    Finding, ForecastReport, MilestoneTime, ReportExclusion, ResourceConsumption, ScenarioName,
    ScenarioReport, SourceAssurance, SourceDigest, Stage,
};

use std::path::Path;

/// Check an explicitly allocated forecast and its pinned local evidence.
///
/// Relative forecast paths are resolved against `repo_root`. All failures are
/// redacted findings in the returned report; no source content is emitted.
/// The source root is canonicalized and anchored by directory handles. Pin reads
/// reject symlink/reparse traversal and ancestor redirection, but do not establish
/// byte origin against hard links or Unix mount arrangements. Windows mounted-
/// folder reparse points are rejected; held Windows handles temporarily deny
/// deletion/rename sharing.
#[must_use]
pub fn check_forecast(repo_root: &Path, forecast_path: &Path) -> ForecastReport {
    let mut report = ForecastReport::new();
    let input = if forecast_path.is_absolute() {
        forecast_path.to_path_buf()
    } else {
        repo_root.join(forecast_path)
    };
    if sources::secret_path(&input) {
        report.finding(
            "unsafe_input_path",
            "input",
            "Credential and secret file paths are not forecast inputs.",
        );
        return report;
    }
    let Ok(bytes) = crate::bounded_file::read_bounded_regular(&input, model::MAX_INPUT_BYTES)
    else {
        report.finding(
            "input_read",
            "input",
            "Forecast is unreadable, changed, non-regular, or exceeds 1 MiB.",
        );
        return report;
    };
    report.input_sha256 = Some(sources::sha256(&bytes));
    let forecast: model::Forecast = if let Ok(forecast) = serde_json::from_slice(&bytes) {
        forecast
    } else {
        report.finding("invalid_json","input","Forecast does not match the closed JSON schema (including duplicate or unknown fields).");
        return report;
    };
    shape::check(&forecast, &mut report);
    if !report.is_valid() {
        return report;
    }
    report.checked_package_count = forecast.packages.len();
    for p in &forecast.packages {
        for e in &p.stage_exclusions {
            report.exclusions.push(ReportExclusion {
                package_id: p.id.clone(),
                stage: e.stage,
                reason: e.reason.clone(),
            });
        }
    }
    report
        .exclusions
        .sort_by(|a, b| (&a.package_id, a.stage).cmp(&(&b.package_id, b.stage)));
    let Some(mut reader) = sources::Reader::new(repo_root) else {
        report.finding(
            "project_root",
            "input",
            "Project root is not an accessible directory.",
        );
        return report;
    };
    let graph = sources::check(&forecast, &mut reader, &mut report);
    checks::check(&forecast, &graph, &mut report);
    report.source_digests.sort_by(|a, b| a.path.cmp(&b.path));
    report
        .findings
        .sort_by(|a, b| (&a.location, a.code).cmp(&(&b.location, b.code)));
    report
}

#[cfg(test)]
mod tests;
