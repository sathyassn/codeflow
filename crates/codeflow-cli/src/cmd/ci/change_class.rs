//! Shared range classification for CI and landing candidates.
#[cfg(test)]
pub(super) use codeflow_core::workgraph::change_class::RangeEntry;
pub(super) use codeflow_core::workgraph::change_class::{
    classify, project_paths, range_inventory, ChangeClass,
};
