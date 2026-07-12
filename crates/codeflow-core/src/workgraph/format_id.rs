//! Format-id shape validation for epics and tasks.
//!
//! v2 uses simplified format ids (charter §7.2):
//! - Epics: `EPC-{NNN}` (e.g. `EPC-001`)
//! - Tasks: `TSK-{EPIC_SEQ}-{TASK_SEQ}` (e.g. `TSK-001-002`)
//!
//! The shape validators here are the single source of truth for format-id
//! syntax — `validate` reuses them for frontmatter checks.

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

#[cfg(test)]
mod tests {
    use super::{is_valid_epic_format_id, is_valid_task_format_id};

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
