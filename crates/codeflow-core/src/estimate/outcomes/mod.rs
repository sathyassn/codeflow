//! `codeflow estimate outcomes`: completed outcomes derived from git,
//! compared with frozen forecasts (issue 77).
//!
//! Read only. It writes no file, record, forecast or registry entry, and it
//! never edits a forecast: a revision stays a linked record with a reason.
//! The thresholds are printed report defaults, not policy and not a gate.

mod history;
mod render;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

pub use history::{BlockedSpan, Point, Started};

use super::adoption::{self, Adoption, Home};
use super::model::{Forecast, ScenarioName, Stage};

/// The report's options; every threshold is a flag with a printed default.
#[derive(Debug, Clone)]
pub struct Options {
    /// Only tasks completed or cancelled on this UTC date or later.
    pub since: Option<String>,
    /// Only tasks of this epic.
    pub epic: Option<String>,
    /// One forecast to join instead of the adopted home's.
    pub forecast: Option<PathBuf>,
    /// Ratios needed in a group before a median is a verdict.
    pub minimum: usize,
    /// A median ratio below this contradicts the forecast.
    pub low: f64,
    /// A median ratio above this contradicts the forecast.
    pub high: f64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            since: None,
            epic: None,
            forecast: None,
            minimum: 3,
            low: 0.5,
            high: 2.0,
        }
    }
}

/// The versioned outcomes report.
#[derive(Debug, Serialize)]
pub struct OutcomeReport {
    pub schema_version: u32,
    pub thresholds: Thresholds,
    pub adoption: String,
    pub forecasts: Vec<ForecastUse>,
    pub tasks: Vec<TaskOutcome>,
    pub groups: Vec<Group>,
    pub findings: Vec<OutcomeFinding>,
    pub limitation: &'static str,
}

/// The printed thresholds.
#[derive(Debug, Serialize)]
pub struct Thresholds {
    pub minimum: usize,
    pub low: f64,
    pub high: f64,
    pub note: &'static str,
}

/// A forecast file the report read.
#[derive(Debug, Serialize)]
pub struct ForecastUse {
    pub path: String,
    pub sha256: Option<String>,
    pub joined_tasks: usize,
}

/// One task's derived outcome.
#[derive(Debug, Serialize)]
pub struct TaskOutcome {
    pub task_id: String,
    pub title: String,
    pub work_type: String,
    pub epic_id: Option<String>,
    pub status: String,
    pub record: String,
    pub planned: Option<Point>,
    pub started: Option<Started>,
    pub blocked: Vec<BlockedSpan>,
    pub completed: Option<Point>,
    pub cancelled: Option<Point>,
    pub landed: Option<Point>,
    /// Completed (or landed, when that came first) minus started, minus the
    /// blocked time between them.
    pub elapsed_active_seconds: Option<i64>,
    /// Landed minus planned.
    pub lead_seconds: Option<i64>,
    pub forecast: Option<Prediction>,
    /// Elapsed active over the predicted active time.
    pub ratio: Option<f64>,
}

/// The planning scenario's active time for a task's package.
#[derive(Debug, Clone, Serialize)]
pub struct Prediction {
    pub path: String,
    pub package_id: String,
    /// The span of the package's implement, review, verify and rework
    /// activities in the planning scenario.
    pub predicted_active_seconds: u64,
}

/// Ratios of one work type, or of all tasks.
#[derive(Debug, Serialize)]
pub struct Group {
    pub work_type: String,
    pub count: usize,
    pub median: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// `contradicts`, `consistent` or `below_minimum`.
    pub verdict: &'static str,
}

/// A report diagnostic. A blocking one makes the command exit 1.
#[derive(Debug, Serialize)]
pub struct OutcomeFinding {
    pub code: &'static str,
    pub blocking: bool,
    pub message: String,
}

impl OutcomeReport {
    /// Whether no finding blocks.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        !self.findings.iter().any(|finding| finding.blocking)
    }

    fn finding(&mut self, code: &'static str, blocking: bool, message: String) {
        self.findings.push(OutcomeFinding {
            code,
            blocking,
            message,
        });
    }

    /// The plain-text report.
    #[must_use]
    pub fn render(&self) -> String {
        render::text(self)
    }
}

const LIMITATION: &str = "Timings come from commit author times: started is the first commit after the target (a lower bound on the work), waits are never inferred, and a ratio compares one planning scenario, not a probability.";

/// Derive the outcomes of the repository at `repo_root`. `cwd` resolves a
/// relative `--forecast`.
#[must_use]
pub fn outcomes(repo_root: &Path, cwd: &Path, options: &Options) -> OutcomeReport {
    let mut report = OutcomeReport {
        schema_version: 1,
        thresholds: Thresholds {
            minimum: options.minimum,
            low: options.low,
            high: options.high,
            note: "report defaults, not policy",
        },
        adoption: String::new(),
        forecasts: Vec::new(),
        tasks: Vec::new(),
        groups: Vec::new(),
        findings: Vec::new(),
        limitation: LIMITATION,
    };
    let predictions = forecasts(repo_root, cwd, options, &mut report);
    let Ok(repo) = git2::Repository::open(repo_root) else {
        report.finding("not_a_repository", true, "not a git repository".to_string());
        return report;
    };
    let records = match history::records(&repo) {
        Ok(records) => records,
        Err(error) => {
            report.finding("history_unreadable", true, error);
            return report;
        }
    };
    let records: Vec<_> = records
        .into_iter()
        .filter(|record| {
            options
                .epic
                .as_ref()
                .is_none_or(|epic| record.epic_id.as_deref() == Some(epic.as_str()))
        })
        .collect();
    if records.is_empty() {
        report.groups = Vec::new();
        return report;
    }
    let history = match history::History::read(&repo, &records) {
        Ok(history) => history,
        Err(error) => {
            report.finding("history_unreadable", true, error);
            return report;
        }
    };
    for record in &records {
        let timings = history.timings(record);
        // The closing point follows the current status: a task completed,
        // reopened and then cancelled closed when it was cancelled.
        let closed = if record.status == "cancelled" {
            timings.cancelled.as_ref()
        } else {
            timings.completed.as_ref()
        };
        if let Some(since) = &options.since {
            if closed.is_none_or(|point| point.at.get(..10).unwrap_or_default() < since.as_str()) {
                continue;
            }
        }
        let elapsed = elapsed_active(&timings);
        let lead = match (&timings.planned, &timings.landed) {
            (Some(planned), Some(landed)) => Some(landed.epoch_seconds - planned.epoch_seconds),
            _ => None,
        };
        let forecast = predictions.get(&record.id).cloned();
        let ratio = match (elapsed, &forecast) {
            (Some(active), Some(prediction)) if prediction.predicted_active_seconds > 0 => {
                Some(seconds_f64(active) / u64_f64(prediction.predicted_active_seconds))
            }
            _ => None,
        };
        report.tasks.push(TaskOutcome {
            task_id: record.id.clone(),
            title: record.title.clone(),
            work_type: record.work_type.clone(),
            epic_id: record.epic_id.clone(),
            status: record.status.clone(),
            record: record.path.clone(),
            planned: timings.planned,
            started: timings.started,
            blocked: timings.blocked,
            completed: timings.completed,
            cancelled: timings.cancelled,
            landed: timings.landed,
            elapsed_active_seconds: elapsed,
            lead_seconds: lead,
            forecast,
            ratio,
        });
    }
    report.groups = groups(&report.tasks, options);
    report
}

/// Completed (or landed, when that came first) minus started, minus the
/// blocked time between them; `None` without a known start.
fn elapsed_active(timings: &history::Timings) -> Option<i64> {
    match (&timings.started, &timings.completed) {
        (Some(Started::Known(started)), Some(completed)) => {
            // Work ends when it is completed, or when its code landed if a
            // later records change wrote the completion.
            let done = match &timings.landed {
                Some(landed) if landed.epoch_seconds < completed.epoch_seconds => landed,
                _ => completed,
            };
            let blocked = blocked_within(&timings.blocked, started, done);
            let active = done.epoch_seconds - started.epoch_seconds - blocked;
            (active >= 0).then_some(active)
        }
        _ => None,
    }
}

/// Seconds of the blocked spans that fall between `started` and `done`.
fn blocked_within(spans: &[BlockedSpan], started: &Point, done: &Point) -> i64 {
    spans
        .iter()
        .map(|span| {
            let from = span.from.epoch_seconds.max(started.epoch_seconds);
            let to = span
                .to
                .as_ref()
                .map_or(done.epoch_seconds, |to| to.epoch_seconds)
                .min(done.epoch_seconds);
            (to - from).max(0)
        })
        .sum()
}

#[allow(clippy::cast_precision_loss)]
fn seconds_f64(value: i64) -> f64 {
    value as f64
}

#[allow(clippy::cast_precision_loss)]
fn u64_f64(value: u64) -> f64 {
    value as f64
}

/// Read the forecasts to join and map each `codeflow_task` source to its
/// prediction: the explicit `--forecast`, else every frozen forecast under
/// the adopted home, where the first in name then version order that names
/// a task wins (the original before its revisions).
fn forecasts(
    repo_root: &Path,
    cwd: &Path,
    options: &Options,
    report: &mut OutcomeReport,
) -> BTreeMap<String, Prediction> {
    let adoption = adoption_state(repo_root, report);
    let files = forecast_files(repo_root, cwd, options, &adoption);
    let explicit = options.forecast.is_some();
    let mut predictions = BTreeMap::new();
    for (path, bytes) in files {
        let bytes = match bytes {
            Ok(bytes) => bytes,
            Err(reason) => {
                report.finding("forecast_unreadable", explicit, format!("{path}: {reason}"));
                report.forecasts.push(ForecastUse {
                    path,
                    sha256: None,
                    joined_tasks: 0,
                });
                continue;
            }
        };
        let sha256 = Some(super::sources::sha256(&bytes));
        let spans = serde_json::from_slice::<Forecast>(&bytes)
            .ok()
            .and_then(|forecast| {
                let mut shape = super::model::ForecastReport::new();
                super::shape::check(&forecast, &mut shape);
                if shape.is_valid() {
                    planned_spans(&forecast)
                } else {
                    None
                }
            });
        let Some(spans) = spans else {
            report.finding(
                "forecast_invalid",
                explicit,
                format!(
                    "{path}: not a valid version-one forecast (closed JSON schema, structure and activity intervals); nothing joined from it"
                ),
            );
            report.forecasts.push(ForecastUse {
                path,
                sha256,
                joined_tasks: 0,
            });
            continue;
        };
        let mut joined = 0;
        for (task, package, seconds) in spans {
            if let std::collections::btree_map::Entry::Vacant(entry) = predictions.entry(task) {
                entry.insert(Prediction {
                    path: path.clone(),
                    package_id: package,
                    predicted_active_seconds: seconds,
                });
                joined += 1;
            }
        }
        report.forecasts.push(ForecastUse {
            path,
            sha256,
            joined_tasks: joined,
        });
    }
    if predictions.is_empty() {
        let why = match (&adoption, report.forecasts.is_empty()) {
            (_, false) => "no forecast package names a task as its codeflow_task source",
            (Adoption::Adopted { .. }, true) => "the estimate home holds no frozen forecast",
            (_, true) => "no adopted estimate home and no --forecast",
        };
        report.finding(
            "no_forecast_joined",
            false,
            format!("no forecast joined: {why}"),
        );
    }
    predictions
}

/// Read the adoption record, describe it in the report and add a blocking
/// finding for an unreadable record or a missing or unusable home.
fn adoption_state(repo_root: &Path, report: &mut OutcomeReport) -> Adoption {
    let adoption = adoption::read(repo_root);
    report.adoption = match &adoption {
        Adoption::Absent => format!("not adopted: no {}", adoption::ADOPTION_PATH),
        Adoption::Declined => "declined".to_string(),
        Adoption::Invalid(reason) => format!("unreadable: {reason}"),
        Adoption::Adopted { root, home } => match home {
            Home::Present { .. } => format!("adopted; home {root}"),
            Home::Missing => format!("adopted; root {root} missing"),
            Home::Unusable(reason) => format!("adopted; home unusable: {reason}"),
        },
    };
    match &adoption {
        Adoption::Invalid(reason) => {
            report.finding("adoption_invalid", true, reason.clone());
        }
        Adoption::Adopted { root, home: Home::Missing } => report.finding(
            "home_missing",
            true,
            format!(
                "estimate.json adopted; root {root} missing: no profile, no frozen forecast, no outcomes"
            ),
        ),
        Adoption::Adopted { home: Home::Unusable(reason), .. } => {
            report.finding("home_unusable", true, reason.clone());
        }
        _ => {}
    }
    adoption
}

/// The forecast files to join, by display path, with their bytes or why
/// they cannot be read.
fn forecast_files(
    repo_root: &Path,
    cwd: &Path,
    options: &Options,
    adoption: &Adoption,
) -> Vec<(String, Result<Vec<u8>, String>)> {
    let mut files: Vec<(String, Result<Vec<u8>, String>)> = Vec::new();
    if let Some(explicit) = &options.forecast {
        let path = if explicit.is_absolute() {
            explicit.clone()
        } else {
            cwd.join(explicit)
        };
        let bytes = if super::sources::secret_path(&path) {
            Err("credential and secret file paths are not forecast inputs".to_string())
        } else {
            crate::bounded_file::read_bounded_regular(&path, super::model::MAX_INPUT_BYTES)
                .map_err(|_| "unreadable, not a regular file, or over 1 MiB".to_string())
        };
        files.push((explicit.display().to_string(), bytes));
    } else if let Adoption::Adopted {
        home: Home::Present { forecasts },
        ..
    } = adoption
    {
        let confined = std::fs::canonicalize(repo_root)
            .ok()
            .and_then(|root| crate::bounded_file::ConfinedRoot::open(&root).ok());
        for path in forecasts {
            if super::sources::secret_path(Path::new(path)) {
                files.push((
                    path.clone(),
                    Err("credential and secret file paths are not forecast inputs".to_string()),
                ));
                continue;
            }
            let bytes = confined.as_ref().map_or_else(
                || Err("the project root cannot be opened safely".to_string()),
                |root| {
                    root.read(Path::new(path), super::model::MAX_INPUT_BYTES)
                        .map_err(|_| "unreadable, not a regular file, or over 1 MiB".to_string())
                },
            );
            files.push((path.clone(), bytes));
        }
    }
    files
}

/// For each canonical package, the span of its implement, review, verify
/// and rework activities in the planning scenario; `None` when an activity
/// of the planning scenario overflows or ends past the horizon.
fn planned_spans(forecast: &Forecast) -> Option<Vec<(String, String, u64)>> {
    let planning = forecast
        .scenarios
        .iter()
        .find(|scenario| scenario.name == ScenarioName::Planning)?;
    let mut ends = Vec::new();
    for activity in &planning.activities {
        let end = activity
            .start_seconds
            .checked_add(activity.duration_seconds)
            .filter(|end| *end <= forecast.horizon_seconds)?;
        ends.push(end);
    }
    let mut out = Vec::new();
    for package in &forecast.packages {
        let Some(task) = package.source.task_id() else {
            continue;
        };
        let spans: Vec<(u64, u64)> = planning
            .activities
            .iter()
            .zip(&ends)
            .filter(|(activity, _)| activity.package_id.as_deref() == Some(package.id.as_str()))
            .filter(|(activity, _)| {
                matches!(
                    activity.stage,
                    Stage::Implement | Stage::Review | Stage::Verify | Stage::Rework
                )
            })
            .map(|(activity, end)| (activity.start_seconds, *end))
            .collect();
        let start = spans.iter().map(|span| span.0).min();
        let end = spans.iter().map(|span| span.1).max();
        if let (Some(start), Some(end)) = (start, end) {
            out.push((task.to_string(), package.id.clone(), end - start));
        }
    }
    Some(out)
}

fn groups(tasks: &[TaskOutcome], options: &Options) -> Vec<Group> {
    let mut by_type: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for task in tasks {
        if let Some(ratio) = task.ratio {
            by_type
                .entry(task.work_type.clone())
                .or_default()
                .push(ratio);
            by_type.entry("all".to_string()).or_default().push(ratio);
        }
    }
    by_type
        .into_iter()
        .map(|(work_type, mut ratios)| {
            ratios.sort_by(f64::total_cmp);
            let count = ratios.len();
            let median = (count > 0).then(|| {
                if count % 2 == 1 {
                    ratios[count / 2]
                } else {
                    f64::midpoint(ratios[count / 2 - 1], ratios[count / 2])
                }
            });
            let verdict = match median {
                Some(median) if count >= options.minimum => {
                    if median < options.low || median > options.high {
                        "contradicts"
                    } else {
                        "consistent"
                    }
                }
                _ => "below_minimum",
            };
            Group {
                work_type,
                count,
                median,
                min: ratios.first().copied(),
                max: ratios.last().copied(),
                verdict,
            }
        })
        .collect()
}
