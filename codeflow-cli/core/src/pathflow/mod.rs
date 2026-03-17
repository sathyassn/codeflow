//! `PathFlow` checkpoint, sentinel CRUD, and transition recording.
//!
//! Merges the Go `sentinel` and `pathflow` packages into a single module
//! with submodules for sentinel operations, checkpoint state management,
//! and transition event writing.

pub mod checkpoint;
pub mod file_lock;
pub mod sentinel;
pub mod transitions;
