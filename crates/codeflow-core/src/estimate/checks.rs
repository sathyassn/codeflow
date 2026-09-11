//! Validate the supplied allocation; never infer or place activities.

use super::model::{
    Boundary, Forecast, ForecastReport, Grade, MilestoneTime, ResourceConsumption, Scenario,
    ScenarioName, ScenarioReport, Stage, Window, SCENARIOS,
};
use super::sources::SourceGraph;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

fn acyclic(count: usize, edges: &[(usize, usize)]) -> bool {
    let mut incoming = vec![0; count];
    let mut next = vec![Vec::new(); count];
    for &(a, b) in edges {
        incoming[b] += 1;
        next[a].push(b);
    }
    let mut ready = incoming
        .iter()
        .enumerate()
        .filter_map(|(i, n)| (*n == 0).then_some(i))
        .collect::<VecDeque<_>>();
    let mut visited = 0;
    while let Some(i) = ready.pop_front() {
        visited += 1;
        for &j in &next[i] {
            incoming[j] -= 1;
            if incoming[j] == 0 {
                ready.push_back(j);
            }
        }
    }
    visited == count
}

pub(super) fn check(f: &Forecast, graph: &SourceGraph, report: &mut ForecastReport) {
    if !acyclic(f.packages.len(), &graph.package_edges) {
        report.finding(
            "package_cycle",
            "packages",
            "Canonical package dependencies contain a cycle.",
        );
    }
    let source_valid = report.is_valid();
    for name in SCENARIOS {
        let Some(s) = f.scenarios.iter().find(|s| s.name == name) else {
            continue;
        };
        let before = report.findings.len();
        let mut result = scenario(f, s, graph, report);
        result.valid = source_valid && report.findings.len() == before;
        if !result.valid {
            result.elapsed_seconds = None;
            result.resources.clear();
            result.milestones.clear();
        }
        report.scenarios.push(result);
    }
}

fn scenario(
    f: &Forecast,
    s: &Scenario,
    graph: &SourceGraph,
    report: &mut ForecastReport,
) -> ScenarioReport {
    let loc = format!("scenarios.{:?}", s.name).to_ascii_lowercase();
    let index = s
        .activities
        .iter()
        .enumerate()
        .map(|(i, a)| (a.id.as_str(), i))
        .collect::<BTreeMap<_, _>>();
    let packages = f
        .packages
        .iter()
        .enumerate()
        .map(|(i, p)| (p.id.as_str(), i))
        .collect::<BTreeMap<_, _>>();
    let mut ends = Vec::new();
    let mut elapsed = 0;
    for a in &s.activities {
        let end = a.start_seconds.checked_add(a.duration_seconds);
        if end.is_none_or(|end| {
            end > f.horizon_seconds || f.anchor_epoch_seconds.checked_add(end).is_none()
        }) {
            report.finding(
                "activity_interval",
                &loc,
                "Activity interval overflows or lies outside the finite horizon.",
            );
        }
        if let Some(end) = end {
            elapsed = elapsed.max(end);
        }
        ends.push(end);
        if a.package_id
            .as_ref()
            .is_some_and(|id| !packages.contains_key(id.as_str()))
        {
            report.finding(
                "unknown_package",
                &loc,
                "Activity package reference does not resolve.",
            );
        }
    }
    activity_edges(f, s, &ends, &index, &packages, report, &loc);
    let spans = package_spans(f, s, &ends, report, &loc);
    for &(pred, succ) in &graph.package_edges {
        if spans[pred]
            .zip(spans[succ])
            .is_some_and(|((_, finish), (start, _))| finish > start)
        {
            report.finding(
                "package_precedence",
                &loc,
                "Canonical predecessor package must finish before dependent package starts.",
            );
        }
    }
    for &(boundary, package) in &graph.boundary_edges {
        if spans[package]
            .is_some_and(|(start, _)| available(&f.boundaries[boundary], s.name) > start)
        {
            report.finding(
                "boundary_precedence",
                &loc,
                "Package starts before its canonical boundary is available.",
            );
        }
    }
    let resources = resource_totals(f, s, &ends, report, &loc);
    let mut milestones = Vec::new();
    for m in &s.milestones {
        let mut at = 0;
        for id in &m.after {
            if let Some(&i) = index.get(id.as_str()) {
                at = at.max(ends[i].unwrap_or(0));
            } else {
                report.finding(
                    "unknown_milestone_activity",
                    &loc,
                    "Milestone activity reference does not resolve.",
                );
            }
        }
        milestones.push(MilestoneTime {
            id: m.id.clone(),
            at_seconds: at,
        });
    }
    milestones.sort_by(|a, b| a.id.cmp(&b.id));
    ScenarioReport {
        name: s.name,
        valid: false,
        elapsed_seconds: Some(elapsed),
        resources,
        milestones,
    }
}

fn available(b: &Boundary, name: ScenarioName) -> u64 {
    b.availability
        .iter()
        .find(|a| a.scenario == name)
        .map_or(0, |a| a.available_at_seconds)
}

fn package_spans(
    f: &Forecast,
    s: &Scenario,
    ends: &[Option<u64>],
    report: &mut ForecastReport,
    loc: &str,
) -> Vec<Option<(u64, u64)>> {
    let mut spans = Vec::new();
    for p in &f.packages {
        let activities = s
            .activities
            .iter()
            .enumerate()
            .filter(|(_, a)| a.package_id.as_deref() == Some(p.id.as_str()))
            .collect::<Vec<_>>();
        if activities.is_empty() {
            report.finding(
                "missing_allocation",
                loc,
                "Every package requires an explicit allocation in each scenario.",
            );
            spans.push(None);
            continue;
        }
        let stages = activities
            .iter()
            .map(|(_, a)| a.stage)
            .collect::<BTreeSet<_>>();
        for required in [
            Stage::Implement,
            Stage::Review,
            Stage::Verify,
            Stage::Rework,
        ] {
            if !stages.contains(&required)
                && !p.stage_exclusions.iter().any(|e| e.stage == required)
            {
                report.finding(
                    "missing_stage",
                    loc,
                    "Required package stage is absent without an explicit exclusion.",
                );
            }
        }
        if p.grade == Grade::Unsized
            && (stages.contains(&Stage::Implement) || !stages.contains(&Stage::Discovery))
        {
            report.finding(
                "unsized_implementation",
                loc,
                "UNSIZED work requires bounded discovery and cannot allocate implementation.",
            );
        }
        let start = activities
            .iter()
            .map(|(_, a)| a.start_seconds)
            .min()
            .unwrap_or(0);
        let finish = activities.iter().filter_map(|(i, _)| ends[*i]).max();
        spans.push(finish.map(|finish| (start, finish)));
    }
    spans
}

fn contiguous_spans(windows: &[Window]) -> Vec<(u64, u64)> {
    // Shape validation already proved sorting and non-overlap. Abutting records
    // form continuous availability; a positive gap must remain a separate span.
    let mut spans = Vec::<(u64, u64)>::new();
    for window in windows {
        match spans.last_mut() {
            Some((_, end)) if *end == window.start_seconds => *end = window.end_seconds,
            _ => spans.push((window.start_seconds, window.end_seconds)),
        }
    }
    spans
}

fn resource_totals(
    f: &Forecast,
    s: &Scenario,
    ends: &[Option<u64>],
    report: &mut ForecastReport,
    loc: &str,
) -> Vec<ResourceConsumption> {
    for a in &s.activities {
        for d in &a.demands {
            if !f.resources.iter().any(|r| r.id == d.resource_id) {
                report.finding("unknown_resource", loc, "Resource demand does not resolve.");
            }
        }
    }
    let mut totals = Vec::new();
    for r in &f.resources {
        let availability = contiguous_spans(&r.windows);
        let mut events = Vec::new();
        let mut consumption = Some(0_u64);
        for (i, a) in s.activities.iter().enumerate() {
            let Some(d) = a.demands.iter().find(|d| d.resource_id == r.id) else {
                continue;
            };
            let Some(end) = ends[i] else {
                continue;
            };
            if !availability
                .iter()
                .any(|&(start, finish)| start <= a.start_seconds && end <= finish)
            {
                report.finding(
                    "resource_calendar",
                    loc,
                    "A non-preemptive activity must fit one contiguous availability span.",
                );
            }
            events.push((a.start_seconds, true, d.units));
            events.push((end, false, d.units));
            consumption = consumption.and_then(|n| {
                d.units
                    .checked_mul(a.duration_seconds)
                    .and_then(|v| n.checked_add(v))
            });
        }
        events.sort_unstable();
        let mut occupied = 0_u64;
        for (_, start, units) in events {
            let next = if start {
                occupied.checked_add(units)
            } else {
                occupied.checked_sub(units)
            };
            if let Some(value) = next {
                occupied = value;
                if occupied > r.capacity {
                    report.finding(
                        "resource_capacity",
                        loc,
                        "Simultaneous resource demands exceed declared capacity.",
                    );
                    break;
                }
            } else {
                report.finding(
                    "resource_overflow",
                    loc,
                    "Resource demand arithmetic overflows.",
                );
                break;
            }
        }
        if let Some(consumption_seconds) = consumption {
            totals.push(ResourceConsumption {
                resource_id: r.id.clone(),
                consumption_seconds,
            });
        } else {
            report.finding(
                "resource_overflow",
                loc,
                "Resource consumption arithmetic overflows.",
            );
        }
    }
    totals.sort_by(|a, b| a.resource_id.cmp(&b.resource_id));
    totals
}

fn activity_edges(
    f: &Forecast,
    s: &Scenario,
    ends: &[Option<u64>],
    index: &BTreeMap<&str, usize>,
    packages: &BTreeMap<&str, usize>,
    report: &mut ForecastReport,
    loc: &str,
) {
    let mut edges = Vec::new();
    for (i, a) in s.activities.iter().enumerate() {
        for predecessor in &a.after {
            let Some(&j) = index.get(predecessor.as_str()) else {
                report.finding(
                    "unknown_activity",
                    loc,
                    "Activity predecessor does not resolve.",
                );
                continue;
            };
            edges.push((j, i));
            if ends[j].is_some_and(|end| end > a.start_seconds) {
                report.finding(
                    "activity_precedence",
                    loc,
                    "Activity starts before a predecessor finishes.",
                );
            }
            if let (Some(left), Some(right)) = (&s.activities[j].package_id, &a.package_id) {
                if left != right {
                    let external = packages
                        .get(left.as_str())
                        .zip(packages.get(right.as_str()))
                        .is_some_and(|(&x, &y)| {
                            f.packages[x].source.task_id().is_none()
                                && f.packages[y].source.task_id().is_none()
                        });
                    if !external {
                        report.finding("copied_package_edge",loc,"Cross-package activity edges require two external/declared sources; canonical dependencies come from work records.");
                    }
                }
            }
        }
        for id in &a.after_boundaries {
            match f.boundaries.iter().find(|b| b.id == *id) {
                Some(b) if available(b, s.name) <= a.start_seconds => {}
                Some(_) => report.finding(
                    "boundary_precedence",
                    loc,
                    "Activity starts before declared boundary availability.",
                ),
                None => report.finding(
                    "unknown_boundary",
                    loc,
                    "Activity boundary reference does not resolve.",
                ),
            }
        }
    }
    if !acyclic(s.activities.len(), &edges) {
        report.finding(
            "activity_cycle",
            loc,
            "Activity dependencies contain a cycle.",
        );
    }
}
