//! The versioned fact model `codeflow status --json` prints
//! (`report-v1.schema.json`). Every field is always present: a fact that is
//! missing reads as a word (`unknown`, `not recorded`, `unverified`, `not
//! yet estimable`), never as an omitted field or a zero.

use serde::Serialize;

/// The schema version of [`Facts`].
pub const SCHEMA_VERSION: u32 = 1;

/// A value, or the word that says why there is none.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Known<T> {
    Value(T),
    Word(String),
}

impl<T> Known<T> {
    /// `unknown`.
    #[must_use]
    pub fn unknown() -> Self {
        Self::Word("unknown".into())
    }

    /// `not recorded`.
    #[must_use]
    pub fn not_recorded() -> Self {
        Self::Word("not recorded".into())
    }

    /// The value from an option, or `word`.
    pub fn or_word(value: Option<T>, word: &str) -> Self {
        value.map_or_else(|| Self::Word(word.into()), Self::Value)
    }

    /// The value, when there is one.
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Value(value) => Some(value),
            Self::Word(_) => None,
        }
    }
}

/// The whole snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct Facts {
    pub schema_version: u32,
    pub kind: &'static str,
    /// The repository identity (`host/owner/name`, or `local`) TSK-311's
    /// envelope shares.
    pub repository: String,
    pub snapshot: Snapshot,
    pub refs: Vec<RefFact>,
    pub scope: ScopeFact,
    pub ladder: Vec<Rung>,
    pub epics: Vec<EpicFact>,
    pub tasks: Vec<TaskFact>,
    pub pulls_into_main: Known<Vec<PullFact>>,
    pub order: OrderFact,
    pub lanes: LanesFact,
    pub windows: WindowsFact,
    pub issues: IssuesFact,
    pub pace: PaceFact,
    pub since: Known<serde_json::Value>,
    pub evidence: Vec<EvidenceFact>,
}

/// The snapshot time and its one zone.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub at: String,
    pub zone: &'static str,
}

/// A ref a remote claim was read from.
#[derive(Debug, Clone, Serialize)]
pub struct RefFact {
    pub name: String,
    pub sha: Known<String>,
    pub fetched_at: Known<String>,
    /// `checked` or `unverified`.
    pub state: String,
    pub reason: Known<String>,
}

/// One rung of the stage ladder.
#[derive(Debug, Clone, Serialize)]
pub struct Rung {
    pub stage: &'static str,
    pub percent: u32,
    pub meaning: &'static str,
}

/// Counts of tasks by stage.
#[derive(Debug, Clone, Default, Serialize)]
pub struct StageCounts {
    pub not_started: usize,
    pub in_progress: usize,
    pub in_review: usize,
    pub reviewed: usize,
    pub landed: usize,
}

/// The named scope: the "overall" of every report.
#[derive(Debug, Clone, Serialize)]
pub struct ScopeFact {
    pub kind: &'static str,
    pub names: Vec<String>,
    /// The target the scope was read against.
    pub target: String,
    pub deliverable: Known<String>,
    pub percent: Known<f64>,
    pub basis: String,
    pub stage_counts: StageCounts,
    pub tasks: usize,
    pub cancelled: Vec<String>,
    pub unknown_reason: Known<String>,
}

/// A capability an epic serves.
#[derive(Debug, Clone, Serialize)]
pub struct FeatureFact {
    pub id: String,
    pub name: String,
    pub about: Known<String>,
}

/// One theme of an epic's tasks.
#[derive(Debug, Clone, Serialize)]
pub struct ThemeFact {
    pub name: String,
    /// The epic section's text under the theme's heading, or `not recorded`.
    pub about: Known<String>,
    pub tasks: Vec<String>,
}

/// Counts of tasks by location, on one denominator.
#[derive(Debug, Clone, Default, Serialize)]
pub struct LocationCounts {
    pub main: usize,
    pub line: usize,
    pub pull_request: usize,
    pub branch: usize,
    pub none: usize,
    pub unknown: usize,
    pub denominator: usize,
}

/// An open pull request in the landing chain.
#[derive(Debug, Clone, Serialize)]
pub struct ChainPull {
    pub number: u64,
    pub task: Known<String>,
    pub draft: bool,
    /// The summary of the runs: `failed`, `pending`, `missing`, `skipped`
    /// or `passed`; a skip is never a pass.
    pub checks: String,
    /// Which checks are required: `not read` (the forge read does not say).
    pub required: &'static str,
    /// Each check run as read.
    pub runs: Vec<CheckRunFact>,
}

/// One check run of a pull request, as the forge reported it.
#[derive(Debug, Clone, Serialize)]
pub struct CheckRunFact {
    pub name: String,
    pub state: String,
    pub completed_at: Known<String>,
}

/// A landed task in the landing chain.
#[derive(Debug, Clone, Serialize)]
pub struct ChainLanded {
    pub task: String,
    pub landed_at: Known<String>,
}

/// The landing chain into the epic's line, defined once.
#[derive(Debug, Clone, Serialize)]
pub struct LandingChain {
    pub line: String,
    pub open: Known<Vec<ChainPull>>,
    pub landed: Vec<ChainLanded>,
}

/// One step on the path to `main`.
#[derive(Debug, Clone, Serialize)]
pub struct StepFact {
    pub step: String,
    /// `agent` or `operator`.
    pub owner: &'static str,
    pub done: bool,
    /// For an operator step: `ready for the operator` or `done by the
    /// operator`; `agent work` otherwise.
    pub event: &'static str,
    pub window: WindowFact,
    pub conditional_on: Known<String>,
}

/// The hop to `main`, in words and as an earliest conditional end.
#[derive(Debug, Clone, Serialize)]
pub struct HopFact {
    pub condition: String,
    pub earliest_conditional_end_seconds: Known<i64>,
}

/// One epic.
#[derive(Debug, Clone, Serialize)]
pub struct EpicFact {
    pub id: String,
    pub title: Known<String>,
    pub status: Known<String>,
    pub what_it_is: Known<String>,
    pub about: Known<String>,
    pub features: Vec<FeatureFact>,
    pub percent: Known<f64>,
    pub stage_counts: StageCounts,
    pub done: Vec<String>,
    pub not_done: Vec<String>,
    pub cancelled: Vec<String>,
    pub reasoning: String,
    pub themes: Vec<ThemeFact>,
    pub location_counts: LocationCounts,
    pub landing_chain: LandingChain,
    pub path_to_main: Vec<StepFact>,
    pub hop_to_main: HopFact,
}

/// Criteria verified against criteria total, beside the stage percent.
#[derive(Debug, Clone, Serialize)]
pub struct CriteriaFact {
    pub label: &'static str,
    pub verified: usize,
    pub total: usize,
    pub reviewed: String,
    /// `current` (bound to its reviewed commit), `stale` (valid but not
    /// bound) or `invalid` (not one active block that passes the
    /// acceptance rules against the current criteria).
    pub state: &'static str,
    /// Why the block is stale or invalid, or `none`.
    pub note: Known<String>,
}

/// Where a task's work is.
#[derive(Debug, Clone, Serialize)]
pub struct LocationFact {
    /// `main`, `line`, `pull request`, `branch`, `none` or `unknown`.
    pub kind: &'static str,
    pub substate: Known<String>,
    pub reviewed_sha: Known<String>,
    pub not_the_head: bool,
    pub note: Known<String>,
    pub read_from: Known<String>,
    pub ref_sha: Known<String>,
    pub read_at: Known<String>,
    /// `checked` or `unverified`.
    pub state: &'static str,
}

/// A time window as durations from the snapshot time.
#[derive(Debug, Clone, Serialize)]
pub struct WindowFact {
    /// `estimable` or `not yet estimable`.
    pub state: &'static str,
    pub start_seconds: Known<i64>,
    pub earliest_end_seconds: Known<i64>,
    pub planning_end_seconds: Known<i64>,
    pub latest_end_seconds: Known<i64>,
    /// `forecast`, `observed` or `assumed`.
    pub basis_kind: Known<String>,
    pub basis: Known<String>,
    pub computed_at: String,
    pub conditional_on: Known<String>,
    pub reason: Known<String>,
}

/// The drill-down facts of one task.
#[derive(Debug, Clone, Serialize)]
pub struct ContextFact {
    pub outcome: Known<String>,
    pub location: &'static str,
    pub lane: Known<String>,
    pub state: Known<String>,
    pub window: Known<String>,
    pub last_moved: Known<String>,
    pub open_pull_request: Known<u64>,
}

/// One task.
#[derive(Debug, Clone, Serialize)]
pub struct TaskFact {
    pub id: String,
    pub title: Known<String>,
    pub epic: Known<String>,
    pub status: Known<String>,
    pub target: Known<String>,
    pub state: Known<String>,
    pub stage: &'static str,
    pub percent: Known<f64>,
    pub criteria: Known<CriteriaFact>,
    pub location: LocationFact,
    pub lane: Known<String>,
    pub window: WindowFact,
    pub theme: String,
    pub about: Known<String>,
    pub depends_on: Vec<String>,
    pub context: ContextFact,
}

/// An open pull request into `main`.
#[derive(Debug, Clone, Serialize)]
pub struct PullFact {
    pub number: u64,
    pub title: String,
    pub head: String,
    pub task: Known<String>,
    pub task_state: Known<String>,
}

/// One row of the order view.
#[derive(Debug, Clone, Serialize)]
pub struct OrderRow {
    pub position: usize,
    pub id: String,
    pub state: String,
    pub waits_on: Vec<String>,
    pub reason: Known<String>,
    pub critical: bool,
}

/// The order of work.
#[derive(Debug, Clone, Serialize)]
pub struct OrderFact {
    pub rows: Vec<OrderRow>,
    pub critical_path: Vec<String>,
    pub critical_path_seconds: Known<i64>,
    pub critical_basis: String,
    pub depth: usize,
    pub depth_chain: Vec<String>,
}

/// One lane.
#[derive(Debug, Clone, Serialize)]
pub struct LaneFact {
    pub id: String,
    /// `ledger` or `observed`.
    pub label: &'static str,
    pub stream: Known<String>,
    pub tasks: Vec<String>,
    pub attempt: Known<u64>,
    pub seat: Known<String>,
    pub worktree: Known<String>,
    pub branch: Known<String>,
    pub pull_request: Known<u64>,
    pub head_age_seconds: Known<i64>,
    pub percent: Known<f64>,
}

/// One session fact, labelled `session`.
#[derive(Debug, Clone, Serialize)]
pub struct SessionLaneFact {
    pub label: &'static str,
    pub lane: Known<String>,
    pub task: Known<String>,
    pub builder: Known<String>,
    pub reviewer: Known<String>,
    pub seat: Known<String>,
    pub verdict_in_flight: Known<String>,
    pub blocker: Known<String>,
    pub observed_at: String,
}

/// The lanes view.
#[derive(Debug, Clone, Serialize)]
pub struct LanesFact {
    /// `ledger` or `observed`.
    pub source: &'static str,
    pub note: Known<String>,
    pub lanes: Vec<LaneFact>,
    pub session: Known<Vec<SessionLaneFact>>,
    pub overall_percent: Known<f64>,
}

/// One finish scenario.
#[derive(Debug, Clone, Serialize)]
pub struct ScenarioFact {
    pub name: &'static str,
    /// The rate the scenario uses, in words.
    pub rate: String,
    pub open_tasks: usize,
    pub finish: WindowFact,
    pub words: String,
    pub intake_since_previous: Known<Vec<String>>,
}

/// The windows view's shared basis.
#[derive(Debug, Clone, Serialize)]
pub struct WindowsFact {
    pub basis_kind: String,
    pub basis: String,
    pub computed_at: String,
    pub lanes_used: usize,
    pub forecast: Known<String>,
    pub scenarios: Vec<ScenarioFact>,
}

/// Who or what clears an issue.
#[derive(Debug, Clone, Serialize)]
pub struct Unblocker {
    /// `lane`, `ci run`, `predecessor`, `operator` or `agent`.
    pub kind: &'static str,
    pub name: Known<String>,
}

/// One issue.
#[derive(Debug, Clone, Serialize)]
pub struct IssueFact {
    pub subject: String,
    pub kind: &'static str,
    pub detail: String,
    pub last_check: Known<String>,
    pub unblocker: Unblocker,
    pub clears: Known<WindowFact>,
    pub recommended_action: Known<String>,
    pub why: Known<String>,
    pub alternatives: Known<Vec<String>>,
    pub command: Known<String>,
    /// Where the fields came from: `records`, `forge`, `ledger`,
    /// `session` or an owed-item source.
    pub source: String,
}

/// A decision put to the operator.
#[derive(Debug, Clone, Serialize)]
pub struct DecisionFact {
    pub context: Known<String>,
    pub options: Known<Vec<String>>,
    pub recommendation: Known<String>,
    pub reasoning: Known<String>,
}

/// One item the operator owes.
#[derive(Debug, Clone, Serialize)]
pub struct OperatorItemFact {
    pub id: String,
    pub source: String,
    pub summary: String,
    pub awaiting: Known<String>,
    pub decision: DecisionFact,
    pub last_check: String,
}

/// A source the operator set reads.
#[derive(Debug, Clone, Serialize)]
pub struct SourceCheck {
    pub source: String,
    pub checked: bool,
    pub reason: Known<String>,
}

/// The operator-owned set.
#[derive(Debug, Clone, Serialize)]
pub struct OperatorSet {
    /// `none owed`, `owed` or `not established`.
    pub state: &'static str,
    pub items: Vec<OperatorItemFact>,
    pub sources: Vec<SourceCheck>,
    pub unchecked: Vec<String>,
}

/// The issues view.
#[derive(Debug, Clone, Serialize)]
pub struct IssuesFact {
    pub tasks: Vec<IssueFact>,
    pub lanes: Vec<IssueFact>,
    pub operator: OperatorSet,
    /// Every source the issues read was checked.
    pub complete: bool,
}

/// One day of pace.
#[derive(Debug, Clone, Serialize)]
pub struct PaceDay {
    pub date: String,
    pub landed: Known<Vec<String>>,
    pub added: Known<Vec<String>>,
    pub open_at_end: Known<usize>,
}

/// Pace and intake.
#[derive(Debug, Clone, Serialize)]
pub struct PaceFact {
    pub days: Vec<PaceDay>,
    pub landed_total: Known<usize>,
    pub added_total: Known<usize>,
    pub lanes_observed: usize,
    pub observed_landing_rate_per_day: Known<f64>,
    pub planned_capacity_rate_per_day: f64,
    pub intake_rate_per_day: Known<f64>,
    pub basis: String,
}

/// Where a measure came from and whether its source was checked.
#[derive(Debug, Clone, Serialize)]
pub struct EvidenceFact {
    pub measure: &'static str,
    pub source: String,
    pub checked: bool,
    pub checked_at: Known<String>,
    pub note: Known<String>,
}
