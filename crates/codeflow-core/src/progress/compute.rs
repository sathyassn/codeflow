//! Scope, stage ladder, percents and locations, and the assembly of the
//! whole fact model from [`Inputs`]. Pure: nothing here reads git, files or
//! the network.

use std::collections::{BTreeMap, BTreeSet};

use super::inputs::{ForgeIn, Inputs, PullIn, ScopeSpec, TaskIn};
use super::model::{
    ContextFact, CriteriaFact, Facts, Known, LocationFact, RefFact, Rung, ScopeFact, Snapshot,
    StageCounts, TaskFact, SCHEMA_VERSION,
};

/// The stage ladder printed with every report.
pub const LADDER: [(&str, u32, &str); 5] = [
    ("not started", 0, "no claim and no task branch with commits"),
    ("in progress", 20, "a claim or a task branch with commits"),
    ("in review", 50, "an open pull request"),
    (
        "reviewed",
        75,
        "an approving review bound to the head (the record's acceptance block)",
    ),
    ("landed", 100, "landed on its home target"),
];

/// RFC 3339 UTC text for a time in seconds since the epoch.
pub(super) fn time_text(seconds: i64) -> String {
    crate::coord::format_time(u64::try_from(seconds).unwrap_or(0))
}

/// A count as a float for a mean, without a lossy cast.
pub(super) fn count_f64(count: usize) -> f64 {
    f64::from(u32::try_from(count).unwrap_or(u32::MAX))
}

/// A mean of stage percents, rounded to one decimal.
pub(super) fn mean(percents: &[u32]) -> Option<f64> {
    if percents.is_empty() {
        return None;
    }
    let sum: u64 = percents.iter().map(|value| u64::from(*value)).sum();
    let count = u64::try_from(percents.len()).unwrap_or(u64::MAX);
    // Tenths, rounded half up, so the value is exact in the output.
    let tenths = (sum * 10 * 2 + count) / (count * 2);
    Some(f64::from(u32::try_from(tenths).unwrap_or(u32::MAX)) / 10.0)
}

/// The open pull request carrying a task, when the forge was read.
pub(super) fn pull_for<'a>(inputs: &'a Inputs, task: &TaskIn) -> Option<&'a PullIn> {
    let ForgeIn::Read { pulls, .. } = &inputs.forge else {
        return None;
    };
    pulls
        .iter()
        .filter(|pull| pull.task.as_deref() == Some(task.id.as_str()))
        .filter(|pull| pull.base == task.target || pull.base == inputs.default_branch)
        .min_by_key(|pull| pull.number)
}

/// A task's stage on the ladder and its percent; `None` for a cancelled
/// task, which every denominator excludes.
pub(super) fn stage(inputs: &Inputs, task: &TaskIn) -> Option<(&'static str, u32)> {
    if task.status == "cancelled" {
        return None;
    }
    if task.status == "complete" || task.state.as_deref() == Some("landed") {
        return Some(("landed", 100));
    }
    let approved = task
        .acceptance
        .as_ref()
        .is_some_and(|block| block.approved && block.bound);
    let pull = pull_for(inputs, task);
    if approved && (pull.is_some() || task.branch.is_some()) {
        return Some(("reviewed", 75));
    }
    if pull.is_some() {
        return Some(("in review", 50));
    }
    let commits = task.branch.as_ref().is_some_and(|b| b.commits_ahead > 0);
    if commits || task.state.as_deref() == Some("active") {
        return Some(("in progress", 20));
    }
    Some(("not started", 0))
}

pub(super) fn count_stage(counts: &mut StageCounts, stage: &str) {
    match stage {
        "not started" => counts.not_started += 1,
        "in progress" => counts.in_progress += 1,
        "in review" => counts.in_review += 1,
        "reviewed" => counts.reviewed += 1,
        "landed" => counts.landed += 1,
        _ => {}
    }
}

/// The ids of the tasks in scope, sorted.
pub(super) fn scope_ids(inputs: &Inputs) -> BTreeSet<String> {
    let epics = scope_epics(inputs);
    inputs
        .tasks
        .iter()
        .filter(|task| match &inputs.scope {
            ScopeSpec::Task(id) => &task.id == id,
            ScopeSpec::Line(line) => &task.target == line,
            ScopeSpec::Epics(_) | ScopeSpec::Default => {
                task.epic.as_ref().is_some_and(|epic| epics.contains(epic))
            }
        })
        .map(|task| task.id.clone())
        .collect()
}

/// The epics the scope names or reaches, sorted.
pub(super) fn scope_epics(inputs: &Inputs) -> BTreeSet<String> {
    match &inputs.scope {
        ScopeSpec::Epics(ids) => ids.iter().cloned().collect(),
        ScopeSpec::Default => inputs
            .epics
            .iter()
            .filter(|epic| !matches!(epic.status.as_str(), "complete" | "cancelled"))
            .filter(|epic| {
                inputs.tasks.iter().any(|task| {
                    task.epic.as_deref() == Some(epic.id.as_str())
                        && task.target == inputs.current_target
                })
            })
            .map(|epic| epic.id.clone())
            .collect(),
        ScopeSpec::Task(id) => inputs
            .tasks
            .iter()
            .filter(|task| &task.id == id)
            .filter_map(|task| task.epic.clone())
            .collect(),
        ScopeSpec::Line(line) => inputs
            .tasks
            .iter()
            .filter(|task| &task.target == line)
            .filter_map(|task| task.epic.clone())
            .collect(),
    }
}

/// Why an unreadable record makes a percent over `ids` unknown, when it
/// does: a scope of one readable task is not touched by another record.
pub(super) fn unreadable_reason(inputs: &Inputs, single_task: Option<&str>) -> Option<String> {
    let relevant: Vec<&str> = inputs
        .unreadable
        .iter()
        .filter(|record| {
            record.task_id.is_some()
                && single_task.is_none_or(|id| record.task_id.as_deref() == Some(id))
        })
        .map(|record| record.path.as_str())
        .collect();
    (!relevant.is_empty()).then(|| {
        format!(
            "an unreadable task record may belong here: {}",
            relevant.join(", ")
        )
    })
}

fn ref_state(inputs: &Inputs, sha: Option<&String>) -> (&'static str, Option<String>) {
    match (sha, inputs.fetched_at) {
        (None, _) => ("unverified", Some("the ref is not present here".into())),
        (Some(_), None) => ("unverified", Some("the refs were never fetched".into())),
        (Some(_), Some(at)) if inputs.now - at > inputs.sweep_seconds => (
            "unverified",
            Some(format!(
                "fetched {} ago, longer than the sweep interval",
                duration_words(inputs.now - at)
            )),
        ),
        (Some(_), Some(_)) => ("checked", None),
    }
}

/// The refs remote claims were read from.
fn refs(inputs: &Inputs) -> Vec<RefFact> {
    inputs
        .default_ref
        .iter()
        .chain(inputs.line_refs.values())
        .map(|reference| {
            let (state, reason) = ref_state(inputs, reference.sha.as_ref());
            RefFact {
                name: reference.name.clone(),
                sha: Known::or_word(reference.sha.clone(), "unknown"),
                fetched_at: Known::or_word(inputs.fetched_at.map(time_text), "unknown"),
                state: state.into(),
                reason: Known::or_word(reason, "none"),
            }
        })
        .collect()
}

/// A duration in plain words, rounded to the largest whole unit.
#[must_use]
pub fn duration_words(seconds: i64) -> String {
    let seconds = seconds.max(0);
    if seconds >= 2 * 86_400 {
        format!("about {} days", (seconds + 43_200) / 86_400)
    } else if seconds >= 2 * 3_600 {
        format!("about {} hours", (seconds + 1_800) / 3_600)
    } else {
        format!("about {} minutes", (seconds + 30) / 60)
    }
}

/// Where a task's work is, bound to the ref or forge read it came from.
pub(super) fn location(inputs: &Inputs, task: &TaskIn) -> LocationFact {
    let mut fact = LocationFact {
        kind: "none",
        substate: Known::Word("none".into()),
        reviewed_sha: Known::Word("none".into()),
        not_the_head: false,
        note: Known::Word("none".into()),
        read_from: Known::unknown(),
        ref_sha: Known::unknown(),
        read_at: Known::unknown(),
        state: "unverified",
    };
    let landed = task.status == "complete" || task.state.as_deref() == Some("landed");
    if task.status == "cancelled" {
        fact.note = Known::Value("cancelled".into());
        return fact;
    }
    if landed {
        let on_main = task.target == inputs.default_branch || task.on_main == Some(true);
        let reference = if on_main {
            inputs.default_ref.as_ref()
        } else {
            inputs.line_refs.get(&task.target)
        };
        fact.kind = if on_main { "main" } else { "line" };
        bind_ref(inputs, &mut fact, reference);
        if !on_main && task.on_main.is_none() {
            fact.state = "unverified";
            fact.note = Known::Value("the default branch ref is not present here".into());
        }
        return fact;
    }
    if let Some(pull) = pull_for(inputs, task) {
        fact.kind = "pull request";
        let approved = task.acceptance.as_ref().filter(|block| block.approved);
        fact.substate = Known::Value(match (pull.draft, approved) {
            (true, _) => "draft".into(),
            (false, Some(block)) => format!("approved at {}", block.reviewed),
            (false, None) => "open".into(),
        });
        if let Some(block) = approved {
            fact.reviewed_sha = Known::Value(block.reviewed.clone());
            fact.not_the_head = !block.bound;
        }
        if let ForgeIn::Read { read_at, .. } = &inputs.forge {
            fact.read_from = Known::Value(format!("pull request #{}", pull.number));
            fact.ref_sha = Known::Value(pull.head_sha.clone());
            fact.read_at = Known::Value(time_text(*read_at));
            fact.state = "checked";
        }
        return fact;
    }
    if let Some(branch) = task.branch.as_ref().filter(|b| b.commits_ahead > 0) {
        fact.kind = "branch";
        fact.read_from = Known::Value(branch.name.clone());
        fact.ref_sha = Known::Value(branch.head.clone());
        fact.read_at = Known::Value(time_text(inputs.now));
        fact.state = forge_state(inputs);
        return fact;
    }
    if task.record_on_line {
        fact.note = Known::Value("record on line".into());
    }
    let reference = inputs.line_refs.get(&task.target);
    bind_ref(inputs, &mut fact, reference);
    fact
}

/// A `branch` or `none` location is only as sure as the forge read that
/// rules out an open pull request.
fn forge_state(inputs: &Inputs) -> &'static str {
    match inputs.forge {
        ForgeIn::Read { .. } => "checked",
        ForgeIn::Unverified { .. } => "unverified",
    }
}

fn bind_ref(inputs: &Inputs, fact: &mut LocationFact, reference: Option<&super::inputs::RefIn>) {
    match reference {
        Some(reference) => {
            let (state, _) = ref_state(inputs, reference.sha.as_ref());
            fact.read_from = Known::Value(reference.name.clone());
            fact.ref_sha = Known::or_word(reference.sha.clone(), "unknown");
            fact.read_at = Known::or_word(inputs.fetched_at.map(time_text), "unknown");
            fact.state = state;
        }
        None => fact.state = "unverified",
    }
}

fn criteria(task: &TaskIn) -> Known<CriteriaFact> {
    task.acceptance
        .as_ref()
        .map_or_else(Known::not_recorded, |block| {
            Known::Value(CriteriaFact {
                label: "criteria",
                verified: block.verified,
                total: block.total,
                reviewed: block.reviewed.clone(),
                state: if block.bound { "current" } else { "stale" },
            })
        })
}

/// The task facts, in id order, with the plan views' lane and window.
pub(super) fn tasks(
    inputs: &Inputs,
    ids: &BTreeSet<String>,
    plan: &super::plan::Plan,
) -> Vec<TaskFact> {
    let mut out: Vec<TaskFact> = inputs
        .tasks
        .iter()
        .filter(|task| ids.contains(&task.id))
        .map(|task| task_fact(inputs, task, plan))
        .collect();
    for record in &inputs.unreadable {
        let Some(id) = &record.task_id else { continue };
        let in_scope = match &inputs.scope {
            ScopeSpec::Task(task) => task == id,
            _ => true,
        };
        if in_scope && !ids.contains(id) {
            out.push(unreadable_task(inputs, id, &record.reason, plan));
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn task_fact(inputs: &Inputs, task: &TaskIn, plan: &super::plan::Plan) -> TaskFact {
    let staged = stage(inputs, task);
    let location = location(inputs, task);
    let window = plan.window(&task.id);
    let lane = plan.lane_of(&task.id);
    let pull = pull_for(inputs, task).map(|pull| pull.number);
    let last_moved = task
        .landed_at
        .or(task.branch.as_ref().map(|branch| branch.head_time))
        .map(time_text);
    let state = task
        .state
        .clone()
        .or_else(|| (task.status == "complete").then(|| "complete".to_string()))
        .or_else(|| (task.status == "cancelled").then(|| "cancelled".to_string()));
    TaskFact {
        id: task.id.clone(),
        title: Known::Value(task.title.clone()),
        epic: Known::or_word(task.epic.clone(), "none"),
        status: Known::Value(task.status.clone()),
        target: Known::Value(task.target.clone()),
        state: Known::or_word(state.clone(), "unknown"),
        stage: staged.map_or("cancelled", |(name, _)| name),
        percent: staged.map_or(Known::Word("excluded".into()), |(_, percent)| {
            Known::Value(f64::from(percent))
        }),
        criteria: criteria(task),
        context: ContextFact {
            outcome: Known::or_word(task.outcome.clone(), "not recorded"),
            location: location.kind,
            lane: Known::or_word(lane.clone(), "none"),
            state: Known::or_word(state, "unknown"),
            window: Known::or_word(super::plan::window_summary(&window), "not yet estimable"),
            last_moved: Known::or_word(last_moved, "unknown"),
            open_pull_request: Known::or_word(pull, "none"),
        },
        location,
        lane: Known::or_word(lane, "none"),
        window,
        theme: theme(inputs, task),
        about: Known::or_word(task.about.clone(), "not recorded"),
        depends_on: task.depends_on.clone(),
    }
}

fn unreadable_task(inputs: &Inputs, id: &str, reason: &str, plan: &super::plan::Plan) -> TaskFact {
    let mut location = location(inputs, &blank_task(id));
    location.kind = "unknown";
    location.note = Known::Value(format!("unreadable record: {reason}"));
    TaskFact {
        id: id.to_string(),
        title: Known::unknown(),
        epic: Known::unknown(),
        status: Known::unknown(),
        target: Known::unknown(),
        state: Known::unknown(),
        stage: "unknown",
        percent: Known::unknown(),
        criteria: Known::unknown(),
        context: ContextFact {
            outcome: Known::unknown(),
            location: "unknown",
            lane: Known::unknown(),
            state: Known::unknown(),
            window: Known::Word("not yet estimable".into()),
            last_moved: Known::unknown(),
            open_pull_request: Known::unknown(),
        },
        location,
        lane: Known::unknown(),
        window: plan.window(id),
        theme: "ungrouped".into(),
        about: Known::unknown(),
        depends_on: Vec::new(),
    }
}

fn blank_task(id: &str) -> TaskIn {
    TaskIn {
        id: id.to_string(),
        title: String::new(),
        epic: None,
        status: "unknown".into(),
        target: String::new(),
        depends_on: Vec::new(),
        created: None,
        state: None,
        state_reason: String::new(),
        blocker: None,
        theme: None,
        outcome: None,
        about: None,
        follow_ups: None,
        acceptance: None,
        branch: None,
        on_main: None,
        record_on_line: false,
        landed_at: None,
    }
}

/// A task's theme: its `theme:` line, else the epic section that
/// introduced it, else `ungrouped`.
pub(super) fn theme(inputs: &Inputs, task: &TaskIn) -> String {
    if let Some(theme) = &task.theme {
        return theme.clone();
    }
    task.epic
        .as_ref()
        .and_then(|epic| inputs.epics.iter().find(|record| &record.id == epic))
        .and_then(|epic| epic.sections.get(&task.id).cloned())
        .unwrap_or_else(|| "ungrouped".into())
}

/// The scope fact: its deliverable and its percent over its unique tasks.
fn scope(inputs: &Inputs, ids: &BTreeSet<String>) -> ScopeFact {
    let mut counts = StageCounts::default();
    let mut percents = Vec::new();
    let mut cancelled = Vec::new();
    for task in inputs.tasks.iter().filter(|task| ids.contains(&task.id)) {
        match stage(inputs, task) {
            Some((name, percent)) => {
                count_stage(&mut counts, name);
                percents.push(percent);
            }
            None => cancelled.push(task.id.clone()),
        }
    }
    let single = match &inputs.scope {
        ScopeSpec::Task(id) => Some(id.as_str()),
        _ => None,
    };
    let unreadable = unreadable_reason(inputs, single);
    let reason = if let Some(reason) = unreadable {
        Some(reason)
    } else if percents.is_empty() {
        Some("no open or complete task in scope".into())
    } else {
        None
    };
    let percent = if reason.is_some() {
        None
    } else {
        mean(&percents)
    };
    ScopeFact {
        kind: inputs.scope.kind(),
        names: inputs.scope.names(),
        target: inputs.current_target.clone(),
        deliverable: Known::or_word(deliverable(inputs), "not recorded"),
        percent: Known::or_word(percent, "unknown"),
        basis: format!(
            "the mean of the stage percents of the scope's {} unique open and complete tasks; cancelled tasks are excluded",
            percents.len()
        ),
        stage_counts: counts,
        tasks: percents.len(),
        cancelled,
        unknown_reason: Known::or_word(reason, "none"),
    }
}

fn deliverable(inputs: &Inputs) -> Option<String> {
    match &inputs.scope {
        ScopeSpec::Task(id) => inputs
            .tasks
            .iter()
            .find(|task| &task.id == id)
            .and_then(|task| task.outcome.clone()),
        ScopeSpec::Epics(ids) if ids.len() == 1 => inputs
            .epics
            .iter()
            .find(|epic| epic.id == ids[0])
            .and_then(|epic| epic.outcome.clone()),
        _ => {
            let outcomes: Vec<String> = scope_epics(inputs)
                .iter()
                .filter_map(|id| inputs.epics.iter().find(|epic| &epic.id == id))
                .filter_map(|epic| {
                    epic.outcome
                        .as_ref()
                        .map(|text| format!("{}: {text}", epic.id))
                })
                .collect();
            (!outcomes.is_empty()).then(|| outcomes.join("\n"))
        }
    }
}

fn ladder() -> Vec<Rung> {
    LADDER
        .iter()
        .map(|(stage, percent, meaning)| Rung {
            stage,
            percent: *percent,
            meaning,
        })
        .collect()
}

/// Compute the whole fact model.
#[must_use]
pub fn facts(inputs: &Inputs) -> Facts {
    // The same state gives the same bytes, whatever order it was read in.
    let mut sorted = inputs.clone();
    sorted.tasks.sort_by(|a, b| a.id.cmp(&b.id));
    sorted.epics.sort_by(|a, b| a.id.cmp(&b.id));
    sorted.unreadable.sort_by(|a, b| a.path.cmp(&b.path));
    let inputs = &sorted;
    let ids = scope_ids(inputs);
    let plan = super::plan::Plan::new(inputs, &ids);
    let tasks = tasks(inputs, &ids, &plan);
    let epics = super::epics::epics(inputs, &plan);
    let issues = super::issues::issues(inputs, &ids, &plan);
    let lanes = super::issues::lanes(inputs, &ids, &plan);
    let pace = super::issues::pace(inputs, &ids, &plan);
    Facts {
        schema_version: SCHEMA_VERSION,
        kind: "codeflow.status",
        repository: inputs.repository.clone(),
        snapshot: Snapshot {
            at: time_text(inputs.now),
            zone: "UTC",
        },
        refs: refs(inputs),
        scope: scope(inputs, &ids),
        ladder: ladder(),
        epics,
        tasks,
        pulls_into_main: super::epics::pulls_into_main(inputs),
        order: plan.order_fact(),
        windows: plan.windows_fact(),
        evidence: super::issues::evidence(inputs, &plan),
        lanes,
        issues,
        pace,
        since: Known::Word("not requested".into()),
    }
}

/// Tasks by id, for lookups.
pub(super) fn by_id(inputs: &Inputs) -> BTreeMap<&str, &TaskIn> {
    inputs
        .tasks
        .iter()
        .map(|task| (task.id.as_str(), task))
        .collect()
}
