//! The facts `codeflow status` reads, gathered once from records, git, the
//! forge, the ledger, a forecast, a session file and the owed-item
//! sources. The computation in [`super::compute`] reads only this, so every
//! view can be tested on plain data.

use std::collections::BTreeMap;

use super::owed::OwedRead;
use super::session::SessionFile;

/// What the reader asked about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeSpec {
    /// One task.
    Task(String),
    /// One or more epics.
    Epics(Vec<String>),
    /// Every task whose home is this integration line.
    Line(String),
    /// Every open epic whose home is the current target.
    Default,
}

impl ScopeSpec {
    /// The scope's kind as the fact model names it.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Task(_) => "task",
            Self::Epics(_) => "epic",
            Self::Line(_) => "line",
            Self::Default => "default",
        }
    }

    /// The ids or line the scope names, in order.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        match self {
            Self::Task(id) | Self::Line(id) => vec![id.clone()],
            Self::Epics(ids) => {
                let mut ids = ids.clone();
                ids.sort();
                ids.dedup();
                ids
            }
            Self::Default => Vec::new(),
        }
    }
}

/// A `## Blocker` section as recorded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlockerIn {
    pub reason: Option<String>,
    pub owner: Option<String>,
    pub revisit: Option<String>,
}

/// The record's acceptance block, as read from the most advanced copy of
/// the record (the home tip once complete, else the head the pull request
/// or branch reports).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceIn {
    /// The record's current criteria whose result is `verified`.
    pub verified: usize,
    /// Criteria the record lists now.
    pub total: usize,
    /// The reviewed commit the block names.
    pub reviewed: String,
    /// The block's verdict is `approved`.
    pub approved: bool,
    /// The record has one active block and it passes the structural
    /// acceptance rules against the record's current criteria.
    pub valid: bool,
    /// Bound to its reviewed commit: for a closed task, its completion is
    /// bound on the home target as the landing gate binds it; for an open
    /// task, nothing but the record changed between the reviewed commit and
    /// the reported head, and the criteria are the ones the review saw.
    pub bound: bool,
    /// Why the block is invalid or not bound, when it is either.
    pub note: Option<String>,
}

/// One check run of a pull request, as the forge reported it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRunIn {
    pub name: String,
    /// `success`, `failure`, `pending`, `skipped`, `neutral`, `cancelled`
    /// or another conclusion as read, lower case.
    pub state: String,
    /// When the run completed, as the forge gave it.
    pub completed_at: Option<String>,
}

/// The task's work branch as git holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchIn {
    pub name: String,
    pub head: String,
    /// Commits on the branch that its target does not have.
    pub commits_ahead: usize,
    /// Commit time of the head, seconds since the epoch.
    pub head_time: i64,
    /// A linked worktree with this branch checked out.
    pub worktree: Option<String>,
}

/// One task as the gatherer found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskIn {
    pub id: String,
    pub title: String,
    pub epic: Option<String>,
    /// `todo`, `blocked`, `complete` or `cancelled` on its home tip.
    pub status: String,
    /// Its declared integration target, as a bare branch name.
    pub target: String,
    pub depends_on: Vec<String>,
    /// `created:` from the record, `YYYY-MM-DD`.
    pub created: Option<String>,
    /// The readiness core's state (`ready`, `active`, `landed`, `waiting`,
    /// `blocked`, `invalid`) for an open task; `None` when it is closed or
    /// the core could not judge the tip.
    pub state: Option<String>,
    pub state_reason: String,
    pub blocker: Option<BlockerIn>,
    pub theme: Option<String>,
    /// The `## Outcome` text, when stated.
    pub outcome: Option<String>,
    /// Outcome and Context, or Description where there is no Context.
    pub about: Option<String>,
    /// `## Follow-ups` or the acceptance block's `follow_ups`, when present.
    pub follow_ups: Option<String>,
    pub acceptance: Option<AcceptanceIn>,
    pub branch: Option<BranchIn>,
    /// The task's current completion is on the trusted remote's default
    /// branch: its landing transition commit is reachable from that ref.
    /// `None` when the ref or the transition cannot be read.
    pub on_main: Option<bool>,
    /// The same proof against the remote ref of the task's home target.
    pub on_home_ref: Option<bool>,
    /// Why the task's stage cannot be known (its home target could not be
    /// read, its status is not one the ladder knows, or the readiness core
    /// could not judge it); `None` when it can.
    pub unknown: Option<String>,
    /// The record exists on its home tip.
    pub record_on_line: bool,
    /// When the task landed on its home target, seconds since the epoch.
    pub landed_at: Option<i64>,
}

/// One epic record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpicIn {
    pub id: String,
    pub title: String,
    pub status: String,
    pub outcome: Option<String>,
    pub about: Option<String>,
    /// The `##` heading of the first section that names each task id.
    pub sections: BTreeMap<String, String>,
    /// Each `##` heading's section text, for a theme's about.
    pub section_text: BTreeMap<String, String>,
    /// Capability entries in `docs/capabilities.md` that list this epic:
    /// `(CAP-NNN, name, prose after its block)`.
    pub capabilities: Vec<(String, String, String)>,
}

/// A record that could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnreadableIn {
    pub path: String,
    pub reason: String,
    /// The task id its file name carries, when it is a task record.
    pub task_id: Option<String>,
}

/// A remote ref and when it was fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefIn {
    pub name: String,
    pub sha: Option<String>,
}

/// One open pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullIn {
    pub number: u64,
    pub title: String,
    pub head_ref: String,
    pub head_sha: String,
    pub base: String,
    pub draft: bool,
    /// The checks summary: `failed`, `pending`, `missing`, `skipped` or
    /// `passed`, never erasing a failure or a skip.
    pub checks: String,
    /// Each check run as read.
    pub runs: Vec<CheckRunIn>,
    /// The task id its head branch carries.
    pub task: Option<String>,
}

/// What the forge read gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForgeIn {
    /// The open pull requests, read at this time.
    Read { read_at: i64, pulls: Vec<PullIn> },
    /// No read this time: why, and the last read time when one is known.
    Unverified { reason: String },
}

/// A ledger lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneIn {
    pub id: String,
    pub stream: String,
    pub scope: Vec<String>,
    pub attempt: u64,
    pub seat: Option<String>,
    pub reported_at: Option<i64>,
}

/// A ledger follow-up the operator must settle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorItemIn {
    pub id: String,
    pub title: String,
    pub awaiting: String,
    pub done_when: String,
    pub added_at: Option<i64>,
}

/// What the coordination ledger gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerIn {
    /// No ledger in this clone.
    Absent,
    /// The ledger exists but could not be read.
    Unreadable(String),
    /// Open lanes and open operator follow-ups.
    Read {
        lanes: Vec<LaneIn>,
        operator_items: Vec<OperatorItemIn>,
    },
}

/// Per-task end offsets from an adopted forecast.
#[derive(Debug, Clone, PartialEq)]
pub struct ForecastIn {
    /// The forecast file, repository-relative.
    pub path: String,
    /// A pinned record changed since it was frozen.
    pub stale: bool,
    /// Task id to `(favorable, planning, adverse)` end in seconds from the
    /// snapshot time (zero when already past).
    pub ends: BTreeMap<String, (i64, i64, i64)>,
}

/// Everything the computation reads.
#[derive(Debug, Clone)]
pub struct Inputs {
    /// The snapshot time, seconds since the epoch, UTC.
    pub now: i64,
    pub repository: String,
    /// The repository's absolute path, for cited commands.
    pub repo_path: String,
    pub scope: ScopeSpec,
    /// The current target: the line or default branch the scope is read
    /// against, as a bare branch name.
    pub current_target: String,
    /// The default work target as a bare branch name (`main`).
    pub default_branch: String,
    pub default_ref: Option<RefIn>,
    pub line_refs: BTreeMap<String, RefIn>,
    /// When the refs were last fetched (`FETCH_HEAD`).
    pub fetched_at: Option<i64>,
    /// The sweep interval in seconds (`[sweep] interval_minutes`).
    pub sweep_seconds: i64,
    pub tasks: Vec<TaskIn>,
    pub epics: Vec<EpicIn>,
    pub unreadable: Vec<UnreadableIn>,
    /// Why the readiness core could not judge, when it could not.
    pub readiness_problem: Option<String>,
    pub forge: ForgeIn,
    pub ledger: LedgerIn,
    /// Linked worktrees with their checked-out branch, for observed lanes.
    pub session: Option<SessionFile>,
    pub owed: Vec<(String, OwedRead)>,
    pub forecast: Option<ForecastIn>,
    /// Why an adopted forecast was not used, when one exists.
    pub forecast_note: Option<String>,
    /// The project keeps a release step (`docs/releasing.md`).
    pub release_step: bool,
    /// The history walk of the home tips succeeded and reached back past
    /// the seven-day pace window.
    pub history_read: bool,
    /// Record coverage lost: the target that could not be resolved or read
    /// (`None` when the targets could not be listed at all), and why. A
    /// loss that touches the scope makes its percents unknown and the
    /// records source unchecked.
    pub record_losses: Vec<(Option<String>, String)>,
}
