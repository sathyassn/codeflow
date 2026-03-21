//! Git operations: merge conflict detection, branch analysis.
//!
//! Uses `git2` for native git operations instead of subprocess shelling.

pub mod conflict;

pub use conflict::{ConflictResult, RebaseResult, attempt_rebase, check_merge_conflicts};
