//! Schema 5 model catalog and deterministic duty resolution (ADR-0069).
//!
//! This module reads data only. It never launches, probes, reads a clock, or
//! authenticates an operator. Callers supply fresh native facts and anchored
//! plan records. Product lines order versions from oldest to newest.

mod authority;
mod diagnostics;
mod inputs;
mod operator;
mod overlay;
mod resolve;
pub mod scan;
mod selection;
mod validate;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use crate::model_qualification::RouteEffort as Effort;
pub use authority::{
    AnchoredAuthority, AuthorityRecord, AuthorityRole, DesignAuthority, LineAuthority, ReviewMode,
    ReviewPer, StandingReview, DESIGN_APPROVAL,
};
pub use inputs::{load_catalog, CatalogInputs};
pub use operator::{anchored_override, parse_override_route};
pub use overlay::{CandidateAddition, PersonalOverlay};
pub use resolve::{
    Eligibility, EligibilityRequest, Exclusion, ExclusionScope, OpenParticipant, OperatorOverride,
    Resolution, ResolveRequest, ResolvedAlternative, ResolvedParticipant,
};
pub use selection::{BindingReference, ProjectSelection};

/// Managed schema 5 data. Load through [`Catalog::parse`] before resolving.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    #[serde(skip)]
    pub(crate) selection: Option<ProjectSelection>,
    /// Design authority committed on the task's integration target; never
    /// the working tree (issue 43).
    #[serde(skip)]
    pub(crate) authority: Option<AnchoredAuthority>,
    /// Why a working-tree design-authority block was not applied.
    #[serde(skip)]
    pub(crate) authority_note: Option<String>,
    pub schema_version: u64,
    pub policy_id: String,
    pub families: Vec<Family>,
    pub lines: Vec<ProductLine>,
    pub seats: Vec<Seat>,
    pub standing_seats: Vec<String>,
    pub design_owner: String,
    pub duties: BTreeMap<String, Duty>,
    pub high_triggers: Vec<String>,
    pub xhigh_triggers: Vec<String>,
    /// Named policy to its trigger facts; no expressions or executable content.
    pub named_policies: BTreeMap<String, Vec<String>>,
    pub rules: Vec<String>,
    #[serde(default)]
    pub bindings: Vec<crate::model_qualification::QualifiedBinding>,
}

/// A family retains its provider, independent lineage and usage boundary.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Family {
    pub id: String,
    pub provider: String,
    pub lineage: String,
    pub harnesses: Vec<String>,
    pub probes: Vec<String>,
    pub usage_bucket: String,
}

/// Manual adoption pins worker selection until this managed reference changes.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProductLine {
    pub id: String,
    pub family: String,
    pub adoption: Adoption,
    pub adopted_version: String,
    pub versions: Vec<Version>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Adoption {
    Manual,
    Workers,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Lifecycle {
    Active,
    FallbackOnly,
    Retired,
}

/// Designation and qualification are independent, non-ordinal evidence.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Version {
    pub id: String,
    pub alias: String,
    pub pinned_id: String,
    pub selectors: BTreeMap<String, String>,
    pub efforts: Vec<Effort>,
    pub lifecycle: Lifecycle,
    pub designations: Vec<Designation>,
    pub qualification: Vec<ScopedEvidence>,
    /// Overlay provenance cannot be supplied by catalog JSON.
    #[serde(skip)]
    pub(crate) personal_candidate: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Designation {
    pub seat: String,
    pub date: String,
    pub record: String,
}

/// Worker evidence authorizes exactly one tuple, never a primary seat.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScopedEvidence {
    pub harness: String,
    pub selector: String,
    pub effort: Effort,
    pub duty: String,
    pub record: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Seat {
    pub id: String,
    pub family: String,
    pub role: String,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Duty {
    pub required: Vec<Participant>,
    pub triggered: Vec<TriggeredParticipant>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Participant {
    pub id: String,
    pub alternatives: Vec<Alternative>,
    pub relation: LineageRelation,
    pub label: ParticipantLabel,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ParticipantLabel {
    Required,
    SecondOpinion,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LineageRelation {
    Any,
    OppositeAuthor,
    SameAuthor,
    SameHost,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TriggeredParticipant {
    pub trigger: String,
    pub unless_author: Option<String>,
    pub participant: Participant,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Alternative {
    pub target: Target,
    /// None uses the host harness when supported, else the family's first one.
    pub harness: Option<String>,
    pub effort: Effort,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
pub enum Target {
    Seat(String),
    Line(String),
    HostSeat,
}

impl Catalog {
    /// Parse strictly, rejecting duplicate keys before typed deserialization.
    ///
    /// # Errors
    /// Returns the first malformed field, reference or policy invariant.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let catalog: Self =
            crate::strict_json::parse_strict_json(bytes).map_err(|error| error.to_string())?;
        catalog.validate()?;
        Ok(catalog)
    }

    pub(crate) fn family(&self, id: &str) -> Option<&Family> {
        self.families.iter().find(|family| family.id == id)
    }

    pub(crate) fn seat(&self, id: &str) -> Option<&Seat> {
        self.seats.iter().find(|seat| seat.id == id)
    }

    pub(crate) fn line(&self, id: &str) -> Option<&ProductLine> {
        self.lines.iter().find(|line| line.id == id)
    }
}

#[cfg(test)]
mod tests;
