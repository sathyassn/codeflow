//! `codeflow-core` — engine library for the `CodeFlow` v2 discipline layer.
//!
//! Modules arrive via the charter's import waves (docs/plan/v2/00-charter.md §3.2):
//! testing, workgraph/ledger, guards, scaffold, recall.

pub mod capability;
pub mod delegate;
pub mod doctor;
pub mod error;
pub mod file_lock;
pub mod git;
pub mod hooks;
pub mod integrate;
pub mod ledger;
pub mod models;
pub mod recall;
pub mod registry;
pub mod remote;
pub mod scaffold;
pub mod security;
pub mod settings;
pub mod status;
pub mod testing;
pub mod validate;
pub mod workgraph;
