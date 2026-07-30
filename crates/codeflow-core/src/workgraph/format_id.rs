//! Stable-id shape validation for epics, specs, and tasks.
//!
//! Canonical records use independent ids:
//! - Epics: `EPC-{NNN}` (e.g. `EPC-001`)
//! - Specs: `SPC-{NNN}` (e.g. `SPC-001`)
//! - Tasks: `TSK-{NNN}` (e.g. `TSK-001`)
//!
//! Historical tasks using `TSK-{EPIC_SEQ}-{TASK_SEQ}` remain read-compatible.

/// Check whether a string is a well-formed epic format id (`EPC-NNN`).
#[must_use]
pub fn is_valid_epic_format_id(format_id: &str) -> bool {
    format_id
        .strip_prefix("EPC-")
        .is_some_and(is_sequence_segment)
}

/// Check whether a string is a well-formed spec id (`SPC-NNN`).
#[must_use]
pub fn is_valid_spec_format_id(id: &str) -> bool {
    id.strip_prefix("SPC-").is_some_and(is_sequence_segment)
}

/// Check whether a string is a canonical task id (`TSK-NNN`).
#[must_use]
pub fn is_canonical_task_format_id(id: &str) -> bool {
    id.strip_prefix("TSK-").is_some_and(is_sequence_segment)
}

/// Check whether a string is a historical task id (`TSK-NNN-NNN`).
#[must_use]
pub fn is_legacy_task_format_id(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("TSK-") else {
        return false;
    };
    let Some((epic_seq, task_seq)) = rest.split_once('-') else {
        return false;
    };
    is_sequence_segment(epic_seq) && is_sequence_segment(task_seq)
}

/// Check whether a task id is canonical or supported historical syntax.
#[must_use]
pub fn is_valid_task_format_id(format_id: &str) -> bool {
    is_canonical_task_format_id(format_id) || is_legacy_task_format_id(format_id)
}

/// A sequence segment is at least three ASCII digits (zero-padded to 3).
fn is_sequence_segment(s: &str) -> bool {
    s.len() >= 3 && s.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::{
        is_canonical_task_format_id, is_legacy_task_format_id, is_valid_epic_format_id,
        is_valid_spec_format_id, is_valid_task_format_id,
    };

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
    fn canonical_and_legacy_task_ids_are_distinguished() {
        assert!(is_canonical_task_format_id("TSK-001"));
        assert!(!is_canonical_task_format_id("TSK-001-001"));
        assert!(is_legacy_task_format_id("TSK-001-001"));
        assert!(!is_legacy_task_format_id("TSK-001"));

        assert!(is_valid_task_format_id("TSK-001"));
        assert!(is_valid_task_format_id("TSK-001-001"));
        assert!(is_valid_task_format_id("TSK-022-013"));
        assert!(is_valid_task_format_id("TSK-1000-001"));

        assert!(!is_valid_task_format_id("TSK-1-001"));
        assert!(!is_valid_task_format_id("TSK-001-1"));
        assert!(!is_valid_task_format_id("TSK-001-abc"));
        assert!(!is_valid_task_format_id("INF-TSK-001-001"));
        assert!(!is_valid_task_format_id("EPC-001"));
        assert!(!is_valid_task_format_id(""));
    }

    #[test]
    fn test_is_valid_spec_format_id() {
        assert!(is_valid_spec_format_id("SPC-001"));
        assert!(is_valid_spec_format_id("SPC-1000"));
        assert!(!is_valid_spec_format_id("SPC-1"));
        assert!(!is_valid_spec_format_id("EPC-001"));
    }
}
