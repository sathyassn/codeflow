//! Cheap structural/size checks before reading referenced evidence.

use super::model::{Forecast, ForecastReport, Pin, Source, MAX_HORIZON};
use std::collections::BTreeSet;

pub(super) fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
}

fn text(value: &str, limit: usize) -> bool {
    !value.trim().is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}

pub(super) fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn require(ok: bool, report: &mut ForecastReport, location: &str) {
    if !ok {
        report.finding(
            "invalid_field",
            location,
            "Field is empty, out of bounds, duplicated, or has an invalid value.",
        );
    }
}

fn ids<'a>(values: impl IntoIterator<Item = &'a str>, report: &mut ForecastReport, location: &str) {
    let mut seen = BTreeSet::new();
    for value in values {
        require(valid_id(value) && seen.insert(value), report, location);
    }
}

fn pin(value: &Pin, report: &mut ForecastReport, location: &str) {
    require(
        text(&value.path, 1024) && digest(&value.sha256),
        report,
        location,
    );
}

fn source(value: &Source, report: &mut ForecastReport, location: &str) {
    match value {
        Source::CodeflowTask { task_id, sha256 } => require(
            crate::workgraph::is_valid_task_format_id(task_id)
                && valid_id(task_id)
                && digest(sha256),
            report,
            location,
        ),
        Source::External { pin: p, reference } | Source::Declared { pin: p, reference } => {
            pin(p, report, location);
            require(text(reference, 2048), report, location);
        }
    }
}

fn evidence(values: &[String], report: &mut ForecastReport, location: &str) {
    require(
        !values.is_empty() && values.len() <= 256 && values.iter().all(|v| text(v, 4096)),
        report,
        location,
    );
}

pub(super) fn check(f: &Forecast, report: &mut ForecastReport) {
    if f.schema_version != 1 {
        report.finding(
            "unsupported_version",
            "schema_version",
            "Only forecast schema version 1 is supported.",
        );
    }
    require(valid_id(&f.id), report, "id");
    require(
        f.horizon_seconds > 0 && f.horizon_seconds <= MAX_HORIZON,
        report,
        "horizon_seconds",
    );
    require(
        f.anchor_epoch_seconds
            .checked_add(f.horizon_seconds)
            .is_some(),
        report,
        "anchor_epoch_seconds",
    );
    require(
        f.packages.len() + f.boundaries.len() <= 256,
        report,
        "packages",
    );
    require(f.resources.len() <= 64, report, "resources");
    require(
        f.scenarios.len() == 3
            && f.scenarios
                .iter()
                .map(|s| s.name)
                .collect::<BTreeSet<_>>()
                .len()
                == 3,
        report,
        "scenarios",
    );
    require(
        f.packages
            .iter()
            .map(|p| p.context_pins.len())
            .sum::<usize>()
            <= 256,
        report,
        "packages.context_pins",
    );
    // The byte-limited parser is complete; stop before traversing oversized arrays.
    if !report.is_valid() {
        return;
    }
    pin(&f.profile, report, "profile");
    pin(&f.rubric, report, "rubric");
    ids(
        f.packages
            .iter()
            .map(|p| p.id.as_str())
            .chain(f.boundaries.iter().map(|b| b.id.as_str())),
        report,
        "packages/boundaries.id",
    );
    ids(
        f.resources.iter().map(|r| r.id.as_str()),
        report,
        "resources.id",
    );
    ids(
        f.packages
            .iter()
            .map(|p| &p.source)
            .chain(f.boundaries.iter().map(|b| &b.source))
            .filter_map(Source::task_id),
        report,
        "sources.task_id",
    );
    check_packages(f, report);
    check_boundaries(f, report);
    check_resources(f, report);
    check_scenarios(f, report);
}

fn check_packages(f: &Forecast, report: &mut ForecastReport) {
    for (i, p) in f.packages.iter().enumerate() {
        let loc = format!("packages[{i}]");
        source(&p.source, report, &loc);
        evidence(&p.grade_evidence, report, &loc);
        let mut paths = BTreeSet::new();
        for cp in &p.context_pins {
            pin(cp, report, &loc);
            require(paths.insert(&cp.path), report, &loc);
        }
        let mut stages = BTreeSet::new();
        require(p.stage_exclusions.len() <= 8, report, &loc);
        for exclusion in &p.stage_exclusions {
            require(
                text(&exclusion.reason, 4096) && stages.insert(exclusion.stage),
                report,
                &loc,
            );
        }
    }
}

fn check_boundaries(f: &Forecast, report: &mut ForecastReport) {
    for (i, b) in f.boundaries.iter().enumerate() {
        let loc = format!("boundaries[{i}]");
        source(&b.source, report, &loc);
        require(text(&b.evidence, 4096), report, &loc);
        require(
            b.availability.len() == 3
                && b.availability
                    .iter()
                    .map(|a| a.scenario)
                    .collect::<BTreeSet<_>>()
                    .len()
                    == 3,
            report,
            &loc,
        );
        for a in &b.availability {
            require(a.available_at_seconds <= f.horizon_seconds, report, &loc);
        }
    }
}

fn check_resources(f: &Forecast, report: &mut ForecastReport) {
    for (i, r) in f.resources.iter().enumerate() {
        let loc = format!("resources[{i}]");
        require(r.capacity > 0 && r.windows.len() <= 256, report, &loc);
        let mut end = 0;
        for w in &r.windows {
            require(
                w.start_seconds >= end
                    && w.start_seconds < w.end_seconds
                    && w.end_seconds <= f.horizon_seconds,
                report,
                &loc,
            );
            end = w.end_seconds;
        }
    }
}

fn check_scenarios(f: &Forecast, report: &mut ForecastReport) {
    for (i, s) in f.scenarios.iter().enumerate() {
        let loc = format!("scenarios[{i}]");
        evidence(&s.assumptions, report, &loc);
        let edges = s
            .activities
            .iter()
            .map(|a| a.after.len() + a.after_boundaries.len())
            .sum::<usize>();
        require(
            s.activities.len() <= 1024 && s.milestones.len() <= 1024 && edges <= 4096,
            report,
            &loc,
        );
        if s.activities.len() > 1024 || edges > 4096 || s.milestones.len() > 1024 {
            continue;
        }
        ids(s.activities.iter().map(|a| a.id.as_str()), report, &loc);
        ids(s.milestones.iter().map(|m| m.id.as_str()), report, &loc);
        for (j, a) in s.activities.iter().enumerate() {
            let at = format!("{loc}.activities[{j}]");
            require(
                a.package_id.as_ref().is_none_or(|id| valid_id(id))
                    && a.duration_seconds > 0
                    && text(&a.basis, 4096),
                report,
                &at,
            );
            require(!a.demands.is_empty() && a.demands.len() <= 64, report, &at);
            ids(
                a.demands.iter().map(|d| d.resource_id.as_str()),
                report,
                &at,
            );
            for d in &a.demands {
                require(d.units > 0, report, &at);
            }
            ids(a.after.iter().map(String::as_str), report, &at);
            ids(a.after_boundaries.iter().map(String::as_str), report, &at);
        }
        for m in &s.milestones {
            require(!m.after.is_empty() && m.after.len() <= 1024, report, &loc);
            ids(m.after.iter().map(String::as_str), report, &loc);
        }
    }
}
