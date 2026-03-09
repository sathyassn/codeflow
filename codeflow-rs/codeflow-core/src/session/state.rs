//! Session status state machine with validated transitions.
//!
//! Defines which `SessionStatus` transitions are legal. Only these transitions
//! are allowed:
//!
//! - `Active -> Ended` (normal shutdown)
//! - `Active -> Crashed` (abnormal termination)
//! - `Crashed -> Active` (session recovery)

use crate::types::SessionStatus;

impl SessionStatus {
    /// Returns `true` if transitioning from `self` to `target` is a valid
    /// session lifecycle transition.
    ///
    /// # Valid transitions
    ///
    /// | From | To | Reason |
    /// |------|----|--------|
    /// | `Active` | `Ended` | Normal shutdown |
    /// | `Active` | `Crashed` | Abnormal termination |
    /// | `Crashed` | `Active` | Recovery restart |
    ///
    /// All other transitions (including self-transitions like `Active -> Active`)
    /// are invalid.
    #[must_use]
    pub fn can_transition_to(self, target: Self) -> bool {
        matches!(
            (self, target),
            (Self::Active, Self::Ended | Self::Crashed) | (Self::Crashed, Self::Active)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_active_to_ended_valid() {
        assert!(SessionStatus::Active.can_transition_to(SessionStatus::Ended));
    }

    #[test]
    fn test_active_to_crashed_valid() {
        assert!(SessionStatus::Active.can_transition_to(SessionStatus::Crashed));
    }

    #[test]
    fn test_crashed_to_active_valid_recovery() {
        assert!(SessionStatus::Crashed.can_transition_to(SessionStatus::Active));
    }

    #[test]
    fn test_active_to_active_invalid() {
        assert!(!SessionStatus::Active.can_transition_to(SessionStatus::Active));
    }

    #[test]
    fn test_ended_to_active_invalid() {
        assert!(!SessionStatus::Ended.can_transition_to(SessionStatus::Active));
    }

    #[test]
    fn test_ended_to_ended_invalid() {
        assert!(!SessionStatus::Ended.can_transition_to(SessionStatus::Ended));
    }

    #[test]
    fn test_ended_to_crashed_invalid() {
        assert!(!SessionStatus::Ended.can_transition_to(SessionStatus::Crashed));
    }

    #[test]
    fn test_crashed_to_ended_invalid() {
        assert!(!SessionStatus::Crashed.can_transition_to(SessionStatus::Ended));
    }

    #[test]
    fn test_crashed_to_crashed_invalid() {
        assert!(!SessionStatus::Crashed.can_transition_to(SessionStatus::Crashed));
    }

    /// Exhaustive: every (from, to) pair is covered.
    #[test]
    fn test_exhaustive_transition_matrix() {
        let all = [
            SessionStatus::Active,
            SessionStatus::Ended,
            SessionStatus::Crashed,
        ];
        let expected_valid = [
            (SessionStatus::Active, SessionStatus::Ended),
            (SessionStatus::Active, SessionStatus::Crashed),
            (SessionStatus::Crashed, SessionStatus::Active),
        ];

        for from in &all {
            for to in &all {
                let is_valid = from.can_transition_to(*to);
                let should_be_valid = expected_valid.contains(&(*from, *to));
                assert_eq!(
                    is_valid, should_be_valid,
                    "transition {from} -> {to}: expected {should_be_valid}, got {is_valid}"
                );
            }
        }
    }
}
