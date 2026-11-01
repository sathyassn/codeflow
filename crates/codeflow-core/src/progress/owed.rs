//! The owed-item source interface (TSK-1095 AC-6 as amended 2026-11-01).
//!
//! A source of items the project owes, such as deferred acceptance results
//! (TSK-436) or an away activation's owed actions (TSK-1045), supplies them
//! through [`OwedSource`]. The status report reads every source once per
//! snapshot and never writes one. A source that cannot be read makes the
//! operator set `not established`, never empty.

/// What an owed item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwedKind {
    /// A criterion's result proved after merge, still owed.
    DeferredResult,
    /// An action the operator owes: run, review, approve, decide, provide.
    OperatorAction,
}

impl OwedKind {
    /// The word the fact model uses.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DeferredResult => "deferred result",
            Self::OperatorAction => "operator action",
        }
    }
}

/// A decision the operator is asked to make, as its source recorded it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Decision {
    pub context: Option<String>,
    pub options: Vec<String>,
    pub recommendation: Option<String>,
    pub reasoning: Option<String>,
}

/// One owed item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwedItem {
    /// The item's id in its source.
    pub id: String,
    /// The task it belongs to, when it belongs to one.
    pub task: Option<String>,
    pub kind: OwedKind,
    pub summary: String,
    /// Who owes it (`operator`, a lane, an agent).
    pub owner: String,
    /// The event that settles it, when recorded.
    pub event: Option<String>,
    /// When the source recorded it, RFC 3339 UTC.
    pub recorded_at: Option<String>,
    pub decision: Option<Decision>,
}

/// One read of a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwedRead {
    /// The source does not exist in this project (no activation, no
    /// deferred result recorded): checked, nothing owed.
    Absent,
    /// The items the source holds; empty means checked and nothing owed.
    Items(Vec<OwedItem>),
    /// The source exists and could not be read: the reason.
    Unreadable(String),
}

/// A source of owed items. Implementations read only; a read never
/// creates, repairs or writes the source.
pub trait OwedSource {
    /// The source's name as the report prints it (`activation`,
    /// `deferred results`).
    fn label(&self) -> &str;
    /// Read the source once.
    fn read(&self) -> OwedRead;
}
