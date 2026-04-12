use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::phase::Phase;
use super::stage::WorkStage;

/// Sentinel types used by the `PathFlow` enforcement system.
///
/// Phase sentinels (pf-1 through pf-7) gate phase progression.
/// Stage sentinels (ws-dev, ws-rev, etc.) gate stage ordering within PF4-EXECUTE.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Sentinel {
    PathflowPf1,
    PathflowPf2,
    PathflowPf3,
    PathflowPf4,
    PathflowPf5,
    PathflowPf6,
    PathflowPf7,
    PathflowWsDev,
    PathflowWsSec,
    PathflowWsRev,
    PathflowWsQa,
    PathflowWsTest,
    PathflowWsPlan,
    PathflowWsDocs,
}

impl Sentinel {
    /// Return the sentinel file name (e.g., `pathflow-pf-1`, `pathflow-ws-dev`).
    #[must_use]
    pub fn file_name(self) -> &'static str {
        match self {
            Self::PathflowPf1 => "pathflow-pf-1",
            Self::PathflowPf2 => "pathflow-pf-2",
            Self::PathflowPf3 => "pathflow-pf-3",
            Self::PathflowPf4 => "pathflow-pf-4",
            Self::PathflowPf5 => "pathflow-pf-5",
            Self::PathflowPf6 => "pathflow-pf-6",
            Self::PathflowPf7 => "pathflow-pf-7",
            Self::PathflowWsDev => "pathflow-ws-dev",
            Self::PathflowWsSec => "pathflow-ws-sec",
            Self::PathflowWsRev => "pathflow-ws-rev",
            Self::PathflowWsQa => "pathflow-ws-qa",
            Self::PathflowWsTest => "pathflow-ws-test",
            Self::PathflowWsPlan => "pathflow-ws-plan",
            Self::PathflowWsDocs => "pathflow-ws-docs",
        }
    }

    /// Return all phase sentinels in order.
    #[must_use]
    pub const fn phase_sentinels() -> [Self; 7] {
        [
            Self::PathflowPf1,
            Self::PathflowPf2,
            Self::PathflowPf3,
            Self::PathflowPf4,
            Self::PathflowPf5,
            Self::PathflowPf6,
            Self::PathflowPf7,
        ]
    }

    /// Return all stage sentinels.
    #[must_use]
    pub const fn stage_sentinels() -> [Self; 7] {
        [
            Self::PathflowWsDev,
            Self::PathflowWsSec,
            Self::PathflowWsRev,
            Self::PathflowWsQa,
            Self::PathflowWsTest,
            Self::PathflowWsPlan,
            Self::PathflowWsDocs,
        ]
    }
}

impl fmt::Display for Sentinel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.file_name())
    }
}

/// Error returned when parsing a sentinel string fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseSentinelError(pub String);

impl fmt::Display for ParseSentinelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown sentinel: '{}'", self.0)
    }
}

impl std::error::Error for ParseSentinelError {}

impl FromStr for Sentinel {
    type Err = ParseSentinelError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pathflow-pf-1" => Ok(Self::PathflowPf1),
            "pathflow-pf-2" => Ok(Self::PathflowPf2),
            "pathflow-pf-3" => Ok(Self::PathflowPf3),
            "pathflow-pf-4" => Ok(Self::PathflowPf4),
            "pathflow-pf-5" => Ok(Self::PathflowPf5),
            "pathflow-pf-6" => Ok(Self::PathflowPf6),
            "pathflow-pf-7" => Ok(Self::PathflowPf7),
            "pathflow-ws-dev" => Ok(Self::PathflowWsDev),
            "pathflow-ws-sec" => Ok(Self::PathflowWsSec),
            "pathflow-ws-rev" => Ok(Self::PathflowWsRev),
            "pathflow-ws-qa" => Ok(Self::PathflowWsQa),
            "pathflow-ws-test" => Ok(Self::PathflowWsTest),
            "pathflow-ws-plan" => Ok(Self::PathflowWsPlan),
            "pathflow-ws-docs" => Ok(Self::PathflowWsDocs),
            _ => Err(ParseSentinelError(s.to_string())),
        }
    }
}

impl From<Phase> for Sentinel {
    fn from(phase: Phase) -> Self {
        match phase {
            Phase::Pf1Init => Self::PathflowPf1,
            Phase::Pf2Context => Self::PathflowPf2,
            Phase::Pf3Classify => Self::PathflowPf3,
            Phase::Pf4Execute => Self::PathflowPf4,
            Phase::Pf5Verify => Self::PathflowPf5,
            Phase::Pf6Complete => Self::PathflowPf6,
            Phase::Pf7End => Self::PathflowPf7,
        }
    }
}

impl From<WorkStage> for Sentinel {
    fn from(stage: WorkStage) -> Self {
        match stage {
            WorkStage::WsDev => Self::PathflowWsDev,
            WorkStage::WsSec => Self::PathflowWsSec,
            WorkStage::WsRev => Self::PathflowWsRev,
            WorkStage::WsQa => Self::PathflowWsQa,
            WorkStage::WsTest => Self::PathflowWsTest,
            WorkStage::WsPlan => Self::PathflowWsPlan,
            WorkStage::WsDocs => Self::PathflowWsDocs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sentinel_display_phase() {
        assert_eq!(Sentinel::PathflowPf1.to_string(), "pathflow-pf-1");
        assert_eq!(Sentinel::PathflowPf3.to_string(), "pathflow-pf-3");
        assert_eq!(Sentinel::PathflowPf7.to_string(), "pathflow-pf-7");
    }

    #[test]
    fn test_sentinel_display_stage() {
        assert_eq!(Sentinel::PathflowWsDev.to_string(), "pathflow-ws-dev");
        assert_eq!(Sentinel::PathflowWsRev.to_string(), "pathflow-ws-rev");
        assert_eq!(Sentinel::PathflowWsQa.to_string(), "pathflow-ws-qa");
        assert_eq!(Sentinel::PathflowWsDocs.to_string(), "pathflow-ws-docs");
    }

    #[test]
    fn test_sentinel_from_str_valid() {
        assert_eq!(
            "pathflow-pf-1".parse::<Sentinel>().unwrap(),
            Sentinel::PathflowPf1
        );
        assert_eq!(
            "pathflow-ws-dev".parse::<Sentinel>().unwrap(),
            Sentinel::PathflowWsDev
        );
    }

    #[test]
    fn test_sentinel_from_str_invalid() {
        assert!("unknown".parse::<Sentinel>().is_err());
        assert!("pathflow-pf-8".parse::<Sentinel>().is_err());
        assert!("".parse::<Sentinel>().is_err());
    }

    #[test]
    fn test_sentinel_from_phase() {
        assert_eq!(Sentinel::from(Phase::Pf1Init), Sentinel::PathflowPf1);
        assert_eq!(Sentinel::from(Phase::Pf4Execute), Sentinel::PathflowPf4);
        assert_eq!(Sentinel::from(Phase::Pf7End), Sentinel::PathflowPf7);
    }

    #[test]
    fn test_sentinel_from_stage() {
        assert_eq!(Sentinel::from(WorkStage::WsDev), Sentinel::PathflowWsDev);
        assert_eq!(Sentinel::from(WorkStage::WsQa), Sentinel::PathflowWsQa);
        assert_eq!(Sentinel::from(WorkStage::WsDocs), Sentinel::PathflowWsDocs);
    }

    #[test]
    fn test_sentinel_serde_roundtrip() {
        let sentinel = Sentinel::PathflowPf3;
        let json = serde_json::to_string(&sentinel).unwrap();
        let parsed: Sentinel = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, sentinel);
    }

    #[test]
    fn test_phase_sentinels_count() {
        assert_eq!(Sentinel::phase_sentinels().len(), 7);
    }

    #[test]
    fn test_stage_sentinels_count() {
        assert_eq!(Sentinel::stage_sentinels().len(), 7);
    }

    #[test]
    fn test_pathflow_ws_sec_sentinel() {
        assert_eq!(Sentinel::PathflowWsSec.file_name(), "pathflow-ws-sec");
        assert_eq!(Sentinel::PathflowWsSec.to_string(), "pathflow-ws-sec");
        assert_eq!(
            "pathflow-ws-sec".parse::<Sentinel>().unwrap(),
            Sentinel::PathflowWsSec
        );
        assert_eq!(Sentinel::from(WorkStage::WsSec), Sentinel::PathflowWsSec);
        assert!(Sentinel::stage_sentinels().contains(&Sentinel::PathflowWsSec));
    }

    #[test]
    fn test_file_name_matches_display() {
        for sentinel in Sentinel::phase_sentinels() {
            assert_eq!(sentinel.file_name(), sentinel.to_string());
        }
        for sentinel in Sentinel::stage_sentinels() {
            assert_eq!(sentinel.file_name(), sentinel.to_string());
        }
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_sentinel() -> impl Strategy<Value = Sentinel> {
        prop_oneof![
            Just(Sentinel::PathflowPf1),
            Just(Sentinel::PathflowPf2),
            Just(Sentinel::PathflowPf3),
            Just(Sentinel::PathflowPf4),
            Just(Sentinel::PathflowPf5),
            Just(Sentinel::PathflowPf6),
            Just(Sentinel::PathflowPf7),
            Just(Sentinel::PathflowWsDev),
            Just(Sentinel::PathflowWsSec),
            Just(Sentinel::PathflowWsRev),
            Just(Sentinel::PathflowWsQa),
            Just(Sentinel::PathflowWsTest),
            Just(Sentinel::PathflowWsPlan),
            Just(Sentinel::PathflowWsDocs),
        ]
    }

    proptest! {
        #[test]
        fn serde_roundtrip(sentinel in arb_sentinel()) {
            let json = serde_json::to_string(&sentinel).unwrap();
            let parsed: Sentinel = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(parsed, sentinel);
        }

        #[test]
        fn display_fromstr_roundtrip(sentinel in arb_sentinel()) {
            let s = sentinel.to_string();
            let parsed: Sentinel = s.parse().unwrap();
            prop_assert_eq!(parsed, sentinel);
        }
    }
}
