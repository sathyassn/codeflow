use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Pipeline shapes that determine which work stages execute.
///
/// Maps 1:1 to the pipeline table in CLAUDE.md Section 6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PipelineType {
    /// WS-DEV -> WS-REV -> WS-QA (FEAT, FIX, RFCT, CICD, HTFX, CHOR)
    DevRevQa,
    /// WS-DOCS -> WS-REV (DOCS)
    DocsRev,
    /// WS-TEST -> WS-REV -> WS-QA (TEST)
    TestRevQa,
    /// WS-PLAN -> WS-REV (PLAN, SPKE)
    PlanRev,
}

impl fmt::Display for PipelineType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DevRevQa => f.write_str("DEV_REV_QA"),
            Self::DocsRev => f.write_str("DOCS_REV"),
            Self::TestRevQa => f.write_str("TEST_REV_QA"),
            Self::PlanRev => f.write_str("PLAN_REV"),
        }
    }
}

/// Error for failed enum parsing from string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseEnumError {
    pub enum_name: &'static str,
    pub value: String,
}

impl fmt::Display for ParseEnumError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown {}: '{}'", self.enum_name, self.value)
    }
}

impl std::error::Error for ParseEnumError {}

impl FromStr for PipelineType {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "DEV_REV_QA" | "dev_rev_qa" => Ok(Self::DevRevQa),
            "DOCS_REV" | "docs_rev" => Ok(Self::DocsRev),
            "TEST_REV_QA" | "test_rev_qa" => Ok(Self::TestRevQa),
            "PLAN_REV" | "plan_rev" => Ok(Self::PlanRev),
            _ => Err(ParseEnumError {
                enum_name: "PipelineType",
                value: s.to_string(),
            }),
        }
    }
}

/// Decision classification tiers (from CLAUDE.md Section 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionTier {
    /// Standard, reversible decisions. Proceed autonomously.
    Tier1,
    /// Trade-off decisions. Recommend approach, note in commit.
    Tier2,
    /// Architectural/breaking decisions. ADR required, ask user.
    Tier3,
}

impl fmt::Display for DecisionTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tier1 => f.write_str("tier1"),
            Self::Tier2 => f.write_str("tier2"),
            Self::Tier3 => f.write_str("tier3"),
        }
    }
}

impl FromStr for DecisionTier {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "tier1" | "Tier1" | "1" => Ok(Self::Tier1),
            "tier2" | "Tier2" | "2" => Ok(Self::Tier2),
            "tier3" | "Tier3" | "3" => Ok(Self::Tier3),
            _ => Err(ParseEnumError {
                enum_name: "DecisionTier",
                value: s.to_string(),
            }),
        }
    }
}

/// Domain classification for work tracking (from schema `domains` table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DomainType {
    Genl,
    Core,
    Hook,
    Ledg,
    Sess,
}

impl fmt::Display for DomainType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Genl => f.write_str("GENL"),
            Self::Core => f.write_str("CORE"),
            Self::Hook => f.write_str("HOOK"),
            Self::Ledg => f.write_str("LEDG"),
            Self::Sess => f.write_str("SESS"),
        }
    }
}

impl FromStr for DomainType {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "GENL" | "genl" => Ok(Self::Genl),
            "CORE" | "core" => Ok(Self::Core),
            "HOOK" | "hook" => Ok(Self::Hook),
            "LEDG" | "ledg" => Ok(Self::Ledg),
            "SESS" | "sess" => Ok(Self::Sess),
            _ => Err(ParseEnumError {
                enum_name: "DomainType",
                value: s.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- PipelineType tests --

    #[test]
    fn test_pipeline_type_display() {
        assert_eq!(PipelineType::DevRevQa.to_string(), "DEV_REV_QA");
        assert_eq!(PipelineType::DocsRev.to_string(), "DOCS_REV");
        assert_eq!(PipelineType::TestRevQa.to_string(), "TEST_REV_QA");
        assert_eq!(PipelineType::PlanRev.to_string(), "PLAN_REV");
    }

    #[test]
    fn test_pipeline_type_from_str() {
        assert_eq!(
            "DEV_REV_QA".parse::<PipelineType>().unwrap(),
            PipelineType::DevRevQa
        );
        assert_eq!(
            "docs_rev".parse::<PipelineType>().unwrap(),
            PipelineType::DocsRev
        );
    }

    #[test]
    fn test_pipeline_type_from_str_invalid() {
        assert!("unknown".parse::<PipelineType>().is_err());
    }

    #[test]
    fn test_pipeline_type_serde_roundtrip() {
        let pt = PipelineType::TestRevQa;
        let json = serde_json::to_string(&pt).unwrap();
        assert_eq!(json, "\"TEST_REV_QA\"");
        let parsed: PipelineType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, pt);
    }

    // -- DecisionTier tests --

    #[test]
    fn test_decision_tier_display() {
        assert_eq!(DecisionTier::Tier1.to_string(), "tier1");
        assert_eq!(DecisionTier::Tier2.to_string(), "tier2");
        assert_eq!(DecisionTier::Tier3.to_string(), "tier3");
    }

    #[test]
    fn test_decision_tier_from_str() {
        assert_eq!(
            "tier1".parse::<DecisionTier>().unwrap(),
            DecisionTier::Tier1
        );
        assert_eq!(
            "Tier2".parse::<DecisionTier>().unwrap(),
            DecisionTier::Tier2
        );
        assert_eq!("3".parse::<DecisionTier>().unwrap(), DecisionTier::Tier3);
    }

    #[test]
    fn test_decision_tier_from_str_invalid() {
        assert!("tier4".parse::<DecisionTier>().is_err());
    }

    #[test]
    fn test_decision_tier_serde_roundtrip() {
        let dt = DecisionTier::Tier2;
        let json = serde_json::to_string(&dt).unwrap();
        assert_eq!(json, "\"tier2\"");
        let parsed: DecisionTier = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, dt);
    }

    // -- DomainType tests --

    #[test]
    fn test_domain_type_display() {
        assert_eq!(DomainType::Genl.to_string(), "GENL");
        assert_eq!(DomainType::Core.to_string(), "CORE");
        assert_eq!(DomainType::Hook.to_string(), "HOOK");
        assert_eq!(DomainType::Ledg.to_string(), "LEDG");
        assert_eq!(DomainType::Sess.to_string(), "SESS");
    }

    #[test]
    fn test_domain_type_from_str() {
        assert_eq!("GENL".parse::<DomainType>().unwrap(), DomainType::Genl);
        assert_eq!("core".parse::<DomainType>().unwrap(), DomainType::Core);
    }

    #[test]
    fn test_domain_type_from_str_invalid() {
        assert!("UNKNOWN".parse::<DomainType>().is_err());
    }

    #[test]
    fn test_domain_type_serde_roundtrip() {
        let dt = DomainType::Hook;
        let json = serde_json::to_string(&dt).unwrap();
        assert_eq!(json, "\"HOOK\"");
        let parsed: DomainType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, dt);
    }

    #[test]
    fn test_parse_enum_error_display() {
        let err = ParseEnumError {
            enum_name: "PipelineType",
            value: "bad".to_string(),
        };
        assert_eq!(err.to_string(), "unknown PipelineType: 'bad'");
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_pipeline_type() -> impl Strategy<Value = PipelineType> {
        prop_oneof![
            Just(PipelineType::DevRevQa),
            Just(PipelineType::DocsRev),
            Just(PipelineType::TestRevQa),
            Just(PipelineType::PlanRev),
        ]
    }

    fn arb_decision_tier() -> impl Strategy<Value = DecisionTier> {
        prop_oneof![
            Just(DecisionTier::Tier1),
            Just(DecisionTier::Tier2),
            Just(DecisionTier::Tier3),
        ]
    }

    fn arb_domain_type() -> impl Strategy<Value = DomainType> {
        prop_oneof![
            Just(DomainType::Genl),
            Just(DomainType::Core),
            Just(DomainType::Hook),
            Just(DomainType::Ledg),
            Just(DomainType::Sess),
        ]
    }

    proptest! {
        #[test]
        fn pipeline_type_serde_roundtrip(pt in arb_pipeline_type()) {
            let json = serde_json::to_string(&pt).unwrap();
            let parsed: PipelineType = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(parsed, pt);
        }

        #[test]
        fn decision_tier_serde_roundtrip(dt in arb_decision_tier()) {
            let json = serde_json::to_string(&dt).unwrap();
            let parsed: DecisionTier = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(parsed, dt);
        }

        #[test]
        fn domain_type_serde_roundtrip(dt in arb_domain_type()) {
            let json = serde_json::to_string(&dt).unwrap();
            let parsed: DomainType = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(parsed, dt);
        }
    }
}
