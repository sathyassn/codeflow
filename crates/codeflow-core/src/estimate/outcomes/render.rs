//! The plain-text outcomes report: durations in hours, or days past 48
//! hours, never in seconds or an undefined session unit.

use std::fmt::Write as _;

use super::{OutcomeReport, Point, Started};

pub(super) fn text(report: &OutcomeReport) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "estimate outcomes: {}", report.adoption);
    let t = &report.thresholds;
    let _ = writeln!(
        out,
        "thresholds: a verdict needs {} ratios; outcomes contradict the forecast when the median ratio is below {} or above {} ({})",
        t.minimum, t.low, t.high, t.note
    );
    for forecast in &report.forecasts {
        let _ = writeln!(
            out,
            "forecast: {} ({} task(s) joined)",
            forecast.path, forecast.joined_tasks
        );
    }
    if report.tasks.is_empty() {
        let _ = writeln!(out, "tasks: no complete or cancelled task record matches");
    }
    for task in &report.tasks {
        let _ = writeln!(
            out,
            "{} {} {}: {}",
            task.task_id, task.work_type, task.status, task.title
        );
        line(&mut out, "planned", task.planned.as_ref());
        match &task.started {
            Some(Started::Known(point)) => line(&mut out, "started", Some(point)),
            Some(Started::Unknown { unknown }) => {
                let _ = writeln!(out, "  started    unknown: {unknown}");
            }
            None => {}
        }
        for span in &task.blocked {
            let until = span
                .to
                .as_ref()
                .map_or_else(|| "still blocked".to_string(), describe);
            let _ = writeln!(out, "  blocked    {} until {until}", describe(&span.from));
        }
        if task.status == "cancelled" {
            line(&mut out, "cancelled", task.cancelled.as_ref());
        } else {
            line(&mut out, "completed", task.completed.as_ref());
            match &task.landed {
                Some(point) => line(&mut out, "landed", Some(point)),
                None => {
                    let _ = writeln!(out, "  landed     not on its target yet");
                }
            }
        }
        let mut summary = Vec::new();
        if let Some(active) = task.elapsed_active_seconds {
            summary.push(format!("active {}", duration(active)));
        }
        if let Some(lead) = task.lead_seconds {
            summary.push(format!("lead {}", duration(lead)));
        }
        if let Some(prediction) = &task.forecast {
            summary.push(format!(
                "planning {} ({} package {})",
                duration(i64::try_from(prediction.predicted_active_seconds).unwrap_or(i64::MAX)),
                prediction.path,
                prediction.package_id
            ));
        }
        if let Some(ratio) = task.ratio {
            summary.push(format!("ratio {ratio:.2}"));
        }
        if !summary.is_empty() {
            let _ = writeln!(out, "  {}", summary.join("; "));
        }
    }
    for group in &report.groups {
        let range = match (group.min, group.max) {
            (Some(min), Some(max)) => format!(", range {min:.2} to {max:.2}"),
            _ => String::new(),
        };
        let verdict = match group.verdict {
            "contradicts" => "outcomes contradict the forecast".to_string(),
            "consistent" => "within the thresholds".to_string(),
            _ => format!(
                "below minimum ({} of {}), no verdict",
                group.count, t.minimum
            ),
        };
        let median = match (group.verdict, group.median) {
            ("below_minimum", _) | (_, None) => String::new(),
            (_, Some(median)) => format!(", median {median:.2}"),
        };
        let _ = writeln!(
            out,
            "group {}: {} ratio(s){median}{range}: {verdict}",
            group.work_type, group.count
        );
    }
    for finding in &report.findings {
        let _ = writeln!(out, "{}: {}", finding.code, finding.message);
    }
    let _ = writeln!(out, "{}", report.limitation);
    out
}

fn line(out: &mut String, label: &str, point: Option<&Point>) {
    let text = point.map_or_else(|| "not found in history".to_string(), describe);
    let _ = writeln!(out, "  {label:<10} {text}");
}

fn describe(point: &Point) -> String {
    format!(
        "{} {}",
        point.at,
        point.commit.get(..12).unwrap_or(&point.commit)
    )
}

/// Hours with one decimal, or days past 48 hours.
fn duration(seconds: i64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let hours = seconds as f64 / 3600.0;
    if hours >= 48.0 {
        format!("{:.1} days", hours / 24.0)
    } else {
        format!("{hours:.1} hours")
    }
}
