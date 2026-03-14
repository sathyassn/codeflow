//! Status transition validation for tasks and epics.
//!
//! Implements state machine rules as standalone functions rather than methods
//! on `TaskStatus`/`EpicStatus` enums, since transition rules are workgraph
//! business logic, not type-level concerns.

use crate::error::WorkgraphError;
use crate::types::{EpicStatus, TaskStatus};

/// Validate a task status transition.
///
/// Valid transitions:
/// - `Todo` -> `InProgress`, `Blocked`, `Cancelled`
/// - `Blocked` -> `Todo`, `InProgress`, `Cancelled`
/// - `InProgress` -> `Complete`, `Blocked`, `Cancelled`
/// - `Complete` -> (terminal)
/// - `Cancelled` -> (terminal)
///
/// Self-transitions (e.g., `InProgress` -> `InProgress`) are rejected.
///
/// # Errors
///
/// Returns `WorkgraphError::InvalidTransition` if the transition is not allowed.
pub fn validate_task_transition(from: TaskStatus, to: TaskStatus) -> Result<(), WorkgraphError> {
    if from == to {
        return Err(WorkgraphError::InvalidTransition {
            entity: "task".to_string(),
            from: from.to_string(),
            to: to.to_string(),
        });
    }

    let valid = match from {
        TaskStatus::Todo => matches!(
            to,
            TaskStatus::InProgress | TaskStatus::Blocked | TaskStatus::Cancelled
        ),
        TaskStatus::Blocked => matches!(
            to,
            TaskStatus::Todo | TaskStatus::InProgress | TaskStatus::Cancelled
        ),
        TaskStatus::InProgress => matches!(
            to,
            TaskStatus::Complete | TaskStatus::Blocked | TaskStatus::Cancelled
        ),
        TaskStatus::Complete | TaskStatus::Cancelled => false,
    };

    if valid {
        Ok(())
    } else {
        Err(WorkgraphError::InvalidTransition {
            entity: "task".to_string(),
            from: from.to_string(),
            to: to.to_string(),
        })
    }
}

/// Validate an epic status transition.
///
/// Valid transitions:
/// - `Draft` -> `Planning`, `InProgress`, `Archived`
/// - `Planning` -> `InProgress`, `Blocked`, `Archived`
/// - `InProgress` -> `Complete`, `Blocked`, `Archived`
/// - `Blocked` -> `Planning`, `InProgress`, `Archived`
/// - `Complete` -> `Archived`
/// - `Archived` -> (terminal)
///
/// Self-transitions are rejected.
///
/// # Errors
///
/// Returns `WorkgraphError::InvalidTransition` if the transition is not allowed.
pub fn validate_epic_transition(from: EpicStatus, to: EpicStatus) -> Result<(), WorkgraphError> {
    if from == to {
        return Err(WorkgraphError::InvalidTransition {
            entity: "epic".to_string(),
            from: from.to_string(),
            to: to.to_string(),
        });
    }

    let valid = match from {
        EpicStatus::Draft => matches!(
            to,
            EpicStatus::Planning | EpicStatus::InProgress | EpicStatus::Archived
        ),
        EpicStatus::Planning => matches!(
            to,
            EpicStatus::InProgress | EpicStatus::Blocked | EpicStatus::Archived
        ),
        EpicStatus::InProgress => matches!(
            to,
            EpicStatus::Complete | EpicStatus::Blocked | EpicStatus::Archived
        ),
        EpicStatus::Blocked => matches!(
            to,
            EpicStatus::Planning | EpicStatus::InProgress | EpicStatus::Archived
        ),
        EpicStatus::Complete => matches!(to, EpicStatus::Archived),
        EpicStatus::Archived => false,
    };

    if valid {
        Ok(())
    } else {
        Err(WorkgraphError::InvalidTransition {
            entity: "epic".to_string(),
            from: from.to_string(),
            to: to.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_task_transitions() {
        assert!(validate_task_transition(TaskStatus::Todo, TaskStatus::InProgress).is_ok());
        assert!(validate_task_transition(TaskStatus::Todo, TaskStatus::Blocked).is_ok());
        assert!(validate_task_transition(TaskStatus::Todo, TaskStatus::Cancelled).is_ok());

        assert!(validate_task_transition(TaskStatus::Blocked, TaskStatus::Todo).is_ok());
        assert!(validate_task_transition(TaskStatus::Blocked, TaskStatus::InProgress).is_ok());
        assert!(validate_task_transition(TaskStatus::Blocked, TaskStatus::Cancelled).is_ok());

        assert!(validate_task_transition(TaskStatus::InProgress, TaskStatus::Complete).is_ok());
        assert!(validate_task_transition(TaskStatus::InProgress, TaskStatus::Blocked).is_ok());
        assert!(validate_task_transition(TaskStatus::InProgress, TaskStatus::Cancelled).is_ok());
    }

    #[test]
    fn test_invalid_task_transitions() {
        assert!(validate_task_transition(TaskStatus::Todo, TaskStatus::Complete).is_err());
        assert!(validate_task_transition(TaskStatus::Blocked, TaskStatus::Complete).is_err());
        assert!(validate_task_transition(TaskStatus::InProgress, TaskStatus::Todo).is_err());
    }

    #[test]
    fn test_terminal_task_states() {
        assert!(validate_task_transition(TaskStatus::Complete, TaskStatus::Todo).is_err());
        assert!(validate_task_transition(TaskStatus::Complete, TaskStatus::InProgress).is_err());
        assert!(validate_task_transition(TaskStatus::Complete, TaskStatus::Blocked).is_err());
        assert!(validate_task_transition(TaskStatus::Complete, TaskStatus::Cancelled).is_err());

        assert!(validate_task_transition(TaskStatus::Cancelled, TaskStatus::Todo).is_err());
        assert!(validate_task_transition(TaskStatus::Cancelled, TaskStatus::InProgress).is_err());
        assert!(validate_task_transition(TaskStatus::Cancelled, TaskStatus::Blocked).is_err());
        assert!(validate_task_transition(TaskStatus::Cancelled, TaskStatus::Complete).is_err());
    }

    #[test]
    fn test_self_transition_rejected_task() {
        assert!(validate_task_transition(TaskStatus::Todo, TaskStatus::Todo).is_err());
        assert!(validate_task_transition(TaskStatus::InProgress, TaskStatus::InProgress).is_err());
        assert!(validate_task_transition(TaskStatus::Blocked, TaskStatus::Blocked).is_err());
        assert!(validate_task_transition(TaskStatus::Complete, TaskStatus::Complete).is_err());
        assert!(validate_task_transition(TaskStatus::Cancelled, TaskStatus::Cancelled).is_err());
    }

    #[test]
    fn test_valid_epic_transitions() {
        assert!(validate_epic_transition(EpicStatus::Draft, EpicStatus::Planning).is_ok());
        assert!(validate_epic_transition(EpicStatus::Draft, EpicStatus::InProgress).is_ok());
        assert!(validate_epic_transition(EpicStatus::Draft, EpicStatus::Archived).is_ok());

        assert!(validate_epic_transition(EpicStatus::Planning, EpicStatus::InProgress).is_ok());
        assert!(validate_epic_transition(EpicStatus::Planning, EpicStatus::Blocked).is_ok());
        assert!(validate_epic_transition(EpicStatus::Planning, EpicStatus::Archived).is_ok());

        assert!(validate_epic_transition(EpicStatus::InProgress, EpicStatus::Complete).is_ok());
        assert!(validate_epic_transition(EpicStatus::InProgress, EpicStatus::Blocked).is_ok());
        assert!(validate_epic_transition(EpicStatus::InProgress, EpicStatus::Archived).is_ok());

        assert!(validate_epic_transition(EpicStatus::Blocked, EpicStatus::Planning).is_ok());
        assert!(validate_epic_transition(EpicStatus::Blocked, EpicStatus::InProgress).is_ok());
        assert!(validate_epic_transition(EpicStatus::Blocked, EpicStatus::Archived).is_ok());

        assert!(validate_epic_transition(EpicStatus::Complete, EpicStatus::Archived).is_ok());
    }

    #[test]
    fn test_invalid_epic_transitions() {
        assert!(validate_epic_transition(EpicStatus::Draft, EpicStatus::Complete).is_err());
        assert!(validate_epic_transition(EpicStatus::Draft, EpicStatus::Blocked).is_err());
        assert!(validate_epic_transition(EpicStatus::Planning, EpicStatus::Complete).is_err());
        assert!(validate_epic_transition(EpicStatus::Complete, EpicStatus::InProgress).is_err());
    }

    #[test]
    fn test_terminal_epic_state() {
        assert!(validate_epic_transition(EpicStatus::Archived, EpicStatus::Draft).is_err());
        assert!(validate_epic_transition(EpicStatus::Archived, EpicStatus::Planning).is_err());
        assert!(validate_epic_transition(EpicStatus::Archived, EpicStatus::InProgress).is_err());
        assert!(validate_epic_transition(EpicStatus::Archived, EpicStatus::Blocked).is_err());
        assert!(validate_epic_transition(EpicStatus::Archived, EpicStatus::Complete).is_err());
    }

    #[test]
    fn test_self_transition_rejected_epic() {
        assert!(validate_epic_transition(EpicStatus::Draft, EpicStatus::Draft).is_err());
        assert!(validate_epic_transition(EpicStatus::Planning, EpicStatus::Planning).is_err());
        assert!(validate_epic_transition(EpicStatus::InProgress, EpicStatus::InProgress).is_err());
        assert!(validate_epic_transition(EpicStatus::Blocked, EpicStatus::Blocked).is_err());
        assert!(validate_epic_transition(EpicStatus::Complete, EpicStatus::Complete).is_err());
        assert!(validate_epic_transition(EpicStatus::Archived, EpicStatus::Archived).is_err());
    }
}
