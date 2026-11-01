//! Order, critical path, lane count and windows. A window is a duration
//! from the snapshot time, never a date: from an adopted forecast that
//! covers the scope, otherwise from the planned throughput rule
//! (`cf-method/references/workflow-lifecycle.md` "Planned work moves at
//! speed": at least one task per build lane per hour).

use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

use super::compute::{count_f64, duration_words, stage, time_text};
use super::inputs::{Inputs, LedgerIn, TaskIn};
use super::model::{Known, OrderFact, OrderRow, ScenarioFact, WindowFact, WindowsFact};

/// One review cycle, one repair round or one landing cycle: one hour.
const HOUR: i64 = 3_600;
/// Earliest: two tasks per lane-hour, plus one review and landing cycle.
const UNIT_EARLIEST: i64 = HOUR / 2 + HOUR;
/// Planning: one task per lane-hour, one review cycle, one repair round.
const UNIT_PLANNING: i64 = HOUR + HOUR + HOUR;
/// Latest: one task per lane-hour, two review rounds, one landing cycle.
const UNIT_LATEST: i64 = HOUR + 2 * HOUR + HOUR;

/// The planned-throughput basis, printed with every window it gives.
pub const PLANNED_BASIS: &str = "planned throughput: earliest at two tasks per lane-hour plus one review and landing cycle (1.5 h a unit), planning at one task per lane-hour with one review cycle and one repair round (3 h), latest at one task per lane-hour with two review rounds and one landing cycle (4 h); a unit already in progress counts only its remaining share by stage";

/// A computed window, in seconds from the snapshot time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Win {
    Estimable {
        start: i64,
        ends: [i64; 3],
        conditional: Option<String>,
    },
    NotEstimable(String),
}

/// The plan views, computed once per snapshot.
#[derive(Debug, Clone)]
pub struct Plan {
    rows: Vec<(String, String, Vec<String>, String)>,
    windows: BTreeMap<String, Win>,
    pub(super) lanes_used: usize,
    pub(super) basis_kind: &'static str,
    pub(super) basis: String,
    lane_map: BTreeMap<String, String>,
    critical: Vec<String>,
    critical_seconds: Option<i64>,
    depth_chain: Vec<String>,
    computed_at: String,
    forecast: Option<String>,
    /// Tasks added to the scope over the last seven days.
    pub(super) intake_added: Option<usize>,
}

fn rank(state: Option<&str>) -> u8 {
    match state {
        Some("active") => 0,
        Some("ready") => 1,
        Some("waiting") => 2,
        Some("blocked") => 3,
        _ => 4,
    }
}

/// A task's best chain so far: planning seconds, depth, and the
/// predecessors on the duration and depth chains.
type Best<'a> = (i64, usize, Option<&'a str>, Option<&'a str>);

/// Open: not complete, not cancelled and not already landed.
pub(super) fn is_open(task: &TaskIn) -> bool {
    matches!(task.status.as_str(), "todo" | "blocked") && task.state.as_deref() != Some("landed")
}

impl Plan {
    /// Compute the plan for the tasks in `ids`.
    #[must_use]
    pub fn new(inputs: &Inputs, ids: &BTreeSet<String>) -> Self {
        let open: BTreeMap<&str, &TaskIn> = inputs
            .tasks
            .iter()
            .filter(|task| ids.contains(&task.id) && is_open(task))
            .map(|task| (task.id.as_str(), task))
            .collect();
        // Windows are scheduled over the scope and every open task before
        // it, in or out of the scope, so an outside predecessor still
        // delays its successors; the rows and paths stay the scope's.
        let graph = with_predecessors(inputs, &open);
        let graph_order = topological(&graph);
        let order: Vec<String> = graph_order
            .iter()
            .filter(|id| open.contains_key(id.as_str()))
            .cloned()
            .collect();
        let (lanes_used, lane_map, observed) = lanes(inputs, ids);
        let covering = inputs
            .forecast
            .as_ref()
            .filter(|forecast| graph.keys().all(|id| forecast.ends.contains_key(*id)));
        let (basis_kind, basis) = match (covering, observed) {
            (Some(forecast), _) => (
                "forecast",
                format!(
                    "the adopted forecast {}{}",
                    forecast.path,
                    if forecast.stale {
                        " (stale: a task's criteria changed since it was frozen)"
                    } else {
                        ""
                    }
                ),
            ),
            (None, true) => (
                "observed",
                format!("{PLANNED_BASIS}; over the {lanes_used} lane(s) observed"),
            ),
            (None, false) => (
                "assumed",
                format!(
                    "{PLANNED_BASIS}; no lane observed, so one lane is assumed (assumed one lane)"
                ),
            ),
        };
        let mut plan = Self {
            rows: Vec::new(),
            windows: BTreeMap::new(),
            lanes_used,
            basis_kind,
            basis,
            lane_map,
            critical: Vec::new(),
            critical_seconds: None,
            depth_chain: Vec::new(),
            computed_at: time_text(inputs.now),
            forecast: covering.map(|forecast| forecast.path.clone()),
            intake_added: intake_added(inputs, ids),
        };
        plan.schedule(
            inputs,
            &graph,
            &graph_order,
            covering.map(|forecast| &forecast.ends),
        );
        plan.paths(inputs, &open, &order);
        plan.rows = order
            .iter()
            .map(|id| {
                let task = open[id.as_str()];
                let waits_on = task
                    .depends_on
                    .iter()
                    .filter(|dep| open.contains_key(dep.as_str()))
                    .cloned()
                    .collect();
                (
                    id.clone(),
                    task.state.clone().unwrap_or_else(|| "unknown".into()),
                    waits_on,
                    task.state_reason.clone(),
                )
            })
            .collect();
        for record in &inputs.unreadable {
            if let Some(id) = &record.task_id {
                plan.windows.insert(
                    id.clone(),
                    Win::NotEstimable(format!("the record cannot be read: {}", record.reason)),
                );
            }
        }
        plan
    }

    fn schedule(
        &mut self,
        inputs: &Inputs,
        open: &BTreeMap<&str, &TaskIn>,
        order: &[String],
        forecast: Option<&BTreeMap<String, (i64, i64, i64)>>,
    ) {
        let known = super::compute::by_id(inputs);
        let unreadable: BTreeSet<&str> = inputs
            .unreadable
            .iter()
            .filter_map(|record| record.task_id.as_deref())
            .collect();
        let mut free: [BinaryHeap<std::cmp::Reverse<i64>>; 3] = Default::default();
        for heap in &mut free {
            for _ in 0..self.lanes_used.max(1) {
                heap.push(std::cmp::Reverse(0));
            }
        }
        for id in order {
            let task = open[id.as_str()];
            let window = match no_basis(task, open, &known, &unreadable, &self.windows) {
                Some(reason) => Win::NotEstimable(reason),
                None => self.place(inputs, task, open, forecast, &mut free),
            };
            self.windows.insert(id.clone(), window);
        }
    }

    fn place(
        &self,
        inputs: &Inputs,
        task: &TaskIn,
        open: &BTreeMap<&str, &TaskIn>,
        forecast: Option<&BTreeMap<String, (i64, i64, i64)>>,
        free: &mut [BinaryHeap<std::cmp::Reverse<i64>>; 3],
    ) -> Win {
        // AC-5: the start is the as-of moment or the predecessors' latest
        // end, and every scenario's end runs from that one start.
        let mut start = 0_i64;
        for dep in task
            .depends_on
            .iter()
            .filter(|dep| open.contains_key(dep.as_str()))
        {
            if let Some(Win::Estimable { ends, .. }) = self.windows.get(dep) {
                start = start.max(ends[2]);
            }
        }
        let ends = if let Some(forecast) = forecast {
            let (e, p, l) = forecast[&task.id];
            [e.max(start), p.max(start), l.max(start)]
        } else {
            let remaining = 100 - i64::from(stage(inputs, task).percent().unwrap_or(0));
            let units = [UNIT_EARLIEST, UNIT_PLANNING, UNIT_LATEST];
            let mut ends = [0_i64; 3];
            for (index, heap) in free.iter_mut().enumerate() {
                let lane = heap.pop().map_or(0, |std::cmp::Reverse(at)| at);
                ends[index] = lane.max(start) + units[index] * remaining / 100;
                heap.push(std::cmp::Reverse(ends[index]));
            }
            ends
        };
        let conditional = (task.status == "blocked" || task.state.as_deref() == Some("blocked"))
            .then(|| {
                let reason = task
                    .blocker
                    .as_ref()
                    .and_then(|blocker| blocker.reason.clone())
                    .unwrap_or_else(|| task.state_reason.clone());
                format!("the blocker clearing: {reason}")
            });
        Win::Estimable {
            start,
            ends,
            conditional,
        }
    }

    /// The critical path (planning durations) and the dependency depth.
    fn paths(&mut self, inputs: &Inputs, open: &BTreeMap<&str, &TaskIn>, order: &[String]) {
        let mut best: BTreeMap<&str, Best<'_>> = BTreeMap::new();
        for id in order {
            let task = open[id.as_str()];
            let duration = self.planning_duration(inputs, task, open);
            let mut weight = (0_i64, None);
            let mut count = (0_usize, None);
            for dep in task
                .depends_on
                .iter()
                .filter(|dep| open.contains_key(dep.as_str()))
            {
                if let Some((w, c, _, _)) = best.get(dep.as_str()) {
                    if *w > weight.0 || (*w == weight.0 && weight.1.is_none()) {
                        weight = (*w, Some(dep.as_str()));
                    }
                    if *c > count.0 || (*c == count.0 && count.1.is_none()) {
                        count = (*c, Some(dep.as_str()));
                    }
                }
            }
            best.insert(
                id.as_str(),
                (weight.0 + duration, count.0 + 1, weight.1, count.1),
            );
        }
        let pick = |by_weight: bool| {
            best.iter()
                .max_by(|a, b| {
                    let key = |entry: &Best<'_>| {
                        if by_weight {
                            (entry.0, 0)
                        } else {
                            (i64::try_from(entry.1).unwrap_or(i64::MAX), 0)
                        }
                    };
                    key(a.1).cmp(&key(b.1)).then_with(|| b.0.cmp(a.0))
                })
                .map(|(id, _)| *id)
        };
        let chain = |start: Option<&str>, by_weight: bool| {
            let mut out = Vec::new();
            let mut at = start;
            while let Some(id) = at {
                out.push(id.to_string());
                at = best
                    .get(id)
                    .and_then(|entry| if by_weight { entry.2 } else { entry.3 });
            }
            out.reverse();
            out
        };
        let critical = pick(true);
        self.critical_seconds = critical.and_then(|id| best.get(id)).map(|entry| entry.0);
        self.critical = chain(critical, true);
        self.depth_chain = chain(pick(false), false);
    }

    fn planning_duration(
        &self,
        inputs: &Inputs,
        task: &TaskIn,
        open: &BTreeMap<&str, &TaskIn>,
    ) -> i64 {
        if let Some(forecast) = inputs.forecast.as_ref().filter(|_| self.forecast.is_some()) {
            let before = task
                .depends_on
                .iter()
                .filter(|dep| open.contains_key(dep.as_str()))
                .filter_map(|dep| forecast.ends.get(dep).map(|ends| ends.1))
                .max()
                .unwrap_or(0);
            return forecast
                .ends
                .get(&task.id)
                .map_or(0, |ends| (ends.1 - before).max(0));
        }
        let remaining = 100 - i64::from(stage(inputs, task).percent().unwrap_or(0));
        UNIT_PLANNING * remaining / 100
    }

    /// A task's window as the fact model prints it.
    #[must_use]
    pub fn window(&self, id: &str) -> WindowFact {
        match self.windows.get(id) {
            Some(window) => self.fact(window),
            None => not_estimable(&self.computed_at, "the task is not open: no window"),
        }
    }

    /// A window as a fact.
    #[must_use]
    pub fn fact(&self, window: &Win) -> WindowFact {
        match window {
            Win::NotEstimable(reason) => not_estimable(&self.computed_at, reason),
            Win::Estimable {
                start,
                ends,
                conditional,
            } => WindowFact {
                state: "estimable",
                start_seconds: Known::Value(*start),
                earliest_end_seconds: Known::Value(ends[0]),
                planning_end_seconds: Known::Value(ends[1]),
                latest_end_seconds: Known::Value(ends[2]),
                basis_kind: Known::Value(self.basis_kind.into()),
                basis: Known::Value(self.basis.clone()),
                computed_at: self.computed_at.clone(),
                conditional_on: Known::or_word(conditional.clone(), "none"),
                reason: Known::Word("none".into()),
            },
        }
    }

    /// The lane carrying a task, when one does.
    #[must_use]
    pub fn lane_of(&self, id: &str) -> Option<String> {
        self.lane_map.get(id).cloned()
    }

    /// The latest window over a set of open tasks: their finish, or `None`
    /// when none is open (already done).
    pub(super) fn finish<'a>(&self, ids: impl Iterator<Item = &'a String>) -> Option<Win> {
        let mut out: Option<Win> = None;
        for id in ids {
            let Some(window) = self.windows.get(id) else {
                continue;
            };
            out = Some(match (out, window) {
                (Some(Win::NotEstimable(reason)), _) => Win::NotEstimable(reason),
                (_, Win::NotEstimable(reason)) => {
                    Win::NotEstimable(format!("{id} or a task before it: {reason}"))
                }
                (None, window) => window.clone(),
                (
                    Some(Win::Estimable {
                        start,
                        ends,
                        conditional,
                    }),
                    Win::Estimable {
                        ends: next,
                        conditional: other,
                        ..
                    },
                ) => Win::Estimable {
                    start,
                    ends: [
                        ends[0].max(next[0]),
                        ends[1].max(next[1]),
                        ends[2].max(next[2]),
                    ],
                    conditional: conditional.or_else(|| other.clone()),
                },
            });
        }
        out
    }

    /// The order view.
    #[must_use]
    pub fn order_fact(&self) -> OrderFact {
        let critical: BTreeSet<&String> = self.critical.iter().collect();
        OrderFact {
            rows: self
                .rows
                .iter()
                .enumerate()
                .map(|(index, (id, state, waits_on, reason))| OrderRow {
                    position: index + 1,
                    id: id.clone(),
                    state: state.clone(),
                    waits_on: waits_on.clone(),
                    reason: if reason.is_empty() {
                        Known::Word("none".into())
                    } else {
                        Known::Value(reason.clone())
                    },
                    critical: critical.contains(id),
                })
                .collect(),
            critical_path: self.critical.clone(),
            critical_path_seconds: Known::or_word(self.critical_seconds, "unknown"),
            critical_basis: format!(
                "the chain whose planning durations sum highest, from {}",
                if self.forecast.is_some() {
                    "the adopted forecast"
                } else {
                    "the planned throughput rule"
                }
            ),
            depth: self.depth_chain.len(),
            depth_chain: self.depth_chain.clone(),
        }
    }

    /// The windows view with the two finish scenarios.
    #[must_use]
    pub fn windows_fact(&self) -> WindowsFact {
        let open: Vec<&String> = self.rows.iter().map(|row| &row.0).collect();
        let stops = self.finish(open.iter().copied());
        let stops_fact = stops.as_ref().map_or_else(
            || done_window(&self.computed_at),
            |window| self.fact(window),
        );
        // Planned capacity over seven days: one task per lane-hour.
        let week_capacity = self.lanes_used.max(1) * 24 * 7;
        let capacity = count_f64(week_capacity) / 7.0;
        let (continues, words) = match (self.intake_added, &stops) {
            (None, _) => (
                not_estimable(&self.computed_at, "no intake history: the rate of added tasks is unknown"),
                "intake rate unknown, so no finish is given for continued intake".to_string(),
            ),
            (Some(added), _) if added >= week_capacity => (
                not_estimable(&self.computed_at, "intake meets or exceeds the planned capacity"),
                format!(
                    "no finite finish: tasks are added at {:.1} a day, at or above the planned capacity of {capacity:.1} a day",
                    count_f64(added) / 7.0
                ),
            ),
            (Some(added), Some(Win::Estimable { start, ends, conditional })) => {
                let full = i64::try_from(week_capacity).unwrap_or(i64::MAX);
                let net = full - i64::try_from(added).unwrap_or(i64::MAX);
                let stretch = |end: i64| end.saturating_mul(full) / net.max(1);
                (
                    self.fact(&Win::Estimable {
                        start: *start,
                        ends: [stretch(ends[0]), stretch(ends[1]), stretch(ends[2])],
                        conditional: conditional.clone(),
                    }),
                    format!(
                        "added tasks arrive at {:.1} a day against a planned capacity of {capacity:.1} a day",
                        count_f64(added) / 7.0
                    ),
                )
            }
            (Some(_), Some(Win::NotEstimable(reason))) => (
                not_estimable(&self.computed_at, reason),
                "the intake-stops finish has no basis, so neither has this one".into(),
            ),
            (Some(_), None) => (done_window(&self.computed_at), "nothing is open".into()),
        };
        WindowsFact {
            basis_kind: self.basis_kind.into(),
            basis: self.basis.clone(),
            computed_at: self.computed_at.clone(),
            lanes_used: self.lanes_used.max(1),
            forecast: Known::or_word(self.forecast.clone(), "none covers the scope"),
            scenarios: vec![
                ScenarioFact {
                    name: "intake stops",
                    rate: "the planned capacity rate over the lanes used".into(),
                    open_tasks: open.len(),
                    words: stops.as_ref().map_or_else(
                        || "nothing is open".into(),
                        |window| window_summary(&self.fact(window)).unwrap_or_else(|| "not yet estimable".into()),
                    ),
                    finish: stops_fact,
                    intake_since_previous: Known::Word("no previous snapshot given".into()),
                },
                ScenarioFact {
                    name: "intake continues",
                    rate: "the planned capacity rate less the intake rate observed over the last seven days".into(),
                    open_tasks: open.len(),
                    finish: continues,
                    words,
                    intake_since_previous: Known::Word("no previous snapshot given".into()),
                },
            ],
        }
    }

    pub(super) fn computed_at(&self) -> &str {
        &self.computed_at
    }
}

/// A one-line summary of an estimable window.
#[must_use]
pub fn window_summary(window: &WindowFact) -> Option<String> {
    let (Some(e), Some(p), Some(l)) = (
        window.earliest_end_seconds.value(),
        window.planning_end_seconds.value(),
        window.latest_end_seconds.value(),
    ) else {
        return None;
    };
    let condition = window
        .conditional_on
        .value()
        .filter(|text| text.as_str() != "none")
        .map(|text| format!(", conditional on {text}"))
        .unwrap_or_default();
    Some(format!(
        "ends in {} (earliest {}, latest {}){condition}",
        duration_words(*p),
        duration_words(*e),
        duration_words(*l)
    ))
}

pub(super) fn not_estimable(computed_at: &str, reason: &str) -> WindowFact {
    WindowFact {
        state: "not yet estimable",
        start_seconds: Known::Word("not yet estimable".into()),
        earliest_end_seconds: Known::Word("not yet estimable".into()),
        planning_end_seconds: Known::Word("not yet estimable".into()),
        latest_end_seconds: Known::Word("not yet estimable".into()),
        basis_kind: Known::Word("none".into()),
        basis: Known::Word("none".into()),
        computed_at: computed_at.to_string(),
        conditional_on: Known::Word("none".into()),
        reason: Known::Value(reason.to_string()),
    }
}

pub(super) fn done_window(computed_at: &str) -> WindowFact {
    let mut fact = not_estimable(computed_at, "done: nothing is open");
    fact.state = "estimable";
    for slot in [
        &mut fact.start_seconds,
        &mut fact.earliest_end_seconds,
        &mut fact.planning_end_seconds,
        &mut fact.latest_end_seconds,
    ] {
        *slot = Known::Value(0);
    }
    fact.basis_kind = Known::Value("observed".into());
    fact.basis = Known::Value("nothing is open".into());
    fact.reason = Known::Word("none".into());
    fact
}

/// Why a task's window has no honest basis, when it has none.
fn no_basis(
    task: &TaskIn,
    open: &BTreeMap<&str, &TaskIn>,
    known: &BTreeMap<&str, &TaskIn>,
    unreadable: &BTreeSet<&str>,
    done: &BTreeMap<String, Win>,
) -> Option<String> {
    if let Some(reason) = &task.unknown {
        return Some(format!("its stage is unknown: {reason}"));
    }
    if task.state.is_none() || task.state.as_deref() == Some("invalid") {
        return Some("the readiness core could not judge the task".into());
    }
    for dep in &task.depends_on {
        if unreadable.contains(dep.as_str()) {
            return Some(format!("its predecessor {dep} cannot be read"));
        }
        match known.get(dep.as_str()) {
            None => return Some(format!("its predecessor {dep} is not in the records read")),
            Some(pred) if pred.status == "cancelled" => {
                return Some(format!("its predecessor {dep} is cancelled"))
            }
            Some(_) if open.contains_key(dep.as_str()) => {
                if let Some(Win::NotEstimable(_)) = done.get(dep) {
                    return Some(format!("its predecessor {dep} is not yet estimable"));
                }
            }
            Some(_) => {}
        }
    }
    None
}

/// `open` with every open task any of them waits on, transitively.
fn with_predecessors<'a>(
    inputs: &'a Inputs,
    open: &BTreeMap<&'a str, &'a TaskIn>,
) -> BTreeMap<&'a str, &'a TaskIn> {
    let known = super::compute::by_id(inputs);
    let mut graph = open.clone();
    let mut pending: Vec<&str> = open.keys().copied().collect();
    while let Some(id) = pending.pop() {
        let task: &'a TaskIn = graph[id];
        for dep in &task.depends_on {
            let Some(before) = known.get(dep.as_str()).copied().filter(|t| is_open(t)) else {
                continue;
            };
            if !graph.contains_key(before.id.as_str()) {
                graph.insert(before.id.as_str(), before);
                pending.push(before.id.as_str());
            }
        }
    }
    graph
}

/// Kahn's order: no task before a predecessor; among free tasks, active,
/// ready, waiting, blocked, then the rest, each by id.
fn topological(open: &BTreeMap<&str, &TaskIn>) -> Vec<String> {
    let mut indegree: BTreeMap<&str, usize> = open.keys().map(|id| (*id, 0)).collect();
    let mut after: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (id, task) in open {
        for dep in task
            .depends_on
            .iter()
            .filter(|dep| open.contains_key(dep.as_str()))
        {
            *indegree.entry(id).or_default() += 1;
            after.entry(dep.as_str()).or_default().push(id);
        }
    }
    let mut free: BTreeSet<(u8, &str)> = indegree
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(id, _)| (rank(open[id].state.as_deref()), *id))
        .collect();
    let mut out = Vec::new();
    while let Some(next) = free.pop_first() {
        out.push(next.1.to_string());
        for follower in after.get(next.1).into_iter().flatten() {
            let count = indegree.entry(follower).or_default();
            *count -= 1;
            if *count == 0 {
                free.insert((rank(open[follower].state.as_deref()), follower));
            }
        }
    }
    // A cycle the records should never hold: list what is left by id so no
    // task is dropped.
    for id in open.keys() {
        if !out.iter().any(|seen| seen == id) {
            out.push((*id).to_string());
        }
    }
    out
}

/// The lanes a window uses: open ledger lanes, else the observed claims,
/// else one assumed lane. Returns the count, task to lane, and whether any
/// lane was observed.
fn lanes(inputs: &Inputs, ids: &BTreeSet<String>) -> (usize, BTreeMap<String, String>, bool) {
    let mut map = BTreeMap::new();
    if let LedgerIn::Read { lanes, .. } = &inputs.ledger {
        if !lanes.is_empty() {
            for lane in lanes {
                for task in &lane.scope {
                    map.insert(task.clone(), lane.id.clone());
                }
            }
            return (lanes.len(), map, true);
        }
    }
    let observed: Vec<&TaskIn> = inputs
        .tasks
        .iter()
        .filter(|task| task.state.as_deref() == Some("active"))
        .collect();
    for task in &observed {
        if ids.contains(&task.id) {
            map.insert(task.id.clone(), format!("observed {}", task.id));
        }
    }
    (observed.len().max(1), map, !observed.is_empty())
}

fn intake_added(inputs: &Inputs, ids: &BTreeSet<String>) -> Option<usize> {
    if !inputs.history_read {
        return None;
    }
    let days = super::issues::last_days(inputs.now);
    let dated: Vec<&TaskIn> = inputs
        .tasks
        .iter()
        .filter(|task| ids.contains(&task.id) && task.created.is_some())
        .collect();
    if dated.is_empty() {
        return None;
    }
    let added = dated
        .iter()
        .filter(|task| {
            task.created
                .as_ref()
                .is_some_and(|date| days.contains(date))
        })
        .count();
    Some(added)
}
