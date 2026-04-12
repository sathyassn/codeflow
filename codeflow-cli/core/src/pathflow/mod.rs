//! `PathFlow` checkpoint, sentinel CRUD, and transition recording.
//!
//! Merges the Go `sentinel` and `pathflow` packages into a single module
//! with submodules for sentinel operations, checkpoint state management,
//! and transition event writing.

pub mod checkpoint;
pub mod gates;
pub mod sentinel;
pub mod transitions;

/// Re-export for backward compatibility. New code should use `crate::file_lock` directly.
pub mod file_lock {
    pub use crate::file_lock::*;
}
