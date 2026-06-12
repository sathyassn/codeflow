//! Format id generation and shape validation for epics and tasks.
//!
//! v2 uses simplified format ids (charter §7.2):
//! - Epics: `EPC-{NNN}` (e.g. `EPC-001`)
//! - Tasks: `TSK-{EPIC_SEQ}-{TASK_SEQ}` (e.g. `TSK-001-002`)
//!
//! Sequence numbers are determined by querying the `RecordStore` for
//! existing records, then incrementing. The shape validators here are the
//! single source of truth for format-id syntax — `validate` reuses them
//! for frontmatter checks.

use crate::models::{EpicFilter, TaskFilter};

use super::store::RecordStore;
use super::WorkgraphError;

/// Generate the next epic format id.
///
/// # Errors
///
/// Returns `WorkgraphError::Store` on store failure.
pub fn generate_epic_format_id(store: &impl RecordStore) -> Result<String, WorkgraphError> {
    let existing = store.list_epics(EpicFilter::default())?;

    let max_seq = existing
        .iter()
        .filter_map(|e| parse_epic_sequence(&e.format_id))
        .max()
        .unwrap_or(0);

    Ok(format!("EPC-{:03}", max_seq + 1))
}

/// Generate the next task format id within an epic.
///
/// The format is `TSK-{EPIC_SEQ}-{TASK_SEQ}`; the epic sequence is
/// extracted from the parent epic's format id.
///
/// # Errors
///
/// Returns `WorkgraphError::FormatIdGeneration` if the epic format id
/// cannot be parsed, or `WorkgraphError::Store` on store failure.
pub fn generate_task_format_id(
    store: &impl RecordStore,
    epic_format_id: &str,
) -> Result<String, WorkgraphError> {
    let epic_seq = parse_epic_sequence(epic_format_id).ok_or_else(|| {
        WorkgraphError::FormatIdGeneration(format!(
            "cannot parse epic sequence from '{epic_format_id}'"
        ))
    })?;

    let existing = store.list_tasks(TaskFilter::default())?;

    let max_task_seq = existing
        .iter()
        .filter_map(|t| parse_task_sequence(&t.format_id, epic_seq))
        .max()
        .unwrap_or(0);

    Ok(format!("TSK-{epic_seq:03}-{:03}", max_task_seq + 1))
}

/// Check whether a string is a well-formed epic format id (`EPC-NNN`).
#[must_use]
pub fn is_valid_epic_format_id(format_id: &str) -> bool {
    format_id
        .strip_prefix("EPC-")
        .is_some_and(is_sequence_segment)
}

/// Check whether a string is a well-formed task format id (`TSK-NNN-NNN`).
#[must_use]
pub fn is_valid_task_format_id(format_id: &str) -> bool {
    let Some(rest) = format_id.strip_prefix("TSK-") else {
        return false;
    };
    let Some((epic_seq, task_seq)) = rest.split_once('-') else {
        return false;
    };
    is_sequence_segment(epic_seq) && is_sequence_segment(task_seq)
}

/// A sequence segment is at least three ASCII digits (zero-padded to 3).
fn is_sequence_segment(s: &str) -> bool {
    s.len() >= 3 && s.bytes().all(|b| b.is_ascii_digit())
}

/// Parse the epic sequence number from a format id like `EPC-022`.
fn parse_epic_sequence(format_id: &str) -> Option<u32> {
    let suffix = format_id.strip_prefix("EPC-")?;
    suffix.parse().ok()
}

/// Parse the task sequence number from a format id like `TSK-022-013`,
/// matching only tasks within the given epic sequence.
fn parse_task_sequence(format_id: &str, epic_seq: u32) -> Option<u32> {
    let prefix = format!("TSK-{epic_seq:03}-");
    let suffix = format_id.strip_prefix(&prefix)?;
    suffix.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{default_epic_input, default_task_input, new_service};
    use super::{
        is_valid_epic_format_id, is_valid_task_format_id, parse_epic_sequence,
        parse_task_sequence,
    };

    #[test]
    fn test_epic_format_id_generation() {
        let (service, _, _dir) = new_service();

        let epic1 = service.create_epic(default_epic_input()).unwrap();
        assert_eq!(epic1.format_id, "EPC-001");

        let epic2 = service.create_epic(default_epic_input()).unwrap();
        assert_eq!(epic2.format_id, "EPC-002");

        let epic3 = service.create_epic(default_epic_input()).unwrap();
        assert_eq!(epic3.format_id, "EPC-003");
    }

    #[test]
    fn test_task_format_id_generation() {
        let (service, _, _dir) = new_service();
        let epic = service.create_epic(default_epic_input()).unwrap();

        let task1 = service.create_task(default_task_input(&epic.id)).unwrap();
        assert_eq!(task1.format_id, "TSK-001-001");

        let task2 = service.create_task(default_task_input(&epic.id)).unwrap();
        assert_eq!(task2.format_id, "TSK-001-002");
    }

    #[test]
    fn test_task_format_id_across_epics() {
        let (service, _, _dir) = new_service();

        let epic1 = service.create_epic(default_epic_input()).unwrap();
        let epic2 = service.create_epic(default_epic_input()).unwrap();

        let task1_e1 = service.create_task(default_task_input(&epic1.id)).unwrap();
        assert_eq!(task1_e1.format_id, "TSK-001-001");

        let task1_e2 = service.create_task(default_task_input(&epic2.id)).unwrap();
        assert_eq!(task1_e2.format_id, "TSK-002-001");

        let task2_e1 = service.create_task(default_task_input(&epic1.id)).unwrap();
        assert_eq!(task2_e1.format_id, "TSK-001-002");
    }

    // -- Direct tests for parse functions --

    #[test]
    fn test_parse_epic_sequence_valid() {
        assert_eq!(parse_epic_sequence("EPC-001"), Some(1));
        assert_eq!(parse_epic_sequence("EPC-022"), Some(22));
        assert_eq!(parse_epic_sequence("EPC-100"), Some(100));
    }

    #[test]
    fn test_parse_epic_sequence_malformed() {
        assert_eq!(parse_epic_sequence("EPC-"), None);
        assert_eq!(parse_epic_sequence("EPC-abc"), None);
        assert_eq!(parse_epic_sequence("INF-EPC-001"), None);
        assert_eq!(parse_epic_sequence("not-a-format-id"), None);
        assert_eq!(parse_epic_sequence(""), None);
    }

    #[test]
    fn test_parse_task_sequence_valid() {
        assert_eq!(parse_task_sequence("TSK-001-001", 1), Some(1));
        assert_eq!(parse_task_sequence("TSK-022-013", 22), Some(13));
        assert_eq!(parse_task_sequence("TSK-005-042", 5), Some(42));
    }

    #[test]
    fn test_parse_task_sequence_wrong_epic_seq() {
        assert_eq!(parse_task_sequence("TSK-001-001", 2), None);
    }

    #[test]
    fn test_parse_task_sequence_malformed() {
        assert_eq!(parse_task_sequence("TSK-001-", 1), None);
        assert_eq!(parse_task_sequence("TSK-001-abc", 1), None);
        assert_eq!(parse_task_sequence("INF-TSK-001-001", 1), None);
        assert_eq!(parse_task_sequence("not-a-format-id", 1), None);
        assert_eq!(parse_task_sequence("", 1), None);
    }

    // -- Shape validators --

    #[test]
    fn test_is_valid_epic_format_id() {
        assert!(is_valid_epic_format_id("EPC-001"));
        assert!(is_valid_epic_format_id("EPC-999"));
        assert!(is_valid_epic_format_id("EPC-1000"));

        assert!(!is_valid_epic_format_id("EPC-1"));
        assert!(!is_valid_epic_format_id("EPC-01"));
        assert!(!is_valid_epic_format_id("EPC-abc"));
        assert!(!is_valid_epic_format_id("INF-EPC-001"));
        assert!(!is_valid_epic_format_id("TSK-001-001"));
        assert!(!is_valid_epic_format_id(""));
    }

    #[test]
    fn test_is_valid_task_format_id() {
        assert!(is_valid_task_format_id("TSK-001-001"));
        assert!(is_valid_task_format_id("TSK-022-013"));
        assert!(is_valid_task_format_id("TSK-1000-001"));

        assert!(!is_valid_task_format_id("TSK-001"));
        assert!(!is_valid_task_format_id("TSK-1-001"));
        assert!(!is_valid_task_format_id("TSK-001-1"));
        assert!(!is_valid_task_format_id("TSK-001-abc"));
        assert!(!is_valid_task_format_id("INF-TSK-001-001"));
        assert!(!is_valid_task_format_id("EPC-001"));
        assert!(!is_valid_task_format_id(""));
    }
}
