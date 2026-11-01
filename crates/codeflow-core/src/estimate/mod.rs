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

/// Per-task end offsets a valid forecast gives, for `codeflow status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskEnds {
    /// The forecast's anchor, seconds since the epoch.
    pub anchor_epoch_seconds: u64,
    /// A pinned source changed since the forecast was frozen.
    pub stale: bool,
    /// Task id to the latest end of its activities under the favorable,
    /// planning and adverse scenarios, in seconds from the anchor.
    pub ends: std::collections::BTreeMap<String, [u64; 3]>,
}

/// Read a frozen forecast's per-task ends. A forecast that fails a check
/// other than a stale pin gives the reason instead.
///
/// # Errors
///
/// The reason when the forecast cannot be read or fails its checks.
pub fn task_ends(repo_root: &Path, forecast_path: &Path) -> Result<TaskEnds, String> {
    let report = check_forecast(repo_root, forecast_path);
    let stale = report.findings.iter().any(|f| f.code == "stale_pin");
    if let Some(finding) = report.findings.iter().find(|f| f.code != "stale_pin") {
        return Err(format!("the forecast fails its check: {}", finding.message));
    }
    let bytes = crate::bounded_file::read_bounded_regular(
        &repo_root.join(forecast_path),
        model::MAX_INPUT_BYTES,
    )
    .map_err(|error| format!("the forecast cannot be read: {error}"))?;
    let forecast: model::Forecast = serde_json::from_slice(&bytes)
        .map_err(|_| "the forecast does not match its schema".to_string())?;
    let tasks: std::collections::BTreeMap<&str, &str> = forecast
        .packages
        .iter()
        .filter_map(|p| p.source.task_id().map(|task| (p.id.as_str(), task)))
        .collect();
    let mut ends: std::collections::BTreeMap<String, [u64; 3]> = std::collections::BTreeMap::new();
    for scenario in &forecast.scenarios {
        let slot = match scenario.name {
            ScenarioName::Favorable => 0,
            ScenarioName::Planning => 1,
            ScenarioName::Adverse => 2,
        };
        for activity in &scenario.activities {
            let Some(task) = activity.package_id.as_deref().and_then(|p| tasks.get(p)) else {
                continue;
            };
            let end = activity
                .start_seconds
                .saturating_add(activity.duration_seconds);
            let entry = ends.entry((*task).to_string()).or_default();
            entry[slot] = entry[slot].max(end);
        }
    }
    Ok(TaskEnds {
        anchor_epoch_seconds: forecast.anchor_epoch_seconds,
        stale,
        ends,
    })
}

#[cfg(test)]
mod tests;
