//! The progress text view of [`Facts`]: the same facts as `--json`, in
//! plain lines. Every window prints its basis and computation time and
//! durations from the snapshot, never a date.

use std::fmt::Write as _;

use super::model::{Facts, Known, WindowFact};
use super::plan::window_summary;

fn word<T: std::fmt::Display>(value: &Known<T>) -> String {
    match value {
        Known::Value(value) => value.to_string(),
        Known::Word(word) => word.clone(),
    }
}

fn percent(value: &Known<f64>) -> String {
    match value {
        Known::Value(value) => format!("{value:.1}%"),
        Known::Word(word) => word.clone(),
    }
}

fn rate(value: &Known<f64>) -> String {
    match value {
        Known::Value(value) => format!("{value:.1}"),
        Known::Word(word) => word.clone(),
    }
}

fn window(window: &WindowFact) -> String {
    // The full basis prints once, under Windows; each window names its kind.
    let basis = format!(
        "basis: {}, computed {}",
        word(&window.basis_kind),
        window.computed_at
    );
    match window_summary(window) {
        Some(summary) => format!("{summary}; {basis}"),
        None => format!("{}: {}", window.state, word(&window.reason)),
    }
}

/// Render the facts as the progress text view.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn render(facts: &Facts) -> String {
    let mut out = String::new();
    let scope = &facts.scope;
    let names = if scope.names.is_empty() {
        format!("open epics on {}", scope.target)
    } else {
        scope.names.join(", ")
    };
    let _ = writeln!(
        out,
        "Progress: {} {names}, as of {} {}",
        scope.kind, facts.snapshot.at, facts.snapshot.zone
    );
    let _ = writeln!(
        out,
        "Overall: {} of {} task(s); {}",
        percent(&scope.percent),
        scope.tasks,
        scope.basis
    );
    if let Known::Value(reason) = &scope.unknown_reason {
        if reason != "none" {
            let _ = writeln!(out, "  unknown because {reason}");
        }
    }
    let _ = writeln!(out, "Deliverable: {}", word(&scope.deliverable));
    let ladder: Vec<String> = facts
        .ladder
        .iter()
        .map(|rung| format!("{} {}%", rung.stage, rung.percent))
        .collect();
    let _ = writeln!(out, "Ladder: {}", ladder.join(", "));
    for reference in &facts.refs {
        let _ = writeln!(
            out,
            "Ref {}: {} fetched {} ({})",
            reference.name,
            word(&reference.sha),
            word(&reference.fetched_at),
            reference.state
        );
    }
    for epic in &facts.epics {
        let _ = writeln!(
            out,
            "\nEpic {} {}: {}",
            epic.id,
            word(&epic.title),
            percent(&epic.percent)
        );
        let _ = writeln!(out, "  {}", epic.reasoning);
        let _ = writeln!(
            out,
            "  done: {}",
            if epic.done.is_empty() {
                "none".into()
            } else {
                epic.done.join(", ")
            }
        );
        let _ = writeln!(
            out,
            "  not done: {}",
            if epic.not_done.is_empty() {
                "none".into()
            } else {
                epic.not_done.join(", ")
            }
        );
        let c = &epic.location_counts;
        let _ = writeln!(
            out,
            "  locations of {}: main {}, line {}, pull request {}, branch {}, none {}, unknown {}",
            c.denominator, c.main, c.line, c.pull_request, c.branch, c.none, c.unknown
        );
        let _ = writeln!(out, "  path to main:");
        for step in &epic.path_to_main {
            let state = if step.done {
                "done".to_string()
            } else {
                window(&step.window)
            };
            let _ = writeln!(
                out,
                "    {} ({}, {}): {state}",
                step.step, step.owner, step.event
            );
        }
        let _ = writeln!(out, "  hop to main: {}", epic.hop_to_main.condition);
    }
    let _ = writeln!(out, "\nTasks:");
    for task in &facts.tasks {
        let criteria = match &task.criteria {
            Known::Value(c) => format!(", criteria {}/{} ({})", c.verified, c.total, c.state),
            Known::Word(word) => format!(", criteria {word}"),
        };
        let _ = writeln!(
            out,
            "  {} {} {}{criteria}; at {} ({}); {}",
            task.id,
            task.stage,
            percent(&task.percent),
            task.location.kind,
            task.location.state,
            window(&task.window)
        );
    }
    let _ = writeln!(
        out,
        "\nOrder (critical path {}):",
        facts.order.critical_path.join(" > ")
    );
    for row in &facts.order.rows {
        let mark = if row.critical { " *critical" } else { "" };
        let waits = if row.waits_on.is_empty() {
            String::new()
        } else {
            format!(" after {}", row.waits_on.join(", "))
        };
        let _ = writeln!(
            out,
            "  {}. {} {}{waits}{mark}",
            row.position, row.id, row.state
        );
    }
    let _ = writeln!(
        out,
        "  depth {}: {}",
        facts.order.depth,
        facts.order.depth_chain.join(" > ")
    );
    let _ = writeln!(
        out,
        "\nLanes ({}): overall {}",
        facts.lanes.source,
        percent(&facts.lanes.overall_percent)
    );
    if let Known::Value(note) = &facts.lanes.note {
        let _ = writeln!(out, "  {note}");
    }
    for lane in &facts.lanes.lanes {
        let _ = writeln!(
            out,
            "  {} [{}] {}: {}",
            lane.id,
            lane.label,
            lane.tasks.join(", "),
            percent(&lane.percent)
        );
    }
    if let Known::Value(session) = &facts.lanes.session {
        for fact in session {
            let _ = writeln!(
                out,
                "  [session] lane {} task {}: builder {}, reviewer {}, observed {}",
                word(&fact.lane),
                word(&fact.task),
                word(&fact.builder),
                word(&fact.reviewer),
                fact.observed_at
            );
        }
    }
    let _ = writeln!(
        out,
        "\nWindows ({}, {} lane(s)):",
        facts.windows.basis_kind, facts.windows.lanes_used
    );
    let _ = writeln!(out, "  basis: {}", facts.windows.basis);
    for scenario in &facts.windows.scenarios {
        let _ = writeln!(
            out,
            "  {}: {} ({})",
            scenario.name, scenario.words, scenario.rate
        );
    }
    let _ = writeln!(out, "\nIssues:");
    for issue in facts.issues.tasks.iter().chain(&facts.issues.lanes) {
        let _ = writeln!(
            out,
            "  {} {}: {} (checked {}, cleared by {} {})",
            issue.subject,
            issue.kind,
            issue.detail,
            word(&issue.last_check),
            issue.unblocker.kind,
            word(&issue.unblocker.name)
        );
    }
    let operator = &facts.issues.operator;
    let _ = write!(out, "  operator: {}", operator.state);
    if !operator.unchecked.is_empty() {
        let _ = write!(out, " (not checked: {})", operator.unchecked.join(", "));
    }
    let _ = writeln!(out);
    for item in &operator.items {
        let _ = writeln!(
            out,
            "    {} [{}] {}: awaiting {}",
            item.id,
            item.source,
            item.summary,
            word(&item.awaiting)
        );
    }
    let _ = writeln!(
        out,
        "\nPace: landed {} per day observed, planned capacity {} per day, intake {} per day",
        rate(&facts.pace.observed_landing_rate_per_day),
        rate(&Known::Value(facts.pace.planned_capacity_rate_per_day)),
        rate(&facts.pace.intake_rate_per_day)
    );
    out
}
