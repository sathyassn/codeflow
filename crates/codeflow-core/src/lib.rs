//! `codeflow-core` — engine library for the `CodeFlow` v2 discipline layer.
//!
//! Modules arrive via the charter's import waves (docs/plan/v2/00-charter.md §3.2):
//! testing, workgraph/ledger, guards, scaffold, recall.

mod bounded_file;
pub mod capability;
pub mod delegate;
pub mod doctor;
pub mod error;
pub mod estimate;
pub mod file_lock;
pub mod git;
pub mod hooks;
pub mod ids;
pub mod integrate;
pub mod ledger;
pub mod model_qualification;
pub mod models;
pub mod reading;
pub mod recall;
pub mod registry;
pub mod release_local;
pub mod remote;
pub mod root_checkout;
pub mod scaffold;
pub mod security;
pub mod settings;
pub mod status;
mod strict_json;
pub mod testing;
pub mod validate;
pub mod workgraph;
