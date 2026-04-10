//! Reusable TUI widgets for CodeFlow dashboards.

pub mod checklist;
pub mod detail_pane;
pub mod duration_cell;
pub mod phase_badge;
pub mod selection;
pub mod status_badge;
pub mod step_progress;
pub mod text_input;

pub use checklist::{CheckItem, CheckStatus, Checklist};
pub use detail_pane::DetailPane;
pub use duration_cell::DurationCell;
pub use phase_badge::PhaseBadge;
pub use selection::{SelectOption, SelectionList};
pub use status_badge::StatusBadge;
pub use step_progress::{StepEntry, StepProgress, StepStatus};
pub use text_input::TextInput;
