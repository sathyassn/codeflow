//! The epic view: percent, done and not done, themes, location counts, the
//! landing chain and the path to `main`.

use std::collections::BTreeMap;

use super::compute::{
    count_stage, coverage_reason, location, mean, pull_for, scope_epics, stage, theme, time_text,
    Stage,
};
use super::inputs::{EpicIn, ForgeIn, Inputs, TaskIn};
use super::model::{
    ChainLanded, ChainPull, CheckRunFact, EpicFact, FeatureFact, HopFact, Known, LandingChain,
    LocationCounts, PullFact, StageCounts, StepFact, ThemeFact,
};
use super::plan::{done_window, is_open, not_estimable, Plan, Win};

/// One fact per epic the scope reaches, in id order.
#[must_use]
pub fn epics(inputs: &Inputs, plan: &Plan) -> Vec<EpicFact> {
    scope_epics(inputs)
        .iter()
        .map(|id| {
            let record = inputs.epics.iter().find(|epic| &epic.id == id);
            let tasks: Vec<&TaskIn> = inputs
                .tasks
                .iter()
                .filter(|task| task.epic.as_deref() == Some(id.as_str()))
                .collect();
            epic(inputs, plan, id, record, &tasks)
        })
        .collect()
}

fn epic(
    inputs: &Inputs,
    plan: &Plan,
    id: &str,
    record: Option<&EpicIn>,
    tasks: &[&TaskIn],
) -> EpicFact {
    let mut counts = StageCounts::default();
    let mut percents = Vec::new();
    let (mut done, mut not_done, mut cancelled) = (Vec::new(), Vec::new(), Vec::new());
    let mut locations = LocationCounts::default();
    for task in tasks {
        match stage(inputs, task) {
            Stage::Excluded => {
                cancelled.push(task.id.clone());
                continue;
            }
            Stage::Unknown => not_done.push(task.id.clone()),
            Stage::On(name, percent) => {
                count_stage(&mut counts, name);
                percents.push(percent);
                if percent == 100 {
                    done.push(task.id.clone());
                } else {
                    not_done.push(task.id.clone());
                }
            }
        }
        locations.denominator += 1;
        match location(inputs, task).kind {
            "main" => locations.main += 1,
            "line" => locations.line += 1,
            "pull request" => locations.pull_request += 1,
            "branch" => locations.branch += 1,
            "none" => locations.none += 1,
            _ => locations.unknown += 1,
        }
    }
    let unreadable = coverage_reason(inputs, tasks, None);
    let percent = if unreadable.is_some() {
        None
    } else {
        mean(&percents)
    };
    let reasoning = format!(
        "{} task(s) counted ({} not started, {} in progress, {} in review, {} reviewed, {} landed); the percent is their mean on the ladder{}{}",
        percents.len(),
        counts.not_started,
        counts.in_progress,
        counts.in_review,
        counts.reviewed,
        counts.landed,
        if cancelled.is_empty() { String::new() } else { format!("; {} cancelled excluded", cancelled.len()) },
        unreadable.map(|reason| format!("; unknown because {reason}")).unwrap_or_default(),
    );
    let line = home_line(inputs, tasks);
    EpicFact {
        id: id.to_string(),
        title: Known::or_word(record.map(|epic| epic.title.clone()), "unknown"),
        status: Known::or_word(record.map(|epic| epic.status.clone()), "unknown"),
        what_it_is: Known::or_word(record.and_then(|epic| epic.outcome.clone()), "not recorded"),
        about: Known::or_word(record.and_then(|epic| epic.about.clone()), "not recorded"),
        features: record.map_or_else(Vec::new, |epic| {
            epic.capabilities
                .iter()
                .map(|(id, name, about)| FeatureFact {
                    id: id.clone(),
                    name: name.clone(),
                    about: if about.trim().is_empty() {
                        Known::not_recorded()
                    } else {
                        Known::Value(about.clone())
                    },
                })
                .collect()
        }),
        percent: Known::or_word(percent, "unknown"),
        stage_counts: counts,
        done,
        not_done,
        cancelled,
        reasoning,
        themes: themes(inputs, record, tasks),
        location_counts: locations,
        landing_chain: landing_chain(inputs, plan, &line, tasks),
        path_to_main: path(inputs, plan, &line, tasks),
        hop_to_main: hop(inputs, plan, &line, tasks),
    }
}

fn themes(inputs: &Inputs, record: Option<&EpicIn>, tasks: &[&TaskIn]) -> Vec<ThemeFact> {
    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for task in tasks {
        grouped
            .entry(theme(inputs, task))
            .or_default()
            .push(task.id.clone());
    }
    grouped
        .into_iter()
        .map(|(name, tasks)| ThemeFact {
            about: Known::or_word(
                record.and_then(|epic| epic.section_text.get(&name).cloned()),
                "not recorded",
            ),
            name,
            tasks,
        })
        .collect()
}

/// The line most of an epic's tasks declare as their target.
fn home_line(inputs: &Inputs, tasks: &[&TaskIn]) -> String {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for task in tasks {
        *counts.entry(task.target.as_str()).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(a.0)))
        .map_or_else(
            || inputs.current_target.clone(),
            |(line, _)| line.to_string(),
        )
}

fn landing_chain(inputs: &Inputs, plan: &Plan, line: &str, tasks: &[&TaskIn]) -> LandingChain {
    let order = plan.order_fact();
    let position = |id: &str| {
        order
            .rows
            .iter()
            .position(|row| row.id == id)
            .unwrap_or(usize::MAX)
    };
    let open = match &inputs.forge {
        ForgeIn::Read { pulls, .. } => {
            let mut chain: Vec<ChainPull> = pulls
                .iter()
                .filter(|pull| pull.base == line)
                .filter(|pull| {
                    pull.task
                        .as_ref()
                        .is_some_and(|id| tasks.iter().any(|task| &task.id == id))
                })
                .map(|pull| ChainPull {
                    number: pull.number,
                    task: Known::or_word(pull.task.clone(), "none"),
                    draft: pull.draft,
                    checks: pull.checks.clone(),
                    required: "not read",
                    runs: pull
                        .runs
                        .iter()
                        .map(|run| CheckRunFact {
                            name: run.name.clone(),
                            state: run.state.clone(),
                            completed_at: Known::or_word(run.completed_at.clone(), "unknown"),
                        })
                        .collect(),
                })
                .collect();
            chain.sort_by_key(|pull| {
                (
                    pull.task.value().map_or(usize::MAX, |id| position(id)),
                    pull.number,
                )
            });
            Known::Value(chain)
        }
        ForgeIn::Unverified { .. } => Known::Word("unverified".into()),
    };
    let mut landed: Vec<(Option<i64>, String)> = tasks
        .iter()
        .filter(|task| stage(inputs, task).percent() == Some(100))
        .map(|task| (task.landed_at, task.id.clone()))
        .collect();
    landed.sort();
    LandingChain {
        line: line.to_string(),
        open,
        landed: landed
            .into_iter()
            .map(|(at, task)| ChainLanded {
                task,
                landed_at: Known::or_word(at.map(time_text), "unknown"),
            })
            .collect(),
    }
}

fn shift(win: &Win, by: [i64; 3]) -> Win {
    match win {
        Win::Estimable {
            ends, conditional, ..
        } => Win::Estimable {
            // The step starts when the work before it can first end, so
            // every end is at or after the start.
            start: ends[0],
            ends: [ends[0] + by[0], ends[1] + by[1], ends[2] + by[2]],
            conditional: conditional.clone(),
        },
        Win::NotEstimable(reason) => Win::NotEstimable(reason.clone()),
    }
}

fn step(
    plan: &Plan,
    name: &str,
    owner: &'static str,
    event: &'static str,
    done: bool,
    window: Option<&Win>,
    conditional: Option<&str>,
) -> StepFact {
    StepFact {
        step: name.into(),
        owner,
        done,
        event,
        window: match (done, window) {
            (true, _) => done_window(plan.computed_at()),
            (false, Some(window)) => plan.fact(window),
            (false, None) => not_estimable(plan.computed_at(), "awaiting the operator: undated"),
        },
        conditional_on: Known::or_word(conditional.map(str::to_string), "none"),
    }
}

/// The remaining steps on the recorded landing route.
fn path(inputs: &Inputs, plan: &Plan, line: &str, tasks: &[&TaskIn]) -> Vec<StepFact> {
    let open: Vec<String> = tasks
        .iter()
        .filter(|t| is_open(t))
        .map(|t| t.id.clone())
        .collect();
    let on_main = tasks
        .iter()
        .filter(|task| task.status != "cancelled")
        .all(|task| location(inputs, task).kind == "main");
    let land = plan.finish(open.iter()).unwrap_or(Win::Estimable {
        start: 0,
        ends: [0; 3],
        conditional: None,
    });
    let mut steps = vec![step(
        plan,
        &format!("land the open units on {line}"),
        "agent",
        "agent work",
        open.is_empty(),
        Some(&land),
        None,
    )];
    if line == inputs.default_branch {
        if inputs.release_step {
            steps.push(step(
                plan,
                "release",
                "operator",
                "done by the operator",
                false,
                None,
                Some("the units landing on main"),
            ));
        }
        return steps;
    }
    let gate = shift(&land, [HALF, HOUR, 2 * HOUR]);
    let pull_open = matches!(&inputs.forge, ForgeIn::Read { pulls, .. }
        if pulls.iter().any(|pull| pull.head_ref == line && pull.base == inputs.default_branch));
    let pull = shift(&gate, [HALF, HALF, HOUR]);
    steps.push(step(
        plan,
        "the line's full gate",
        "agent",
        "agent work",
        on_main,
        Some(&gate),
        None,
    ));
    steps.push(step(
        plan,
        "the pull request to main",
        "agent",
        "agent work",
        on_main || pull_open,
        Some(&pull),
        None,
    ));
    steps.push(step(
        plan,
        "the operator's merge",
        "operator",
        "ready for the operator",
        on_main,
        Some(&pull),
        None,
    ));
    steps.push(step(
        plan,
        "the operator's merge",
        "operator",
        "done by the operator",
        on_main,
        None,
        None,
    ));
    if inputs.release_step {
        steps.push(step(
            plan,
            "release",
            "operator",
            "done by the operator",
            false,
            None,
            Some("the operator's merge into main"),
        ));
    }
    steps
}

const HOUR: i64 = 3_600;
const HALF: i64 = HOUR / 2;

fn hop(inputs: &Inputs, plan: &Plan, line: &str, tasks: &[&TaskIn]) -> HopFact {
    if line == inputs.default_branch {
        return HopFact {
            condition: "the epic's home is main: each unit reaches main when it lands".into(),
            earliest_conditional_end_seconds: Known::Word("not applicable".into()),
        };
    }
    let steps = path(inputs, plan, line, tasks);
    let ready = steps
        .iter()
        .find(|step| step.event == "ready for the operator")
        .and_then(|step| step.window.earliest_end_seconds.value().copied());
    let condition = match ready {
        Some(end) if steps.iter().all(|step| step.done) => {
            let _ = end;
            "the epic's tasks are already on main".to_string()
        }
        Some(end) => format!(
            "after the epic is complete on the line, earliest {} if intake stops and capacity holds; then the operator's merge",
            super::compute::duration_words(end)
        ),
        None => "after the epic is complete on the line and the operator merges; no earliest end can be given yet".into(),
    };
    HopFact {
        condition,
        earliest_conditional_end_seconds: Known::or_word(ready, "not yet estimable"),
    }
}

/// The open pull requests into `main`, each with its task's state.
#[must_use]
pub fn pulls_into_main(inputs: &Inputs) -> Known<Vec<PullFact>> {
    let ForgeIn::Read { pulls, .. } = &inputs.forge else {
        return Known::Word("unverified".into());
    };
    let tasks = super::compute::by_id(inputs);
    Known::Value(
        pulls
            .iter()
            .filter(|pull| pull.base == inputs.default_branch)
            .map(|pull| PullFact {
                number: pull.number,
                title: pull.title.clone(),
                head: pull.head_ref.clone(),
                task: Known::or_word(pull.task.clone(), "none"),
                task_state: Known::or_word(
                    pull.task
                        .as_deref()
                        .and_then(|id| tasks.get(id))
                        .map(|task| task.status.clone()),
                    "not in the records read",
                ),
            })
            .collect(),
    )
}

/// The open pull request of a task, when the forge was read.
#[must_use]
pub fn open_pull(inputs: &Inputs, task: &TaskIn) -> Option<u64> {
    pull_for(inputs, task).map(|pull| pull.number)
}
