use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::enums::ParseEnumError;

/// Work stages within PF4-EXECUTE.
///
/// Each stage maps 1:1 to a teammate role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING-KEBAB-CASE")]
pub enum WorkStage {
    WsDev,
    WsRev,
    WsQa,
    WsTest,
    WsPlan,
    WsDocs,
}

impl fmt::Display for WorkStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WsDev => f.write_str("WS-DEV"),
            Self::WsRev => f.write_str("WS-REV"),
            Self::WsQa => f.write_str("WS-QA"),
            Self::WsTest => f.write_str("WS-TEST"),
            Self::WsPlan => f.write_str("WS-PLAN"),
            Self::WsDocs => f.write_str("WS-DOCS"),
        }
    }
}

impl FromStr for WorkStage {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "WS-DEV" | "ws-dev" | "WsDev" => Ok(Self::WsDev),
            "WS-REV" | "ws-rev" | "WsRev" => Ok(Self::WsRev),
            "WS-QA" | "ws-qa" | "WsQa" => Ok(Self::WsQa),
            "WS-TEST" | "ws-test" | "WsTest" => Ok(Self::WsTest),
            "WS-PLAN" | "ws-plan" | "WsPlan" => Ok(Self::WsPlan),
            "WS-DOCS" | "ws-docs" | "WsDocs" => Ok(Self::WsDocs),
            _ => Err(ParseEnumError {
                enum_name: "WorkStage",
                value: s.to_string(),
            }),
        }
    }
}

impl WorkStage {
    /// Return all stages.
    #[must_use]
    pub const fn all() -> [Self; 6] {
        [
            Self::WsDev,
            Self::WsRev,
            Self::WsQa,
            Self::WsTest,
            Self::WsPlan,
            Self::WsDocs,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stage_display() {
        assert_eq!(WorkStage::WsDev.to_string(), "WS-DEV");
        assert_eq!(WorkStage::WsRev.to_string(), "WS-REV");
        assert_eq!(WorkStage::WsQa.to_string(), "WS-QA");
        assert_eq!(WorkStage::WsTest.to_string(), "WS-TEST");
        assert_eq!(WorkStage::WsPlan.to_string(), "WS-PLAN");
        assert_eq!(WorkStage::WsDocs.to_string(), "WS-DOCS");
    }

    #[test]
    fn test_stage_from_str() {
        assert_eq!("WS-DEV".parse::<WorkStage>().unwrap(), WorkStage::WsDev);
        assert_eq!("ws-rev".parse::<WorkStage>().unwrap(), WorkStage::WsRev);
        assert_eq!("WsQa".parse::<WorkStage>().unwrap(), WorkStage::WsQa);
    }

    #[test]
    fn test_stage_from_str_invalid() {
        assert!("WS-UNKNOWN".parse::<WorkStage>().is_err());
        assert!("".parse::<WorkStage>().is_err());
    }

    #[test]
    fn test_stage_serde_roundtrip() {
        let stage = WorkStage::WsDev;
        let json = serde_json::to_string(&stage).unwrap();
        assert_eq!(json, "\"WS-DEV\"");
        let parsed: WorkStage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, stage);
    }

    #[test]
    fn test_stage_all_count() {
        assert_eq!(WorkStage::all().len(), 6);
    }

    #[test]
    fn test_stage_variant_count() {
        let stages = [
            WorkStage::WsDev,
            WorkStage::WsRev,
            WorkStage::WsQa,
            WorkStage::WsTest,
            WorkStage::WsPlan,
            WorkStage::WsDocs,
        ];
        assert_eq!(stages.len(), 6);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_work_stage() -> impl Strategy<Value = WorkStage> {
        prop_oneof![
            Just(WorkStage::WsDev),
            Just(WorkStage::WsRev),
            Just(WorkStage::WsQa),
            Just(WorkStage::WsTest),
            Just(WorkStage::WsPlan),
            Just(WorkStage::WsDocs),
        ]
    }

    proptest! {
        #[test]
        fn serde_roundtrip(stage in arb_work_stage()) {
            let json = serde_json::to_string(&stage).unwrap();
            let parsed: WorkStage = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(parsed, stage);
        }

        #[test]
        fn display_fromstr_roundtrip(stage in arb_work_stage()) {
            let s = stage.to_string();
            let parsed: WorkStage = s.parse().unwrap();
            prop_assert_eq!(parsed, stage);
        }
    }
}
