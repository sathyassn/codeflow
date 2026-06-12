//! Git operations: merge conflict detection, branch analysis, CI polling.
//!
//! Uses `git2` for native git operations instead of subprocess shelling;
//! CI polling shells to the `gh` CLI.

pub mod ci;
pub mod conflict;

pub use ci::{CiOutcome, CiWaitConfig, CiWaitError, wait_for_ci_green};
pub use conflict::{ConflictResult, RebaseResult, attempt_rebase, check_merge_conflicts};
