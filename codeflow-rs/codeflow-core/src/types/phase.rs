use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::enums::ParseEnumError;

/// `PathFlow` phases (PF1 through PF7).
///
/// Phases progress strictly sequentially: PF1 -> PF2 -> ... -> PF7.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING-KEBAB-CASE")]
pub enum Phase {
    Pf1Init,
    Pf2Context,
    Pf3Classify,
    Pf4Execute,
    Pf5Verify,
    Pf6Complete,
    Pf7End,
}

impl Phase {
    /// Check if a transition from `self` to `next` is valid.
    ///
    /// Phases are strictly sequential: PF1->PF2, PF2->PF3, ..., PF6->PF7.
    #[must_use]
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Pf1Init, Self::Pf2Context)
                | (Self::Pf2Context, Self::Pf3Classify)
                | (Self::Pf3Classify, Self::Pf4Execute)
                | (Self::Pf4Execute, Self::Pf5Verify)
                | (Self::Pf5Verify, Self::Pf6Complete)
                | (Self::Pf6Complete, Self::Pf7End)
        )
    }

    /// Return the numeric phase index (1-7).
    #[must_use]
    pub const fn index(self) -> u8 {
        match self {
            Self::Pf1Init => 1,
            Self::Pf2Context => 2,
            Self::Pf3Classify => 3,
            Self::Pf4Execute => 4,
            Self::Pf5Verify => 5,
            Self::Pf6Complete => 6,
            Self::Pf7End => 7,
        }
    }

    /// Return all phases in order.
    #[must_use]
    pub const fn all() -> [Self; 7] {
        [
            Self::Pf1Init,
            Self::Pf2Context,
            Self::Pf3Classify,
            Self::Pf4Execute,
            Self::Pf5Verify,
            Self::Pf6Complete,
            Self::Pf7End,
        ]
    }
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pf1Init => f.write_str("PF1-INIT"),
            Self::Pf2Context => f.write_str("PF2-CONTEXT"),
            Self::Pf3Classify => f.write_str("PF3-CLASSIFY"),
            Self::Pf4Execute => f.write_str("PF4-EXECUTE"),
            Self::Pf5Verify => f.write_str("PF5-VERIFY"),
            Self::Pf6Complete => f.write_str("PF6-COMPLETE"),
            Self::Pf7End => f.write_str("PF7-END"),
        }
    }
}

impl FromStr for Phase {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "PF1-INIT" | "pf1-init" | "Pf1Init" => Ok(Self::Pf1Init),
            "PF2-CONTEXT" | "pf2-context" | "Pf2Context" => Ok(Self::Pf2Context),
            "PF3-CLASSIFY" | "pf3-classify" | "Pf3Classify" => Ok(Self::Pf3Classify),
            "PF4-EXECUTE" | "pf4-execute" | "Pf4Execute" => Ok(Self::Pf4Execute),
            "PF5-VERIFY" | "pf5-verify" | "Pf5Verify" => Ok(Self::Pf5Verify),
            "PF6-COMPLETE" | "pf6-complete" | "Pf6Complete" => Ok(Self::Pf6Complete),
            "PF7-END" | "pf7-end" | "Pf7End" => Ok(Self::Pf7End),
            _ => Err(ParseEnumError {
                enum_name: "Phase",
                value: s.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_display() {
        assert_eq!(Phase::Pf1Init.to_string(), "PF1-INIT");
        assert_eq!(Phase::Pf4Execute.to_string(), "PF4-EXECUTE");
        assert_eq!(Phase::Pf7End.to_string(), "PF7-END");
    }

    #[test]
    fn test_phase_from_str() {
        assert_eq!("PF1-INIT".parse::<Phase>().unwrap(), Phase::Pf1Init);
        assert_eq!("PF4-EXECUTE".parse::<Phase>().unwrap(), Phase::Pf4Execute);
        assert_eq!("pf7-end".parse::<Phase>().unwrap(), Phase::Pf7End);
    }

    #[test]
    fn test_phase_from_str_invalid() {
        assert!("PF8-UNKNOWN".parse::<Phase>().is_err());
        assert!("".parse::<Phase>().is_err());
    }

    #[test]
    fn test_phase_serde_roundtrip() {
        let phase = Phase::Pf3Classify;
        let json = serde_json::to_string(&phase).unwrap();
        assert_eq!(json, "\"PF3-CLASSIFY\"");
        let parsed: Phase = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, phase);
    }

    #[test]
    fn test_can_transition_valid() {
        assert!(Phase::Pf1Init.can_transition_to(Phase::Pf2Context));
        assert!(Phase::Pf2Context.can_transition_to(Phase::Pf3Classify));
        assert!(Phase::Pf3Classify.can_transition_to(Phase::Pf4Execute));
        assert!(Phase::Pf4Execute.can_transition_to(Phase::Pf5Verify));
        assert!(Phase::Pf5Verify.can_transition_to(Phase::Pf6Complete));
        assert!(Phase::Pf6Complete.can_transition_to(Phase::Pf7End));
    }

    #[test]
    fn test_can_transition_invalid_skip() {
        assert!(!Phase::Pf1Init.can_transition_to(Phase::Pf3Classify));
        assert!(!Phase::Pf2Context.can_transition_to(Phase::Pf4Execute));
    }

    #[test]
    fn test_can_transition_invalid_backward() {
        assert!(!Phase::Pf3Classify.can_transition_to(Phase::Pf2Context));
        assert!(!Phase::Pf7End.can_transition_to(Phase::Pf1Init));
    }

    #[test]
    fn test_can_transition_self() {
        for phase in Phase::all() {
            assert!(!phase.can_transition_to(phase));
        }
    }

    #[test]
    fn test_phase_index() {
        assert_eq!(Phase::Pf1Init.index(), 1);
        assert_eq!(Phase::Pf4Execute.index(), 4);
        assert_eq!(Phase::Pf7End.index(), 7);
    }

    #[test]
    fn test_phase_all_count() {
        assert_eq!(Phase::all().len(), 7);
    }

    #[test]
    fn test_phase_variant_count() {
        let phases = [
            Phase::Pf1Init,
            Phase::Pf2Context,
            Phase::Pf3Classify,
            Phase::Pf4Execute,
            Phase::Pf5Verify,
            Phase::Pf6Complete,
            Phase::Pf7End,
        ];
        assert_eq!(phases.len(), 7);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_phase() -> impl Strategy<Value = Phase> {
        prop_oneof![
            Just(Phase::Pf1Init),
            Just(Phase::Pf2Context),
            Just(Phase::Pf3Classify),
            Just(Phase::Pf4Execute),
            Just(Phase::Pf5Verify),
            Just(Phase::Pf6Complete),
            Just(Phase::Pf7End),
        ]
    }

    proptest! {
        #[test]
        fn serde_roundtrip(phase in arb_phase()) {
            let json = serde_json::to_string(&phase).unwrap();
            let parsed: Phase = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(parsed, phase);
        }

        #[test]
        fn display_fromstr_roundtrip(phase in arb_phase()) {
            let s = phase.to_string();
            let parsed: Phase = s.parse().unwrap();
            prop_assert_eq!(parsed, phase);
        }
    }
}
