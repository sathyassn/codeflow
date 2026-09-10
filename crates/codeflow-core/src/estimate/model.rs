//! Closed version-one forecast input and independently versioned report.

use serde::{Deserialize, Serialize};

pub(super) const MAX_INPUT_BYTES: u64 = 1024 * 1024;
pub(super) const MAX_SOURCE_BYTES: u64 = 1024 * 1024;
pub(super) const MAX_TOTAL_SOURCE_BYTES: u64 = 16 * 1024 * 1024;
pub(super) const MAX_HORIZON: u64 = 366 * 24 * 60 * 60;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Forecast {
    pub schema_version: u32,
    pub id: String,
    pub anchor_epoch_seconds: u64,
    pub horizon_seconds: u64,
    pub profile: Pin,
    pub rubric: Pin,
    pub packages: Vec<Package>,
    pub boundaries: Vec<Boundary>,
    pub resources: Vec<Resource>,
    pub scenarios: Vec<Scenario>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Pin {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Source {
    CodeflowTask { task_id: String, sha256: String },
    External { pin: Pin, reference: String },
    Declared { pin: Pin, reference: String },
}

impl Source {
    pub fn task_id(&self) -> Option<&str> {
        match self {
            Self::CodeflowTask { task_id, .. } => Some(task_id),
            Self::External { .. } | Self::Declared { .. } => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Package {
    pub id: String,
    pub source: Source,
    pub context_pins: Vec<Pin>,
    pub grade: Grade,
    pub grade_evidence: Vec<String>,
    #[serde(rename = "duration_basis")]
    pub _duration_basis: DurationBasis,
    pub stage_exclusions: Vec<StageExclusion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Grade {
    Easy,
    Medium,
    Hard,
    VeryHard,
    Unsized,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum DurationBasis {
    Judgment,
    Analogue,
    Probe,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StageExclusion {
    pub stage: Stage,
    pub reason: String,
}

/// A forecast stage, not permission to execute or an assertion of quality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Discovery,
    Implement,
    Review,
    Verify,
    Rework,
    Integrate,
    Human,
    Release,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Boundary {
    pub id: String,
    pub source: Source,
    pub evidence: String,
    pub availability: Vec<Availability>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Availability {
    pub scenario: ScenarioName,
    pub available_at_seconds: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Resource {
    pub id: String,
    pub capacity: u64,
    pub windows: Vec<Window>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Window {
    pub start_seconds: u64,
    pub end_seconds: u64,
}

/// Named scenarios do not represent percentiles or guaranteed bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioName {
    Favorable,
    Planning,
    Adverse,
}

pub(super) const SCENARIOS: [ScenarioName; 3] = [
    ScenarioName::Favorable,
    ScenarioName::Planning,
    ScenarioName::Adverse,
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Scenario {
    pub name: ScenarioName,
    pub assumptions: Vec<String>,
    pub activities: Vec<Activity>,
    pub milestones: Vec<Milestone>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Activity {
    pub id: String,
    #[serde(deserialize_with = "required_nullable")]
    pub package_id: Option<String>,
    pub stage: Stage,
    pub start_seconds: u64,
    pub duration_seconds: u64,
    pub demands: Vec<Demand>,
    pub after: Vec<String>,
    pub after_boundaries: Vec<String>,
    pub basis: String,
}

fn required_nullable<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(d)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Demand {
    pub resource_id: String,
    pub units: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Milestone {
    pub id: String,
    pub after: Vec<String>,
}

/// Computed local source assurance; never authority or predictive accuracy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceAssurance {
    Unverified,
    CanonicalLocal,
    Limited,
}

/// A deterministic diagnostic. Locations contain field names/indices, not source text.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub code: &'static str,
    pub location: String,
    pub message: &'static str,
}

/// A whole-file digest of successfully read local evidence.
#[derive(Debug, Clone, Serialize)]
pub struct SourceDigest {
    pub path: String,
    pub sha256: String,
}

/// An explicit exclusion, surfaced without judging whether it is adequate.
#[derive(Debug, Serialize)]
pub struct ReportExclusion {
    pub package_id: String,
    pub stage: Stage,
    pub reason: String,
}

/// Resource-unit seconds, not elapsed wall time.
#[derive(Debug, Serialize)]
pub struct ResourceConsumption {
    pub resource_id: String,
    pub consumption_seconds: u64,
}

/// A computed milestone offset from the forecast anchor.
#[derive(Debug, Serialize)]
pub struct MilestoneTime {
    pub id: String,
    pub at_seconds: u64,
}

/// Invalid scenarios have no totals or milestone times.
#[derive(Debug, Serialize)]
pub struct ScenarioReport {
    pub name: ScenarioName,
    pub valid: bool,
    pub elapsed_seconds: Option<u64>,
    pub resources: Vec<ResourceConsumption>,
    pub milestones: Vec<MilestoneTime>,
}

/// Versioned machine output of the read-only forecast checker.
#[derive(Debug, Serialize)]
pub struct ForecastReport {
    pub schema_version: u32,
    pub checked_package_count: usize,
    pub source_assurance: SourceAssurance,
    pub input_sha256: Option<String>,
    pub source_digests: Vec<SourceDigest>,
    pub exclusions: Vec<ReportExclusion>,
    pub scenarios: Vec<ScenarioReport>,
    pub findings: Vec<Finding>,
    pub limitation: &'static str,
}

impl ForecastReport {
    pub(super) fn new() -> Self {
        Self { schema_version: 1, checked_package_count: 0, source_assurance: SourceAssurance::Unverified,
            input_sha256: None, source_digests: Vec::new(), exclusions: Vec::new(), scenarios: Vec::new(),
            findings: Vec::new(), limitation: "Checks supplied allocations and local evidence links only; does not establish source truth, authorization, quality completeness, or predictive accuracy." }
    }

    /// Whether the supplied forecast passed all deterministic checks.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.findings.is_empty()
    }

    pub(super) fn finding(
        &mut self,
        code: &'static str,
        location: impl Into<String>,
        message: &'static str,
    ) {
        self.findings.push(Finding {
            code,
            location: location.into(),
            message,
        });
    }
}
