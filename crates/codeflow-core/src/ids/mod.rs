//! The shared id registry (SPC-013 R-1 to R-25, R-108 to R-111; ADR-0072).
//!
//! Every repository that tracks work keeps one data branch,
//! `codeflow/registry`, on the authority remote (`origin`). It holds one file
//! per issued id at `ids/<KIND>/<N>.toml`, binding the number to the record's
//! hidden `uid`. Issuing an id is a plain, never forced push of one new file on
//! top of the fetched tip: the host accepts it only if it extends the tip, so
//! the tip is the compare-and-swap that serialises issuers.
//!
//! ```text
//!   fetch (non-forced) --> check history --> N = 1 + max(history, refs)
//!        ^                                         |
//!        |  moved tip (retry, at most 5)           v
//!        +------------------------------ push ids/<KIND>/<N>.toml
//!                                                  |
//!                             accepted: write the record with its uid
//! ```
//!
//! Offline, the reservation is a commit on the local `codeflow/registry`,
//! "pending, not unique yet", which `ids sync` publishes. The branch only
//! grows; pre-push, git-guard and `ids check` enforce it, and a typed restore
//! is the only repair.

pub mod check;
pub mod entry;
pub mod git;
pub mod inventory;
pub mod issue;
pub mod ledger;
pub mod seed;
pub mod state;

pub use entry::{is_uid, new_uid, record_id_from_path, Entry, Kind, RegId};
pub use git::Git;
pub use ledger::Ledger;

/// The registry data branch.
pub const REGISTRY_BRANCH: &str = "codeflow/registry";

/// The local registry ref, which holds pending reservations.
pub const REGISTRY_REF: &str = "refs/heads/codeflow/registry";

/// The authority remote.
pub const AUTHORITY: &str = "origin";

/// Attempts at one push before giving up: the first and five retries
/// (SPC-013 planning resolution 4).
pub const PUSH_ATTEMPTS: usize = 6;

/// Today's UTC date, `YYYY-MM-DD`.
#[must_use]
pub fn today() -> String {
    crate::workgraph::now_rfc3339()[..10].to_string()
}

/// The remote-tracking registry ref for `remote`.
#[must_use]
pub fn tracking_ref(remote: &str) -> String {
    format!("refs/remotes/{remote}/{REGISTRY_BRANCH}")
}

/// Registry failures. Each names what happened and what to do.
#[derive(Debug, thiserror::Error)]
pub enum IdsError {
    #[error("{0}")]
    Git(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("the registry is damaged, so no id is issued until a typed restore lands:\n  {}\nrepair: `codeflow ids restore <id>...` (a maintainer with push rights)", .0.join("\n  "))]
    Damaged(Vec<String>),
    #[error("the registry `codeflow/registry` does not exist yet but records do; a maintainer seeds it once with `codeflow ids seed`")]
    NotSeeded,
    #[error("the authority's registry was rewritten: {0}; no id is issued (R-10)")]
    Rewritten(String),
    #[error("push to the registry was refused for permission: {0}")]
    Permission(String),
    #[error("push to the registry is not supported by the remote: {0}")]
    Unsupported(String),
    #[error("push to the registry failed in transport: {0}")]
    Transport(String),
    #[error("push to the registry was refused by a hook: {0}")]
    Refused(String),
    #[error("the registry tip kept moving; gave up after {0} attempts: {1}")]
    Contended(usize, String),
    #[error("the authority is unreachable: {0}")]
    Offline(String),
    #[error("{0}")]
    Clash(String),
    #[error("{0}")]
    Invalid(String),
}

/// How one reservation stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// Accepted by the authority: unique.
    Reserved,
    /// No authority remote: reserved in this repository's own registry.
    Local,
    /// Offline or unclear: a local commit, "pending, not unique yet".
    Pending,
}

impl Standing {
    /// A one-line description for reports.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Standing::Reserved => "reserved on the authority",
            Standing::Local => "reserved in the local registry (no authority remote)",
            Standing::Pending => "pending, not unique yet; publish it with `codeflow ids sync`",
        }
    }
}
